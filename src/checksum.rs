//! Checksum algorithms used when sending HEX frames and exposed to Lua scripts.

use serde::{Deserialize, Serialize};

/// Supported checksum algorithms. The result is appended to a frame in the byte order
/// conventionally used by each protocol (CRC-16 Modbus little-endian, others big-endian).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Checksum {
    #[default]
    None,
    Sum8,
    Xor8,
    Crc8,
    Crc16Modbus,
    Crc16Ccitt,
    Crc32,
}

impl Checksum {
    pub const ALL: [Checksum; 7] = [
        Self::None,
        Self::Sum8,
        Self::Xor8,
        Self::Crc8,
        Self::Crc16Modbus,
        Self::Crc16Ccitt,
        Self::Crc32,
    ];

    /// Parses the snake_case algorithm name used by Lua scripts.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "sum8" => Some(Self::Sum8),
            "xor8" => Some(Self::Xor8),
            "crc8" => Some(Self::Crc8),
            "crc16_modbus" => Some(Self::Crc16Modbus),
            "crc16_ccitt" => Some(Self::Crc16Ccitt),
            "crc32" => Some(Self::Crc32),
            _ => None,
        }
    }

    pub fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Sum8 => "Sum8",
            Self::Xor8 => "XOR8",
            Self::Crc8 => "CRC-8",
            Self::Crc16Modbus => "CRC16/Modbus",
            Self::Crc16Ccitt => "CRC16/CCITT",
            Self::Crc32 => "CRC-32",
        }
    }

    /// Checksum bytes for `data`, in on-wire order. Empty for [`Checksum::None`].
    pub fn compute(self, data: &[u8]) -> Vec<u8> {
        match self {
            Self::None => Vec::new(),
            Self::Sum8 => vec![sum8(data)],
            Self::Xor8 => vec![xor8(data)],
            Self::Crc8 => vec![crc8(data)],
            Self::Crc16Modbus => crc16_modbus(data).to_le_bytes().to_vec(),
            Self::Crc16Ccitt => crc16_ccitt(data).to_be_bytes().to_vec(),
            Self::Crc32 => crc32(data).to_be_bytes().to_vec(),
        }
    }

    /// `data` followed by its checksum.
    pub fn append_to(self, mut data: Vec<u8>) -> Vec<u8> {
        let sum = self.compute(&data);
        data.extend_from_slice(&sum);
        data
    }
}

pub fn sum8(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |acc, b| acc.wrapping_add(*b))
}

pub fn xor8(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |acc, b| acc ^ *b)
}

/// CRC-8 (poly 0x07, init 0x00).
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &byte in data {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// CRC-16/MODBUS (poly 0xA001 reflected, init 0xFFFF).
pub fn crc16_modbus(data: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for &byte in data {
        crc ^= u16::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// CRC-16/CCITT-FALSE (poly 0x1021, init 0xFFFF).
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for &byte in data {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// CRC-32/ISO-HDLC (zlib / Ethernet).
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHECK: &[u8] = b"123456789";

    #[test]
    fn standard_check_values() {
        assert_eq!(crc8(CHECK), 0xF4);
        assert_eq!(crc16_modbus(CHECK), 0x4B37);
        assert_eq!(crc16_ccitt(CHECK), 0x29B1);
        assert_eq!(crc32(CHECK), 0xCBF4_3926);
        assert_eq!(sum8(&[0xFF, 0x02]), 0x01);
        assert_eq!(xor8(&[0x0F, 0xF0, 0x01]), 0xFE);
    }

    #[test]
    fn empty_input() {
        assert_eq!(crc16_modbus(&[]), 0xFFFF);
        assert_eq!(crc32(&[]), 0);
        assert!(Checksum::None.compute(CHECK).is_empty());
    }

    #[test]
    fn wire_byte_order() {
        // Classic Modbus request: 01 03 00 00 00 0A -> CRC bytes C5 CD (low byte first).
        let frame = Checksum::Crc16Modbus.append_to(vec![0x01, 0x03, 0x00, 0x00, 0x00, 0x0A]);
        assert_eq!(&frame[6..], &[0xC5, 0xCD]);
        assert_eq!(Checksum::Crc16Ccitt.compute(CHECK), vec![0x29, 0xB1]);
        assert_eq!(Checksum::Crc32.compute(CHECK), vec![0xCB, 0xF4, 0x39, 0x26]);
    }
}
