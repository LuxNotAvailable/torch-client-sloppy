//! Particles blocks give off on their own (`animateTick`): torches, drips, leaves, fluids.

#![allow(unused_imports)]

use super::*;

pub(super) const ANIMATE_SAMPLES: u32 = 667;

/// The day clock's tick, for the day timeline's night tracks.
static DAY_TICK: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(6000);

pub(crate) fn set_day_tick(ticks: f64) {
    DAY_TICK.store(
        ticks.rem_euclid(24000.0) as u32,
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// `minecraft:timeline/day`'s `firefly_bush_sounds` and `creaking_active`
/// tracks: on from tick 12600 to 23401, in the overworld, which runs that
/// timeline.
fn night_tracks() -> bool {
    let tick = DAY_TICK.load(std::sync::atomic::Ordering::Relaxed);
    crate::renderer::dimension::current().registry_id() == "minecraft:overworld"
        && (12600..23401).contains(&tick)
}

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
    let Some(eye) = camera.iter().next().map(GlobalTransform::translation) else {
        return;
    };
    let world_arc = particle_world();
    let Some(world) = world_arc
        .as_deref()
        .map(crate::client::tracking::read_world)
    else {
        return;
    };

    let center = [
        eye.x.floor() as i32,
        eye.y.floor() as i32,
        eye.z.floor() as i32,
    ];
    crate::prof_span!("particles:animate_tick");
    let near = NearChunks::around(&world, center);
    let rng = &mut particle_rng.0;
    let before = live.0.len();
    for _ in 0..ANIMATE_SAMPLES {
        for radius in ANIMATE_RADII {
            let cell = [
                center[0] + rng.next_int(radius) as i32 - rng.next_int(radius) as i32,
                center[1] + rng.next_int(radius) as i32 - rng.next_int(radius) as i32,
                center[2] + rng.next_int(radius) as i32 - rng.next_int(radius) as i32,
            ];
            let Some(state) = near.block(cell) else {
                continue;
            };
            do_animate_tick(cell, state, &world, atlas, status, eye, &mut live.0, rng);
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

/// Chunk columns the samples can reach: up to 31 blocks from the camera's
/// cell, which spans at most 5 columns along each axis.
const NEAR_SPAN: i32 = 5;

/// The columns around the camera, looked up once per tick so the 1,334
/// samples each skip the storage's map lookup and `Weak` upgrade.
pub(super) struct NearChunks {
    origin: [i32; 2],
    min_y: i32,
    columns: Vec<Option<std::sync::Arc<parking_lot::RwLock<azalea_world::Chunk>>>>,
}

impl NearChunks {
    pub(super) fn around(world: &azalea_world::World, center: [i32; 3]) -> NearChunks {
        let origin = [
            (center[0] >> 4) - NEAR_SPAN / 2,
            (center[2] >> 4) - NEAR_SPAN / 2,
        ];
        let columns = (0..NEAR_SPAN * NEAR_SPAN)
            .map(|i| {
                world.chunks.get(&azalea_core::position::ChunkPos {
                    x: origin[0] + i % NEAR_SPAN,
                    z: origin[1] + i / NEAR_SPAN,
                })
            })
            .collect();
        NearChunks {
            origin,
            min_y: world.chunks.min_y(),
            columns,
        }
    }

    /// The block at `cell`, or `None` past the columns held (the caller falls
    /// back to the world) or outside the world's height. Each call takes the
    /// column's read lock only for the lookup: the emitters read neighbours
    /// through the world, and holding a column across them could deadlock
    /// against a waiting writer.
    pub(super) fn block(&self, cell: [i32; 3]) -> Option<azalea::block::BlockState> {
        let dx = (cell[0] >> 4) - self.origin[0];
        let dz = (cell[2] >> 4) - self.origin[1];
        if !(0..NEAR_SPAN).contains(&dx) || !(0..NEAR_SPAN).contains(&dz) {
            return None;
        }
        let column = self.columns[(dz * NEAR_SPAN + dx) as usize].as_ref()?;
        column.read().get_block_state(
            &azalea_core::position::ChunkBlockPos::from(azalea_core::position::BlockPos {
                x: cell[0],
                y: cell[1],
                z: cell[2],
            }),
            self.min_y,
        )
    }
}

#[allow(clippy::too_many_arguments, reason = "one sample's whole context")]
pub(super) fn do_animate_tick(
    cell: [i32; 3],
    state: azalea::block::BlockState,
    world: &azalea_world::World,
    atlas: &Atlas,
    status: ParticleStatus,
    eye: Vec3,
    live: &mut Vec<Particle>,
    rng: &mut JavaRandom,
) {
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
    block_sounds(state, &mut ctx);

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

    /// `level.playLocalSound` at a point in the cell.
    #[allow(unused_variables)]
    pub(crate) fn sound(
        &mut self,
        event: &str,
        offset: [f32; 3],
        category: SoundBus,
        volume: f32,
        pitch: f32,
    ) {
        #[cfg(audio)]
        {
            let at = rel(self.cell, offset);
            let category = match category {
                SoundBus::Blocks => crate::audio::SoundCategory::Blocks,
                SoundBus::Ambient => crate::audio::SoundCategory::Ambient,
            };
            crate::audio::play_at(event, category, [at.x, at.y, at.z], volume, pitch);
        }
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

/// The sound source a block's ambient sound plays on.
#[derive(Clone, Copy)]
pub(crate) enum SoundBus {
    Blocks,
    Ambient,
}

/// `getHeight(MOTION_BLOCKING_NO_LEAVES) <= y`: nothing above blocks motion
/// but leaves, up to the loaded height.
fn open_sky_above(ctx: &Emitter, cell: [i32; 3]) -> bool {
    let top = ctx.world.chunks.min_y() + ctx.world.chunks.height() as i32;
    (cell[1] + 1..top).all(|y| {
        ctx.block([cell[0], y, cell[2]]).is_none_or(|s| {
            !crate::util::block_model::is_solid(s) || crate::blocks::tags::has(s, "leaves")
        })
    })
}

/// Blocks whose `animateTick` only plays a sound.
fn block_sounds(state: azalea::block::BlockState, ctx: &mut Emitter) {
    let cell = ctx.cell;
    let id = crate::util::block_model::block_id(state);
    let below_cell = [cell[0], cell[1] - 1, cell[2]];
    let tagged = |s: Option<azalea::block::BlockState>, tag: &str| {
        s.is_some_and(|s| crate::blocks::tags::has(s, tag))
    };
    let at = [0.0, 0.0, 0.0];
    match id {
        // `CreakingHeartBlock`: at night, rooted, wrapped in logs.
        "creaking_heart" => {
            if crate::blocks::info::property(state, "creaking_heart_state") != Some("uprooted")
                && night_tracks()
                && ctx.one_in(16)
                && surrounded_by_logs(ctx, cell)
            {
                ctx.sound("block.creaking_heart.idle", at, SoundBus::Blocks, 1.0, 1.0);
            }
        }
        // `EyeblossomBlock`: an open one on pale moss.
        "open_eyeblossom" => {
            if ctx.one_in(700)
                && ctx
                    .block(below_cell)
                    .is_some_and(|b| crate::util::block_model::block_id(b) == "pale_moss_block")
            {
                ctx.sound("block.eyeblossom.idle", at, SoundBus::Ambient, 1.0, 1.0);
            }
        }
        // `HangingMossBlock`: under pale oak.
        "pale_hanging_moss" => {
            if ctx.one_in(500) {
                let above = ctx.block([cell[0], cell[1] + 1, cell[2]]);
                if tagged(above, "pale_oak_logs")
                    || above
                        .is_some_and(|a| crate::util::block_model::block_id(a) == "pale_oak_leaves")
                {
                    ctx.sound(
                        "block.pale_hanging_moss.idle",
                        at,
                        SoundBus::Ambient,
                        1.0,
                        1.0,
                    );
                }
            }
        }
        // `AmbientDesertBlockSoundsPlayer`.
        "sand" | "red_sand" | "suspicious_sand" => {
            if ctx
                .block([cell[0], cell[1] + 1, cell[2]])
                .is_some_and(|a| a.is_air())
                && ctx.one_in(2100)
                && sand_surrounded(ctx, cell)
            {
                ctx.sound("block.sand.idle", at, SoundBus::Ambient, 1.0, 1.0);
            }
        }
        "short_dry_grass" | "tall_dry_grass" => {
            if ctx.one_in(200) && dry_vegetation_ground(ctx, cell) {
                // `playPlayerSound`: at the listener.
                #[cfg(audio)]
                crate::audio::play(
                    "block.dry_grass.ambient",
                    crate::audio::SoundCategory::Ambient,
                    1.0,
                    1.0,
                );
            }
        }
        "dead_bush" => {
            if ctx.one_in(130) {
                let under = ctx.block(below_cell);
                let badlands = under.is_some_and(|b| {
                    crate::util::block_model::block_id(b) == "red_sand"
                        || crate::blocks::tags::has(b, "terracotta")
                });
                if (!badlands || ctx.one_in(3)) && dry_vegetation_ground(ctx, cell) {
                    ctx.sound("block.deadbush.idle", at, SoundBus::Ambient, 1.0, 1.0);
                }
            }
        }
        _ => {}
    }
}

/// `CreakingHeartBlock.isSurroundedByLogs`: a log on every side.
fn surrounded_by_logs(ctx: &Emitter, cell: [i32; 3]) -> bool {
    [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 1],
        [0, 0, -1],
    ]
    .iter()
    .all(|d| {
        ctx.block([cell[0] + d[0], cell[1] + d[1], cell[2] + d[2]])
            .is_some_and(|s| crate::blocks::tags::has(s, "pale_oak_logs"))
    })
}

/// `shouldPlayDesertDryVegetationBlockSounds`: two tagged blocks beneath.
fn dry_vegetation_ground(ctx: &Emitter, cell: [i32; 3]) -> bool {
    const TAG: &str = "triggers_ambient_desert_dry_vegetation_block_sounds";
    (1..=2).all(|d| {
        ctx.block([cell[0], cell[1] - d, cell[2]])
            .is_some_and(|s| crate::blocks::tags::has(s, TAG))
    })
}

/// `shouldPlayAmbientSandSound`: the columns 8 out on all four sides hold
/// tagged sand at their surface (vanilla's `matchingBlocksFound++ >= 3`
/// needs the fourth).
fn sand_surrounded(ctx: &Emitter, cell: [i32; 3]) -> bool {
    [[8, 0], [-8, 0], [0, 8], [0, -8]]
        .iter()
        .all(|[dx, dz]| sand_column(ctx, [cell[0] + dx, cell[1], cell[2] + dz]))
}

/// `columnContainsTriggeringBlock`: near the surface, the surface block;
/// otherwise the first tagged block under air within 5 of `at`.
fn sand_column(ctx: &Emitter, at: [i32; 3]) -> bool {
    const TAG: &str = "triggers_ambient_desert_sand_block_sounds";
    let state = |y: i32| ctx.block([at[0], y, at[2]]);
    let air = |y: i32| state(y).is_none_or(|s| s.is_air());
    let tagged = |y: i32| state(y).is_some_and(|s| crate::blocks::tags::has(s, TAG));
    let top = ctx.world.chunks.min_y() + ctx.world.chunks.height() as i32;
    // `getHeight(WORLD_SURFACE) - 1`: the highest block that isn't air.
    let surface = (ctx.world.chunks.min_y()..top).rev().find(|&y| !air(y));
    let Some(surface) = surface else {
        return false;
    };
    if (surface - at[1]).abs() > 5 {
        (0..10).any(|i| {
            let y = at[1] + 5 - i;
            air(y + 1) && tagged(y)
        })
    } else {
        air(surface + 1) && tagged(surface)
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
                let chance = ctx.f32();
                if chance < 0.3 {
                    ctx.emit(&P::Smoke, pos, Vec3::ZERO, false);
                    if chance < 0.17 {
                        // Vanilla adds half a block to the wick's position.
                        let at = [wick[0] + 0.5, wick[1] + 0.5, wick[2] + 0.5];
                        let volume = 1.0 + ctx.f32();
                        let pitch = ctx.f32() * 0.7 + 0.3;
                        ctx.sound("block.candle.ambient", at, SoundBus::Blocks, volume, pitch);
                    }
                }
                ctx.emit(&P::SmallFlame, pos, Vec3::ZERO, false);
            }
        }
        Ambient::Fire => {
            // `BaseFireBlock.animateTick`.
            if ctx.one_in(24) {
                let volume = 1.0 + ctx.f32();
                let pitch = ctx.f32() * 0.7 + 0.3;
                ctx.sound(
                    "block.fire.ambient",
                    [0.5, 0.5, 0.5],
                    SoundBus::Blocks,
                    volume,
                    pitch,
                );
            }
            for _ in 0..3 {
                let x = ctx.f32();
                let y = ctx.f32() * 0.5 + 0.5;
                let z = ctx.f32();
                ctx.emit(&P::LargeSmoke, rel(cell, [x, y, z]), Vec3::ZERO, false);
            }
        }
        Ambient::Campfire => {
            if ctx.one_in(10) {
                let volume = 0.5 + ctx.f32();
                let pitch = ctx.f32() * 0.7 + 0.6;
                ctx.sound(
                    "block.campfire.crackle",
                    [0.5, 0.5, 0.5],
                    SoundBus::Blocks,
                    volume,
                    pitch,
                );
            }
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
            if ctx.rng.next_f64() < 0.1 {
                let event = match kind {
                    Furnace::Furnace => "block.furnace.fire_crackle",
                    Furnace::Blast => "block.blastfurnace.fire_crackle",
                    Furnace::Smoker => "block.smoker.smoke",
                };
                ctx.sound(event, [0.5, 0.0, 0.5], SoundBus::Blocks, 1.0, 1.0);
            }
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
            if ctx.one_in(100) {
                let pitch = ctx.f32() * 0.4 + 0.8;
                ctx.sound(
                    "block.portal.ambient",
                    [0.5, 0.5, 0.5],
                    SoundBus::Blocks,
                    0.5,
                    pitch,
                );
            }
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
            if ctx.one_in(100) {
                ctx.sound(
                    "block.respawn_anchor.ambient",
                    [0.5, 0.5, 0.5],
                    SoundBus::Blocks,
                    1.0,
                    1.0,
                );
            }
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
            // `FireflyBushBlock.animateTick`: at night, under open sky.
            if ctx.one_in(30) && night_tracks() && open_sky_above(ctx, cell) {
                ctx.sound(
                    "block.firefly_bush.idle",
                    [0.5, 0.5, 0.5],
                    SoundBus::Ambient,
                    1.0,
                    1.0,
                );
            }
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
            let event = if drag {
                ctx.emit(
                    &P::CurrentDown,
                    rel(cell, [0.5, 0.8, 0.0]),
                    Vec3::ZERO,
                    true,
                );
                "block.bubble_column.whirlpool_ambient"
            } else {
                let up = Vec3::new(0.0, 0.04, 0.0);
                ctx.emit(&P::BubbleColumnUp, rel(cell, [0.5, 0.0, 0.5]), up, true);
                let x = ctx.f32();
                let y = ctx.f32();
                let z = ctx.f32();
                ctx.emit(&P::BubbleColumnUp, rel(cell, [x, y, z]), up, true);
                "block.bubble_column.upwards_ambient"
            };
            if ctx.one_in(200) {
                let volume = 0.2 + ctx.f32() * 0.2;
                let pitch = 0.9 + ctx.f32() * 0.15;
                ctx.sound(event, [0.0; 3], SoundBus::Blocks, volume, pitch);
            }
        }
        Ambient::DriedGhast { waterlogged } => {
            // `DriedGhastBlock.animateTick`: dry, it rustles only on the
            // blocks tagged for it.
            if ctx.one_in(40)
                && (waterlogged
                    || ctx
                        .block([cell[0], cell[1] - 1, cell[2]])
                        .is_some_and(|below| {
                            crate::blocks::tags::has(
                                below,
                                "triggers_ambient_dried_ghast_block_sounds",
                            )
                        }))
            {
                let event = if waterlogged {
                    "block.dried_ghast.ambient_water"
                } else {
                    "block.dried_ghast.ambient"
                };
                ctx.sound(event, [0.5, 0.5, 0.5], SoundBus::Blocks, 1.0, 1.0);
            }
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
            // `LavaFluid.animateTick`.
            if ctx.one_in(100) {
                let x = ctx.rng.next_f64() as f32;
                let z = ctx.rng.next_f64() as f32;
                ctx.emit(&P::Lava, rel(cell, [x, 1.0, z]), Vec3::ZERO, false);
                let volume = 0.2 + ctx.f32() * 0.2;
                let pitch = 0.9 + ctx.f32() * 0.15;
                ctx.sound(
                    "block.lava.pop",
                    [x, 1.0, z],
                    SoundBus::Ambient,
                    volume,
                    pitch,
                );
            }
            if ctx.one_in(200) {
                let volume = 0.2 + ctx.f32() * 0.2;
                let pitch = 0.9 + ctx.f32() * 0.15;
                ctx.sound(
                    "block.lava.ambient",
                    [0.0; 3],
                    SoundBus::Ambient,
                    volume,
                    pitch,
                );
            }
        }
        // `WaterFluid.animateTick`: flowing water murmurs, still and
        // falling water bubbles.
        FluidKind::Water => {
            let source = fluid.amount >= 8 && !fluid.falling;
            if !source && !fluid.falling {
                if ctx.one_in(64) {
                    let volume = ctx.f32() * 0.25 + 0.75;
                    let pitch = ctx.f32() + 0.5;
                    ctx.sound(
                        "block.water.ambient",
                        [0.5, 0.5, 0.5],
                        SoundBus::Ambient,
                        volume,
                        pitch,
                    );
                }
            } else if ctx.one_in(10) {
                let x = ctx.rng.next_f64() as f32;
                let y = ctx.rng.next_f64() as f32;
                let z = ctx.rng.next_f64() as f32;
                ctx.emit(&P::Underwater, rel(cell, [x, y, z]), Vec3::ZERO, false);
            }
        }
        FluidKind::Empty => {}
    }
}

#[cfg(test)]
mod near_chunk_tests {
    use super::*;
    use azalea_core::position::{BlockPos, ChunkPos};

    #[test]
    fn near_columns_read_what_the_world_reads() {
        let mut world = azalea_world::World {
            chunks: azalea_world::ChunkStorage::new(384, -64),
            ..Default::default()
        };
        // The storage holds columns weakly; keep them alive. Leave one
        // column out to check an unloaded one reads as nothing.
        let mut held = Vec::new();
        for x in -3..=3 {
            for z in -3..=3 {
                if (x, z) != (1, -1) {
                    held.push(world.chunks.upsert(ChunkPos { x, z }, Default::default()));
                }
            }
        }
        let stone = azalea::block::BlockState::try_from(1u32).unwrap();
        for (i, pos) in [[3, 70, -5], [-20, -64, 30], [31, 319, 0], [7, 64, 7]]
            .into_iter()
            .enumerate()
        {
            let pos = BlockPos {
                x: pos[0] + i as i32,
                y: pos[1],
                z: pos[2],
            };
            world.set_block_state(pos, stone);
        }
        let center = [5, 64, 3];
        let near = NearChunks::around(&world, center);
        let mut rng = JavaRandom::new(7);
        for _ in 0..4000 {
            let cell = [
                center[0] + rng.next_int(32) as i32 - rng.next_int(32) as i32,
                center[1] - 400 + rng.next_int(800) as i32,
                center[2] + rng.next_int(32) as i32 - rng.next_int(32) as i32,
            ];
            assert_eq!(near.block(cell), block_at(&world, cell), "{cell:?}");
        }
        for pos in [[3, 70, -5], [-19, -64, 30], [33, 319, 0], [10, 64, 7]] {
            assert_eq!(near.block(pos), Some(stone), "{pos:?}");
        }
        drop(held);
    }
}
