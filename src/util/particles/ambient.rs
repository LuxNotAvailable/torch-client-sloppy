//! Particles blocks give off on their own (`animateTick`): torches, drips, leaves, fluids.

#![allow(unused_imports)]

use super::*;

pub(super) const ANIMATE_SAMPLES: u32 = 667;

pub(super) const ANIMATE_RADII: [u32; 2] = [16, 32];

#[derive(Resource, Default)]
pub(super) struct AmbientClock(f32);

pub(super) fn animate_tick_particles(
    time: Res<Time>,
    gui: Res<crate::gui::GuiState>,
    atlas: Res<ParticleAtlas>,
    camera: Query<&GlobalTransform, With<WorldCamera>>,
    mut live: ResMut<LiveParticles>,
    mut particle_rng: ResMut<ParticleRng>,
    mut clock: ResMut<AmbientClock>,
) {
    clock.0 += time.delta_secs();
    if clock.0 < TICK {
        return;
    }
    clock.0 = (clock.0 - TICK).min(TICK);

    let Some(atlas) = atlas.0.as_ref() else {
        return;
    };
    let status = gui.options.particles;
    if status == ParticleStatus::Minimal {
        return;
    }
    let Some(eye) = camera.iter().next().map(GlobalTransform::translation) else {
        return;
    };
    let world_arc = particle_world();
    let Some(world) = world_arc.as_deref().map(|w| w.read()) else {
        return;
    };

    let center = [
        eye.x.floor() as i32,
        eye.y.floor() as i32,
        eye.z.floor() as i32,
    ];
    let rng = &mut particle_rng.0;
    let before = live.0.len();
    'sampling: for _ in 0..ANIMATE_SAMPLES {
        for radius in ANIMATE_RADII {
            if live.0.len() >= MAX_LIVE_PARTICLES {
                break 'sampling;
            }
            let cell = [
                center[0] + rng.next_int(radius) as i32 - rng.next_int(radius) as i32,
                center[1] + rng.next_int(radius) as i32 - rng.next_int(radius) as i32,
                center[2] + rng.next_int(radius) as i32 - rng.next_int(radius) as i32,
            ];
            do_animate_tick(cell, &world, atlas, status, eye, &mut live.0, rng);
        }
    }

    if live.0.len() != before {
        crate::log_debug!(
            "particles",
            "animate tick spawned {}: {} live",
            live.0.len() - before,
            live.0.len()
        );
    }
}

pub(super) fn do_animate_tick(
    cell: [i32; 3],
    world: &azalea_world::World,
    atlas: &Atlas,
    status: ParticleStatus,
    eye: Vec3,
    live: &mut Vec<Particle>,
    rng: &mut JavaRandom,
) {
    let Some(state) = block_at(world, cell) else {
        return;
    };
    let mut ctx = Emitter {
        cell,
        world,
        atlas,
        status,
        eye,
        live,
        rng,
    };
    if let Some(ambient) = crate::blocks::ambient::of(state) {
        emit_ambient(ambient, state, &mut ctx);
    }

    let fluid = azalea::block::fluid_state::FluidState::from(state);
    if fluid.is_empty() {
        return;
    }
    emit_fluid_ambient(&fluid, &mut ctx);

    if ctx.rng.next_int(10) == 0 {
        let below = [cell[0], cell[1] - 1, cell[2]];
        let under = [cell[0], cell[1] - 2, cell[2]];
        if !open_cell(ctx.world, below) && open_cell(ctx.world, under) {
            let drip = match fluid.kind {
                azalea::block::fluid_state::FluidKind::Lava => P::DrippingLava,
                _ => P::DrippingWater,
            };
            let x = ctx.rng.next_f32();
            let z = ctx.rng.next_f32();
            ctx.emit(&drip, rel(below, [x, -0.05, z]), Vec3::ZERO, false);
        }
    }
}

pub(crate) struct Emitter<'a> {
    pub(crate) cell: [i32; 3],
    pub(crate) world: &'a azalea_world::World,
    pub(crate) atlas: &'a Atlas,
    pub(crate) status: ParticleStatus,
    pub(crate) eye: Vec3,
    pub(crate) live: &'a mut Vec<Particle>,
    pub(crate) rng: &'a mut JavaRandom,
}

impl Emitter<'_> {
    pub(crate) fn emit(&mut self, kind: &P, pos: Vec3, velocity: Vec3, always: bool) {
        if self.live.len() >= MAX_LIVE_PARTICLES {
            return;
        }
        add_level_particle(
            self.live,
            self.atlas,
            Some(self.world),
            self.rng,
            self.status,
            kind,
            if always {
                SpawnGate::ALWAYS_SHOW
            } else {
                SpawnGate::normal(kind)
            },
            pos,
            velocity,
            self.eye,
        );
    }

    pub(crate) fn f32(&mut self) -> f32 {
        self.rng.next_f32()
    }

    pub(crate) fn one_in(&mut self, bound: u32) -> bool {
        self.rng.next_int(bound) == 0
    }

    pub(crate) fn block(&self, cell: [i32; 3]) -> Option<azalea::block::BlockState> {
        block_at(self.world, cell)
    }

    pub(crate) fn solid(&self, cell: [i32; 3]) -> bool {
        self.block(cell)
            .is_some_and(crate::util::block_model::is_solid)
    }

    pub(crate) fn below(&mut self, kind: &P) {
        let x = self.f32();
        let z = self.f32();
        let cell = self.cell;
        self.emit(kind, rel(cell, [x, -0.05, z]), Vec3::ZERO, false);
    }
}

pub(crate) fn rel(cell: [i32; 3], offset: [f32; 3]) -> Vec3 {
    Vec3::new(
        cell[0] as f32 + offset[0],
        cell[1] as f32 + offset[1],
        cell[2] as f32 + offset[2],
    )
}

pub(super) fn open_cell(world: &azalea_world::World, cell: [i32; 3]) -> bool {
    block_at(world, cell).is_none_or(|s| {
        !crate::util::block_model::is_solid(s)
            && azalea::block::fluid_state::FluidState::from(s).is_empty()
    })
}

pub(crate) fn block_at(
    world: &azalea_world::World,
    cell: [i32; 3],
) -> Option<azalea::block::BlockState> {
    world.get_block_state(azalea_core::position::BlockPos {
        x: cell[0],
        y: cell[1],
        z: cell[2],
    })
}

pub(crate) fn dust(color: [f32; 3], scale: f32) -> P {
    P::Dust(azalea::entity::particle::DustParticle {
        color: azalea_core::color::RgbColor::new(
            (color[0] * 255.0) as u8,
            (color[1] * 255.0) as u8,
            (color[2] * 255.0) as u8,
        ),
        scale,
    })
}

pub(super) fn emit_ambient(
    ambient: crate::blocks::ambient::Ambient,
    state: azalea::block::BlockState,
    ctx: &mut Emitter,
) {
    use crate::blocks::ambient::Ambient as A;
    match ambient {
        A::Torch { .. }
        | A::Candle { .. }
        | A::Fire { .. }
        | A::Campfire { .. }
        | A::Furnace { .. }
        | A::BrewingStand { .. } => fire_and_light(ambient, state, ctx),
        A::WitherRose { .. }
        | A::EndPortal { .. }
        | A::NetherPortal { .. }
        | A::EnderChest { .. }
        | A::RespawnAnchor { .. }
        | A::EnchantingTable { .. }
        | A::EndRod { .. } => magic_and_portals(ambient, state, ctx),
        A::Mycelium { .. } | A::Leaves { .. } | A::SporeBlossom { .. } | A::FireflyBush { .. } => {
            plants(ambient, state, ctx)
        }
        A::WetSponge { .. }
        | A::Beehive { .. }
        | A::CryingObsidian { .. }
        | A::PointedDripstone { .. }
        | A::BubbleColumn { .. }
        | A::DriedGhast { .. }
        | A::FallingDust { .. } => water_and_drips(ambient, state, ctx),
        A::SculkSensor { .. }
        | A::RedstoneWire { .. }
        | A::RedstoneTorch { .. }
        | A::RedstoneOre { .. }
        | A::Repeater { .. }
        | A::Lever { .. } => redstone_and_sculk(ambient, state, ctx),
    }
}

/// Flames, smoke and furnaces.
#[allow(unused_variables, unused_imports)]
fn fire_and_light(
    ambient: crate::blocks::ambient::Ambient,
    state: azalea::block::BlockState,
    ctx: &mut Emitter,
) {
    use crate::blocks::ambient::{Ambient, Flame, Furnace, HORIZONTAL, Leaf, WireSide};

    let cell = ctx.cell;
    match ambient {
        Ambient::Torch { flame, offset } => {
            let pos = rel(cell, offset);
            ctx.emit(&P::Smoke, pos, Vec3::ZERO, false);
            let flame = match flame {
                Flame::Flame => P::Flame,
                Flame::Soul => P::SoulFireFlame,
                Flame::Copper => P::CopperFireFlame,
            };
            ctx.emit(&flame, pos, Vec3::ZERO, false);
        }
        Ambient::Candle { wicks, count } => {
            for wick in wicks.iter().take(count as usize) {
                let pos = rel(cell, *wick);
                if ctx.f32() < 0.3 {
                    ctx.emit(&P::Smoke, pos, Vec3::ZERO, false);
                }
                ctx.emit(&P::SmallFlame, pos, Vec3::ZERO, false);
            }
        }
        Ambient::Fire => {
            for _ in 0..3 {
                let x = ctx.f32();
                let y = ctx.f32() * 0.5 + 0.5;
                let z = ctx.f32();
                ctx.emit(&P::LargeSmoke, rel(cell, [x, y, z]), Vec3::ZERO, false);
            }
        }
        Ambient::Campfire => {
            if !ctx.one_in(5) {
                return;
            }
            let xa = ctx.f32() / 2.0;
            let za = ctx.f32() / 2.0;
            ctx.emit(
                &P::Lava,
                rel(cell, [0.5, 0.5, 0.5]),
                Vec3::new(xa, 5.0e-5, za),
                false,
            );
        }
        Ambient::Furnace { kind, front } => {
            if kind == Furnace::Smoker {
                ctx.emit(&P::Smoke, rel(cell, [0.5, 1.1, 0.5]), Vec3::ZERO, false);
                return;
            }
            let side = ctx.f32() * 0.6 - 0.3;
            let height = if kind == Furnace::Blast { 9.0 } else { 6.0 } / 16.0;
            let dy = ctx.f32() * height;
            let dx = if front[0] != 0.0 {
                front[0] * 0.52
            } else {
                side
            };
            let dz = if front[2] != 0.0 {
                front[2] * 0.52
            } else {
                side
            };
            let pos = rel(cell, [0.5 + dx, dy, 0.5 + dz]);
            ctx.emit(&P::Smoke, pos, Vec3::ZERO, false);
            if kind == Furnace::Furnace {
                ctx.emit(&P::Flame, pos, Vec3::ZERO, false);
            }
        }
        Ambient::BrewingStand => {
            let x = 0.4 + ctx.f32() * 0.2;
            let y = 0.7 + ctx.f32() * 0.3;
            let z = 0.4 + ctx.f32() * 0.2;
            ctx.emit(&P::Smoke, rel(cell, [x, y, z]), Vec3::ZERO, false);
        }
        _ => {}
    }
}

/// Portals, enchanting and the End.
#[allow(unused_variables, unused_imports)]
fn magic_and_portals(
    ambient: crate::blocks::ambient::Ambient,
    state: azalea::block::BlockState,
    ctx: &mut Emitter,
) {
    use crate::blocks::ambient::{Ambient, Flame, Furnace, HORIZONTAL, Leaf, WireSide};

    let cell = ctx.cell;
    match ambient {
        Ambient::WitherRose => {
            for _ in 0..3 {
                if ctx.rng.next_int(2) != 0 {
                    continue;
                }
                let x = 0.5 + ctx.f32() / 5.0;
                let y = 0.5 - ctx.f32();
                let z = 0.5 + ctx.f32() / 5.0;
                ctx.emit(&P::Smoke, rel(cell, [x, y, z]), Vec3::ZERO, false);
            }
        }
        Ambient::EndPortal => {
            let x = ctx.f32();
            let z = ctx.f32();
            ctx.emit(&P::Smoke, rel(cell, [x, 0.8, z]), Vec3::ZERO, false);
        }
        Ambient::NetherPortal { axis_x } => {
            for _ in 0..4 {
                let mut x = ctx.f32();
                let y = ctx.f32();
                let mut z = ctx.f32();
                let mut xa = (ctx.f32() - 0.5) * 0.5;
                let ya = (ctx.f32() - 0.5) * 0.5;
                let mut za = (ctx.f32() - 0.5) * 0.5;
                let flip = ctx.rng.next_int(2) as f32 * 2.0 - 1.0;
                if axis_x {
                    z = 0.5 + 0.25 * flip;
                    za = ctx.f32() * 2.0 * flip;
                } else {
                    x = 0.5 + 0.25 * flip;
                    xa = ctx.f32() * 2.0 * flip;
                }
                ctx.emit(
                    &P::Portal,
                    rel(cell, [x, y, z]),
                    Vec3::new(xa, ya, za),
                    false,
                );
            }
        }
        Ambient::EnderChest => {
            for _ in 0..3 {
                let flip_x = ctx.rng.next_int(2) as f32 * 2.0 - 1.0;
                let flip_z = ctx.rng.next_int(2) as f32 * 2.0 - 1.0;
                let y = ctx.f32();
                let xa = ctx.f32() * flip_x;
                let ya = (ctx.f32() - 0.5) * 0.125;
                let za = ctx.f32() * flip_z;
                ctx.emit(
                    &P::Portal,
                    rel(cell, [0.5 + 0.25 * flip_x, y, 0.5 + 0.25 * flip_z]),
                    Vec3::new(xa, ya, za),
                    false,
                );
            }
        }
        Ambient::RespawnAnchor => {
            let x = 0.5 + (0.5 - ctx.f32());
            let z = 0.5 + (0.5 - ctx.f32());
            let ya = ctx.f32() * 0.04;
            ctx.emit(
                &P::ReversePortal,
                rel(cell, [x, 1.0, z]),
                Vec3::new(0.0, ya, 0.0),
                false,
            );
        }
        Ambient::EnchantingTable => {
            for dy in 0..2i32 {
                for dx in -2..=2i32 {
                    for dz in -2..=2i32 {
                        if dx.abs() != 2 && dz.abs() != 2 {
                            continue;
                        }
                        if !ctx.one_in(16) {
                            continue;
                        }
                        let shelf = [cell[0] + dx, cell[1] + dy, cell[2] + dz];
                        let between = [cell[0] + dx / 2, cell[1] + dy, cell[2] + dz / 2];
                        let is_shelf = ctx
                            .block(shelf)
                            .is_some_and(|s| s.as_block_kind().to_str() == "minecraft:bookshelf");
                        if !is_shelf || ctx.solid(between) {
                            continue;
                        }
                        let ox = dx as f32 + ctx.f32() - 0.5;
                        let oy = dy as f32 - ctx.f32() - 1.0;
                        let oz = dz as f32 + ctx.f32() - 0.5;
                        ctx.emit(
                            &P::Enchant,
                            rel(cell, [0.5, 2.0, 0.5]),
                            Vec3::new(ox, oy, oz),
                            false,
                        );
                    }
                }
            }
        }
        Ambient::EndRod { step } => {
            if !ctx.one_in(5) {
                return;
            }
            let x = 0.55 - ctx.f32() * 0.1;
            let y = 0.55 - ctx.f32() * 0.1;
            let z = 0.55 - ctx.f32() * 0.1;
            let reach = 0.4 - (ctx.f32() + ctx.f32()) * 0.4;
            let vel = Vec3::new(
                ctx.rng.next_gaussian() as f32 * 0.005,
                ctx.rng.next_gaussian() as f32 * 0.005,
                ctx.rng.next_gaussian() as f32 * 0.005,
            );
            ctx.emit(
                &P::EndRod,
                rel(
                    cell,
                    [
                        x + step[0] * reach,
                        y + step[1] * reach,
                        z + step[2] * reach,
                    ],
                ),
                vel,
                false,
            );
        }
        _ => {}
    }
}

/// Leaves, spores, mycelium and fireflies.
#[allow(unused_variables, unused_imports)]
fn plants(
    ambient: crate::blocks::ambient::Ambient,
    state: azalea::block::BlockState,
    ctx: &mut Emitter,
) {
    use crate::blocks::ambient::{Ambient, Flame, Furnace, HORIZONTAL, Leaf, WireSide};

    let cell = ctx.cell;
    match ambient {
        Ambient::Mycelium => {
            if !ctx.one_in(10) {
                return;
            }
            let x = ctx.f32();
            let z = ctx.f32();
            ctx.emit(&P::Mycelium, rel(cell, [x, 1.1, z]), Vec3::ZERO, false);
        }
        Ambient::Leaves { leaf, chance } => {
            let below = [cell[0], cell[1] - 1, cell[2]];
            let open = !ctx.solid(below);
            let exposed = crate::renderer::environment::rain_level() > 0.0
                && crate::renderer::lightmap::levels_at([
                    cell[0] as f32 + 0.5,
                    cell[1] as f32 + 1.5,
                    cell[2] as f32 + 0.5,
                ])
                .1 >= 15;
            if exposed && open && ctx.rng.next_int(15) == 1 {
                ctx.below(&P::DrippingWater);
            }
            if ctx.f32() < chance && open {
                let particle = match leaf {
                    Leaf::BiomeTinted | Leaf::AzaleaTint => {
                        P::TintedLeaves(azalea::entity::particle::ColorParticle::default())
                    }
                    Leaf::Cherry => P::CherryLeaves,
                    Leaf::PaleOak => P::PaleOakLeaves,
                    Leaf::RedPoplar => P::RedPoplarLeaves,
                    Leaf::OrangePoplar => P::OrangePoplarLeaves,
                    Leaf::YellowPoplar => P::YellowPoplarLeaves,
                };
                ctx.below(&particle);
            }
        }
        Ambient::SporeBlossom => {
            let x = ctx.f32();
            let z = ctx.f32();
            ctx.emit(
                &P::FallingSporeBlossom,
                rel(cell, [x, 0.7, z]),
                Vec3::ZERO,
                false,
            );
            for _ in 0..14 {
                let ax = cell[0] + ctx.rng.next_int(21) as i32 - 10;
                let ay = cell[1] - ctx.rng.next_int(10) as i32;
                let az = cell[2] + ctx.rng.next_int(21) as i32 - 10;
                if ctx.solid([ax, ay, az]) {
                    continue;
                }
                let ox = ctx.f32();
                let oy = ctx.f32();
                let oz = ctx.f32();
                ctx.emit(
                    &P::SporeBlossomAir,
                    rel([ax, ay, az], [ox, oy, oz]),
                    Vec3::ZERO,
                    false,
                );
            }
        }
        Ambient::FireflyBush => {
            let (block_light, sky_light) = crate::renderer::lightmap::levels_at([
                cell[0] as f32 + 0.5,
                cell[1] as f32 + 0.5,
                cell[2] as f32 + 0.5,
            ]);
            if block_light.max(sky_light) > 13 {
                return;
            }
            if ctx.rng.next_f64() > 0.7 {
                return;
            }
            let x = ctx.rng.next_f64() as f32 * 10.0 - 5.0;
            let y = ctx.rng.next_f64() as f32 * 5.0;
            let z = ctx.rng.next_f64() as f32 * 10.0 - 5.0;
            ctx.emit(&P::Firefly, rel(cell, [x, y, z]), Vec3::ZERO, false);
        }
        _ => {}
    }
}

/// Drips, sponges, bubbles and falling dust.
#[allow(unused_variables, unused_imports)]
fn water_and_drips(
    ambient: crate::blocks::ambient::Ambient,
    state: azalea::block::BlockState,
    ctx: &mut Emitter,
) {
    use crate::blocks::ambient::{Ambient, Flame, Furnace, HORIZONTAL, Leaf, WireSide};

    let cell = ctx.cell;
    match ambient {
        Ambient::WetSponge => {
            let Some(pos) = random_open_face(ctx, 0.05) else {
                return;
            };
            ctx.emit(&P::DrippingWater, pos, Vec3::ZERO, false);
        }
        Ambient::Beehive => ctx.below(&P::DrippingHoney),
        Ambient::CryingObsidian => {
            if !ctx.one_in(5) {
                return;
            }
            let Some(pos) = random_open_face(ctx, 0.1) else {
                return;
            };
            ctx.emit(&P::DrippingObsidianTear, pos, Vec3::ZERO, false);
        }
        Ambient::PointedDripstone => {
            if ctx.f32() > 0.12 {
                return;
            }
            let mut probe = cell;
            let mut fluid = None;
            for _ in 0..11 {
                probe[1] += 1;
                let Some(above) = ctx.block(probe) else { break };
                if above
                    .as_block_kind()
                    .to_str()
                    .ends_with("pointed_dripstone")
                {
                    continue;
                }
                let f = azalea::block::fluid_state::FluidState::from(above);
                if !f.is_empty() {
                    fluid = Some(f.kind);
                }
                break;
            }
            let drip = match fluid {
                Some(azalea::block::fluid_state::FluidKind::Water) => P::DrippingDripstoneWater,
                Some(azalea::block::fluid_state::FluidKind::Lava) => P::DrippingDripstoneLava,
                _ => return,
            };
            ctx.emit(&drip, rel(cell, [0.5, 0.25, 0.5]), Vec3::ZERO, false);
        }
        Ambient::BubbleColumn { drag } => {
            if drag {
                ctx.emit(
                    &P::CurrentDown,
                    rel(cell, [0.5, 0.8, 0.0]),
                    Vec3::ZERO,
                    true,
                );
            } else {
                let up = Vec3::new(0.0, 0.04, 0.0);
                ctx.emit(&P::BubbleColumnUp, rel(cell, [0.5, 0.0, 0.5]), up, true);
                let x = ctx.f32();
                let y = ctx.f32();
                let z = ctx.f32();
                ctx.emit(&P::BubbleColumnUp, rel(cell, [x, y, z]), up, true);
            }
        }
        Ambient::DriedGhast { waterlogged } => {
            if !ctx.one_in(6) {
                return;
            }
            if waterlogged {
                let x = 0.5 + (ctx.f32() * 2.0 - 1.0) / 3.0;
                let z = 0.5 + (ctx.f32() * 2.0 - 1.0) / 3.0;
                let ya = ctx.f32();
                ctx.emit(
                    &P::HappyVillager,
                    rel(cell, [x, 0.9, z]),
                    Vec3::new(0.0, ya, 0.0),
                    false,
                );
            } else {
                ctx.emit(
                    &P::WhiteSmoke,
                    rel(cell, [0.5, 0.5, 0.5]),
                    Vec3::new(0.0, 0.02, 0.0),
                    false,
                );
            }
        }
        Ambient::FallingDust => {
            if !ctx.one_in(16) {
                return;
            }
            let below = [cell[0], cell[1] - 1, cell[2]];
            if ctx.solid(below) {
                return;
            }
            let particle =
                P::FallingDust(azalea::entity::particle::BlockParticle { block_state: state });
            ctx.below(&particle);
        }
        _ => {}
    }
}

/// Redstone parts and sculk.
#[allow(unused_variables, unused_imports)]
fn redstone_and_sculk(
    ambient: crate::blocks::ambient::Ambient,
    state: azalea::block::BlockState,
    ctx: &mut Emitter,
) {
    use crate::blocks::ambient::{Ambient, Flame, Furnace, HORIZONTAL, Leaf, WireSide};

    let cell = ctx.cell;
    match ambient {
        Ambient::SculkSensor => {
            let step = HORIZONTAL[ctx.rng.next_int(4) as usize];
            let x = if step[0] == 0.0 {
                0.5 - ctx.f32()
            } else {
                step[0] * 0.6
            };
            let z = if step[2] == 0.0 {
                0.5 - ctx.f32()
            } else {
                step[2] * 0.6
            };
            let ya = ctx.f32() * 0.04;
            let particle =
                P::DustColorTransition(azalea::entity::particle::DustColorTransitionParticle {
                    from: azalea_core::color::RgbColor::new(0x39, 0xD0, 0xE0),
                    to: azalea_core::color::RgbColor::new(0xFF, 0x00, 0x00),
                    scale: 1.0,
                });
            ctx.emit(
                &particle,
                rel(cell, [0.5 + x, 0.25, 0.5 + z]),
                Vec3::new(0.0, ya, 0.0),
                false,
            );
        }
        Ambient::RedstoneWire { power, sides } => {
            let color = crate::util::biome_color::redstone_ratio(power);
            const DOWN: [f32; 3] = [0.0, -1.0, 0.0];
            const UP: [f32; 3] = [0.0, 1.0, 0.0];
            for (i, side) in sides.iter().enumerate() {
                let horizontal = HORIZONTAL[i];
                match side {
                    WireSide::Up => {
                        wire_line(ctx, color, horizontal, UP, -0.5, 0.5);
                        wire_line(ctx, color, DOWN, horizontal, 0.0, 0.5);
                    }
                    WireSide::Side => wire_line(ctx, color, DOWN, horizontal, 0.0, 0.5),
                    WireSide::None => wire_line(ctx, color, DOWN, horizontal, 0.0, 0.3),
                }
            }
        }
        Ambient::RedstoneTorch => {
            let x = 0.5 + (ctx.f32() - 0.5) * 0.2;
            let y = 0.7 + (ctx.f32() - 0.5) * 0.2;
            let z = 0.5 + (ctx.f32() - 0.5) * 0.2;
            let particle = dust([1.0, 0.0, 0.0], 1.0);
            ctx.emit(&particle, rel(cell, [x, y, z]), Vec3::ZERO, false);
        }
        Ambient::RedstoneOre => {
            const FACES: [[f32; 3]; 6] = [
                [0.0, -1.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, -1.0],
                [0.0, 0.0, 1.0],
                [-1.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
            ];
            for step in FACES {
                let neighbour = [
                    cell[0] + step[0] as i32,
                    cell[1] + step[1] as i32,
                    cell[2] + step[2] as i32,
                ];
                if ctx.solid(neighbour) {
                    continue;
                }
                let x = if step[0] == 0.0 {
                    ctx.f32()
                } else {
                    0.5 + 0.5625 * step[0]
                };
                let y = if step[1] == 0.0 {
                    ctx.f32()
                } else {
                    0.5 + 0.5625 * step[1]
                };
                let z = if step[2] == 0.0 {
                    ctx.f32()
                } else {
                    0.5 + 0.5625 * step[2]
                };
                let particle = dust([1.0, 0.0, 0.0], 1.0);
                ctx.emit(&particle, rel(cell, [x, y, z]), Vec3::ZERO, false);
            }
        }
        Ambient::Repeater { step, delay } => {
            let x = 0.5 + (ctx.f32() - 0.5) * 0.2;
            let y = 0.4 + (ctx.f32() - 0.5) * 0.2;
            let z = 0.5 + (ctx.f32() - 0.5) * 0.2;
            let offset = if ctx.rng.next_int(2) == 0 {
                delay as f32 * 2.0 - 1.0
            } else {
                -5.0
            } / 16.0;
            let particle = dust([1.0, 0.0, 0.0], 1.0);
            ctx.emit(
                &particle,
                rel(cell, [x + offset * step[0], y, z + offset * step[2]]),
                Vec3::ZERO,
                false,
            );
        }
        Ambient::Lever { offset } => {
            if ctx.f32() >= 0.25 {
                return;
            }
            let particle = dust([1.0, 0.0, 0.0], 0.5);
            ctx.emit(&particle, rel(cell, offset), Vec3::ZERO, false);
        }
        _ => {}
    }
}

pub(super) fn wire_line(
    ctx: &mut Emitter,
    color: [f32; 3],
    side: [f32; 3],
    along: [f32; 3],
    from: f32,
    to: f32,
) {
    let span = to - from;
    if ctx.f32() >= 0.2 * span {
        return;
    }
    let t = from + span * ctx.f32();
    let cell = ctx.cell;
    let particle = dust(color, 1.0);
    ctx.emit(
        &particle,
        rel(
            cell,
            [
                0.5 + 0.4375 * side[0] + t * along[0],
                0.5 + 0.4375 * side[1] + t * along[1],
                0.5 + 0.4375 * side[2] + t * along[2],
            ],
        ),
        Vec3::ZERO,
        false,
    );
}

pub(super) fn random_open_face(ctx: &mut Emitter, inset: f32) -> Option<Vec3> {
    const FACES: [[f32; 3]; 5] = [
        [0.0, -1.0, 0.0],
        [0.0, 0.0, -1.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
    ];
    let step = FACES[ctx.rng.next_int(5) as usize];
    let cell = ctx.cell;
    let neighbour = [
        cell[0] + step[0] as i32,
        cell[1] + step[1] as i32,
        cell[2] + step[2] as i32,
    ];
    if ctx.solid(neighbour) {
        return None;
    }
    let spread = 0.5 + inset;
    let x = if step[0] == 0.0 {
        ctx.f32()
    } else {
        0.5 + step[0] * spread
    };
    let y = if step[1] == 0.0 {
        ctx.f32() * 0.8
    } else {
        0.5 + step[1] * spread
    };
    let z = if step[2] == 0.0 {
        ctx.f32()
    } else {
        0.5 + step[2] * spread
    };
    Some(rel(cell, [x, y, z]))
}

pub(super) fn emit_fluid_ambient(
    fluid: &azalea::block::fluid_state::FluidState,
    ctx: &mut Emitter,
) {
    use azalea::block::fluid_state::FluidKind;

    let cell = ctx.cell;
    match fluid.kind {
        FluidKind::Lava => {
            let above = [cell[0], cell[1] + 1, cell[2]];
            if ctx.block(above).is_some_and(|s| !s.is_air()) {
                return;
            }
            if !ctx.one_in(100) {
                return;
            }
            let x = ctx.f32();
            let z = ctx.f32();
            ctx.emit(&P::Lava, rel(cell, [x, 1.0, z]), Vec3::ZERO, false);
        }
        FluidKind::Water => {
            if fluid.amount == 0 || fluid.amount >= 8 {
                return;
            }
            if !ctx.one_in(10) {
                return;
            }
            let x = ctx.f32();
            let y = ctx.f32();
            let z = ctx.f32();
            ctx.emit(&P::Underwater, rel(cell, [x, y, z]), Vec3::ZERO, false);
        }
        FluidKind::Empty => {}
    }
}
