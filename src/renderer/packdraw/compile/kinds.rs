use crate::shaderpack::programs::Geometry;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AlphaTest {
    pub(crate) function: AlphaFunction,
    pub(crate) reference: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AlphaFunction {
    Never,
    Less,
    Equal,
    LessEqual,
    Greater,
    NotEqual,
    GreaterEqual,
}

impl AlphaFunction {
    pub(crate) fn operator(self) -> &'static str {
        match self {
            AlphaFunction::Never | AlphaFunction::Less => "<",
            AlphaFunction::Equal => "==",
            AlphaFunction::LessEqual => "<=",
            AlphaFunction::Greater => ">",
            AlphaFunction::NotEqual => "!=",
            AlphaFunction::GreaterEqual => ">=",
        }
    }
}

impl AlphaTest {
    const fn greater(reference: f32) -> AlphaTest {
        AlphaTest {
            function: AlphaFunction::Greater,
            reference,
        }
    }

    pub(crate) fn wrapper_reference(self) -> f32 {
        if self.function == AlphaFunction::Never {
            -1.0
        } else {
            self.reference
        }
    }
}

pub(crate) const ONE_TENTH_ALPHA: AlphaTest = AlphaTest::greater(0.1);
pub(crate) const HALF_ALPHA: AlphaTest = AlphaTest::greater(0.5);
pub(crate) const NON_ZERO_ALPHA: AlphaTest = AlphaTest::greater(1.0e-4);

pub(crate) fn geometry_alpha(geometry: Geometry, shadow: bool) -> Option<AlphaTest> {
    match geometry {
        Geometry::TerrainSolid => None,
        Geometry::TerrainCutout => Some(HALF_ALPHA),
        Geometry::Water if shadow => None,
        Geometry::Water => Some(NON_ZERO_ALPHA),
    }
}

pub(crate) fn alpha_for(
    program_override: Option<Option<AlphaTest>>,
    default: Option<AlphaTest>,
) -> Option<AlphaTest> {
    program_override.unwrap_or(default)
}

pub(super) fn alpha_tests(
    properties: &std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, Option<AlphaTest>> {
    properties
        .iter()
        .filter_map(|(key, value)| {
            let program = key.strip_prefix("alphaTest.")?;
            if value == "off" || value == "false" {
                return Some((program.to_owned(), None));
            }
            let parts: Vec<&str> = value.split(' ').collect();
            let [name, reference, ..] = parts[..] else {
                return None;
            };
            let function = match name {
                "ALWAYS" | "GL_ALWAYS" => None,
                "NEVER" => Some(AlphaFunction::Never),
                "LESS" => Some(AlphaFunction::Less),
                "EQUAL" => Some(AlphaFunction::Equal),
                "LEQUAL" => Some(AlphaFunction::LessEqual),
                "GREATER" => Some(AlphaFunction::Greater),
                "NOTEQUAL" => Some(AlphaFunction::NotEqual),
                "GEQUAL" => Some(AlphaFunction::GreaterEqual),
                _ => return None,
            };
            let reference: f32 = reference.parse().ok()?;
            Some((
                program.to_owned(),
                function.map(|function| AlphaTest {
                    function,
                    reference,
                }),
            ))
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum EntityKind {
    Entity,
    Block,
    Particle,
    SkyBasic,
    SkyTextured,
    Clouds,
    Hand,
    HandWater,
    ShadowCaster,
    EntityTranslucent,
    BlockTranslucent,
    ParticleTranslucent,
    DamagedBlock,
    Line,
    SpiderEyes,
    BeaconBeam,
    /// Rain, snow and entities' round shadows: soft layers over the world.
    Weather,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Segment {
    Shadow,
    Sky,
    World,
    HandSolid,
    WorldTranslucent,
    AfterTranslucent,
    HandTranslucent,
}

impl EntityKind {
    pub(crate) const COUNT: usize = 17;
    pub(crate) const ALL: [EntityKind; EntityKind::COUNT] = [
        EntityKind::Entity,
        EntityKind::Block,
        EntityKind::Particle,
        EntityKind::SkyBasic,
        EntityKind::SkyTextured,
        EntityKind::Clouds,
        EntityKind::Hand,
        EntityKind::HandWater,
        EntityKind::ShadowCaster,
        EntityKind::EntityTranslucent,
        EntityKind::BlockTranslucent,
        EntityKind::ParticleTranslucent,
        EntityKind::DamagedBlock,
        EntityKind::Line,
        EntityKind::SpiderEyes,
        EntityKind::BeaconBeam,
        EntityKind::Weather,
    ];

    pub(crate) fn index(self) -> usize {
        self as usize
    }

    fn programs(self) -> &'static [&'static str] {
        match self {
            EntityKind::Entity => &[
                "gbuffers_entities",
                "gbuffers_textured_lit",
                "gbuffers_textured",
                "gbuffers_basic",
            ],
            EntityKind::Block => &[
                "gbuffers_block",
                "gbuffers_terrain",
                "gbuffers_textured_lit",
                "gbuffers_textured",
                "gbuffers_basic",
            ],
            EntityKind::Particle => &[
                "gbuffers_particles",
                "gbuffers_textured_lit",
                "gbuffers_textured",
                "gbuffers_basic",
            ],
            EntityKind::SkyBasic => &["gbuffers_skybasic", "gbuffers_basic"],
            EntityKind::SkyTextured => &[
                "gbuffers_skytextured",
                "gbuffers_textured",
                "gbuffers_basic",
            ],
            EntityKind::Clouds => &["gbuffers_clouds", "gbuffers_textured", "gbuffers_basic"],
            EntityKind::Hand => &[
                "gbuffers_hand",
                "gbuffers_textured_lit",
                "gbuffers_textured",
                "gbuffers_basic",
            ],
            EntityKind::HandWater => &[
                "gbuffers_hand_water",
                "gbuffers_hand",
                "gbuffers_textured_lit",
                "gbuffers_textured",
                "gbuffers_basic",
            ],
            EntityKind::ShadowCaster => &["shadow_entities", "shadow"],
            EntityKind::EntityTranslucent => &[
                "gbuffers_entities_translucent",
                "gbuffers_entities",
                "gbuffers_textured_lit",
                "gbuffers_textured",
                "gbuffers_basic",
            ],
            EntityKind::BlockTranslucent => &[
                "gbuffers_block_translucent",
                "gbuffers_block",
                "gbuffers_terrain",
                "gbuffers_textured_lit",
                "gbuffers_textured",
                "gbuffers_basic",
            ],
            EntityKind::ParticleTranslucent => &[
                "gbuffers_particles_translucent",
                "gbuffers_particles",
                "gbuffers_textured_lit",
                "gbuffers_textured",
                "gbuffers_basic",
            ],
            EntityKind::DamagedBlock => &[
                "gbuffers_damagedblock",
                "gbuffers_terrain",
                "gbuffers_textured_lit",
                "gbuffers_textured",
                "gbuffers_basic",
            ],
            EntityKind::Line => &["gbuffers_line", "gbuffers_basic"],
            EntityKind::SpiderEyes => {
                &["gbuffers_spidereyes", "gbuffers_textured", "gbuffers_basic"]
            }
            EntityKind::BeaconBeam => {
                &["gbuffers_beaconbeam", "gbuffers_textured", "gbuffers_basic"]
            }
            EntityKind::Weather => &[
                "gbuffers_weather",
                "gbuffers_textured_lit",
                "gbuffers_textured",
                "gbuffers_basic",
            ],
        }
    }

    pub(crate) fn resolve(self, has: impl Fn(&str) -> bool) -> Option<&'static str> {
        self.programs().iter().copied().find(|n| has(n))
    }

    pub(crate) fn segment(self) -> Segment {
        match self {
            EntityKind::Entity | EntityKind::Block | EntityKind::Particle => Segment::World,
            EntityKind::SkyBasic | EntityKind::SkyTextured => Segment::Sky,
            EntityKind::Clouds => Segment::AfterTranslucent,
            EntityKind::Hand => Segment::HandSolid,
            EntityKind::HandWater => Segment::HandTranslucent,
            EntityKind::ShadowCaster => Segment::Shadow,
            EntityKind::EntityTranslucent
            | EntityKind::BlockTranslucent
            | EntityKind::ParticleTranslucent => Segment::WorldTranslucent,
            EntityKind::DamagedBlock | EntityKind::Line | EntityKind::SpiderEyes => Segment::World,
            // Iris draws beams with the block entities; their glow blends, so
            // they go after the translucent terrain they shine through.
            EntityKind::BeaconBeam => Segment::WorldTranslucent,
            // Iris draws weather after the translucent terrain.
            EntityKind::Weather => Segment::AfterTranslucent,
        }
    }

    pub(crate) fn render_stage(self) -> i32 {
        crate::shaderpack::features::render_stage(match self {
            EntityKind::Entity
            | EntityKind::ShadowCaster
            | EntityKind::EntityTranslucent
            | EntityKind::SpiderEyes => "ENTITIES",
            EntityKind::DamagedBlock => "DESTROY",
            EntityKind::Line => "OUTLINE",
            EntityKind::Block | EntityKind::BlockTranslucent | EntityKind::BeaconBeam => {
                "BLOCK_ENTITIES"
            }
            EntityKind::Particle | EntityKind::ParticleTranslucent => "PARTICLES",
            EntityKind::SkyBasic => "SKY",
            EntityKind::SkyTextured => "SUN",
            EntityKind::Clouds => "CLOUDS",
            EntityKind::Hand => "HAND_SOLID",
            EntityKind::HandWater => "HAND_TRANSLUCENT",
            EntityKind::Weather => "RAIN_SNOW",
        })
    }

    pub(crate) fn fallback(self) -> Option<EntityKind> {
        match self {
            EntityKind::Block | EntityKind::Particle => Some(EntityKind::Entity),
            EntityKind::BlockTranslucent | EntityKind::ParticleTranslucent => {
                Some(EntityKind::EntityTranslucent)
            }
            _ => None,
        }
    }

    pub(crate) fn translucent(self) -> EntityKind {
        match self {
            EntityKind::Entity => EntityKind::EntityTranslucent,
            EntityKind::Block => EntityKind::BlockTranslucent,
            EntityKind::Particle => EntityKind::ParticleTranslucent,
            other => other,
        }
    }

    pub(crate) fn alpha_tests(self) -> [Option<AlphaTest>; 2] {
        match self {
            EntityKind::SkyBasic | EntityKind::SpiderEyes | EntityKind::Weather => {
                [Some(NON_ZERO_ALPHA); 2]
            }
            EntityKind::SkyTextured | EntityKind::Line => [None; 2],
            _ => [Some(ONE_TENTH_ALPHA); 2],
        }
    }

    pub(crate) fn is_hand(self) -> bool {
        matches!(self, EntityKind::Hand | EntityKind::HandWater)
    }
}

pub(super) fn true_key(
    properties: &std::collections::HashMap<String, String>,
    key: &str,
    default: bool,
) -> bool {
    properties.get(key).map_or(default, |v| v.trim() == "true")
}

pub(super) fn false_key(properties: &std::collections::HashMap<String, String>, key: &str) -> bool {
    properties.get(key).is_none_or(|v| v.trim() != "false")
}

pub(super) fn clouds_off(properties: &std::collections::HashMap<String, String>) -> bool {
    properties.get("clouds").is_some_and(|v| v.trim() == "off")
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ShadowCulling {
    #[default]
    Default,
    Distance,
    Advanced,
    SafeZone,
}

impl ShadowCulling {
    pub(crate) fn mode(
        self,
        voxelizes: bool,
    ) -> crate::renderer::terrain_pool::cull::ShadowCullMode {
        use crate::renderer::terrain_pool::cull::ShadowCullMode;
        match self {
            ShadowCulling::Distance => ShadowCullMode::Distance,
            ShadowCulling::Advanced => ShadowCullMode::Advanced,
            ShadowCulling::SafeZone => ShadowCullMode::SafeZone,
            ShadowCulling::Default if voxelizes => ShadowCullMode::Distance,
            ShadowCulling::Default => ShadowCullMode::Advanced,
        }
    }

    pub(super) fn parse(value: Option<&str>) -> ShadowCulling {
        match value.map(str::trim) {
            Some("false") => ShadowCulling::Distance,
            Some("true") => ShadowCulling::Advanced,
            Some("reversed" | "safe_zone") => ShadowCulling::SafeZone,
            _ => ShadowCulling::Default,
        }
    }
}

/// `flip.<pass>.<buffer>=true|false` from `shaders.properties`: after that
/// pass, swap the buffer's two copies whether or not the pass wrote it
/// (`true`), or keep them as they were even though it did (`false`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FlipOverrides {
    passes: std::collections::HashMap<String, Vec<(u8, bool)>>,
}

impl FlipOverrides {
    pub(crate) fn read(properties: &std::collections::HashMap<String, String>) -> FlipOverrides {
        let mut out = FlipOverrides::default();
        for (key, value) in properties {
            let Some((pass, buffer)) = key
                .strip_prefix("flip.")
                .and_then(|rest| rest.rsplit_once('.'))
            else {
                continue;
            };
            let Some(index) = crate::shaderpack::directives::target_index(buffer) else {
                continue;
            };
            let flip = match value.trim() {
                "true" => true,
                "false" => false,
                _ => continue,
            };
            out.passes
                .entry(pass.to_owned())
                .or_default()
                .push((index as u8, flip));
        }
        out
    }

    /// The buffers whose swap after `pass` differs from what drawing
    /// `written` alone would do.
    pub(crate) fn toggles(&self, pass: &str, written: &[u8]) -> Vec<u8> {
        self.passes
            .get(pass)
            .into_iter()
            .flatten()
            .filter(|(buffer, flip)| *flip != written.contains(buffer))
            .map(|(buffer, _)| *buffer)
            .collect()
    }
}

/// `blend.<program>` and `blend.<program>.<buffer>` from `shaders.properties`:
/// `off`, or four GL factors (source and destination colour, then alpha).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Blends {
    programs: std::collections::HashMap<String, ProgramBlend>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ProgramBlend {
    all: Option<Option<bevy::render::render_resource::BlendState>>,
    buffers: std::collections::HashMap<u8, Option<bevy::render::render_resource::BlendState>>,
}

fn blend_factor(name: &str) -> Option<bevy::render::render_resource::BlendFactor> {
    use bevy::render::render_resource::BlendFactor as F;
    Some(match name {
        "ZERO" => F::Zero,
        "ONE" => F::One,
        "SRC_COLOR" => F::Src,
        "ONE_MINUS_SRC_COLOR" => F::OneMinusSrc,
        "DST_COLOR" => F::Dst,
        "ONE_MINUS_DST_COLOR" => F::OneMinusDst,
        "SRC_ALPHA" => F::SrcAlpha,
        "ONE_MINUS_SRC_ALPHA" => F::OneMinusSrcAlpha,
        "DST_ALPHA" => F::DstAlpha,
        "ONE_MINUS_DST_ALPHA" => F::OneMinusDstAlpha,
        "SRC_ALPHA_SATURATE" => F::SrcAlphaSaturated,
        _ => return None,
    })
}

fn blend_value(value: &str) -> Option<Option<bevy::render::render_resource::BlendState>> {
    let words: Vec<&str> = value.split_whitespace().collect();
    match words.as_slice() {
        [off] if off.eq_ignore_ascii_case("off") => Some(None),
        [src, dst, src_a, dst_a] => {
            let component = |src: &str, dst: &str| {
                Some(bevy::render::render_resource::BlendComponent {
                    src_factor: blend_factor(src)?,
                    dst_factor: blend_factor(dst)?,
                    operation: bevy::render::render_resource::BlendOperation::Add,
                })
            };
            Some(Some(bevy::render::render_resource::BlendState {
                color: component(src, dst)?,
                alpha: component(src_a, dst_a)?,
            }))
        }
        _ => None,
    }
}

impl Blends {
    pub(crate) fn read(properties: &std::collections::HashMap<String, String>) -> Blends {
        let mut blends = Blends::default();
        for (key, value) in properties {
            let Some(rest) = key.strip_prefix("blend.") else {
                continue;
            };
            let Some(state) = blend_value(value) else {
                crate::log_warn!("shaders", "{key}: unreadable blend mode {value:?}");
                continue;
            };
            let (program, buffer) = match rest.rsplit_once('.') {
                Some((program, buffer)) => match crate::shaderpack::directives::target_index(buffer) {
                    Some(index) => (program, Some(index as u8)),
                    None => (rest, None),
                },
                None => (rest, None),
            };
            let entry = blends.programs.entry(program.to_owned()).or_default();
            match buffer {
                Some(buffer) => {
                    entry.buffers.insert(buffer, state);
                }
                None => entry.all = Some(state),
            }
        }
        blends
    }

    /// The blend for `program` writing `buffer`, else `default`.
    pub(crate) fn resolve(
        &self,
        program: &str,
        buffer: u8,
        default: Option<bevy::render::render_resource::BlendState>,
    ) -> Option<bevy::render::render_resource::BlendState> {
        let Some(entry) = self.programs.get(program) else {
            return default;
        };
        entry
            .buffers
            .get(&buffer)
            .copied()
            .or(entry.all)
            .unwrap_or(default)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ShadowCasters {
    pub(crate) entities: bool,
    pub(crate) player: bool,
    pub(crate) block_entities: bool,
    /// `shadowTerrain`: terrain casts at all.
    pub(crate) terrain: bool,
    /// `shadowTranslucent`: water and other translucent terrain casts.
    pub(crate) translucent: bool,
    /// `shadowSolidBackFaces` (this client's): solid terrain draws only the
    /// faces turned from the light. Solid terrain is closed, so a ray from a
    /// surface to the light that passes through it leaves through one of
    /// those, and the shadows are the same for half the raster work, a block
    /// behind every lit face (so lit faces cannot shadow themselves).
    pub(crate) solid_back_faces: bool,
}

/// The shadow program a terrain layer draws with: its own, else `shadow`.
pub(crate) fn shadow_chain(geometry: Geometry) -> [&'static str; 2] {
    match geometry {
        Geometry::TerrainSolid => ["shadow_solid", "shadow"],
        Geometry::TerrainCutout => ["shadow_cutout", "shadow"],
        Geometry::Water => ["shadow_water", "shadow"],
    }
}

impl ShadowCasters {
    pub(super) fn read(properties: &std::collections::HashMap<String, String>) -> ShadowCasters {
        ShadowCasters {
            entities: true_key(properties, "shadowEntities", true),
            player: true_key(properties, "shadowPlayer", false),
            block_entities: true_key(properties, "shadowBlockEntities", true),
            terrain: true_key(properties, "shadowTerrain", true),
            translucent: true_key(properties, "shadowTranslucent", true),
            solid_back_faces: true_key(properties, "shadowSolidBackFaces", false),
        }
    }

    pub(crate) fn any(self) -> bool {
        self.entities || self.player || self.block_entities
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SkyParts {
    pub(crate) sky: bool,
    pub(crate) sun: bool,
    pub(crate) moon: bool,
    pub(crate) stars: bool,
}

impl SkyParts {
    pub(super) fn read(properties: &std::collections::HashMap<String, String>) -> SkyParts {
        SkyParts {
            sky: false_key(properties, "sky"),
            sun: false_key(properties, "sun"),
            moon: false_key(properties, "moon"),
            stars: false_key(properties, "stars"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flip_lines_force_or_hold_a_swap() {
        let props: std::collections::HashMap<String, String> = [
            ("flip.composite2.colortex3", "true"),
            ("flip.composite2.colortex5", "false"),
            ("flip.composite2.gaux1", "maybe"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        let flips = FlipOverrides::read(&props);
        let mut toggles = flips.toggles("composite2", &[5, 7]);
        toggles.sort();
        assert_eq!(toggles, vec![3, 5], "3 swaps unwritten, 5 holds though written");
        assert!(flips.toggles("composite2", &[3]).is_empty());
        assert!(flips.toggles("composite1", &[3]).is_empty());
    }

    #[test]
    fn blend_lines_set_programs_and_single_buffers() {
        use bevy::render::render_resource::{BlendFactor, BlendState};
        let props: std::collections::HashMap<String, String> = [
            ("blend.gbuffers_water", "off"),
            ("blend.gbuffers_clouds", "SRC_ALPHA ONE_MINUS_SRC_ALPHA ONE ONE_MINUS_SRC_ALPHA"),
            ("blend.gbuffers_clouds.colortex3", "off"),
            ("blend.gbuffers_hand.gaux1", "ONE ONE ONE ONE"),
            ("blend.composite", "NOT A MODE"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        let blends = Blends::read(&props);
        let default = Some(BlendState::ALPHA_BLENDING);
        assert_eq!(blends.resolve("gbuffers_water", 0, default), None);
        let clouds = blends.resolve("gbuffers_clouds", 0, None).expect("blends");
        assert_eq!(clouds.color.dst_factor, BlendFactor::OneMinusSrcAlpha);
        assert_eq!(blends.resolve("gbuffers_clouds", 3, default), None);
        let hand = blends.resolve("gbuffers_hand", 4, None).expect("gaux1 is colortex4");
        assert_eq!(hand.color.src_factor, BlendFactor::One);
        assert_eq!(blends.resolve("gbuffers_hand", 0, default), default);
        assert_eq!(blends.resolve("composite", 0, default), default);
    }

    #[test]
    fn kinds_fall_back_as_iris_does() {
        let solas = [
            "gbuffers_basic",
            "gbuffers_textured",
            "gbuffers_hand",
            "gbuffers_entities",
            "gbuffers_block",
        ];
        let resolved = |kind: EntityKind, names: &[&str]| kind.resolve(|n| names.contains(&n));
        assert_eq!(
            resolved(EntityKind::SkyBasic, &solas),
            Some("gbuffers_basic")
        );
        assert_eq!(
            resolved(EntityKind::SkyTextured, &solas),
            Some("gbuffers_textured")
        );
        assert_eq!(
            resolved(EntityKind::Clouds, &solas),
            Some("gbuffers_textured")
        );
        assert_eq!(resolved(EntityKind::Hand, &solas), Some("gbuffers_hand"));
        assert_eq!(
            resolved(EntityKind::HandWater, &solas),
            Some("gbuffers_hand")
        );
        let minimal = ["gbuffers_terrain"];
        for kind in [
            EntityKind::SkyBasic,
            EntityKind::SkyTextured,
            EntityKind::Clouds,
            EntityKind::Hand,
            EntityKind::HandWater,
        ] {
            assert_eq!(resolved(kind, &minimal), None, "{kind:?}");
        }
        assert_eq!(EntityKind::Block.fallback(), Some(EntityKind::Entity));
        assert_eq!(EntityKind::Hand.fallback(), None);
        assert_eq!(
            resolved(EntityKind::ParticleTranslucent, &solas),
            Some("gbuffers_textured")
        );
        assert_eq!(
            resolved(EntityKind::EntityTranslucent, &solas),
            Some("gbuffers_entities")
        );
        assert_eq!(
            EntityKind::ALL.map(EntityKind::index),
            std::array::from_fn(|i| i)
        );
    }

    #[test]
    fn shadow_caster_keys_follow_iris() {
        let none = ShadowCasters::read(&std::collections::HashMap::new());
        assert_eq!(
            none,
            ShadowCasters {
                entities: true,
                player: false,
                block_entities: true,
                terrain: true,
                translucent: true,
                solid_back_faces: false,
            }
        );
        let solas = std::collections::HashMap::from([
            ("shadowEntities".to_string(), "false".to_string()),
            ("shadowBlockEntities".to_string(), "false".to_string()),
            ("shadowPlayer".to_string(), "true".to_string()),
            ("shadowTranslucent".to_string(), "false".to_string()),
        ]);
        let casters = ShadowCasters::read(&solas);
        assert_eq!(
            casters,
            ShadowCasters {
                entities: false,
                player: true,
                block_entities: false,
                terrain: true,
                translucent: false,
                solid_back_faces: false,
            }
        );
        assert!(casters.any());
        assert_eq!(EntityKind::ShadowCaster.segment(), Segment::Shadow);
        assert!(
            Segment::Shadow < Segment::Sky,
            "drawn first, in the shadow pass"
        );
    }

    #[test]
    fn shadow_culling_keys_follow_iris() {
        assert_eq!(
            ShadowCulling::parse(Some("reversed")),
            ShadowCulling::SafeZone
        );
        assert_eq!(
            ShadowCulling::parse(Some("safe_zone")),
            ShadowCulling::SafeZone
        );
        assert_eq!(ShadowCulling::parse(Some("false")), ShadowCulling::Distance);
        assert_eq!(ShadowCulling::parse(Some("true")), ShadowCulling::Advanced);
        assert_eq!(ShadowCulling::parse(None), ShadowCulling::Default);
        use crate::renderer::terrain_pool::cull::ShadowCullMode;
        assert_eq!(ShadowCulling::Default.mode(true), ShadowCullMode::Distance);
        assert_eq!(ShadowCulling::Default.mode(false), ShadowCullMode::Advanced);
        assert_eq!(ShadowCulling::SafeZone.mode(true), ShadowCullMode::SafeZone);
    }

    #[test]
    fn cloud_and_sky_keys_are_read() {
        let clouds = |value: &str| {
            clouds_off(&std::collections::HashMap::from([(
                "clouds".to_string(),
                value.to_string(),
            )]))
        };
        assert!(clouds(" off "));
        assert!(!clouds("fancy") && !clouds("sometimes"));
        assert!(!clouds_off(&std::collections::HashMap::new()));
        let props = std::collections::HashMap::from([("stars".to_string(), "false".to_string())]);
        let parts = SkyParts::read(&props);
        assert!(parts.sky && parts.sun && parts.moon && !parts.stars);
    }

    #[test]
    fn alpha_test_keys_are_read() {
        let props = std::collections::HashMap::from([
            (
                "alphaTest.gbuffers_water".to_string(),
                "GREATER 0.02".to_string(),
            ),
            ("alphaTest.gbuffers_terrain".to_string(), "off".to_string()),
            (
                "alphaTest.gbuffers_hand".to_string(),
                "LESS 0.5".to_string(),
            ),
            ("alphaTest.gbuffers_block".to_string(), "false".to_string()),
            ("alphaTest.gbuffers_basic".to_string(), "ALWAYS".to_string()),
            (
                "alphaTest.gbuffers_textured".to_string(),
                "GL_ALWAYS 0.1".to_string(),
            ),
            ("clouds".to_string(), "true".to_string()),
        ]);
        let tests = alpha_tests(&props);
        assert_eq!(
            tests.get("gbuffers_water"),
            Some(&Some(AlphaTest::greater(0.02)))
        );
        assert_eq!(tests.get("gbuffers_terrain"), Some(&None));
        assert_eq!(
            tests.get("gbuffers_block"),
            Some(&None),
            "Iris reads false as off"
        );
        assert_eq!(
            tests.get("gbuffers_hand"),
            Some(&Some(AlphaTest {
                function: AlphaFunction::Less,
                reference: 0.5
            }))
        );
        assert_eq!(
            tests.get("gbuffers_basic"),
            None,
            "a bare function is refused, as Iris refuses it"
        );
        assert_eq!(tests.get("gbuffers_textured"), Some(&None));
        assert_eq!(tests.len(), 5);
    }
}
