//! `CompoundTag`/`ListTag` reads over a view's NBT (JSON: compounds are
//! objects, lists and arrays are arrays, numbers are numbers), with Java's
//! defaults: a missing or mistyped key reads 0, `false`, `""` or empty.
//! `nbt.getFloat("Speed")` is `view.nbt.get_float("Speed")`.

use serde_json::Value;

use super::vec::{AABB, BlockPos, Vec3};

static EMPTY: Value = Value::Null;

fn num(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::Bool(b)) => *b as i32 as f64,
        _ => 0.0,
    }
}

/// Java's `NumericTag.getAsInt` etc. on a number of any tag type: floats
/// floor, as `FloatTag.getAsInt` does.
fn as_long(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n
            .as_i64()
            .unwrap_or_else(|| n.as_f64().map_or(0, |f| f.floor() as i64)),
        Some(Value::Bool(b)) => *b as i64,
        _ => 0,
    }
}

/// `CompoundTag`'s getters, on a compound's JSON.
pub trait CompoundTag {
    fn tag(&self, key: &str) -> Option<&Value>;

    /// `contains(key)`.
    fn contains(&self, key: &str) -> bool {
        self.tag(key).is_some()
    }

    fn get_int(&self, key: &str) -> i32 {
        as_long(self.tag(key)) as i32
    }

    fn get_long(&self, key: &str) -> i64 {
        as_long(self.tag(key))
    }

    fn get_short(&self, key: &str) -> i16 {
        as_long(self.tag(key)) as i16
    }

    fn get_byte(&self, key: &str) -> i8 {
        as_long(self.tag(key)) as i8
    }

    fn get_float(&self, key: &str) -> f32 {
        num(self.tag(key)) as f32
    }

    fn get_double(&self, key: &str) -> f64 {
        num(self.tag(key))
    }

    /// `getBoolean`: a byte other than 0.
    fn get_boolean(&self, key: &str) -> bool {
        num(self.tag(key)) != 0.0
    }

    fn get_string(&self, key: &str) -> &str {
        self.tag(key).and_then(Value::as_str).unwrap_or("")
    }

    /// `getCompound`: null (which reads as empty) when missing.
    fn get_compound(&self, key: &str) -> &Value {
        match self.tag(key) {
            Some(v @ Value::Object(_)) => v,
            _ => &EMPTY,
        }
    }

    /// `getList(key, type)`: the elements, empty when missing.
    fn get_list(&self, key: &str) -> &[Value] {
        self.tag(key)
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice)
    }

    fn get_int_array(&self, key: &str) -> Vec<i32> {
        self.get_list(key)
            .iter()
            .map(|v| as_long(Some(v)) as i32)
            .collect()
    }

    fn get_long_array(&self, key: &str) -> Vec<i64> {
        self.get_list(key)
            .iter()
            .map(|v| as_long(Some(v)))
            .collect()
    }

    /// `getUUID(key)`: four ints, most significant first.
    fn get_uuid(&self, key: &str) -> Option<u128> {
        NbtUtils::load_uuid(self.tag(key)?)
    }

    /// `hasUUID(key)`.
    fn has_uuid(&self, key: &str) -> bool {
        self.get_uuid(key).is_some()
    }

    /// `getAllKeys()`.
    fn get_all_keys(&self) -> Vec<&str>;

    /// `isEmpty()`.
    fn is_empty_tag(&self) -> bool {
        self.get_all_keys().is_empty()
    }
}

impl CompoundTag for Value {
    fn tag(&self, key: &str) -> Option<&Value> {
        self.as_object()?.get(key)
    }

    fn get_all_keys(&self) -> Vec<&str> {
        self.as_object()
            .map(|o| o.keys().map(String::as_str).collect())
            .unwrap_or_default()
    }
}

/// `ListTag`'s getters by index, with Java's defaults past the end.
pub trait ListTag {
    fn at(&self, i: usize) -> Option<&Value>;

    fn get_int(&self, i: usize) -> i32 {
        as_long(self.at(i)) as i32
    }

    fn get_float(&self, i: usize) -> f32 {
        num(self.at(i)) as f32
    }

    fn get_double(&self, i: usize) -> f64 {
        num(self.at(i))
    }

    fn get_string(&self, i: usize) -> &str {
        self.at(i).and_then(Value::as_str).unwrap_or("")
    }

    fn get_compound(&self, i: usize) -> &Value {
        match self.at(i) {
            Some(v @ Value::Object(_)) => v,
            _ => &EMPTY,
        }
    }
}

impl ListTag for [Value] {
    fn at(&self, i: usize) -> Option<&Value> {
        self.get(i)
    }
}

/// Vanilla's `NbtUtils`.
pub struct NbtUtils;

impl NbtUtils {
    /// `readBlockPos(tag, key)`: an int array of three.
    pub fn read_block_pos(nbt: &Value, key: &str) -> Option<BlockPos> {
        match nbt.get_int_array(key)[..] {
            [x, y, z] => Some(BlockPos::new(x, y, z)),
            _ => None,
        }
    }

    /// `loadUUID(tag)`.
    pub fn load_uuid(tag: &Value) -> Option<u128> {
        let a = tag.as_array()?;
        if a.len() != 4 {
            return None;
        }
        Some(a.iter().fold(0u128, |acc, v| {
            (acc << 32) | (as_long(Some(v)) as u32 as u128)
        }))
    }
}

/// Catnip's `NBTHelper`.
pub struct NBTHelper;

impl NBTHelper {
    /// `readBlockPos(nbt, key)`: the int array, or the old `{X, Y, Z}`
    /// compound (zeros when neither).
    pub fn read_block_pos(nbt: &Value, key: &str) -> BlockPos {
        NbtUtils::read_block_pos(nbt, key).unwrap_or_else(|| {
            let old = nbt.get_compound(key);
            BlockPos::new(old.get_int("X"), old.get_int("Y"), old.get_int("Z"))
        })
    }

    /// `readEnum(nbt, key, Enum.class)`: the index of the constant named
    /// by the string at `key` in `names` (the enum's `name()`s in
    /// declaration order), 0 when missing or unknown.
    pub fn read_enum(nbt: &Value, key: &str, names: &[&str]) -> usize {
        let s = nbt.get_string(key);
        names.iter().position(|n| *n == s).unwrap_or(0)
    }

    /// `readAABB(list)`: six floats; `None` for an empty list.
    pub fn read_aabb(list: &[Value]) -> Option<AABB> {
        if list.is_empty() {
            return None;
        }
        let f = |i| list.get_float(i) as f64;
        Some(AABB::new(f(0), f(1), f(2), f(3), f(4), f(5)))
    }

    /// `readVec3i(list)`.
    pub fn read_vec3i(list: &[Value]) -> BlockPos {
        BlockPos::new(list.get_int(0), list.get_int(1), list.get_int(2))
    }

    /// `intFromCompound(tag)`: its `V`.
    pub fn int_from_compound(nbt: &Value) -> i32 {
        nbt.get_int("V")
    }

    /// `readCompoundList(list, f)`.
    pub fn read_compound_list<T>(list: &[Value], f: impl FnMut(&Value) -> T) -> Vec<T> {
        list.iter().map(f).collect()
    }

    /// `readItemList(list, ...)`.
    pub fn read_item_list(list: &[Value]) -> Vec<ItemStack> {
        list.iter().map(ItemStack::parse_optional).collect()
    }
}

/// An `ItemStack` as NBT holds it (`{id, count, components}`, or the old
/// `{id, Count}`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemStack {
    pub id: String,
    pub count: i32,
    pub components: Value,
}

impl ItemStack {
    pub const EMPTY: ItemStack = ItemStack {
        id: String::new(),
        count: 0,
        components: Value::Null,
    };

    pub fn new(id: impl Into<String>, count: i32) -> ItemStack {
        ItemStack {
            id: id.into(),
            count,
            components: Value::Null,
        }
    }

    /// `ItemStack.parseOptional(registries, tag)`: empty when unreadable.
    pub fn parse_optional(nbt: &Value) -> ItemStack {
        let id = nbt.get_string("id");
        if id.is_empty() {
            return ItemStack::EMPTY;
        }
        let count = if nbt.contains("count") {
            nbt.get_int("count")
        } else if nbt.contains("Count") {
            nbt.get_int("Count")
        } else {
            1
        };
        ItemStack {
            id: id.to_string(),
            count,
            components: nbt.tag("components").cloned().unwrap_or(Value::Null),
        }
    }

    /// `isEmpty()`: air, or a count under 1.
    pub fn is_empty(&self) -> bool {
        self.id.is_empty() || self.id == "minecraft:air" || self.id == "air" || self.count <= 0
    }

    pub fn get_count(&self) -> i32 {
        self.count
    }

    /// The item's id (`getItem()` compared by id).
    pub fn item(&self) -> &str {
        &self.id
    }
}

/// Catnip's `VecHelper.readNBT(list)`: three doubles, zero when empty.
pub fn read_vec3(list: &[Value]) -> Vec3 {
    if list.is_empty() {
        Vec3::ZERO
    } else {
        Vec3::new(list.get_double(0), list.get_double(1), list.get_double(2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_use_javas_defaults() {
        let n = json!({"Speed": 32.5, "Count": 3, "Powered": 1, "Name": "x",
            "Source": [1, 2, 3], "Old": {"X": 4, "Y": 5, "Z": 6},
            "Items": [{"id": "minecraft:stone", "count": 2}], "Mode": "PULL"});
        assert_eq!(n.get_float("Speed"), 32.5);
        assert_eq!(n.get_int("Speed"), 32);
        assert_eq!(n.get_int("Missing"), 0);
        assert!(n.get_boolean("Powered"));
        assert_eq!(n.get_string("Missing"), "");
        assert!(n.get_compound("Missing").get_all_keys().is_empty());
        assert_eq!(n.get_compound("Missing").get_int("X"), 0);
        assert_eq!(
            NbtUtils::read_block_pos(&n, "Source"),
            Some(BlockPos::new(1, 2, 3))
        );
        assert_eq!(NBTHelper::read_block_pos(&n, "Old"), BlockPos::new(4, 5, 6));
        assert_eq!(NBTHelper::read_enum(&n, "Mode", &["PUSH", "PULL"]), 1);
        let items = NBTHelper::read_item_list(n.get_list("Items"));
        assert_eq!(items[0], ItemStack::new("minecraft:stone", 2));
        assert_eq!(n.get_list("Source").get_int(5), 0);
    }

    #[test]
    fn uuids_pack_most_significant_first() {
        let n = json!({"Owner": [0, 0, 0, -1]});
        assert_eq!(n.get_uuid("Owner"), Some(0xFFFF_FFFF));
    }
}
