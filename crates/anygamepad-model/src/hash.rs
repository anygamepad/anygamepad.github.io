//! Deterministic hashes: identical results on every platform, in WebAssembly
//! and across library versions, unlike `std::hash`.

/// FNV-1a, 64 bits.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Fnv64(u64);

impl Fnv64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    pub(crate) const fn new() -> Self {
        Self(Self::OFFSET)
    }

    pub(crate) fn bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    /// A length-prefixed string, so that consecutive fields cannot run into
    /// each other.
    pub(crate) fn str(&mut self, s: &str) {
        self.bytes(&(s.len() as u32).to_le_bytes());
        self.bytes(s.as_bytes());
    }

    pub(crate) fn opt_str(&mut self, s: Option<&str>) {
        match s {
            None => self.bytes(&[0]),
            Some(s) => {
                self.bytes(&[1]);
                self.str(s);
            }
        }
    }

    pub(crate) fn opt_u16(&mut self, value: Option<u16>) {
        match value {
            None => self.bytes(&[0]),
            Some(value) => {
                self.bytes(&[1]);
                self.bytes(&value.to_le_bytes());
            }
        }
    }

    pub(crate) fn opt_u64(&mut self, value: Option<u64>) {
        match value {
            None => self.bytes(&[0]),
            Some(value) => {
                self.bytes(&[1]);
                self.bytes(&value.to_le_bytes());
            }
        }
    }

    pub(crate) const fn finish(self) -> u64 {
        self.0
    }
}

/// CRC-16/ARC, as `SDL_crc16` computes it.
pub(crate) fn crc16(mut crc: u16, data: &[u8]) -> u16 {
    for &byte in data {
        crc ^= u16::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xa001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fnv(bytes: &[u8]) -> u64 {
        let mut hash = Fnv64::new();
        hash.bytes(bytes);
        hash.finish()
    }

    #[test]
    fn fnv1a_64_reference_values() {
        assert_eq!(fnv(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn crc16_arc_reference_value() {
        assert_eq!(crc16(0, b"123456789"), 0xbb3d);
        assert_eq!(crc16(0, b""), 0);
        // Incremental, as SDL hashes "<vendor> <product>" in three calls.
        assert_eq!(crc16(crc16(0, b"1234"), b"56789"), 0xbb3d);
    }
}
