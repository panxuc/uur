//! Wire protocol spoken between the in-client PE hook (`hook/`) and the host
//! bridge.  Kept in one place so both sides stay in lock-step.

pub const MAGIC: u32 = 0x50495555; // "UUIP" little-endian
pub const VERSION: u32 = 1;

pub const RECORD_HELLO: u32 = 1;
pub const RECORD_MOUSE: u32 = 2;
pub const RECORD_KEYBOARD: u32 = 3;

/// One input record.  Fixed 16-byte payload after the 16-byte header keeps
/// parsing trivial on both sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    pub kind: u32,
    /// Keyboard: Windows virtual-key code (as sent by SendInput).
    /// Mouse: button number for press/release, axis selector for motion.
    pub code: u16,
    /// Keyboard: 1 = key-down, 0 = key-up.
    /// Mouse motion: 1 = absolute, 0 = relative.
    pub state: u16,
    pub a: i32,
    pub b: i32,
}

pub const HEADER_BYTES: usize = 16;
pub const RECORD_BYTES: usize = 16;

#[derive(Debug)]
pub struct Header {
    pub magic: u32,
    pub version: u32,
    pub kind: u32,
    pub length: u32,
}

pub fn read_header(buf: &[u8; HEADER_BYTES]) -> Header {
    Header {
        magic: u32::from_le_bytes(buf[0..4].try_into().unwrap()),
        version: u32::from_le_bytes(buf[4..8].try_into().unwrap()),
        kind: u32::from_le_bytes(buf[8..12].try_into().unwrap()),
        length: u32::from_le_bytes(buf[12..16].try_into().unwrap()),
    }
}

pub fn read_record(buf: &[u8; RECORD_BYTES]) -> Record {
    Record {
        kind: u32::from_le_bytes(buf[0..4].try_into().unwrap()),
        code: u16::from_le_bytes(buf[4..6].try_into().unwrap()),
        state: u16::from_le_bytes(buf[6..8].try_into().unwrap()),
        a: i32::from_le_bytes(buf[8..12].try_into().unwrap()),
        b: i32::from_le_bytes(buf[12..16].try_into().unwrap()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let mut buf = [0u8; HEADER_BYTES];
        buf[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        buf[4..8].copy_from_slice(&VERSION.to_le_bytes());
        buf[8..12].copy_from_slice(&RECORD_MOUSE.to_le_bytes());
        buf[12..16].copy_from_slice(&16u32.to_le_bytes());
        let header = read_header(&buf);
        assert_eq!(header.magic, MAGIC);
        assert_eq!(header.kind, RECORD_MOUSE);
        assert_eq!(header.length, 16);
    }

    #[test]
    fn record_roundtrip() {
        let mut buf = [0u8; RECORD_BYTES];
        buf[0..4].copy_from_slice(&RECORD_KEYBOARD.to_le_bytes());
        buf[4..6].copy_from_slice(&0x41u16.to_le_bytes());
        buf[6..8].copy_from_slice(&1u16.to_le_bytes());
        buf[8..12].copy_from_slice(&(-5i32).to_le_bytes());
        buf[12..16].copy_from_slice(&(7i32).to_le_bytes());
        let record = read_record(&buf);
        assert_eq!(record.kind, RECORD_KEYBOARD);
        assert_eq!(record.code, 0x41);
        assert_eq!(record.state, 1);
        assert_eq!(record.a, -5);
        assert_eq!(record.b, 7);
    }
}
