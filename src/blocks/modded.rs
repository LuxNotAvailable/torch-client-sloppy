//! Modded block states from a modpack dump (`tools/dump_mods.py`,
//! `MOD_TRANSLATOR_PLAN.md` step 3). The dump lists every block with its
//! states in the server's order; the server's synced block registry gives
//! the blocks' order. Together they name each modded state id, so it can
//! draw with the mod's own model instead of the stand-in cube.
//!
//! The table changes with the server: [`generation`] counts the changes, so
//! caches indexed by state id can drop their modded rows.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;
use serde_json::Value;

/// One modded state: its block id (`create:shaft`), properties sorted by
/// name, and the behaviour the dump measured.
#[derive(Debug)]
pub struct ModdedState {
    pub name: &'static str,
    pub props: Box<[(&'static str, &'static str)]>,
    pub fact: Arc<Fact>,
    /// The row `blocks::info` answers with.
    pub info: &'static super::info::StateInfo,
    /// Its block's kind for connected textures (`terrain::ctm::kind_of`):
    /// past every vanilla kind, one per block.
    pub kind: u32,
}

/// How vanilla's renderer draws a state (`RenderShape`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Render {
    #[default]
    Model,
    /// Nothing at all.
    Invisible,
    /// By a block entity renderer only; its model, if it has one, is the
    /// closest this client can draw.
    Animated,
}

/// A state's behaviour, shared by every state that behaves the same.
#[derive(Debug, Default, PartialEq)]
pub struct Fact {
    /// Collision boxes, in block units; `None` when the mod's shape needed
    /// a real level.
    pub collision: Option<Arc<[[f32; 6]]>>,
    pub outline: Option<Arc<[[f32; 6]]>>,
    pub light: u8,
    pub occludes: bool,
    pub solid_render: bool,
    pub render: Render,
    pub destroy_time: f32,
    pub needs_tool: bool,
    pub air: bool,
    pub friction: f32,
}

/// A parsed dump: blocks by name, each with its states in order.
pub struct Dump {
    blocks: HashMap<String, Vec<ModdedStateRow>>,
}

struct ModdedStateRow {
    props: Vec<(String, String)>,
    fact: Arc<Fact>,
}

impl Dump {
    pub fn parse(text: &str) -> Result<Dump, String> {
        let root: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if root["format"].as_u64() != Some(1) {
            return Err(format!("dump format {} isn't 1", root["format"]));
        }
        let boxes = |v: &Value| -> Option<Arc<[[f32; 6]]>> {
            v.as_array()?
                .iter()
                .map(|b| {
                    let a = b.as_array()?;
                    let mut out = [0.0; 6];
                    for (slot, v) in out.iter_mut().zip(a) {
                        *slot = v.as_f64()? as f32;
                    }
                    (a.len() == 6).then_some(out)
                })
                .collect()
        };
        let shapes: Vec<Option<Arc<[[f32; 6]]>>> = root["shapes"]
            .as_array()
            .ok_or("no shapes")?
            .iter()
            .map(boxes)
            .collect();
        let shape = |v: &Value| -> Option<Arc<[[f32; 6]]>> {
            shapes.get(usize::try_from(v.as_u64()?).ok()?)?.clone()
        };
        let facts: Vec<Value> = root["facts"].as_array().ok_or("no facts")?.clone();
        let mut out = HashMap::new();
        for block in root["blocks"].as_array().ok_or("no blocks")? {
            let name = block["name"].as_str().ok_or("a block has no name")?;
            let properties: Vec<(&str, Vec<&str>)> = block["properties"]
                .as_array()
                .ok_or_else(|| format!("{name} has no properties list"))?
                .iter()
                .filter_map(|p| {
                    let values = p[1].as_array()?.iter().filter_map(Value::as_str).collect();
                    Some((p[0].as_str()?, values))
                })
                .collect();
            let friction = block["friction"].as_f64().unwrap_or(0.6) as f32;
            let mut states = Vec::new();
            for row in block["states"]
                .as_array()
                .ok_or_else(|| format!("{name} has no states"))?
            {
                let row = row
                    .as_array()
                    .ok_or_else(|| format!("{name} has a bad state"))?;
                let f = row
                    .first()
                    .and_then(Value::as_u64)
                    .and_then(|i| facts.get(i as usize))
                    .ok_or_else(|| format!("{name} has a state with no facts"))?;
                let mut props: Vec<(String, String)> = properties
                    .iter()
                    .zip(&row[1..])
                    .filter_map(|((prop, values), at)| {
                        let value = values.get(usize::try_from(at.as_u64()?).ok()?)?;
                        Some(((*prop).to_owned(), (*value).to_owned()))
                    })
                    .collect();
                if props.len() != properties.len() {
                    return Err(format!("{name} has a state with bad property values"));
                }
                props.sort_unstable();
                let fact = Fact {
                    collision: shape(&f["collision"]),
                    outline: shape(&f["outline"]),
                    light: f["light"].as_u64().unwrap_or(0).min(15) as u8,
                    occludes: f["occludes"].as_bool().unwrap_or(true),
                    solid_render: f["solid_render"].as_bool().unwrap_or(false),
                    render: match f["render"].as_str() {
                        Some("INVISIBLE") => Render::Invisible,
                        Some("ENTITYBLOCK_ANIMATED") => Render::Animated,
                        _ => Render::Model,
                    },
                    destroy_time: f["destroy_time"].as_f64().unwrap_or(1.0) as f32,
                    needs_tool: f["needs_tool"].as_bool().unwrap_or(false),
                    air: f["air"].as_bool().unwrap_or(false),
                    friction,
                };
                states.push(ModdedStateRow {
                    props,
                    fact: Arc::new(fact),
                });
            }
            out.insert(name.to_owned(), states);
        }
        Ok(Dump { blocks: out })
    }
}

/// The first modded block's connected-texture kind: past vanilla's
/// (`BlockKind`, a `u16`).
pub const FIRST_KIND: u32 = 0x1_0000;

/// Where vanilla ends in a server's own numbering: its block count and
/// state count. Modded blocks and their states follow.
#[derive(Clone, Copy, Debug)]
pub struct WireVanilla {
    pub blocks: u32,
    pub states: u32,
}

/// The modded states for this server, by the client's state id.
pub struct Table {
    /// By the server's id, past vanilla's.
    states: HashMap<u32, ModdedState>,
    /// The first server block no package had: its states and every later
    /// block's stay stand-ins.
    pub missing: Option<String>,
    /// A mod's `minecraft:` block this client has natively (a backport's
    /// `pale_oak_log`): the server's id to the vanilla state it draws as.
    aliases: HashMap<u32, u16>,
}

impl Table {
    /// Names each modded state: `wire_blocks` is the server's block
    /// registry (its id to name), `vanilla` where its own blocks start,
    /// `to_native` turns its state ids into this client's (the hops' shift
    /// past vanilla), `first_modded` is this client's first id past
    /// vanilla's states. Each modded block's states start where the one
    /// before it ends, so counting stops at the first block no package has.
    pub fn build(
        dump: &Dump,
        wire_blocks: &HashMap<u32, String>,
        vanilla: WireVanilla,
        to_native: impl Fn(u32) -> Option<u32>,
        first_modded: u32,
    ) -> Result<Table, String> {
        let mut ids: Vec<(&u32, &String)> = wire_blocks.iter().collect();
        ids.sort_unstable();
        if ids
            .iter()
            .enumerate()
            .any(|(i, (id, _))| **id as usize != i)
        {
            return Err("the server's block ids have gaps".into());
        }
        let has_modded = ids.len() as u32 > vanilla.blocks;
        if has_modded && to_native(vanilla.states) != Some(first_modded) {
            return Err(format!(
                "its first modded state lands at {:?}, not {first_modded}: \
                 its vanilla states don't match this client's",
                to_native(vanilla.states)
            ));
        }
        let mut states = HashMap::new();
        let mut aliases = HashMap::new();
        let mut missing = None;
        let mut wire_state = vanilla.states;
        for (block, (_, name)) in ids.into_iter().skip(vanilla.blocks as usize).enumerate() {
            let kind = FIRST_KIND + block as u32;
            let Some(rows) = dump.blocks.get(name.as_str()) else {
                missing = Some(name.clone());
                break;
            };
            let natives = name
                .strip_prefix("minecraft:")
                .and_then(|path| vanilla_states().get(path));
            for (i, row) in rows.iter().enumerate() {
                let Some(native) = to_native(wire_state + i as u32) else {
                    continue;
                };
                if let Some(natives) = natives {
                    aliases.insert(native, closest(natives, &row.props));
                    continue;
                }
                let props: Box<[(&'static str, &'static str)]> = row
                    .props
                    .iter()
                    .map(|(k, v)| (intern(k), intern(v)))
                    .collect();
                let name = intern(name);
                states.insert(
                    native,
                    ModdedState {
                        name,
                        info: info_row(name, &props, &row.fact),
                        props,
                        fact: row.fact.clone(),
                        kind,
                    },
                );
            }
            wire_state += rows.len() as u32;
        }
        Ok(Table {
            states,
            missing,
            aliases,
        })
    }

    /// The state by its id in this client's numbering past vanilla (the
    /// hops' shift), before it's made compact.
    pub fn get(&self, id: u32) -> Option<&ModdedState> {
        self.states.get(&id)
    }

    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// The vanilla state a server's id draws as, for a mod's `minecraft:`
    /// block this client has.
    #[cfg(test)]
    pub fn alias(&self, id: u32) -> Option<u16> {
        self.aliases.get(&id).copied()
    }

    pub fn alias_count(&self) -> usize {
        self.aliases.len()
    }
}

/// This client's vanilla states by block name (no namespace), each with
/// its properties.
fn vanilla_states()
-> &'static HashMap<&'static str, Vec<(u16, Box<[(&'static str, &'static str)]>)>> {
    type Index = HashMap<&'static str, Vec<(u16, Box<[(&'static str, &'static str)]>)>>;
    static INDEX: OnceLock<Index> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut out: Index = HashMap::new();
        for id in 0..=azalea::block::BlockState::MAX_STATE {
            let Ok(state) = azalea::block::BlockState::try_from(id) else {
                continue;
            };
            let info = super::info::info(state);
            out.entry(info.id)
                .or_default()
                .push((id, info.props().into()));
        }
        out
    })
}

/// The state among `natives` sharing the most of `props`; the first on a
/// tie.
fn closest(
    natives: &[(u16, Box<[(&'static str, &'static str)]>)],
    props: &[(String, String)],
) -> u16 {
    let shared = |have: &[(&str, &str)]| {
        props
            .iter()
            .filter(|(k, v)| have.iter().any(|(hk, hv)| hk == k && hv == v))
            .count()
    };
    natives
        .iter()
        .max_by_key(|(id, have)| (shared(have), std::cmp::Reverse(*id)))
        .map_or(0, |(id, _)| *id)
}

/// Names and values live for the program: the same modpack's strings come
/// back on every reconnect, so this grows only with new mods.
fn intern(s: &str) -> &'static str {
    static SEEN: OnceLock<RwLock<std::collections::HashSet<&'static str>>> = OnceLock::new();
    let seen = SEEN.get_or_init(Default::default);
    if let Some(hit) = seen.read().get(s) {
        return hit;
    }
    let mut write = seen.write();
    if let Some(hit) = write.get(s) {
        return hit;
    }
    let leaked: &'static str = Box::leak(s.to_owned().into_boxed_str());
    write.insert(leaked);
    leaked
}

/// The `blocks::info` row for a modded state, made once per distinct
/// state and kept, like [`intern`]'s strings.
fn info_row(
    name: &'static str,
    props: &[(&'static str, &'static str)],
    fact: &Fact,
) -> &'static super::info::StateInfo {
    type Key = (&'static str, Box<[(&'static str, &'static str)]>);
    static ROWS: OnceLock<RwLock<HashMap<Key, &'static super::info::StateInfo>>> = OnceLock::new();
    let rows = ROWS.get_or_init(Default::default);
    let key: Key = (name, props.into());
    if let Some(hit) = rows.read().get(&key) {
        return hit;
    }
    let row: &'static _ = Box::leak(Box::new(super::info::StateInfo::modded(
        name,
        props.into(),
        fact,
    )));
    *rows.write().entry(key).or_insert(row)
}

static TABLE: RwLock<Option<Arc<Table>>> = RwLock::new(None);
static GENERATION: AtomicU32 = AtomicU32::new(0);

/// Bumped whenever the table changes; caches indexed by state id compare it.
pub fn generation() -> u32 {
    GENERATION.load(Ordering::Acquire)
}

/// Runs `f` on modded state `id`, when a dump named it.
pub fn with_state<R>(id: u16, f: impl FnOnce(&ModdedState) -> R) -> Option<R> {
    TABLE.read().as_ref()?.get(to_network(id)).map(f)
}

/// Installs a table built from `text`, for tests elsewhere.
#[cfg(test)]
pub(crate) fn install_for_test(table: Option<Table>) {
    install(table);
}

fn install(table: Option<Table>) {
    let shapes = table.as_ref().map(|t| {
        Arc::new(
            t.states
                .iter()
                .map(|(id, s)| (*id, physics_shape(&s.fact)))
                .collect(),
        )
    });
    if let Ok(mut hook) = azalea_physics::collision::MODDED_SHAPES.write() {
        *hook = shapes;
    }
    *ALIASES.write() = table
        .as_ref()
        .map(|t| t.aliases.clone())
        .unwrap_or_default();
    *TABLE.write() = table.map(Arc::new);
    GENERATION.fetch_add(1, Ordering::AcqRel);
}

/// The table's aliases, where chunk decoding (`to_local`) reads them.
static ALIASES: std::sync::LazyLock<RwLock<HashMap<u32, u16>>> =
    std::sync::LazyLock::new(Default::default);

/// A state's shapes for azalea's physics and picking. A shape the dump
/// couldn't measure stays a full cube.
fn physics_shape(fact: &Fact) -> azalea_physics::collision::ModdedShape {
    let full = [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]];
    let collision = fact.collision.as_deref().unwrap_or(&full);
    let outline = fact.outline.as_deref().unwrap_or(collision);
    azalea_physics::collision::ModdedShape {
        collision: voxel_shape(collision),
        outline: voxel_shape(outline),
        empty: collision.is_empty(),
        full: collision == full,
    }
}

/// Each distinct box list once, kept for the program like [`intern`].
fn voxel_shape(boxes: &[[f32; 6]]) -> &'static azalea_physics::collision::VoxelShape {
    use azalea_physics::collision::{EMPTY_SHAPE, Shapes, VoxelShape, box_shape};
    type Key = Vec<[u32; 6]>;
    static SHAPES: OnceLock<RwLock<HashMap<Key, &'static VoxelShape>>> = OnceLock::new();
    let shapes = SHAPES.get_or_init(Default::default);
    let key: Key = boxes.iter().map(|b| b.map(f32::to_bits)).collect();
    if let Some(hit) = shapes.read().get(&key) {
        return hit;
    }
    let shape = boxes.iter().fold((*EMPTY_SHAPE).clone(), |acc, b| {
        let [x0, y0, z0, x1, y1, z1] = b.map(f64::from);
        Shapes::or(acc, box_shape(x0, y0, z0, x1, y1, z1))
    });
    let leaked: &'static VoxelShape = Box::leak(Box::new(shape));
    *shapes.write().entry(key).or_insert(leaked)
}

/// A new connection: no modded states until its registry sync.
pub fn clear() {
    *IDS.write() = Ids::default();
    install(None);
}

/// A modpack has far more states than a `BlockState`'s `u16` holds past
/// vanilla's, but a loaded world shows few of them. Each modded id a chunk
/// names gets the next compact one, both ways, through azalea's
/// `MODDED_IDS`: palettes and packets keep the server's ids.
#[derive(Default)]
struct Ids {
    local_of: HashMap<u32, u16>,
    /// The server's id by `local - FIRST_LOCAL`.
    network_of: Vec<u32>,
    full: bool,
}

static IDS: std::sync::LazyLock<RwLock<Ids>> = std::sync::LazyLock::new(Default::default);

const FIRST_LOCAL: u16 = azalea::block::BlockState::MAX_STATE + 1;

/// The compact id `u16::MAX` stays a stand-in for states past the room.
fn to_local(network: u32) -> u16 {
    if let Some(&vanilla) = ALIASES.read().get(&network) {
        return vanilla;
    }
    if let Some(&local) = IDS.read().local_of.get(&network) {
        return local;
    }
    let mut ids = IDS.write();
    if let Some(&local) = ids.local_of.get(&network) {
        return local;
    }
    let next = FIRST_LOCAL as usize + ids.network_of.len();
    if next >= u16::MAX as usize {
        if !ids.full {
            ids.full = true;
            crate::log_warn!(
                "mods",
                "more modded states in view than ids; the rest are stand-ins"
            );
        }
        return u16::MAX;
    }
    ids.network_of.push(network);
    ids.local_of.insert(network, next as u16);
    next as u16
}

fn to_network(local: u16) -> u32 {
    local
        .checked_sub(FIRST_LOCAL)
        .and_then(|i| IDS.read().network_of.get(i as usize).copied())
        .unwrap_or(local as u32)
}

/// Gives azalea the compact ids; before any chunk is read.
pub fn install_ids() {
    let _ = azalea::block::block_state::MODDED_IDS.set(azalea::block::block_state::ModdedIds {
        to_local,
        to_network,
    });
}

/// Every loaded package's blocks, merged.
static DUMP: RwLock<Option<Arc<Dump>>> = RwLock::new(None);

/// The packages' `blocks.json` files (`resourcepacks::mods`), as
/// `(package, text)`. A block two packages both have keeps the first.
pub fn load_packages(packages: &[(String, String)]) {
    let mut merged = Dump {
        blocks: HashMap::new(),
    };
    for (package, text) in packages {
        match Dump::parse(text) {
            Ok(dump) => {
                // Two jars can bundle the same library mod; the first wins.
                let mut repeated = 0;
                for (name, rows) in dump.blocks {
                    if merged.blocks.contains_key(&name) {
                        repeated += 1;
                        continue;
                    }
                    merged.blocks.insert(name, rows);
                }
                if repeated > 0 {
                    crate::log_info!(
                        "mods",
                        "{package}: {repeated} block(s) already came from another package"
                    );
                }
            }
            Err(e) => crate::log_warn!("mods", "{package}: its blocks.json doesn't read: {e}"),
        }
    }
    *DUMP.write() = Some(Arc::new(merged));
}

/// The server synced its block registry: name its modded states from the
/// loaded packages.
pub fn on_block_sync(wire_blocks: &HashMap<u32, String>, protocol: i32) {
    let Some(dump) = DUMP.read().clone() else {
        return;
    };
    let first_modded = azalea::block::BlockState::MAX_STATE as u32 + 1;
    let vanilla = crate::protocol::wire_vanilla(protocol);
    let to_native = |id: u32| crate::protocol::native_state_id(protocol, id);
    match Table::build(&dump, wire_blocks, vanilla, to_native, first_modded) {
        Ok(table) => {
            crate::log_info!(
                "mods",
                "packages name {} modded block states; {} more draw as vanilla",
                table.len(),
                table.alias_count()
            );
            if let Some(name) = &table.missing {
                crate::log_warn!(
                    "mods",
                    "no package has {name}: it and the server's later blocks stay stand-ins"
                );
            }
            install(Some(table));
        }
        Err(e) => {
            crate::log_warn!("mods", "the packages don't fit this server: {e}");
            install(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One mod's package: a block with an axis and a flag.
    const DUMP: &str = r#"{"format": 1,
        "shapes": [[], [[0, 0, 0, 1, 1, 1]], [[0.375, 0, 0.375, 0.625, 1, 0.625]]],
        "facts": [{"collision": 2, "outline": null, "light": 3, "occludes": false,
                   "solid_render": false, "render": "MODEL", "destroy_time": 0.8,
                   "needs_tool": false, "air": false}],
        "blocks": [
          {"name": "gizmos:shaft", "properties": [["axis", ["x", "y", "z"]], ["lit", ["true", "false"]]],
           "friction": 0.5,
           "states": [[0, 0, 0], [0, 0, 1], [0, 1, 0], [0, 1, 1], [0, 2, 0], [0, 2, 1]]}
        ]}"#;

    /// Two vanilla blocks with 3 states, then the server's own.
    const VANILLA: WireVanilla = WireVanilla {
        blocks: 2,
        states: 3,
    };

    fn wire(modded: &[&str]) -> HashMap<u32, String> {
        ["minecraft:stone", "minecraft:log"]
            .iter()
            .chain(modded)
            .enumerate()
            .map(|(i, n)| (i as u32, (*n).to_owned()))
            .collect()
    }

    fn shift(first: u32) -> impl Fn(u32) -> Option<u32> {
        move |id| Some(if id < 3 { id } else { id - 3 + first })
    }

    #[test]
    fn modded_states_land_past_vanilla_in_the_servers_order() {
        let dump = Dump::parse(DUMP).unwrap();
        let table =
            Table::build(&dump, &wire(&["gizmos:shaft"]), VANILLA, shift(100), 100).unwrap();
        assert_eq!(table.len(), 6);
        assert_eq!(table.missing, None);
        let s = table.get(103).unwrap();
        assert_eq!(s.name, "gizmos:shaft");
        assert_eq!(&*s.props, &[("axis", "y"), ("lit", "false")]);
        assert_eq!(s.fact.light, 3);
        assert_eq!(s.fact.friction, 0.5);
        assert_eq!(
            s.fact.collision.as_deref(),
            Some(&[[0.375, 0.0, 0.375, 0.625, 1.0, 0.625]][..])
        );
        assert_eq!(s.fact.outline, None);
        assert!(table.get(99).is_none());
    }

    /// Tests that reset or fill the shared ids.
    static IDS_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn a_modded_id_of_any_size_gets_a_compact_one_both_ways() {
        let _lock = IDS_TEST.lock().unwrap_or_else(|e| e.into_inner());
        let big = 400_000;
        let local = to_local(big);
        assert!(local >= FIRST_LOCAL && local < u16::MAX);
        assert_eq!(to_local(big), local);
        assert_eq!(to_network(local), big);
        let other = to_local(big + 1);
        assert_ne!(other, local);
        assert_eq!(to_network(other), big + 1);
        // Vanilla ids aren't touched.
        assert_eq!(to_network(5), 5);
    }

    #[test]
    fn a_dumped_state_answers_info_with_its_own_row() {
        let _lock = IDS_TEST.lock().unwrap_or_else(|e| e.into_inner());
        use azalea::block::BlockState;
        // Bakes read the atlas: build it before anything bakes.
        super::super::tests::atlas_once();
        let dump = Dump::parse(DUMP).unwrap();
        // Far past the ids other tests use for undumped modded states.
        let first = BlockState::MAX_STATE as u32 + 1000;
        let table = Table::build(
            &dump,
            &wire(&["gizmos:shaft"]),
            VANILLA,
            shift(first),
            first,
        );
        install_for_test(Some(table.unwrap()));
        let state = BlockState::from_network(first + 5);
        let info = super::super::info::info(state);
        assert_eq!(info.id, "gizmos:shaft");
        assert_eq!(info.get("axis"), Some("z"));
        assert!(!info.flag("lit"));
        assert_eq!(info.friction, 0.5);
        assert_eq!(info.destroy_time, 0.8);
        // Its shape reaches azalea's physics.
        use azalea_physics::collision::BlockWithShape;
        assert!(!state.is_collision_shape_full());
        assert!(!state.is_collision_shape_empty());
        let aabb = state.base_collision_shape().bounds();
        assert!((aabb.min.x - 0.375).abs() < 1e-6 && (aabb.max.x - 0.625).abs() < 1e-6);
        // No model for it here: the stand-in cube, not a panic.
        let visual = crate::util::block_model::block_visual(state);
        assert!(visual.is_solid);
        let before = generation();
        clear();
        assert!(generation() > before);
        assert!(with_state(state.id(), |_| ()).is_none());
        assert!(state.is_collision_shape_full());
    }

    #[test]
    fn counting_stops_at_a_block_no_package_has() {
        let dump = Dump::parse(DUMP).unwrap();
        let before = wire(&["other:thing", "gizmos:shaft"]);
        let table = Table::build(&dump, &before, VANILLA, shift(100), 100).unwrap();
        assert_eq!(table.len(), 0);
        assert_eq!(table.missing.as_deref(), Some("other:thing"));
        let after = wire(&["gizmos:shaft", "other:thing"]);
        let table = Table::build(&dump, &after, VANILLA, shift(100), 100).unwrap();
        assert_eq!(table.len(), 6);
        assert_eq!(table.missing.as_deref(), Some("other:thing"));
    }

    #[test]
    fn a_backports_minecraft_block_draws_as_the_vanilla_one() {
        let dump = Dump::parse(
            r#"{"format": 1, "shapes": [[]], "facts": [{"collision": 0}],
                "blocks": [{"name": "minecraft:oak_log", "properties": [["axis", ["x", "y", "z"]]],
                            "states": [[0, 0], [0, 1], [0, 2]]}]}"#,
        )
        .unwrap();
        let table = Table::build(
            &dump,
            &wire(&["minecraft:oak_log"]),
            VANILLA,
            shift(100),
            100,
        )
        .unwrap();
        assert_eq!(table.len(), 0);
        let z = table.alias(102).unwrap();
        let info = super::super::info::info(azalea::block::BlockState::try_from(z).unwrap());
        assert_eq!(info.id, "oak_log");
        assert_eq!(info.get("axis"), Some("z"));
    }

    #[test]
    fn packages_from_another_game_version_are_refused() {
        let dump = Dump::parse(DUMP).unwrap();
        // The server's vanilla states counted differently from the hops'.
        let off = |id: u32| Some(if id < 3 { id } else { id - 3 + 101 });
        assert!(Table::build(&dump, &wire(&["gizmos:shaft"]), VANILLA, off, 100).is_err());
    }
}
