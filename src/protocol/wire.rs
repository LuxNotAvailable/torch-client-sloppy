const MAX_VARINT_BYTES: usize = 5;

const MAX_VARLONG_BYTES: usize = 10;

pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    pub(crate) fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub(crate) fn slice_from(&self, start: usize) -> &'a [u8] {
        &self.buf[start..self.pos]
    }

    pub(crate) fn rest(&self) -> &'a [u8] {
        &self.buf[self.pos..]
    }

    pub(crate) fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let out = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(out)
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        let b = *self.buf.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    pub(crate) fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }

    pub(crate) fn u64(&mut self) -> Option<u64> {
        Some(u64::from_be_bytes(self.take(8)?.try_into().ok()?))
    }

    pub(crate) fn varint(&mut self) -> Option<i32> {
        let mut value: u32 = 0;
        for i in 0..MAX_VARINT_BYTES {
            let byte = self.u8()?;
            value |= u32::from(byte & 0x7F) << (7 * i);
            if byte & 0x80 == 0 {
                return Some(value as i32);
            }
        }
        None
    }

    pub(crate) fn varlong(&mut self) -> Option<i64> {
        let mut value: u64 = 0;
        for i in 0..MAX_VARLONG_BYTES {
            let byte = self.u8()?;
            value |= u64::from(byte & 0x7F) << (7 * i);
            if byte & 0x80 == 0 {
                return Some(value as i64);
            }
        }
        None
    }

    pub(crate) fn count(&mut self) -> Option<usize> {
        usize::try_from(self.varint().filter(|v| *v >= 0)?).ok()
    }
}

const NBT_MAX_DEPTH: usize = 512;

impl Reader<'_> {
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    /// Steps over one network NBT value: a type byte, then that tag's payload
    /// with no root name.
    pub(crate) fn skip_nbt(&mut self) -> Option<()> {
        let kind = self.u8()?;
        if kind == 0 {
            return Some(());
        }
        self.skip_nbt_payload(kind, 0)
    }

    fn skip_nbt_payload(&mut self, kind: u8, depth: usize) -> Option<()> {
        if depth > NBT_MAX_DEPTH {
            return None;
        }
        let sized = |r: &mut Self, width: usize| -> Option<()> {
            let len = usize::try_from(r.i32()?).ok()?;
            r.take(len.checked_mul(width)?).map(drop)
        };
        match kind {
            1 => self.take(1).map(drop),
            2 => self.take(2).map(drop),
            3 | 5 => self.take(4).map(drop),
            4 | 6 => self.take(8).map(drop),
            7 => sized(self, 1),
            8 => {
                let len = usize::from(self.u16()?);
                self.take(len).map(drop)
            }
            9 => {
                let element = self.u8()?;
                let len = usize::try_from(self.i32()?).ok()?;
                if element == 0 {
                    return Some(());
                }
                for _ in 0..len {
                    self.skip_nbt_payload(element, depth + 1)?;
                }
                Some(())
            }
            10 => loop {
                let field = self.u8()?;
                if field == 0 {
                    return Some(());
                }
                let name = usize::from(self.u16()?);
                self.take(name)?;
                self.skip_nbt_payload(field, depth + 1)?;
            },
            11 => sized(self, 4),
            12 => sized(self, 8),
            _ => None,
        }
    }
}

pub(crate) fn write_varint(out: &mut Vec<u8>, value: i32) {
    let mut value = value as u32;
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

pub(crate) fn write_varlong(out: &mut Vec<u8>, value: i64) {
    let mut value = value as u64;
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

pub(crate) fn varint_len(value: i32) -> usize {
    let mut value = value as u32;
    let mut n = 1;
    while value >= 0x80 {
        value >>= 7;
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints_round_trip() {
        for value in [
            0,
            1,
            127,
            128,
            255,
            2097151,
            2097152,
            i32::MAX,
            -1,
            i32::MIN,
        ] {
            let mut out = Vec::new();
            write_varint(&mut out, value);
            assert_eq!(out.len(), varint_len(value), "length disagrees for {value}");
            let mut r = Reader::new(&out);
            assert_eq!(r.varint(), Some(value));
            assert_eq!(r.remaining(), 0);
        }
    }

    #[test]
    fn known_encodings() {
        let encode = |v| {
            let mut out = Vec::new();
            write_varint(&mut out, v);
            out
        };
        assert_eq!(encode(0), [0x00]);
        assert_eq!(encode(1), [0x01]);
        assert_eq!(encode(127), [0x7F]);
        assert_eq!(encode(128), [0x80, 0x01]);
        assert_eq!(encode(255), [0xFF, 0x01]);
        assert_eq!(encode(-1), [0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
    }

    #[test]
    fn a_truncated_frame_reads_none() {
        assert_eq!(Reader::new(&[]).varint(), None);
        assert_eq!(Reader::new(&[0x80]).varint(), None);
        assert_eq!(Reader::new(&[0x80; 6]).varint(), None);
        assert_eq!(Reader::new(&[0x01, 0x02]).take(3), None);
        assert_eq!(Reader::new(&[0x01]).u16(), None);
    }

    #[test]
    fn a_negative_count_is_not_a_count() {
        let mut out = Vec::new();
        write_varint(&mut out, -1);
        assert_eq!(Reader::new(&out).count(), None);
        let mut out = Vec::new();
        write_varint(&mut out, 300);
        assert_eq!(Reader::new(&out).count(), Some(300));
    }

    #[test]
    fn nbt_is_skipped_to_its_last_byte() {
        let mut compound = vec![10u8];
        compound.extend_from_slice(&[8, 0, 4]);
        compound.extend_from_slice(b"text");
        compound.extend_from_slice(&[0, 2]);
        compound.extend_from_slice(b"hi");
        compound.extend_from_slice(&[9, 0, 1, b'l', 3]);
        compound.extend_from_slice(&2i32.to_be_bytes());
        compound.extend_from_slice(&[0, 0, 0, 7, 0, 0, 0, 9]);
        compound.push(0);
        let mut bytes = compound.clone();
        bytes.push(0xEE);
        let mut r = Reader::new(&bytes);
        assert_eq!(r.skip_nbt(), Some(()));
        assert_eq!(r.pos(), compound.len());

        let mut plain_string = vec![8u8, 0, 3];
        plain_string.extend_from_slice(b"abc");
        let mut r = Reader::new(&plain_string);
        assert_eq!(r.skip_nbt(), Some(()));
        assert_eq!(r.remaining(), 0);

        assert_eq!(Reader::new(&[0]).skip_nbt(), Some(()));
        assert_eq!(Reader::new(&[10, 8, 0]).skip_nbt(), None);
    }

    #[test]
    fn the_cursor_tracks_what_it_read() {
        let bytes = [0x80, 0x01, 0xAB, 0xCD, 0xEF];
        let mut r = Reader::new(&bytes);
        assert_eq!(r.varint(), Some(128));
        assert_eq!(r.pos(), 2);
        assert_eq!(r.slice_from(0), &[0x80, 0x01]);
        assert_eq!(r.u8(), Some(0xAB));
        assert_eq!(r.rest(), &[0xCD, 0xEF]);
        assert_eq!(r.remaining(), 2);
    }
}
