//! Setting a precompiled module's specialization constants by name: the
//! built-in pack's options, slots, uniform offsets and variants
//! (`builtin/iris/iris.slang`). Each constant's default value is
//! rewritten in place, so the module needs no `VkSpecializationInfo` and
//! goes through the same pipeline code as any other.

const MAGIC: u32 = 0x0723_0203;
const HEADER: usize = 5;

const OP_NAME: u16 = 5;
const OP_TYPE_BOOL: u16 = 20;
const OP_TYPE_INT: u16 = 21;
const OP_TYPE_FLOAT: u16 = 22;
const OP_SPEC_CONSTANT_TRUE: u16 = 48;
const OP_SPEC_CONSTANT_FALSE: u16 = 49;
const OP_SPEC_CONSTANT: u16 = 50;

#[derive(Clone, Copy)]
enum Scalar {
    Bool,
    Int { signed: bool },
    Float,
}

/// The instructions of `words`, as (offset, opcode, word count).
fn instructions(words: &[u32]) -> impl Iterator<Item = (usize, u16, usize)> + '_ {
    let mut at = HEADER;
    std::iter::from_fn(move || {
        let word = *words.get(at)?;
        let count = (word >> 16) as usize;
        if count == 0 || at + count > words.len() {
            return None;
        }
        let item = (at, (word & 0xFFFF) as u16, count);
        at += count;
        Some(item)
    })
}

/// A literal string's text: little-endian bytes up to the first nul.
fn literal(words: &[u32]) -> String {
    let bytes: Vec<u8> = words
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .take_while(|b| *b != 0)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Bytes from `include_bytes!` as SPIR-V words.
pub(crate) fn words(bytes: &[u8]) -> Result<Vec<u32>, String> {
    if bytes.len() % 4 != 0 {
        return Err("SPIR-V is not a whole number of words".to_owned());
    }
    let words: Vec<u32> = bytes
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    if words.first() != Some(&MAGIC) || words.len() < HEADER {
        return Err("not a SPIR-V module".to_owned());
    }
    Ok(words)
}

/// Sets each named specialization constant `value` knows (booleans by
/// nonzero), and returns the names of those it did not.
pub(crate) fn specialize(words: &mut [u32], value: impl Fn(&str) -> Option<f64>) -> Vec<String> {
    let mut names = std::collections::HashMap::new();
    let mut types = std::collections::HashMap::new();
    let mut constants = Vec::new();
    for (at, op, count) in instructions(words) {
        match op {
            OP_NAME if count >= 3 => {
                names.insert(words[at + 1], literal(&words[at + 2..at + count]));
            }
            OP_TYPE_BOOL if count >= 2 => {
                types.insert(words[at + 1], Scalar::Bool);
            }
            OP_TYPE_INT if count >= 4 && words[at + 2] == 32 => {
                types.insert(
                    words[at + 1],
                    Scalar::Int {
                        signed: words[at + 3] != 0,
                    },
                );
            }
            OP_TYPE_FLOAT if count >= 3 && words[at + 2] == 32 => {
                types.insert(words[at + 1], Scalar::Float);
            }
            OP_SPEC_CONSTANT_TRUE | OP_SPEC_CONSTANT_FALSE | OP_SPEC_CONSTANT if count >= 3 => {
                constants.push((at, op, words[at + 1], words[at + 2]));
            }
            _ => {}
        }
    }
    let mut missing = Vec::new();
    for (at, op, ty, id) in constants {
        let Some(name) = names.get(&id) else {
            continue;
        };
        let Some(v) = value(name) else {
            missing.push(name.clone());
            continue;
        };
        match (op, types.get(&ty)) {
            (OP_SPEC_CONSTANT_TRUE | OP_SPEC_CONSTANT_FALSE, _) => {
                let op = if v != 0.0 {
                    OP_SPEC_CONSTANT_TRUE
                } else {
                    OP_SPEC_CONSTANT_FALSE
                };
                words[at] = (words[at] & 0xFFFF_0000) | u32::from(op);
            }
            (OP_SPEC_CONSTANT, Some(Scalar::Float)) => words[at + 3] = (v as f32).to_bits(),
            (OP_SPEC_CONSTANT, Some(Scalar::Int { signed: true })) => {
                words[at + 3] = (v.round() as i32) as u32;
            }
            (OP_SPEC_CONSTANT, Some(Scalar::Int { signed: false })) => {
                words[at + 3] = v.round() as u32;
            }
            _ => missing.push(name.clone()),
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A module with `uint A = 1`, `float B = 2.0`, `int C = 3`, `bool D =
    /// false` and an unnamed constant, as a compiler lays them out.
    fn module() -> Vec<u32> {
        let mut w = vec![MAGIC, 0x0001_0300, 0, 20, 0];
        let name = |w: &mut Vec<u32>, id: u32, text: &str| {
            let mut bytes = text.as_bytes().to_vec();
            bytes.push(0);
            while bytes.len() % 4 != 0 {
                bytes.push(0);
            }
            let words: Vec<u32> = bytes
                .chunks(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            w.push(((2 + words.len() as u32) << 16) | u32::from(OP_NAME));
            w.push(id);
            w.extend(words);
        };
        name(&mut w, 10, "A");
        name(&mut w, 11, "B");
        name(&mut w, 12, "C");
        name(&mut w, 13, "D");
        w.extend([(4 << 16) | u32::from(OP_TYPE_INT), 1, 32, 0]);
        w.extend([(3 << 16) | u32::from(OP_TYPE_FLOAT), 2, 32]);
        w.extend([(4 << 16) | u32::from(OP_TYPE_INT), 3, 32, 1]);
        w.extend([(2 << 16) | u32::from(OP_TYPE_BOOL), 4]);
        w.extend([(4 << 16) | u32::from(OP_SPEC_CONSTANT), 1, 10, 1]);
        w.extend([
            (4 << 16) | u32::from(OP_SPEC_CONSTANT),
            2,
            11,
            2.0f32.to_bits(),
        ]);
        w.extend([(4 << 16) | u32::from(OP_SPEC_CONSTANT), 3, 12, 3]);
        w.extend([(3 << 16) | u32::from(OP_SPEC_CONSTANT_FALSE), 4, 13]);
        w.extend([(4 << 16) | u32::from(OP_SPEC_CONSTANT), 1, 14, 9]);
        w
    }

    #[test]
    fn constants_are_set_by_name_and_type() {
        let mut w = module();
        let missing = specialize(&mut w, |name| match name {
            "A" => Some(4_294_967_295.0),
            "B" => Some(2.4),
            "C" => Some(-7.0),
            "D" => Some(1.0),
            _ => None,
        });
        assert!(missing.is_empty(), "{missing:?}");
        let values: Vec<(u16, u32)> = instructions(&w)
            .filter(|(_, op, _)| matches!(*op, OP_SPEC_CONSTANT | OP_SPEC_CONSTANT_TRUE))
            .map(|(at, op, count)| (op, if count > 3 { w[at + 3] } else { 1 }))
            .collect();
        assert_eq!(
            values,
            [
                (OP_SPEC_CONSTANT, u32::MAX),
                (OP_SPEC_CONSTANT, 2.4f32.to_bits()),
                (OP_SPEC_CONSTANT, (-7i32) as u32),
                (OP_SPEC_CONSTANT_TRUE, 1),
                (OP_SPEC_CONSTANT, 9),
            ]
        );
    }

    #[test]
    fn unknown_names_are_reported_and_left_alone() {
        let mut w = module();
        let before = w.clone();
        let mut missing = specialize(&mut w, |_| None);
        missing.sort();
        assert_eq!(missing, ["A", "B", "C", "D"]);
        assert_eq!(w, before);
    }
}
