//! Java's `FriendlyByteBuf`, for reading a mod's own packets
//! ([`crate::Plugin::payload`]) and writing the ones it sends
//! ([`crate::host::send_to_server`]): big-endian numbers, VarInts,
//! strings, packed block positions, UUIDs and network NBT (as JSON, the way
//! [`crate::BlockEntityView::nbt`] has it; read only).

use serde_json::{Map, Value};

/// Reads a payload front to back; every read is `None` past its end.
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, at: 0 }
    }

    /// Bytes not read yet.
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }

    pub fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let slice = self.bytes.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(slice)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.bytes(N)?.try_into().ok()
    }

    pub fn u8(&mut self) -> Option<u8> {
        Some(self.array::<1>()?[0])
    }

    pub fn bool(&mut self) -> Option<bool> {
        Some(self.u8()? != 0)
    }

    pub fn i16(&mut self) -> Option<i16> {
        Some(i16::from_be_bytes(self.array()?))
    }

    pub fn i32(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes(self.array()?))
    }

    pub fn i64(&mut self) -> Option<i64> {
        Some(i64::from_be_bytes(self.array()?))
    }

    pub fn f32(&mut self) -> Option<f32> {
        Some(f32::from_be_bytes(self.array()?))
    }

    pub fn f64(&mut self) -> Option<f64> {
        Some(f64::from_be_bytes(self.array()?))
    }

    /// `readVarInt`.
    pub fn varint(&mut self) -> Option<i32> {
        let mut value = 0u32;
        for i in 0..5 {
            let b = self.u8()?;
            value |= u32::from(b & 0x7F) << (7 * i);
            if b & 0x80 == 0 {
                return Some(value as i32);
            }
        }
        None
    }

    /// `readVarLong`.
    pub fn varlong(&mut self) -> Option<i64> {
        let mut value = 0u64;
        for i in 0..10 {
            let b = self.u8()?;
            value |= u64::from(b & 0x7F) << (7 * i);
            if b & 0x80 == 0 {
                return Some(value as i64);
            }
        }
        None
    }

    /// `readUtf`: a VarInt length, then UTF-8.
    pub fn string(&mut self) -> Option<String> {
        let len = usize::try_from(self.varint()?).ok()?;
        String::from_utf8(self.bytes(len)?.to_vec()).ok()
    }

    /// `readResourceLocation`, as `ns:path`.
    pub fn identifier(&mut self) -> Option<String> {
        self.string()
    }

    /// `readBlockPos`: x, y and z packed in one long.
    pub fn block_pos(&mut self) -> Option<[i32; 3]> {
        let v = self.i64()?;
        Some([
            (v >> 38) as i32,
            (v << 52 >> 52) as i32,
            (v << 26 >> 38) as i32,
        ])
    }

    /// `readUUID`: two longs.
    pub fn uuid(&mut self) -> Option<u128> {
        let hi = self.i64()? as u64;
        let lo = self.i64()? as u64;
        Some((u128::from(hi) << 64) | u128::from(lo))
    }

    /// `readNbt`: network NBT (a nameless root tag) as JSON; `Null` for none.
    pub fn nbt(&mut self) -> Option<Value> {
        let kind = self.u8()?;
        if kind == 0 {
            return Some(Value::Null);
        }
        self.nbt_payload(kind, 0)
    }

    fn nbt_string(&mut self) -> Option<String> {
        let len = u16::from_be_bytes(self.array()?) as usize;
        // Modified UTF-8: close enough to UTF-8 for ids and text.
        Some(String::from_utf8_lossy(self.bytes(len)?).into_owned())
    }

    fn count(&mut self) -> Option<usize> {
        usize::try_from(self.i32()?).ok()
    }

    fn nbt_payload(&mut self, kind: u8, depth: u32) -> Option<Value> {
        if depth > 64 {
            return None;
        }
        let number = |v: f64| serde_json::Number::from_f64(v).map(Value::Number);
        Some(match kind {
            1 => Value::from(self.u8()? as i8),
            2 => Value::from(self.i16()?),
            3 => Value::from(self.i32()?),
            4 => Value::from(self.i64()?),
            5 => number(f64::from(self.f32()?))?,
            6 => number(self.f64()?)?,
            7 => {
                let n = self.count()?;
                Value::Array(
                    self.bytes(n)?
                        .iter()
                        .map(|b| Value::from(*b as i8))
                        .collect(),
                )
            }
            8 => Value::String(self.nbt_string()?),
            9 => {
                let element = self.u8()?;
                let n = self.count()?;
                let mut out = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    out.push(self.nbt_payload(element, depth + 1)?);
                }
                Value::Array(out)
            }
            10 => {
                let mut map = Map::new();
                loop {
                    let tag = self.u8()?;
                    if tag == 0 {
                        break;
                    }
                    let name = self.nbt_string()?;
                    let value = self.nbt_payload(tag, depth + 1)?;
                    map.insert(name, value);
                }
                Value::Object(map)
            }
            11 => {
                let n = self.count()?;
                let mut out = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    out.push(Value::from(self.i32()?));
                }
                Value::Array(out)
            }
            12 => {
                let n = self.count()?;
                let mut out = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    out.push(Value::from(self.i64()?));
                }
                Value::Array(out)
            }
            _ => return None,
        })
    }
}

/// Writes a payload front to back, as `FriendlyByteBuf.write*` do.
#[derive(Clone, Debug, Default)]
pub struct Writer {
    pub bytes: Vec<u8>,
}

impl Writer {
    pub fn new() -> Writer {
        Writer::default()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.bytes.push(v);
        self
    }

    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.u8(u8::from(v))
    }

    pub fn i16(&mut self, v: i16) -> &mut Self {
        self.bytes.extend(v.to_be_bytes());
        self
    }

    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.bytes.extend(v.to_be_bytes());
        self
    }

    pub fn i64(&mut self, v: i64) -> &mut Self {
        self.bytes.extend(v.to_be_bytes());
        self
    }

    pub fn f32(&mut self, v: f32) -> &mut Self {
        self.bytes.extend(v.to_be_bytes());
        self
    }

    pub fn f64(&mut self, v: f64) -> &mut Self {
        self.bytes.extend(v.to_be_bytes());
        self
    }

    pub fn varint(&mut self, v: i32) -> &mut Self {
        let mut v = v as u32;
        loop {
            if v & !0x7F == 0 {
                self.bytes.push(v as u8);
                return self;
            }
            self.bytes.push((v as u8 & 0x7F) | 0x80);
            v >>= 7;
        }
    }

    pub fn varlong(&mut self, v: i64) -> &mut Self {
        let mut v = v as u64;
        loop {
            if v & !0x7F == 0 {
                self.bytes.push(v as u8);
                return self;
            }
            self.bytes.push((v as u8 & 0x7F) | 0x80);
            v >>= 7;
        }
    }

    /// `writeUtf`: a VarInt length in bytes, then UTF-8.
    pub fn string(&mut self, v: &str) -> &mut Self {
        self.varint(v.len() as i32);
        self.bytes.extend(v.as_bytes());
        self
    }

    pub fn identifier(&mut self, v: &str) -> &mut Self {
        self.string(v)
    }

    /// `writeBlockPos`: x, y and z packed in one long.
    pub fn block_pos(&mut self, [x, y, z]: [i32; 3]) -> &mut Self {
        let v = ((i64::from(x) & 0x3FF_FFFF) << 38)
            | ((i64::from(z) & 0x3FF_FFFF) << 12)
            | (i64::from(y) & 0xFFF);
        self.i64(v)
    }

    /// `writeUUID`: two longs.
    pub fn uuid(&mut self, v: u128) -> &mut Self {
        self.i64((v >> 64) as i64).i64(v as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_friendly_byte_buf_writes() {
        let mut bytes = vec![0xAC, 0x02]; // VarInt 300
        bytes.extend([3, b'a', b':', b'b']); // "a:b"
        // BlockPos(-5, 64, 7).
        let packed: i64 = ((-5i64 & 0x3FF_FFFF) << 38) | ((7i64 & 0x3FF_FFFF) << 12) | (64 & 0xFFF);
        bytes.extend(packed.to_be_bytes());
        // NBT {n: 2}: compound, int tag "n" = 2, end.
        bytes.extend([10, 3, 0, 1, b'n', 0, 0, 0, 2, 0]);
        let mut r = Reader::new(&bytes);
        assert_eq!(r.varint(), Some(300));
        assert_eq!(r.identifier().as_deref(), Some("a:b"));
        assert_eq!(r.block_pos(), Some([-5, 64, 7]));
        assert_eq!(r.nbt(), Some(serde_json::json!({"n": 2})));
        assert_eq!(r.remaining(), 0);
        assert_eq!(r.u8(), None);
    }

    #[test]
    fn writes_what_the_reader_reads() {
        let mut w = Writer::new();
        w.varint(300)
            .varint(-1)
            .identifier("a:b")
            .block_pos([-5, 64, 7])
            .bool(true)
            .uuid(7 << 64 | 9)
            .f32(1.5);
        let bytes = w.into_bytes();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.varint(), Some(300));
        assert_eq!(r.varint(), Some(-1));
        assert_eq!(r.identifier().as_deref(), Some("a:b"));
        assert_eq!(r.block_pos(), Some([-5, 64, 7]));
        assert_eq!(r.bool(), Some(true));
        assert_eq!(r.uuid(), Some(7 << 64 | 9));
        assert_eq!(r.f32(), Some(1.5));
        assert_eq!(r.remaining(), 0);
    }
}
