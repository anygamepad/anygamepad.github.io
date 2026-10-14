//! Device protocols: code that reads and writes the HID reports of known
//! controllers (C7, C15).
//!
//! Protocols are sans-IO: they turn report bytes into canonical state, and
//! never move bytes themselves. The same code serves every transport that
//! gives raw reports: WebHID in the browser, and hidraw, Windows HID and
//! IOKit natively.

pub mod playstation;

/// CRC-32 (IEEE 802.3, reflected polynomial `0xEDB88320`) of `prefix`
/// followed by `data`, as Bluetooth controllers checksum their reports.
pub(crate) fn crc32(prefix: &[u8], data: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut table = [0u32; 256];
        let mut n = 0;
        while n < 256 {
            let mut c = n as u32;
            let mut k = 0;
            while k < 8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
                k += 1;
            }
            table[n] = c;
            n += 1;
        }
        table
    };
    let mut crc = !0u32;
    for &byte in prefix.iter().chain(data) {
        crc = (crc >> 8) ^ TABLE[((crc ^ u32::from(byte)) & 0xff) as usize];
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_check_value() {
        // The standard check value of CRC-32/ISO-HDLC.
        assert_eq!(crc32(b"", b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b"1234", b"56789"), 0xcbf4_3926);
        assert_eq!(crc32(b"", b""), 0);
    }
}
