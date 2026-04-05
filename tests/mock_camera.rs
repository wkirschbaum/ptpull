//! Mock PTP-IP camera server for integration testing.
//!
//! Simulates a camera that:
//! - Accepts PTP-IP connections on a random port
//! - Responds to the handshake (InitCmd, InitEvent, Probe)
//! - Responds to MTP operations (OpenSession, GetDeviceInfo, GetStorageIDs,
//!   GetStorageInfo, GetObjectHandles, GetObjectInfo, GetPartialObject, CloseSession)
//! - Serves a fake JPEG file for download

use std::net::SocketAddr;

use futures_util::sink::SinkExt;
use futures_util::stream::StreamExt;
use tokio::net::TcpListener;
use tokio_util::codec::Framed;

use ptpull::protocol::mtp::OpCode;
use ptpull::protocol::ptp_ip::{PacketType, PtpIpCodec, PtpIpFrame};

/// Fake file served by the mock camera
const FAKE_FILENAME: &str = "DSC_0001.JPG";
const FAKE_FILE_SIZE: u32 = 4096;

/// Start a mock camera server and return its address
pub async fn start_mock_camera() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        // Accept command channel
        let (cmd_stream, _) = listener.accept().await.unwrap();
        cmd_stream.set_nodelay(true).unwrap();
        let mut cmd = Framed::new(cmd_stream, PtpIpCodec);

        // Handle InitCmdRequest -> respond with InitCmdAck
        let init_frame = cmd.next().await.unwrap().unwrap();
        assert_eq!(init_frame.packet_type, PacketType::InitCmdRequest as u32);

        let ack_payload = build_init_cmd_ack(1);
        let ack = PtpIpFrame::new(PacketType::InitCmdAck, ack_payload);
        cmd.send(ack).await.unwrap();

        // Accept event channel
        let (evt_stream, _) = listener.accept().await.unwrap();
        evt_stream.set_nodelay(true).unwrap();
        let mut evt = Framed::new(evt_stream, PtpIpCodec);

        // Handle InitEventRequest -> respond with InitEventAck
        let evt_frame = evt.next().await.unwrap().unwrap();
        assert_eq!(evt_frame.packet_type, PacketType::InitEventRequest as u32);
        let evt_ack = PtpIpFrame::new(PacketType::InitEventAck, Vec::new());
        evt.send(evt_ack).await.unwrap();

        // Handle Probe (just consume it)
        let probe = cmd.next().await.unwrap().unwrap();
        assert_eq!(probe.packet_type, PacketType::Probe as u32);

        // Now handle MTP operations in a loop
        loop {
            let frame = match cmd.next().await {
                Some(Ok(f)) => f,
                _ => break,
            };

            if frame.packet_type != PacketType::CmdRequest as u32 {
                continue;
            }

            if frame.payload.len() < 10 {
                continue;
            }

            // Parse: data_phase(4) + opcode(2) + transaction_id(4)
            let opcode = u16::from_le_bytes([frame.payload[4], frame.payload[5]]);
            let txn_id = u32::from_le_bytes([
                frame.payload[6],
                frame.payload[7],
                frame.payload[8],
                frame.payload[9],
            ]);

            match opcode {
                op if op == OpCode::OpenSession as u16 => {
                    send_response(&mut cmd, 0x2001, txn_id, &[]).await;
                }
                op if op == OpCode::GetDeviceInfo as u16 => {
                    let data = build_device_info();
                    send_data_response(&mut cmd, txn_id, &data).await;
                }
                op if op == OpCode::GetStorageIDs as u16 => {
                    let data = build_u32_array(&[0x00010001]);
                    send_data_response(&mut cmd, txn_id, &data).await;
                }
                op if op == OpCode::GetStorageInfo as u16 => {
                    let data = build_storage_info();
                    send_data_response(&mut cmd, txn_id, &data).await;
                }
                op if op == OpCode::GetObjectHandles as u16 => {
                    let data = build_u32_array(&[1]); // one object with handle=1
                    send_data_response(&mut cmd, txn_id, &data).await;
                }
                op if op == OpCode::GetObjectInfo as u16 => {
                    let data = build_object_info();
                    send_data_response(&mut cmd, txn_id, &data).await;
                }
                op if op == OpCode::GetPartialObject as u16 => {
                    // Parse offset and max_bytes from params
                    let offset = if frame.payload.len() >= 14 {
                        u32::from_le_bytes([
                            frame.payload[10],
                            frame.payload[11],
                            frame.payload[12],
                            frame.payload[13],
                        ])
                    } else {
                        0
                    };
                    let max_bytes = if frame.payload.len() >= 18 {
                        u32::from_le_bytes([
                            frame.payload[14],
                            frame.payload[15],
                            frame.payload[16],
                            frame.payload[17],
                        ])
                    } else {
                        FAKE_FILE_SIZE
                    };

                    let remaining = FAKE_FILE_SIZE.saturating_sub(offset);
                    let chunk_size = remaining.min(max_bytes) as usize;
                    let data: Vec<u8> = (0..chunk_size)
                        .map(|i| ((offset as usize + i) % 256) as u8)
                        .collect();
                    send_data_response(&mut cmd, txn_id, &data).await;
                }
                op if op == OpCode::GetObject as u16 => {
                    let data: Vec<u8> = (0..FAKE_FILE_SIZE as usize)
                        .map(|i| (i % 256) as u8)
                        .collect();
                    send_data_response(&mut cmd, txn_id, &data).await;
                }
                op if op == OpCode::CloseSession as u16 => {
                    send_response(&mut cmd, 0x2001, txn_id, &[]).await;
                    break;
                }
                _ => {
                    // OperationNotSupported
                    send_response(&mut cmd, 0x2005, txn_id, &[]).await;
                }
            }
        }
    });

    addr
}

async fn send_response(
    cmd: &mut Framed<tokio::net::TcpStream, PtpIpCodec>,
    response_code: u16,
    txn_id: u32,
    params: &[u32],
) {
    let mut payload = Vec::new();
    payload.extend_from_slice(&response_code.to_le_bytes());
    payload.extend_from_slice(&txn_id.to_le_bytes());
    for p in params {
        payload.extend_from_slice(&p.to_le_bytes());
    }
    let frame = PtpIpFrame::new(PacketType::CmdResponse, payload);
    cmd.send(frame).await.unwrap();
}

async fn send_data_response(
    cmd: &mut Framed<tokio::net::TcpStream, PtpIpCodec>,
    txn_id: u32,
    data: &[u8],
) {
    // StartData
    let mut start_payload = Vec::new();
    start_payload.extend_from_slice(&txn_id.to_le_bytes());
    start_payload.extend_from_slice(&(data.len() as u64).to_le_bytes());
    let start = PtpIpFrame::new(PacketType::StartData, start_payload);
    cmd.send(start).await.unwrap();

    // EndData (send all data in one chunk)
    let end = PtpIpFrame::new(PacketType::EndData, data.to_vec());
    cmd.send(end).await.unwrap();

    // CmdResponse OK
    send_response(cmd, 0x2001, txn_id, &[]).await;
}

fn build_init_cmd_ack(session_id: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&session_id.to_le_bytes());
    buf.extend_from_slice(&[0u8; 16]); // camera GUID
    // hostname "MockCam\0" in UTF-16LE
    for ch in "MockCam\0".encode_utf16() {
        buf.extend_from_slice(&ch.to_le_bytes());
    }
    buf
}

/// Build a minimal DeviceInfo dataset
fn build_device_info() -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&100u16.to_le_bytes()); // standard version
    buf.extend_from_slice(&0u32.to_le_bytes()); // vendor extension ID
    buf.extend_from_slice(&0u16.to_le_bytes()); // vendor extension version
    write_mtp_string(&mut buf, ""); // vendor extension desc
    buf.extend_from_slice(&0u16.to_le_bytes()); // functional mode

    // Operations supported
    let ops: Vec<u16> = vec![
        OpCode::GetDeviceInfo as u16,
        OpCode::OpenSession as u16,
        OpCode::CloseSession as u16,
        OpCode::GetStorageIDs as u16,
        OpCode::GetStorageInfo as u16,
        OpCode::GetObjectHandles as u16,
        OpCode::GetObjectInfo as u16,
        OpCode::GetObject as u16,
        OpCode::GetPartialObject as u16,
    ];
    write_u16_array(&mut buf, &ops);
    write_u16_array(&mut buf, &[]); // events supported
    write_u16_array(&mut buf, &[]); // device properties
    write_u16_array(&mut buf, &[0x3801]); // capture formats (JPEG)
    write_u16_array(&mut buf, &[0x3801]); // image formats (JPEG)

    write_mtp_string(&mut buf, "MockCorp"); // manufacturer
    write_mtp_string(&mut buf, "MockCam X100"); // model
    write_mtp_string(&mut buf, "1.0.0"); // device version
    write_mtp_string(&mut buf, "MOCK-001"); // serial number

    buf
}

fn build_storage_info() -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&0x0004u16.to_le_bytes()); // RemovableRam
    buf.extend_from_slice(&0x0002u16.to_le_bytes()); // filesystem type (generic hierarchical)
    buf.extend_from_slice(&0x0000u16.to_le_bytes()); // read-write access
    // max capacity (64-bit as two u32s: low, high)
    buf.extend_from_slice(
        &(32u64 * 1024 * 1024 * 1024).to_le_bytes()[..4]
            .to_vec()
            .as_slice(),
    );
    buf.extend_from_slice(
        &(32u64 * 1024 * 1024 * 1024).to_le_bytes()[4..]
            .to_vec()
            .as_slice(),
    );
    // free space
    buf.extend_from_slice(
        &(16u64 * 1024 * 1024 * 1024).to_le_bytes()[..4]
            .to_vec()
            .as_slice(),
    );
    buf.extend_from_slice(
        &(16u64 * 1024 * 1024 * 1024).to_le_bytes()[4..]
            .to_vec()
            .as_slice(),
    );
    buf.extend_from_slice(&100u32.to_le_bytes()); // free objects
    write_mtp_string(&mut buf, "SD Card"); // description
    write_mtp_string(&mut buf, "MOCK_SD"); // volume label
    buf
}

fn build_object_info() -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&0x00010001u32.to_le_bytes()); // storage ID
    buf.extend_from_slice(&0x3801u16.to_le_bytes()); // format: JPEG
    buf.extend_from_slice(&0u16.to_le_bytes()); // protection status
    buf.extend_from_slice(&FAKE_FILE_SIZE.to_le_bytes()); // compressed size
    buf.extend_from_slice(&0u16.to_le_bytes()); // thumb format
    buf.extend_from_slice(&0u32.to_le_bytes()); // thumb compressed size
    buf.extend_from_slice(&0u32.to_le_bytes()); // thumb width
    buf.extend_from_slice(&0u32.to_le_bytes()); // thumb height
    buf.extend_from_slice(&4000u32.to_le_bytes()); // image width
    buf.extend_from_slice(&3000u32.to_le_bytes()); // image height
    buf.extend_from_slice(&24u32.to_le_bytes()); // bit depth
    buf.extend_from_slice(&0u32.to_le_bytes()); // parent object
    buf.extend_from_slice(&0u16.to_le_bytes()); // association type
    buf.extend_from_slice(&0u32.to_le_bytes()); // association desc
    buf.extend_from_slice(&1u32.to_le_bytes()); // sequence number
    write_mtp_string(&mut buf, FAKE_FILENAME); // filename
    write_mtp_string(&mut buf, "20260401T120000"); // capture date
    write_mtp_string(&mut buf, "20260401T120000"); // modification date
    write_mtp_string(&mut buf, ""); // keywords
    buf
}

fn build_u32_array(items: &[u32]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&(items.len() as u32).to_le_bytes());
    for item in items {
        buf.extend_from_slice(&item.to_le_bytes());
    }
    buf
}

fn write_mtp_string(buf: &mut Vec<u8>, s: &str) {
    if s.is_empty() {
        buf.push(0); // zero-length string
        return;
    }
    let utf16: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
    buf.push(utf16.len() as u8); // char count including null
    for ch in &utf16 {
        buf.extend_from_slice(&ch.to_le_bytes());
    }
}

fn write_u16_array(buf: &mut Vec<u8>, items: &[u16]) {
    buf.extend_from_slice(&(items.len() as u32).to_le_bytes());
    for item in items {
        buf.extend_from_slice(&item.to_le_bytes());
    }
}
