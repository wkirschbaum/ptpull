use std::net::Ipv4Addr;

use futures_util::sink::SinkExt;
use futures_util::stream::StreamExt;
use tokio::net::TcpStream;
use tokio_util::codec::Framed;
use tracing::{debug, info};
use uuid::Uuid;

use crate::protocol::ptp_ip::{
    InitCmdAck, InitCmdRequest, InitEventRequest, PTP_IP_PORT, PacketType, PtpIpCodec, PtpIpFrame,
};

/// A PTP-IP connection to a camera with command and event channels
pub struct PtpIpConnection {
    pub cmd: Framed<TcpStream, PtpIpCodec>,
    pub session_id: u32,
    camera_ip: Ipv4Addr,
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unexpected packet type: 0x{0:04x}")]
    UnexpectedPacket(u32),
    #[error("no response from camera")]
    NoResponse,
    #[error("parse error: {0}")]
    Parse(String),
}

impl PtpIpConnection {
    /// Connect to a camera and perform the PTP-IP handshake
    pub async fn connect(ip: Ipv4Addr, port: u16) -> Result<Self, ConnectionError> {
        info!("connecting to camera at {ip}:{port}");

        // Connect command channel
        let cmd_stream = TcpStream::connect((ip, port)).await?;
        cmd_stream.set_nodelay(true)?;
        let mut cmd = Framed::new(cmd_stream, PtpIpCodec);

        // Generate a random GUID for this session
        let guid = *Uuid::new_v4().as_bytes();

        // Send InitCmdRequest
        let init_req = InitCmdRequest::new(guid, "ptpull");
        let frame = PtpIpFrame::new(PacketType::InitCmdRequest, init_req.to_bytes());
        cmd.send(frame).await?;
        debug!("sent InitCmdRequest");

        // Receive InitCmdAck
        let ack_frame = cmd.next().await.ok_or(ConnectionError::NoResponse)??;
        if ack_frame.packet_type != PacketType::InitCmdAck as u32 {
            return Err(ConnectionError::UnexpectedPacket(ack_frame.packet_type));
        }

        let ack: InitCmdAck =
            binrw::BinRead::read_le(&mut std::io::Cursor::new(&ack_frame.payload))
                .map_err(|e| ConnectionError::Parse(format!("InitCmdAck: {e}")))?;

        let session_id = if ack.session_id == 0 {
            1
        } else {
            ack.session_id
        };
        info!("command channel established, session_id={session_id}");

        // Connect event channel
        let evt_stream = TcpStream::connect((ip, port)).await?;
        evt_stream.set_nodelay(true)?;
        let mut evt = Framed::new(evt_stream, PtpIpCodec);

        // Send InitEventRequest
        let evt_req = InitEventRequest { session_id };
        let frame = PtpIpFrame::new(PacketType::InitEventRequest, evt_req.to_bytes());
        evt.send(frame).await?;
        debug!("sent InitEventRequest");

        // Receive InitEventAck
        let evt_ack = evt.next().await.ok_or(ConnectionError::NoResponse)??;
        if evt_ack.packet_type != PacketType::InitEventAck as u32 {
            return Err(ConnectionError::UnexpectedPacket(evt_ack.packet_type));
        }
        info!("event channel established");

        // Send probe on command channel
        cmd.send(PtpIpFrame::probe()).await?;
        debug!("sent probe");

        // We keep the event channel alive but don't actively use it for now.
        // Drop it — cameras tolerate this. For realtime features, we'd keep it.
        drop(evt);

        Ok(Self {
            cmd,
            session_id,
            camera_ip: ip,
        })
    }

    /// Connect using default PTP-IP port
    pub async fn connect_default(ip: Ipv4Addr) -> Result<Self, ConnectionError> {
        Self::connect(ip, PTP_IP_PORT).await
    }

    /// Send a frame and receive the next frame
    pub async fn send_recv(&mut self, frame: PtpIpFrame) -> Result<PtpIpFrame, ConnectionError> {
        self.cmd.send(frame).await?;
        self.cmd
            .next()
            .await
            .ok_or(ConnectionError::NoResponse)?
            .map_err(ConnectionError::Io)
    }

    /// Receive the next frame
    pub async fn recv(&mut self) -> Result<PtpIpFrame, ConnectionError> {
        self.cmd
            .next()
            .await
            .ok_or(ConnectionError::NoResponse)?
            .map_err(ConnectionError::Io)
    }

    /// Send a frame without waiting for response
    pub async fn send(&mut self, frame: PtpIpFrame) -> Result<(), ConnectionError> {
        self.cmd.send(frame).await.map_err(ConnectionError::Io)
    }

    pub fn camera_ip(&self) -> Ipv4Addr {
        self.camera_ip
    }
}
