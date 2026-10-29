//! How options are stored: each field of an options struct names its key in
//! `options.json` and how its value is kept there, once, beside its type and
//! default. The [`options!`](crate::options) macro builds the struct and its
//! reading and writing from that one list.
//!
//! ```ignore
//! crate::options! {
//!     #[derive(Clone, Debug)]
//!     pub struct Example {
//!         /// A plain value under its key.
//!         pub enabled: bool = true => key("enabled"),
//!         /// Kept within a range.
//!         pub opacity: u32 = 50 => key("opacity").clamp(0, 100),
//!         /// Stored as a fraction of 10 (`toastDisplayTime`: 0.5 for 5).
//!         pub time: u32 = 5 => key("toastDisplayTime").scaled(10.0),
//!         /// Another options struct, its keys beside these.
//!         pub chat: ChatOptions = ChatOptions::default() => group(),
//!         /// Anything else, by hand.
//!         pub parts: u8 = 0x7F => custom(read_parts, write_parts),
//!     }
//! }
//! ```

use serde_json::{Map, Value};

/// A lookup of a stored key.
pub type Lookup<'a> = &'a dyn Fn(&str) -> Option<Value>;

/// A value that can sit under a key in `options.json`.
pub trait OptionValue: Sized {
    fn from_json(value: &Value) -> Option<Self>;
    fn to_json(&self) -> Value;
}

impl OptionValue for bool {
    fn from_json(value: &Value) -> Option<Self> {
        value.as_bool()
    }
    fn to_json(&self) -> Value {
        Value::from(*self)
    }
}

impl OptionValue for u32 {
    fn from_json(value: &Value) -> Option<Self> {
        value.as_u64().map(|v| v.min(u32::MAX as u64) as u32)
    }
    fn to_json(&self) -> Value {
        Value::from(*self)
    }
}

impl OptionValue for u8 {
    fn from_json(value: &Value) -> Option<Self> {
        value.as_u64().map(|v| v.min(u8::MAX as u64) as u8)
    }
    fn to_json(&self) -> Value {
        Value::from(*self)
    }
}

impl OptionValue for i32 {
    fn from_json(value: &Value) -> Option<Self> {
        value.as_i64().map(|v| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
    }
    fn to_json(&self) -> Value {
        Value::from(*self)
    }
}

impl OptionValue for String {
    fn from_json(value: &Value) -> Option<Self> {
        value.as_str().map(str::to_owned)
    }
    fn to_json(&self) -> Value {
        Value::from(self.as_str())
    }
}

/// An optional name: stored as the empty string when unset.
impl OptionValue for Option<String> {
    fn from_json(value: &Value) -> Option<Self> {
        value.as_str().map(|s| (!s.is_empty()).then(|| s.to_owned()))
    }
    fn to_json(&self) -> Value {
        Value::from(self.as_deref().unwrap_or(""))
    }
}

/// An enum stored by its `serialized_name`, read back with
/// `from_serialized_name`.
#[macro_export]
macro_rules! serialized_option {
    ($($ty:ty),* $(,)?) => {$(
        impl $crate::gui::option_codec::OptionValue for $ty {
            fn from_json(value: &serde_json::Value) -> Option<Self> {
                value.as_str().and_then(<$ty>::from_serialized_name)
            }
            fn to_json(&self) -> serde_json::Value {
                serde_json::Value::from(self.serialized_name())
            }
        }
    )*};
}

/// A struct of options, read and written as a whole.
pub trait OptionGroup: Sized {
    fn read_from(value: Lookup) -> Self;
    fn write_to(&self, map: &mut Map<String, Value>);
}

/// How one field is kept.
pub trait Codec<T> {
    fn read(&self, value: Lookup, default: T) -> T;
    fn write(&self, map: &mut Map<String, Value>, value: &T);
}

/// A value under `name`.
pub struct Key(&'static str);

pub fn key(name: &'static str) -> Key {
    Key(name)
}

impl<T: OptionValue> Codec<T> for Key {
    fn read(&self, value: Lookup, default: T) -> T {
        value(self.0).and_then(|v| T::from_json(&v)).unwrap_or(default)
    }
    fn write(&self, map: &mut Map<String, Value>, value: &T) {
        map.insert(self.0.to_owned(), value.to_json());
    }
}

impl Key {
    /// Kept within `lo..=hi` (the default too).
    pub fn clamp<T>(self, lo: T, hi: T) -> Clamped<T> {
        Clamped { key: self, lo, hi }
    }

    /// Stored divided by `factor`: vanilla keeps some settings as
    /// fractions (`mouseSensitivity` 0 to 1 for 0 to 200).
    pub fn scaled(self, factor: f64) -> Scaled {
        Scaled { key: self, factor, range: None }
    }
}

pub struct Clamped<T> {
    key: Key,
    lo: T,
    hi: T,
}

impl<T: OptionValue + PartialOrd + Copy> Codec<T> for Clamped<T> {
    fn read(&self, value: Lookup, default: T) -> T {
        let v = self.key.read(value, default);
        if v < self.lo {
            self.lo
        } else if v > self.hi {
            self.hi
        } else {
            v
        }
    }
    fn write(&self, map: &mut Map<String, Value>, value: &T) {
        self.key.write(map, value);
    }
}

pub struct Scaled {
    key: Key,
    factor: f64,
    range: Option<(u32, u32)>,
}

impl Scaled {
    pub fn clamp(mut self, lo: u32, hi: u32) -> Scaled {
        self.range = Some((lo, hi));
        self
    }
}

impl Codec<u32> for Scaled {
    fn read(&self, value: Lookup, default: u32) -> u32 {
        let v = value(self.key.0)
            .and_then(|v| v.as_f64())
            .map(|v| (v * self.factor).round().max(0.0) as u32)
            .unwrap_or(default);
        match self.range {
            Some((lo, hi)) => v.clamp(lo, hi),
            None => v,
        }
    }
    fn write(&self, map: &mut Map<String, Value>, value: &u32) {
        map.insert(self.key.0.to_owned(), Value::from(*value as f64 / self.factor));
    }
}

/// Another options struct, its keys beside the parent's.
pub struct Group;

pub fn group() -> Group {
    Group
}

impl<T: OptionGroup> Codec<T> for Group {
    fn read(&self, value: Lookup, _default: T) -> T {
        T::read_from(value)
    }
    fn write(&self, map: &mut Map<String, Value>, value: &T) {
        value.write_to(map);
    }
}

/// By hand: a reader and a writer.
pub struct Custom<T> {
    read: fn(Lookup) -> T,
    write: fn(&mut Map<String, Value>, &T),
}

pub fn custom<T>(read: fn(Lookup) -> T, write: fn(&mut Map<String, Value>, &T)) -> Custom<T> {
    Custom { read, write }
}

impl<T> Codec<T> for Custom<T> {
    fn read(&self, value: Lookup, _default: T) -> T {
        (self.read)(value)
    }
    fn write(&self, map: &mut Map<String, Value>, value: &T) {
        (self.write)(map, value);
    }
}

/// An options struct: its fields, each with a type, a default and how it is
/// stored. Builds the struct, `OptionGroup` (reading and writing) and
/// `defaults()` (every field at its default, ignoring what is stored).
#[macro_export]
macro_rules! options {
    (
        $(#[$sm:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$fm:meta])*
                $fvis:vis $f:ident : $t:ty = $default:expr => $codec:expr
            ),* $(,)?
        }
    ) => {
        $(#[$sm])*
        $vis struct $name {
            $( $(#[$fm])* $fvis $f: $t, )*
        }

        impl $crate::gui::option_codec::OptionGroup for $name {
            #[allow(unused_doc_comments, unused_imports)]
            fn read_from(value: $crate::gui::option_codec::Lookup) -> Self {
                use $crate::gui::option_codec::*;
                $name {
                    $( $(#[$fm])* $f: Codec::<$t>::read(&$codec, value, $default), )*
                }
            }

            #[allow(unused_doc_comments, unused_imports)]
            fn write_to(&self, map: &mut serde_json::Map<String, serde_json::Value>) {
                use $crate::gui::option_codec::*;
                $( $(#[$fm])* { Codec::<$t>::write(&$codec, map, &self.$f); } )*
            }
        }

        impl $name {
            /// Every field at its default.
            #[allow(dead_code)]
            pub fn defaults() -> Self {
                <Self as $crate::gui::option_codec::OptionGroup>::read_from(&|_| None)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::options! {
        #[derive(Clone, Debug, PartialEq)]
        struct Sample {
            on: bool = true => key("on"),
            level: u32 = 50 => key("level").clamp(0, 100),
            time: u32 = 50 => key("time").scaled(10.0),
            name: Option<String> = None => key("name"),
        }
    }

    #[test]
    fn options_round_trip_through_their_keys() {
        let defaults = Sample::defaults();
        assert_eq!(defaults, Sample { on: true, level: 50, time: 50, name: None });
        let mut map = Map::new();
        Sample { on: false, level: 70, time: 25, name: Some("x".into()) }.write_to(&mut map);
        assert_eq!(map["time"], Value::from(2.5));
        let back = Sample::read_from(&|k| map.get(k).cloned());
        assert_eq!(back, Sample { on: false, level: 70, time: 25, name: Some("x".into()) });
        // Out of range is pulled back in.
        let clamped = Sample::read_from(&|k| (k == "level").then(|| Value::from(500)));
        assert_eq!(clamped.level, 100);
    }
}

#[cfg(test)]
mod gui_options_tests {
    use super::*;

    #[test]
    fn every_option_reads_back_what_it_wrote() {
        let options = crate::gui::GuiOptions::defaults();
        let mut first = Map::new();
        options.write_to(&mut first);
        let back = crate::gui::GuiOptions::read_from(&|k| first.get(k).cloned());
        let mut second = Map::new();
        back.write_to(&mut second);
        assert_eq!(first, second);
        assert!(first.contains_key("chatOpacity") && first.contains_key("guiScale"));
    }
}
