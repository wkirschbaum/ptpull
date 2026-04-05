use std::io::Cursor;

use binrw::BinRead;

/// MTP operation codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum OpCode {
    GetDeviceInfo = 0x1001,
    OpenSession = 0x1002,
    CloseSession = 0x1003,
    GetStorageIDs = 0x1004,
    GetStorageInfo = 0x1005,
    GetObjectHandles = 0x1007,
    GetObjectInfo = 0x1008,
    GetObject = 0x1009,
    GetThumb = 0x100A,
    GetPartialObject = 0x101B,
}

/// MTP response codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ResponseCode {
    Ok = 0x2001,
    GeneralError = 0x2002,
    SessionNotOpen = 0x2003,
    InvalidTransactionId = 0x2004,
    OperationNotSupported = 0x2005,
    ParameterNotSupported = 0x2006,
    IncompleteTransfer = 0x2007,
    InvalidStorageId = 0x2008,
    InvalidObjectHandle = 0x2009,
    StoreNotAvailable = 0x2013,
    SpecificationByFormatUnsupported = 0x2014,
    NoValidObjectInfo = 0x2015,
    InvalidParentObject = 0x201A,
    SessionAlreadyOpen = 0x201E,
}

impl ResponseCode {
    pub fn from_u16(v: u16) -> Self {
        match v {
            0x2001 => Self::Ok,
            0x2002 => Self::GeneralError,
            0x2003 => Self::SessionNotOpen,
            0x2004 => Self::InvalidTransactionId,
            0x2005 => Self::OperationNotSupported,
            0x2006 => Self::ParameterNotSupported,
            0x2007 => Self::IncompleteTransfer,
            0x2008 => Self::InvalidStorageId,
            0x2009 => Self::InvalidObjectHandle,
            0x2013 => Self::StoreNotAvailable,
            0x2014 => Self::SpecificationByFormatUnsupported,
            0x2015 => Self::NoValidObjectInfo,
            0x201A => Self::InvalidParentObject,
            0x201E => Self::SessionAlreadyOpen,
            _ => Self::GeneralError,
        }
    }

    pub fn is_ok(self) -> bool {
        self == Self::Ok
    }
}

/// MTP object format codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ObjectFormat {
    Undefined = 0x3000,
    Association = 0x3001, // folder
    Jpeg = 0x3801,
    Tiff = 0x380D,
    Png = 0x380B,
    Raw = 0x3002, // generic raw
    NikonNef = 0xB001,
    CanonCr2 = 0xB103,
    SonyArw = 0xB101,
    Avi = 0x300A,
    Mpeg = 0x300B,
    Mp4 = 0xB982,
    Mov = 0x300D,
}

impl ObjectFormat {
    pub fn from_u16(v: u16) -> Self {
        match v {
            0x3000 => Self::Undefined,
            0x3001 => Self::Association,
            0x3801 => Self::Jpeg,
            0x380D => Self::Tiff,
            0x380B => Self::Png,
            0x3002 => Self::Raw,
            0xB001 => Self::NikonNef,
            0xB103 => Self::CanonCr2,
            0xB101 => Self::SonyArw,
            0x300A => Self::Avi,
            0x300B => Self::Mpeg,
            0xB982 => Self::Mp4,
            0x300D => Self::Mov,
            _ => Self::Undefined,
        }
    }

    pub fn is_image(self) -> bool {
        matches!(
            self,
            Self::Jpeg
                | Self::Tiff
                | Self::Png
                | Self::Raw
                | Self::NikonNef
                | Self::CanonCr2
                | Self::SonyArw
        )
    }

    pub fn is_video(self) -> bool {
        matches!(self, Self::Avi | Self::Mpeg | Self::Mp4 | Self::Mov)
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Tiff => "tiff",
            Self::Png => "png",
            Self::NikonNef => "nef",
            Self::CanonCr2 => "cr2",
            Self::SonyArw => "arw",
            Self::Avi => "avi",
            Self::Mpeg => "mpg",
            Self::Mp4 => "mp4",
            Self::Mov => "mov",
            _ => "bin",
        }
    }
}

/// Storage type codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum StorageType {
    FixedRom = 0x0001,
    RemovableRom = 0x0002,
    FixedRam = 0x0003,
    RemovableRam = 0x0004,
}

/// Read a MTP-encoded string: u8 length prefix (in chars) + UTF-16LE data
pub fn read_mtp_string(data: &[u8], offset: &mut usize) -> String {
    if *offset >= data.len() {
        return String::new();
    }
    let char_count = data[*offset] as usize;
    *offset += 1;
    if char_count == 0 {
        return String::new();
    }
    let byte_count = char_count * 2;
    if *offset + byte_count > data.len() {
        return String::new();
    }
    let utf16: Vec<u16> = data[*offset..*offset + byte_count]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    *offset += byte_count;
    // Strip null terminator if present
    let end = utf16.iter().position(|&c| c == 0).unwrap_or(utf16.len());
    String::from_utf16_lossy(&utf16[..end])
}

/// Read a MTP u32 array: u32 count + count * u32 elements
pub fn read_mtp_u32_array(data: &[u8], offset: &mut usize) -> Vec<u32> {
    if *offset + 4 > data.len() {
        return Vec::new();
    }
    let mut cursor = Cursor::new(&data[*offset..]);
    let count = u32::read_le(&mut cursor).unwrap_or(0) as usize;
    *offset += 4;
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        if *offset + 4 > data.len() {
            break;
        }
        let mut c = Cursor::new(&data[*offset..]);
        if let Ok(v) = u32::read_le(&mut c) {
            result.push(v);
        }
        *offset += 4;
    }
    result
}

/// Read a MTP u16 array: u32 count + count * u16 elements
pub fn read_mtp_u16_array(data: &[u8], offset: &mut usize) -> Vec<u16> {
    if *offset + 4 > data.len() {
        return Vec::new();
    }
    let mut cursor = Cursor::new(&data[*offset..]);
    let count = u32::read_le(&mut cursor).unwrap_or(0) as usize;
    *offset += 4;
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        if *offset + 2 > data.len() {
            break;
        }
        let mut c = Cursor::new(&data[*offset..]);
        if let Ok(v) = u16::read_le(&mut c) {
            result.push(v);
        }
        *offset += 2;
    }
    result
}

/// Read a little-endian u32 at offset
pub fn read_u32_le(data: &[u8], offset: &mut usize) -> u32 {
    if *offset + 4 > data.len() {
        return 0;
    }
    let v = u32::from_le_bytes([
        data[*offset],
        data[*offset + 1],
        data[*offset + 2],
        data[*offset + 3],
    ]);
    *offset += 4;
    v
}

/// Read a little-endian u16 at offset
pub fn read_u16_le(data: &[u8], offset: &mut usize) -> u16 {
    if *offset + 2 > data.len() {
        return 0;
    }
    let v = u16::from_le_bytes([data[*offset], data[*offset + 1]]);
    *offset += 2;
    v
}

/// Read a single u8 at offset
pub fn read_u8(data: &[u8], offset: &mut usize) -> u8 {
    if *offset >= data.len() {
        return 0;
    }
    let v = data[*offset];
    *offset += 1;
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_mtp_string() {
        // "Hi" in MTP string format: char_count=3 (including null), UTF-16LE 'H','i','\0'
        let data = [
            0x03, // 3 chars
            b'H', 0x00, // 'H' UTF-16LE
            b'i', 0x00, // 'i' UTF-16LE
            0x00, 0x00, // null terminator
        ];
        let mut offset = 0;
        let s = read_mtp_string(&data, &mut offset);
        assert_eq!(s, "Hi");
        assert_eq!(offset, 7);
    }

    #[test]
    fn test_read_mtp_u32_array() {
        let data = [
            0x02, 0x00, 0x00, 0x00, // count = 2
            0x01, 0x00, 0x00, 0x00, // element 1
            0x02, 0x00, 0x00, 0x00, // element 2
        ];
        let mut offset = 0;
        let arr = read_mtp_u32_array(&data, &mut offset);
        assert_eq!(arr, vec![1, 2]);
    }

    #[test]
    fn test_response_code() {
        assert!(ResponseCode::Ok.is_ok());
        assert!(!ResponseCode::GeneralError.is_ok());
        assert_eq!(ResponseCode::from_u16(0x2001), ResponseCode::Ok);
    }

    #[test]
    fn test_object_format() {
        assert!(ObjectFormat::Jpeg.is_image());
        assert!(!ObjectFormat::Jpeg.is_video());
        assert!(ObjectFormat::Mp4.is_video());
        assert_eq!(ObjectFormat::Jpeg.extension(), "jpg");
    }
}
