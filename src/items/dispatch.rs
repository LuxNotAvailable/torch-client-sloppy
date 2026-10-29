//! The runtime half of item model definitions: `select`, `condition` and
//! `range_dispatch` nodes whose choice depends on the stack or the world
//! (a compass's needle, a clock's face, a bow's pull, a loaded crossbow,
//! custom model data, a trim, a broken elytra, damage, a cooldown, a
//! bundle's fill).
//!
//! Each item's tree is walked the same way at bake time and at draw time; a
//! *path* is the list of choices taken at those nodes, in walk order. Every
//! path an item can take is baked once as its own icon, keyed
//! `item@path`, and drawing picks the path the stack takes now.

use std::collections::HashMap;
use std::sync::Mutex;

use super::json::Json;
use super::model::strip_namespace;

/// What a stack carries that its model can depend on, beyond the plain
/// `SlotStack` fields.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelData {
    pub cmd_floats: Vec<f32>,
    pub cmd_flags: Vec<bool>,
    pub cmd_strings: Vec<String>,
    /// The trim material's key, `minecraft:quartz`.
    pub trim_material: Option<String>,
    /// A loaded crossbow's charge: `arrow` or `rocket`.
    pub charge: Option<&'static str>,
    /// `BundleItem.getFullnessDisplay`, 0 to 1.
    pub bundle_fullness: f32,
    /// A lodestone compass's target: block and dimension.
    pub lodestone: Option<([i32; 3], String)>,
    /// The `item_model` component, when it names another model definition.
    pub item_model: Option<String>,
    /// Quick Charge's level, for a crossbow's pull.
    pub quick_charge: u8,
    /// Components the stack has, by name without the namespace, for
    /// `has_component`.
    pub components: Vec<String>,
}

/// The stack facts a property reads.
#[derive(Clone, Copy)]
pub struct StackView<'a> {
    pub item: &'a str,
    pub count: u8,
    pub damage: u16,
    pub max_damage: u16,
    /// Cooldown left, in percent.
    pub cooldown: u8,
    pub charged: bool,
    pub data: Option<&'a ModelData>,
}

/// The holder and the world: the local player, as the GUI and the hand
/// draw for.
#[derive(Clone, Debug, Default)]
pub struct WorldContext {
    pub in_world: bool,
    pub game_time: i64,
    pub day_ticks: f64,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub dimension: String,
    pub spawn: Option<([i32; 3], String)>,
    pub death: Option<([i32; 3], String)>,
    /// The item in use and how many ticks it has been used.
    pub using: Option<(String, i32)>,
    pub left_handed: bool,
    /// The holder's fishing bobber is out.
    pub casting: bool,
}

/// Shared, so each lookup takes a count rather than copying the strings.
static CONTEXT: Mutex<Option<std::sync::Arc<WorldContext>>> = Mutex::new(None);

pub fn set_context(context: WorldContext) {
    *CONTEXT.lock().unwrap() = Some(std::sync::Arc::new(context));
}

pub fn context() -> std::sync::Arc<WorldContext> {
    CONTEXT.lock().unwrap().clone().unwrap_or_default()
}

static SPAWN: Mutex<Option<([i32; 3], String)>> = Mutex::new(None);
static DEATH: Mutex<Option<([i32; 3], String)>> = Mutex::new(None);

/// A new session: spawn, death and needle state start over.
pub fn reset() {
    *SPAWN.lock().unwrap() = None;
    *DEATH.lock().unwrap() = None;
    *WOBBLERS.lock().unwrap() = None;
}

/// The world's spawn (`ClientboundSetDefaultSpawnPosition`).
pub fn set_spawn(at: Option<([i32; 3], String)>) {
    *SPAWN.lock().unwrap() = at;
}

/// The player's last death (login and respawn).
pub fn set_death(at: Option<([i32; 3], String)>) {
    *DEATH.lock().unwrap() = at;
}

pub fn spawn() -> Option<([i32; 3], String)> {
    SPAWN.lock().unwrap().clone()
}

pub fn death() -> Option<([i32; 3], String)> {
    DEATH.lock().unwrap().clone()
}

fn property(node: &Json) -> &str {
    node.get("property")
        .and_then(Json::as_str)
        .map(strip_namespace)
        .unwrap_or("")
}

/// Whether the choice at `node` is made at draw time. A select on the
/// display context is settled by where the item is drawn instead.
pub fn is_runtime(node: &Json) -> bool {
    let kind = node.get("type").and_then(Json::as_str).map(strip_namespace);
    match kind {
        Some("select") => property(node) != "display_context",
        Some("condition") | Some("range_dispatch") => true,
        _ => false,
    }
}

/// How many ways `node` can go. Selects and range dispatches have one more
/// than their cases: the fallback (or nothing).
pub fn arity(node: &Json) -> usize {
    let kind = node.get("type").and_then(Json::as_str).map(strip_namespace);
    match kind {
        Some("select") => node.get("cases").map_or(0, |c| c.arr().len()) + 1,
        Some("range_dispatch") => node.get("entries").map_or(0, |e| e.arr().len()) + 1,
        _ => 2,
    }
}

/// The branch a choice leads to. Conditions go 0 to `on_false`, 1 to
/// `on_true`.
pub fn branch(node: &Json, choice: usize) -> Option<&Json> {
    let kind = node.get("type").and_then(Json::as_str).map(strip_namespace);
    match kind {
        Some("select") => {
            let cases = node.get("cases").map(Json::arr).unwrap_or_default();
            match cases.get(choice) {
                Some(case) => case.get("model"),
                None => node.get("fallback"),
            }
        }
        Some("range_dispatch") => {
            let entries = node.get("entries").map(Json::arr).unwrap_or_default();
            match entries.get(choice) {
                Some(entry) => entry.get("model"),
                None => node.get("fallback"),
            }
        }
        _ => node.get(if choice == 1 { "on_true" } else { "on_false" }),
    }
}

/// `NeedleDirectionHelper.Wobbler`: the needle eases toward its target once
/// a game tick.
#[derive(Clone, Copy, Default)]
struct Wobbler {
    rotation: f32,
    delta: f32,
    last_tick: i64,
}

impl Wobbler {
    fn update(&mut self, tick: i64, target: f32, factor: f32) -> f32 {
        if self.last_tick != tick {
            self.last_tick = tick;
            let step = (target - self.rotation + 0.5).rem_euclid(1.0) - 0.5;
            self.delta += step * 0.1;
            self.delta *= factor;
            self.rotation = (self.rotation + self.delta).rem_euclid(1.0);
        }
        self.rotation
    }
}

static WOBBLERS: Mutex<Option<HashMap<String, Wobbler>>> = Mutex::new(None);

fn wobble(key: &str, tick: i64, target: f32, factor: f32) -> f32 {
    let mut guard = WOBBLERS.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    let w = map.entry(key.to_owned()).or_insert(Wobbler {
        rotation: target,
        delta: 0.0,
        last_tick: tick,
    });
    w.update(tick, target, factor)
}

/// `DimensionType.timeOfDay`: the sun's angle as a fraction of a turn.
fn time_of_day(day_ticks: f64) -> f32 {
    let frac = ((day_ticks / 24000.0) - 0.25).rem_euclid(1.0);
    let eased = 0.5 - (frac * std::f64::consts::PI).cos() / 2.0;
    ((frac * 2.0 + eased) / 3.0) as f32
}

fn compass(node: &Json, stack: &StackView, ctx: &WorldContext) -> f32 {
    if !ctx.in_world {
        return 0.0;
    }
    let target = node.get("target").and_then(Json::as_str).unwrap_or("spawn");
    let pos = match target {
        "spawn" => ctx.spawn.clone(),
        "recovery" => ctx.death.clone(),
        "lodestone" => stack.data.and_then(|d| d.lodestone.clone()),
        _ => None,
    };
    let wobbles = node.get("wobble").and_then(Json::as_bool).unwrap_or(true);
    let factor = if wobbles { 0.8 } else { 0.0 };
    let valid = pos.as_ref().filter(|(at, dimension)| {
        let (dx, dz) = (
            at[0] as f64 + 0.5 - ctx.pos[0],
            at[2] as f64 + 0.5 - ctx.pos[2],
        );
        *dimension == ctx.dimension && dx * dx + dz * dz >= 1.0e-5
    });
    let Some((at, _)) = valid else {
        // `getRandomlySpinningRotation`: a new random heading each tick.
        let spin = crate::util::javarandom::JavaRandom::new(ctx.game_time).next_f32();
        let target = if wobbles {
            wobble(
                &format!("compass:none:{}", stack.item),
                ctx.game_time,
                spin,
                factor,
            )
        } else {
            spin
        };
        return target.rem_euclid(1.0);
    };
    let angle = ((at[2] as f64 + 0.5 - ctx.pos[2]).atan2(at[0] as f64 + 0.5 - ctx.pos[0])
        / std::f64::consts::TAU) as f32;
    let yaw = (ctx.yaw / 360.0).rem_euclid(1.0);
    let facing = 0.5 - (yaw - 0.25);
    let facing = if wobbles {
        wobble(
            &format!("compass:{target}:{}", stack.item),
            ctx.game_time,
            facing,
            factor,
        )
    } else {
        facing
    };
    (angle + facing).rem_euclid(1.0)
}

fn time(node: &Json, ctx: &WorldContext) -> f32 {
    if !ctx.in_world {
        return 0.0;
    }
    let source = node
        .get("source")
        .and_then(Json::as_str)
        .unwrap_or("daytime");
    let target = match source {
        "moon_phase" => ((ctx.day_ticks / 24000.0).floor().rem_euclid(8.0) / 8.0) as f32,
        "random" => crate::util::javarandom::JavaRandom::new(ctx.game_time).next_f32(),
        _ => time_of_day(ctx.day_ticks),
    };
    if node.get("wobble").and_then(Json::as_bool).unwrap_or(true) {
        wobble(&format!("time:{source}"), ctx.game_time, target, 0.9)
    } else {
        target
    }
}

/// Ticks this stack has been in use, when the holder is using it.
fn used_ticks(stack: &StackView, ctx: &WorldContext) -> Option<i32> {
    let (item, ticks) = ctx.using.as_ref()?;
    (strip_namespace(item) == strip_namespace(stack.item)).then_some(*ticks)
}

/// `range_dispatch`'s value for `node`, before its scale.
fn number(node: &Json, stack: &StackView, ctx: &WorldContext) -> f32 {
    let normalize = node
        .get("normalize")
        .and_then(Json::as_bool)
        .unwrap_or(true);
    match property(node) {
        "compass" => compass(node, stack, ctx),
        "time" => time(node, ctx),
        "use_duration" => used_ticks(stack, ctx).map_or(0.0, |t| t as f32),
        "use_cycle" => {
            let period = node
                .get("period")
                .and_then(Json::as_f32)
                .unwrap_or(1.0)
                .max(1.0e-3);
            used_ticks(stack, ctx).map_or(0.0, |t| (t as f32).rem_euclid(period))
        }
        "crossbow/pull" => {
            if stack.charged {
                return 0.0;
            }
            let quick = stack.data.map_or(0, |d| d.quick_charge) as i32;
            let duration = (25 - 5 * quick).max(0) as f32;
            used_ticks(stack, ctx).map_or(0.0, |t| {
                if duration > 0.0 {
                    t as f32 / duration
                } else {
                    1.0
                }
            })
        }
        "damage" => {
            let max = stack.max_damage as f32;
            let damage = stack.damage as f32;
            if normalize {
                if max > 0.0 {
                    (damage / max).clamp(0.0, 1.0)
                } else {
                    0.0
                }
            } else {
                damage.clamp(0.0, max)
            }
        }
        "count" => {
            let count = stack.count as f32;
            if normalize {
                (count / 64.0).clamp(0.0, 1.0)
            } else {
                count
            }
        }
        "cooldown" => stack.cooldown as f32 / 100.0,
        "bundle/fullness" => stack.data.map_or(0.0, |d| d.bundle_fullness),
        "custom_model_data" => {
            let index = node.get("index").and_then(Json::as_i32).unwrap_or(0).max(0) as usize;
            stack
                .data
                .and_then(|d| d.cmd_floats.get(index).copied())
                .unwrap_or(0.0)
        }
        _ => 0.0,
    }
}

/// `select`'s value for `node`.
fn word(node: &Json, stack: &StackView, ctx: &WorldContext) -> Option<String> {
    match property(node) {
        "trim_material" => stack.data.and_then(|d| d.trim_material.clone()),
        "charge_type" => Some(if stack.charged {
            stack
                .data
                .and_then(|d| d.charge)
                .unwrap_or("arrow")
                .to_owned()
        } else {
            "none".to_owned()
        }),
        "custom_model_data" => {
            let index = node.get("index").and_then(Json::as_i32).unwrap_or(0).max(0) as usize;
            stack.data.and_then(|d| d.cmd_strings.get(index).cloned())
        }
        "main_hand" => Some(if ctx.left_handed { "left" } else { "right" }.to_owned()),
        "context_dimension" => Some(ctx.dimension.clone()),
        "context_entity_type" => Some("minecraft:player".to_owned()),
        _ => None,
    }
}

fn same_id(a: &str, b: &str) -> bool {
    strip_namespace(a) == strip_namespace(b)
}

fn when_matches(case: &Json, value: &str) -> bool {
    match case.get("when") {
        Some(Json::Arr(values)) => values
            .iter()
            .any(|v| v.as_str().is_some_and(|s| same_id(s, value))),
        Some(v) => v.as_str().is_some_and(|s| same_id(s, value)),
        None => false,
    }
}

fn condition(node: &Json, stack: &StackView, ctx: &WorldContext) -> bool {
    match property(node) {
        "using_item" => used_ticks(stack, ctx).is_some(),
        "broken" => stack.max_damage > 0 && stack.damage + 1 >= stack.max_damage,
        "damaged" => stack.max_damage > 0 && stack.damage > 0,
        "fishing_rod/cast" => ctx.casting,
        "custom_model_data" => {
            let index = node.get("index").and_then(Json::as_i32).unwrap_or(0).max(0) as usize;
            stack
                .data
                .and_then(|d| d.cmd_flags.get(index).copied())
                .unwrap_or(false)
        }
        "has_component" => {
            let wanted = node
                .get("component")
                .and_then(Json::as_str)
                .map(strip_namespace)
                .unwrap_or("");
            stack
                .data
                .is_some_and(|d| d.components.iter().any(|c| c == wanted))
        }
        "view_entity" => true,
        _ => false,
    }
}

/// The choice `node` makes for this stack now.
pub fn decide(node: &Json, stack: &StackView, ctx: &WorldContext) -> usize {
    let kind = node.get("type").and_then(Json::as_str).map(strip_namespace);
    match kind {
        Some("select") => {
            let cases = node.get("cases").map(Json::arr).unwrap_or_default();
            let value = word(node, stack, ctx);
            value
                .and_then(|v| cases.iter().position(|case| when_matches(case, &v)))
                .unwrap_or(cases.len())
        }
        Some("range_dispatch") => {
            let entries = node.get("entries").map(Json::arr).unwrap_or_default();
            let scale = node.get("scale").and_then(Json::as_f32).unwrap_or(1.0);
            let value = number(node, stack, ctx) * scale;
            // The entry with the highest threshold at or under the value.
            let mut best: Option<(usize, f32)> = None;
            for (i, entry) in entries.iter().enumerate() {
                let threshold = entry.get("threshold").and_then(Json::as_f32).unwrap_or(0.0);
                if threshold <= value && best.is_none_or(|(_, t)| threshold >= t) {
                    best = Some((i, threshold));
                }
            }
            best.map_or(entries.len(), |(i, _)| i)
        }
        _ => usize::from(condition(node, stack, ctx)),
    }
}

/// A path as a key suffix.
pub fn path_key(item: &str, path: &[u16]) -> String {
    let mut key = String::with_capacity(item.len() + path.len() * 3);
    key.push_str(item);
    key.push('@');
    for (i, step) in path.iter().enumerate() {
        if i > 0 {
            key.push('.');
        }
        key.push_str(&step.to_string());
    }
    key
}

/// Splits `item@1.0.3` into the item and its path.
pub fn parse_path_key(key: &str) -> Option<(&str, Vec<u16>)> {
    let (item, path) = key.split_once('@')?;
    let path = if path.is_empty() {
        Vec::new()
    } else {
        path.split('.')
            .map(|s| s.parse().ok())
            .collect::<Option<Vec<u16>>>()?
    };
    Some((item, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_keys_round_trip() {
        let key = path_key("compass", &[3, 0, 17]);
        assert_eq!(key, "compass@3.0.17");
        assert_eq!(parse_path_key(&key), Some(("compass", vec![3, 0, 17])));
        assert_eq!(parse_path_key("compass"), None);
    }

    #[test]
    fn noon_and_midnight_sit_half_a_turn_apart() {
        let noon = time_of_day(6000.0);
        let midnight = time_of_day(18000.0);
        assert!(
            (noon - 0.0).abs() < 1e-4 || (noon - 1.0).abs() < 1e-4,
            "{noon}"
        );
        assert!((midnight - 0.5).abs() < 1e-4, "{midnight}");
    }

    #[test]
    fn a_range_takes_the_highest_threshold_under_the_value() {
        let node = Json::parse(
            r#"{"type":"range_dispatch","property":"damage","entries":[
                {"threshold":0.5,"model":{}},{"threshold":0.0,"model":{}},{"threshold":0.9,"model":{}}]}"#,
        )
        .unwrap();
        let stack = |damage| StackView {
            item: "bow",
            count: 1,
            damage,
            max_damage: 100,
            cooldown: 0,
            charged: false,
            data: None,
        };
        let ctx = WorldContext::default();
        assert_eq!(decide(&node, &stack(10), &ctx), 1);
        assert_eq!(decide(&node, &stack(60), &ctx), 0);
        assert_eq!(decide(&node, &stack(95), &ctx), 2);
    }
}
