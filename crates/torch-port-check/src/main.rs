//! `torch-port-check`: a port's guardrail. Loads a `plugin.wasm` the way
//! the client does, asks it for a rig at every block state of every block
//! entity type it handles (from the packages' `blocks.json`), with a few
//! NBT shapes, and checks each rig against the assets:
//!
//! - the call returns a rig that parses (no trap, no bad JSON);
//! - every model, block, sprite and item a part names exists (in the
//!   packages or vanilla's assets), and a block's properties are its own;
//! - numbers are finite, scales non-zero, paths in time order;
//! - a scroll's target is declared in `Registration::sprites`.
//!
//! ```text
//! torch-port-check <plugin.wasm> <package.torchmod>... [--vanilla <assets/minecraft | index.txt>]
//!     [--types a:b,c:d] [--states N] [--json <report.json>]
//! ```
//!
//! `--vanilla` takes vanilla's `assets/minecraft` folder, or an index file
//! listing its paths (`models/block/stone.json`, one a line: names only,
//! no Mojang content). Without it, `minecraft:` names aren't checked.
//!
//! Exit 0 when nothing's wrong, 1 on errors, 2 when it can't run.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use torch_mod_api::{Animation, Geometry, Part, Registration, Rig, Transform};
use wasmi::{Caller, Config, Engine, Linker, Memory, Module, Store, TypedFunc};

const FUEL: u64 = 50_000_000;

struct Args {
    wasm: PathBuf,
    packages: Vec<PathBuf>,
    vanilla: Option<PathBuf>,
    types: Option<Vec<String>>,
    states: usize,
    json: Option<PathBuf>,
}

fn args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let mut positional = Vec::new();
    let mut a = Args {
        wasm: PathBuf::new(),
        packages: Vec::new(),
        vanilla: None,
        types: None,
        states: 64,
        json: None,
    };
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--vanilla" => a.vanilla = Some(value()?.into()),
            "--types" => a.types = Some(value()?.split(',').map(|s| s.trim().to_owned()).collect()),
            "--states" => a.states = value()?.parse().map_err(|e| format!("--states: {e}"))?,
            "--json" => a.json = Some(value()?.into()),
            "-h" | "--help" => return Err(USAGE.to_owned()),
            _ => positional.push(PathBuf::from(arg)),
        }
    }
    if positional.len() < 2 {
        return Err(USAGE.to_owned());
    }
    a.wasm = positional.remove(0);
    a.packages = positional;
    Ok(a)
}

const USAGE: &str = "usage: torch-port-check <plugin.wasm> <package.torchmod>... \
[--vanilla <assets/minecraft>] [--types a:b,c:d] [--states N] [--json <report.json>]";

// --- What exists ---------------------------------------------------------

/// A block's property names and values.
type Props = Vec<(String, Vec<String>)>;

#[derive(Default)]
struct Index {
    /// `ns:path` of every model, texture and item definition.
    models: BTreeSet<String>,
    textures: BTreeSet<String>,
    items: BTreeSet<String>,
    /// Block id to its properties (empty for vanilla, which isn't checked
    /// that closely).
    blocks: HashMap<String, Option<Props>>,
    /// Whether vanilla's names are known; when not, `minecraft:` ones pass.
    vanilla: bool,
    /// Block entity type to the blocks that carry it, with their states.
    carriers: BTreeMap<String, Vec<(String, Props, Vec<Vec<usize>>)>>,
}

fn strip_ext<'a>(path: &'a str, ext: &str) -> Option<&'a str> {
    path.strip_suffix(ext)
}

impl Index {
    /// Files under `assets/<ns>/...`, as a path relative to `assets/`.
    fn add_asset(&mut self, rel: &str) {
        let Some((ns, rest)) = rel.split_once('/') else {
            return;
        };
        if let Some(m) = rest
            .strip_prefix("models/")
            .and_then(|r| strip_ext(r, ".json"))
        {
            self.models.insert(format!("{ns}:{m}"));
        } else if let Some(t) = rest
            .strip_prefix("textures/")
            .and_then(|r| strip_ext(r, ".png"))
        {
            self.textures.insert(format!("{ns}:{t}"));
        } else if let Some(i) = rest
            .strip_prefix("items/")
            .and_then(|r| strip_ext(r, ".json"))
        {
            self.items.insert(format!("{ns}:{i}"));
        } else if let Some(b) = rest
            .strip_prefix("blockstates/")
            .and_then(|r| strip_ext(r, ".json"))
        {
            self.blocks.entry(format!("{ns}:{b}")).or_insert(None);
        }
    }

    fn add_package(&mut self, path: &Path) -> Result<(), String> {
        let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", path.display()))?;
        let names: Vec<String> = zip.file_names().map(str::to_owned).collect();
        for name in &names {
            if let Some(rel) = name.strip_prefix("assets/") {
                self.add_asset(rel);
            }
        }
        let read = |zip: &mut zip::ZipArchive<std::fs::File>, name: &str| -> Option<Value> {
            let mut f = zip.by_name(name).ok()?;
            let mut s = String::new();
            f.read_to_string(&mut s).ok()?;
            serde_json::from_str(&s).ok()
        };
        if let Some(items) = read(&mut zip, "items.json") {
            for item in items.as_array().into_iter().flatten() {
                if let Some(n) = item["name"].as_str() {
                    self.items.insert(n.to_owned());
                }
            }
        }
        if let Some(blocks) = read(&mut zip, "blocks.json") {
            for block in blocks["blocks"].as_array().into_iter().flatten() {
                let Some(name) = block["name"].as_str() else {
                    continue;
                };
                let props: Props = block["properties"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| {
                        let name = p[0].as_str()?.to_owned();
                        let values = p[1]
                            .as_array()?
                            .iter()
                            .filter_map(|v| v.as_str().map(str::to_owned))
                            .collect();
                        Some((name, values))
                    })
                    .collect();
                let states: Vec<Vec<usize>> = block["states"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|s| {
                        s.as_array().map(|s| {
                            s.iter()
                                .skip(1)
                                .filter_map(|v| v.as_u64().map(|v| v as usize))
                                .collect()
                        })
                    })
                    .collect();
                self.blocks.insert(name.to_owned(), Some(props.clone()));
                if let Some(be) = block["block_entity"].as_str() {
                    self.carriers.entry(be.to_owned()).or_default().push((
                        name.to_owned(),
                        props,
                        states,
                    ));
                }
            }
        }
        Ok(())
    }

    fn add_vanilla(&mut self, root: &Path) {
        self.vanilla = true;
        if root.is_file() {
            // An index: one path under `assets/minecraft/` a line.
            for line in std::fs::read_to_string(root).unwrap_or_default().lines() {
                let line = line.trim();
                if !line.is_empty() {
                    self.add_asset(&format!("minecraft/{line}"));
                }
            }
            return;
        }
        // `root` is `.../assets/minecraft`.
        fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, base, out);
                } else if let Ok(rel) = p.strip_prefix(base) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let mut files = Vec::new();
        for sub in ["models", "textures", "items", "blockstates"] {
            walk(&root.join(sub), root, &mut files);
        }
        for f in files {
            self.add_asset(&format!("minecraft/{f}"));
        }
    }
}

/// `minecraft:` for an id without a namespace.
fn full(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

impl Index {
    /// Whether `set` has `id`; vanilla ids pass when vanilla isn't known.
    fn has(&self, set: &BTreeSet<String>, id: &str) -> bool {
        let id = full(id);
        set.contains(&id) || (!self.vanilla && id.starts_with("minecraft:"))
    }
}

// --- The plugin ------------------------------------------------------------

struct Plugin {
    store: Store<()>,
    memory: Memory,
    alloc: TypedFunc<i32, i32>,
    free: TypedFunc<(i32, i32), ()>,
    block_entity: TypedFunc<(i32, i32), i64>,
    registration: Registration,
}

impl Plugin {
    fn load(wasm: &[u8]) -> Result<Plugin, String> {
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let module = Module::new(&engine, wasm).map_err(|e| e.to_string())?;
        let mut store = Store::new(&engine, ());
        let mut linker = <Linker<()>>::new(&engine);
        linker
            .func_wrap("torch", "log", |_: Caller<'_, ()>, _ptr: i32, _len: i32| {})
            .map_err(|e| e.to_string())?;
        // Nothing around: connection logic sees an empty world.
        linker
            .func_wrap(
                "torch",
                "block_at",
                |_: Caller<'_, ()>, _x: i32, _y: i32, _z: i32, _out: i32, _cap: i32| -> i32 { 0 },
            )
            .map_err(|e| e.to_string())?;
        store.set_fuel(FUEL).map_err(|e| e.to_string())?;
        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .map_err(|e| e.to_string())?;
        let memory = instance
            .get_memory(&store, "memory")
            .ok_or("no memory export")?;
        let get = |name: &str| format!("missing export {name}");
        let alloc = instance
            .get_typed_func::<i32, i32>(&store, "torch_alloc")
            .map_err(|_| get("torch_alloc"))?;
        let free = instance
            .get_typed_func::<(i32, i32), ()>(&store, "torch_free")
            .map_err(|_| get("torch_free"))?;
        let init = instance
            .get_typed_func::<(), i64>(&store, "torch_init")
            .map_err(|_| get("torch_init"))?;
        let block_entity = instance
            .get_typed_func::<(i32, i32), i64>(&store, "torch_block_entity")
            .map_err(|_| get("torch_block_entity"))?;
        let mut plugin = Plugin {
            store,
            memory,
            alloc,
            free,
            block_entity,
            registration: Registration::default(),
        };
        plugin.store.set_fuel(FUEL).map_err(|e| e.to_string())?;
        let packed = init
            .call(&mut plugin.store, ())
            .map_err(|e| e.to_string())?;
        let bytes = plugin.take(packed)?;
        plugin.registration =
            serde_json::from_slice(&bytes).map_err(|e| format!("registration: {e}"))?;
        Ok(plugin)
    }

    fn take(&mut self, packed: i64) -> Result<Vec<u8>, String> {
        let (ptr, len) = ((packed >> 32) as i32, packed as i32);
        let data = self.memory.data(&self.store);
        let bytes = data
            .get(ptr as usize..(ptr as usize).saturating_add(len as usize))
            .ok_or("result past memory")?
            .to_vec();
        self.store.set_fuel(FUEL).map_err(|e| e.to_string())?;
        self.free
            .call(&mut self.store, (ptr, len))
            .map_err(|e| e.to_string())?;
        Ok(bytes)
    }

    fn rig(&mut self, view: &Value) -> Result<Rig, String> {
        let input = view.to_string().into_bytes();
        self.store.set_fuel(FUEL).map_err(|e| e.to_string())?;
        let ptr = self
            .alloc
            .call(&mut self.store, input.len() as i32)
            .map_err(|e| e.to_string())?;
        self.memory
            .write(&mut self.store, ptr as usize, &input)
            .map_err(|e| e.to_string())?;
        self.store.set_fuel(FUEL).map_err(|e| e.to_string())?;
        let result = self
            .block_entity
            .call(&mut self.store, (ptr, input.len() as i32));
        let _ = self.free.call(&mut self.store, (ptr, input.len() as i32));
        let packed = result.map_err(|e| format!("trapped: {e}"))?;
        let bytes = self.take(packed)?;
        serde_json::from_slice(&bytes).map_err(|e| format!("rig doesn't parse: {e}"))
    }
}

// --- Checks ----------------------------------------------------------------

fn finite(v: &[f32]) -> bool {
    v.iter().all(|x| x.is_finite())
}

fn check_transform(t: &Transform, out: &mut Vec<String>) {
    if !finite(&t.pivot) || !finite(&t.rotate) || !finite(&t.translate) || !finite(&t.scale) {
        out.push("a transform has a non-finite number".into());
    }
    if t.scale.iter().any(|s| *s == 0.0) {
        out.push("a transform scales by 0".into());
    }
}

fn check_part(part: &Part, index: &Index, sprites: &BTreeSet<String>, out: &mut Vec<String>) {
    check_transform(&part.transform, out);
    match &part.geometry {
        Geometry::Model { model, .. } => {
            if !index.has(&index.models, model) {
                out.push(format!("model {model} doesn't exist"));
            }
        }
        Geometry::Block { block, props } => match index.blocks.get(&full(block)) {
            None if !index.vanilla && full(block).starts_with("minecraft:") => {}
            None => out.push(format!("block {block} doesn't exist")),
            Some(Some(known)) => {
                for (k, v) in props {
                    match known.iter().find(|(n, _)| n == k) {
                        None => out.push(format!("block {block} has no property {k}")),
                        Some((_, values)) if !values.contains(v) => {
                            out.push(format!("block {block}: {k}={v} isn't one of {values:?}"))
                        }
                        _ => {}
                    }
                }
            }
            Some(None) => {}
        },
        Geometry::Cuboid {
            from, to, texture, ..
        } => {
            if !index.has(&index.textures, texture) {
                out.push(format!("sprite {texture} doesn't exist"));
            }
            if !finite(from) || !finite(to) {
                out.push("a cuboid has a non-finite corner".into());
            }
        }
        Geometry::Item { item, flat } => {
            if !index.has(&index.items, item) {
                out.push(format!("item {item} doesn't exist"));
            }
            if let Some(flat) = flat {
                check_transform(flat, out);
            }
        }
    }
    match &part.animation {
        Animation::None => {}
        Animation::Spin {
            axis,
            pivot,
            degrees_per_tick,
            phase,
        } => {
            if !finite(axis)
                || !finite(pivot)
                || !degrees_per_tick.is_finite()
                || !phase.is_finite()
            {
                out.push("a spin has a non-finite number".into());
            }
            if axis.iter().all(|a| *a == 0.0) {
                out.push("a spin's axis is zero".into());
            }
        }
        Animation::Swing {
            axis,
            pivot,
            amplitude,
            period_ticks,
            phase,
        } => {
            if !finite(axis)
                || !finite(pivot)
                || !finite(&[*amplitude, *period_ticks, *phase])
                || *period_ticks <= 0.0
            {
                out.push("a swing has a bad number (non-finite, or a period <= 0)".into());
            }
        }
        Animation::Slide {
            offset,
            period_ticks,
            phase,
        } => {
            if !finite(offset) || !finite(&[*period_ticks, *phase]) || *period_ticks <= 0.0 {
                out.push("a slide has a bad number (non-finite, or a period <= 0)".into());
            }
        }
        Animation::Path { points } => {
            if points.iter().any(|p| !finite(p)) {
                out.push("a path has a non-finite point".into());
            }
            if points.windows(2).any(|w| w[1][0] < w[0][0]) {
                out.push("a path's points aren't in time order".into());
            }
        }
    }
    if let Some(scroll) = &part.scroll {
        for s in [&scroll.from, &scroll.to] {
            if !index.has(&index.textures, s) {
                out.push(format!("scroll sprite {s} doesn't exist"));
            }
        }
        if !sprites.contains(&scroll.to) {
            out.push(format!(
                "scroll target {} isn't declared in Registration::sprites",
                scroll.to
            ));
        }
    }
}

/// NBT shapes to try: none, a running kinetic one, a stopped one.
fn nbt_shapes() -> Vec<Value> {
    vec![
        json!({}),
        json!({"Speed": 32.0}),
        json!({"Speed": -64.0}),
        json!({"Speed": 0.0}),
    ]
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    match run(&a) {
        Ok(ok) => std::process::exit(if ok { 0 } else { 1 }),
        Err(e) => {
            eprintln!("torch-port-check: {e}");
            std::process::exit(2);
        }
    }
}

fn run(a: &Args) -> Result<bool, String> {
    let wasm = std::fs::read(&a.wasm).map_err(|e| format!("{}: {e}", a.wasm.display()))?;
    let mut index = Index::default();
    for p in &a.packages {
        index.add_package(p)?;
    }
    if let Some(v) = &a.vanilla {
        index.add_vanilla(v);
    }
    let mut plugin = Plugin::load(&wasm)?;
    let sprites: BTreeSet<String> = plugin.registration.sprites.iter().cloned().collect();
    let mut errors: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut calls = 0usize;

    for s in &plugin.registration.sprites {
        if !index.has(&index.textures, s) {
            errors
                .entry("registration".into())
                .or_default()
                .insert(format!("declared sprite {s} doesn't exist"), 1);
        }
    }
    let types: Vec<String> = match &a.types {
        Some(t) => t.clone(),
        None => plugin.registration.block_entities.clone(),
    };
    for kind in &types {
        if !plugin.registration.block_entities.contains(kind) {
            errors
                .entry(kind.clone())
                .or_default()
                .insert("not registered in Registration::block_entities".into(), 1);
            continue;
        }
        let Some(carriers) = index.carriers.get(kind).cloned() else {
            warnings.push(format!(
                "{kind}: no block in the packages carries this block entity type (a typo?)"
            ));
            continue;
        };
        let mut drew = false;
        for (block, props, states) in carriers {
            // An even sample when a block has many states.
            let step = (states.len() / a.states.max(1)).max(1);
            for state in states.iter().step_by(step) {
                let props_map: BTreeMap<&str, &str> = props
                    .iter()
                    .zip(state)
                    .filter_map(|((name, values), i)| {
                        Some((name.as_str(), values.get(*i)?.as_str()))
                    })
                    .collect();
                for nbt in nbt_shapes() {
                    let view = json!({
                        "type": kind, "pos": [3, 64, -5], "block": block,
                        "props": props_map, "nbt": nbt, "time": 1000, "nbt_time": 990,
                    });
                    calls += 1;
                    let mut problems = Vec::new();
                    match plugin.rig(&view) {
                        Ok(rig) => {
                            drew |= !rig.parts.is_empty();
                            for part in &rig.parts {
                                check_part(part, &index, &sprites, &mut problems);
                            }
                        }
                        Err(e) => {
                            problems.push(e);
                            // A trap can leave the instance broken: start again.
                            plugin = Plugin::load(&wasm)?;
                        }
                    }
                    for p in problems {
                        *errors
                            .entry(kind.clone())
                            .or_default()
                            .entry(format!("{p} ({block})"))
                            .or_insert(0) += 1;
                    }
                }
            }
        }
        if !drew {
            warnings.push(format!("{kind}: every state gave an empty rig"));
        }
    }

    let ok = errors.is_empty();
    for w in &warnings {
        println!("warning: {w}");
    }
    for (kind, problems) in &errors {
        for (p, n) in problems {
            println!("error: {kind}: {p} [{n}x]");
        }
    }
    println!(
        "{} type(s), {calls} call(s): {} error(s), {} warning(s)",
        types.len(),
        errors.values().map(BTreeMap::len).sum::<usize>(),
        warnings.len()
    );
    if let Some(path) = &a.json {
        let report = json!({"ok": ok, "types": types, "calls": calls, "errors": errors, "warnings": warnings});
        std::fs::write(
            path,
            serde_json::to_string_pretty(&report).unwrap_or_default(),
        )
        .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Index {
        let mut i = Index {
            vanilla: true,
            ..Index::default()
        };
        i.add_asset("ex/models/block/gear.json");
        i.add_asset("ex/textures/block/belt.png");
        i.blocks.insert(
            "ex:gear".into(),
            Some(vec![("axis".into(), vec!["x".into(), "y".into()])]),
        );
        i
    }

    fn problems(part: Part) -> Vec<String> {
        let mut out = Vec::new();
        check_part(&part, &index(), &BTreeSet::new(), &mut out);
        out
    }

    #[test]
    fn names_that_exist_pass_and_made_up_ones_fail() {
        assert!(problems(Part::model("ex:block/gear")).is_empty());
        assert_eq!(
            problems(Part::model("ex:block/gear_typo")),
            ["model ex:block/gear_typo doesn't exist"]
        );
        let block = |axis: &str| Part {
            geometry: Geometry::Block {
                block: "ex:gear".into(),
                props: [("axis".to_owned(), axis.to_owned())].into(),
            },
            ..Part::model("x")
        };
        assert!(problems(block("y")).is_empty());
        assert_eq!(problems(block("q")).len(), 1);
        assert_eq!(
            problems(Part::item("ex:nothing")),
            ["item ex:nothing doesn't exist"]
        );
    }

    #[test]
    fn bad_numbers_fail() {
        let mut part = Part::model("ex:block/gear");
        part.transform.scale = [0.0, 1.0, 1.0];
        part.animation = Animation::Spin {
            axis: [0.0; 3],
            pivot: [8.0; 3],
            degrees_per_tick: f32::NAN,
            phase: 0.0,
        };
        assert_eq!(problems(part).len(), 3);
        let mut path = Part::model("ex:block/gear");
        path.animation = Animation::Path {
            points: vec![[10.0, 0.0, 0.0, 0.0], [5.0, 1.0, 0.0, 0.0]],
        };
        assert_eq!(problems(path), ["a path's points aren't in time order"]);
    }
}
