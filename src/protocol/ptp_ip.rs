use binrw::{BinRead, BinWrite, binrw};
use bytes::{Buf, BufMut, BytesMut};
use std::io::Cursor;
use tokio_util::codec::{Decoder, Encoder};

/// PTP-IP default port
pub const PTP_IP_PORT: u16 = 15740;

/// PTP-IP packet type identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PacketType {
    InitCmdRequest = 0x01,
    InitCmdAck = 0x02,
    InitEventRequest = 0x03,
    InitEventAck = 0x04,
    CmdRequest = 0x06,
    CmdResponse = 0x07,
    StartData = 0x09,
    Data = 0x0A,
    Cancel = 0x0B,
    EndData = 0x0C,
    Probe = 0x0D,
}

impl PacketType {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0x01 => Some(Self::InitCmdRequest),
            0x02 => Some(Self::InitCmdAck),
            0x03 => Some(Self::InitEventRequest),
            0x04 => Some(Self::InitEventAck),
            0x06 => Some(Self::CmdRequest),
            0x07 => Some(Self::CmdResponse),
            0x09 => Some(Self::StartData),
            0x0A => Some(Self::Data),
            0x0B => Some(Self::Cancel),
            0x0C => Some(Self::EndData),
            0x0D => Some(Self::Probe),
            _ => None,
        }
    }
}

/// Raw PTP-IP frame: length-prefixed with a type tag
#[derive(Debug, Clone)]
pub struct PtpIpFrame {
    pub packet_type: u32,
    pub payload: Vec<u8>,
}

/// Init command request — sent to establish the command channel
#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub struct InitCmdRequest {
    pub guid: [u8; 16],
    /// UTF-16LE null-terminated hostname
    #[bw(map = |s: &Vec<u16>| s.clone())]
    #[br(parse_with = read_utf16le_null_terminated)]
    pub hostname: Vec<u16>,
    pub version: u32,
}

/// Init command acknowledgment — returned by camera
#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub struct InitCmdAck {
    pub session_id: u32,
    pub guid: [u8; 16],
    #[bw(map = |s: &Vec<u16>| s.clone())]
    #[br(parse_with = read_utf16le_null_terminated)]
    pub hostname: Vec<u16>,
}

/// Init event request — sent on event channel with session_id from InitCmdAck
#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub struct InitEventRequest {
    pub session_id: u32,
}

/// Command request
#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub struct CmdRequest {
    pub data_phase: u32,
    pub opcode: u16,
    pub transaction_id: u32,
    // Parameters are appended as raw u32s after these fields
}

/// Command response
#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub struct CmdResponse {
    pub response_code: u16,
    pub transaction_id: u32,
    // Optional parameters follow
}

/// Data start header
#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub struct StartDataHeader {
    pub transaction_id: u32,
    pub total_length: u64,
}

/// Read a UTF-16LE null-terminated string from a binrw reader
#[binrw::parser(reader)]
fn read_utf16le_null_terminated() -> binrw::BinResult<Vec<u16>> {
    let mut result = Vec::new();
    loop {
        let ch = u16::read_le(reader)?;
        if ch == 0 {
            break;
        }
        result.push(ch);
    }
    Ok(result)
}

impl InitCmdRequest {
    pub fn new(guid: [u8; 16], hostname: &str) -> Self {
        let mut utf16: Vec<u16> = hostname.encode_utf16().collect();
        utf16.push(0); // null terminator
        Self {
            guid,
            hostname: utf16,
            version: 0x0001_0000, // version 1.0
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut cursor = Cursor::new(&mut buf);
        BinWrite::write_le(self, &mut cursor).expect("serialize InitCmdRequest");
        buf
    }
}

impl InitEventRequest {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut cursor = Cursor::new(&mut buf);
        BinWrite::write_le(self, &mut cursor).expect("serialize InitEventRequest");
        buf
    }
}

impl PtpIpFrame {
    /// Build a frame with type and payload
    pub fn new(packet_type: PacketType, payload: Vec<u8>) -> Self {
        Self {
            packet_type: packet_type as u32,
            payload,
        }
    }

    /// Serialize frame to wire format: [length(4)] [type(4)] [payload...]
    pub fn to_bytes(&self) -> Vec<u8> {
        let length = (8 + self.payload.len()) as u32;
        let mut buf = Vec::with_capacity(length as usize);
        buf.extend_from_slice(&length.to_le_bytes());
        buf.extend_from_slice(&self.packet_type.to_le_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Build a CmdRequest frame
    pub fn cmd_request(opcode: u16, transaction_id: u32, data_phase: u32, params: &[u32]) -> Self {
        let mut payload = Vec::with_capacity(10 + params.len() * 4);
        payload.extend_from_slice(&data_phase.to_le_bytes());
        payload.extend_from_slice(&opcode.to_le_bytes());
        payload.extend_from_slice(&transaction_id.to_le_bytes());
        for p in params {
            payload.extend_from_slice(&p.to_le_bytes());
        }
        Self::new(PacketType::CmdRequest, payload)
    }

    /// Build a Probe frame (empty payload)
    pub fn probe() -> Self {
        Self::new(PacketType::Probe, Vec::new())
    }

    /// Parse response code from a CmdResponse frame
    pub fn parse_cmd_response(&self) -> Option<(u16, u32, Vec<u32>)> {
        if self.payload.len() < 6 {
            return None;
        }
        let mut cursor = Cursor::new(&self.payload);
        let resp: CmdResponse = BinRead::read_le(&mut cursor).ok()?;
        let mut params = Vec::new();
        while cursor.position() + 4 <= self.payload.len() as u64 {
            let p = u32::read_le(&mut cursor).ok()?;
            params.push(p);
        }
        Some((resp.response_code, resp.transaction_id, params))
    }

    /// Parse StartData header
    pub fn parse_start_data(&self) -> Option<StartDataHeader> {
        let mut cursor = Cursor::new(&self.payload);
        StartDataHeader::read_le(&mut cursor).ok()
    }
}

/// Codec for PTP-IP length-prefixed framing over TCP
pub struct PtpIpCodec;

impl Decoder for PtpIpCodec {
    type Item = PtpIpFrame;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Need at least 4 bytes for the length prefix
        if src.len() < 4 {
            return Ok(None);
        }

        let length = u32::from_le_bytes([src[0], src[1], src[2], src[3]]) as usize;

        if length < 8 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("frame length too small: {length}"),
            ));
        }

        if src.len() < length {
            src.reserve(length - src.len());
            return Ok(None);
        }

        src.advance(4); // skip length
        let packet_type = src.get_u32_le();
        let payload = src.split_to(length - 8).to_vec();

        Ok(Some(PtpIpFrame {
            packet_type,
            payload,
        }))
    }
}

impl Encoder<PtpIpFrame> for PtpIpCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: PtpIpFrame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let length = (8 + item.payload.len()) as u32;
        dst.reserve(length as usize);
        dst.put_u32_le(length);
        dst.put_u32_le(item.packet_type);
        dst.extend_from_slice(&item.payload);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_roundtrip() {
        let frame = PtpIpFrame::new(PacketType::Probe, Vec::new());
        let bytes = frame.to_bytes();

        assert_eq!(bytes.len(), 8);
        assert_eq!(
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            8
        );
        assert_eq!(
            u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            PacketType::Probe as u32,
        );
    }

    #[test]
    fn test_cmd_request_frame() {
        let frame = PtpIpFrame::cmd_request(0x1002, 1, 1, &[1]); // OpenSession
        let bytes = frame.to_bytes();

        // length(4) + type(4) + data_phase(4) + opcode(2) + txn_id(4) + param(4) = 22
        assert_eq!(bytes.len(), 22);
    }

    #[test]
    fn test_codec_decode() {
        let frame = PtpIpFrame::new(
            PacketType::CmdResponse,
            vec![0x01, 0x20, 0x01, 0x00, 0x00, 0x00],
        );
        let wire = frame.to_bytes();

        let mut codec = PtpIpCodec;
        let mut buf = BytesMut::from(&wire[..]);

        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded.packet_type, PacketType::CmdResponse as u32);
        assert_eq!(decoded.payload, vec![0x01, 0x20, 0x01, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_init_cmd_request_serialize() {
        let req = InitCmdRequest::new([0xAA; 16], "ptpull");
        let bytes = req.to_bytes();

        // 16 (guid) + 7*2 (hostname "ptpull\0" in UTF-16LE) + 4 (version) = 34
        assert_eq!(bytes.len(), 34);
        assert_eq!(&bytes[0..16], &[0xAA; 16]);
    }
}
