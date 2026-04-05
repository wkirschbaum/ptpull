use tracing::{debug, trace};

use crate::protocol::mtp::{OpCode, ResponseCode};
use crate::protocol::ptp_ip::{PacketType, PtpIpFrame};
use crate::transport::connection::{ConnectionError, PtpIpConnection};

/// Manages an MTP session over a PTP-IP connection
pub struct MtpSession {
    conn: PtpIpConnection,
    transaction_id: u32,
    session_open: bool,
}

#[derive(Debug)]
pub struct MtpResponse {
    pub code: ResponseCode,
    pub transaction_id: u32,
    pub params: Vec<u32>,
    pub data: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("connection error: {0}")]
    Connection(#[from] ConnectionError),
    #[error("MTP error: {0:?}")]
    Mtp(ResponseCode),
    #[error("unexpected packet type during data transfer: 0x{0:04x}")]
    UnexpectedDataPacket(u32),
    #[error("session not open")]
    SessionNotOpen,
}

/// Data transfer direction
#[derive(Debug, Clone, Copy)]
pub enum DataPhase {
    NoData = 1,
    DataIn = 2, // camera -> host
}

impl MtpSession {
    pub fn new(conn: PtpIpConnection) -> Self {
        Self {
            conn,
            transaction_id: 0,
            session_open: false,
        }
    }

    fn next_transaction_id(&mut self) -> u32 {
        self.transaction_id += 1;
        self.transaction_id
    }

    /// Open an MTP session
    pub async fn open(&mut self) -> Result<(), SessionError> {
        let txn = self.next_transaction_id();
        let frame = PtpIpFrame::cmd_request(
            OpCode::OpenSession as u16,
            txn,
            DataPhase::NoData as u32,
            &[1], // session ID parameter
        );
        let resp = self.conn.send_recv(frame).await?;

        if let Some((code, _, _)) = resp.parse_cmd_response() {
            let rc = ResponseCode::from_u16(code);
            if rc.is_ok() || rc == ResponseCode::SessionAlreadyOpen {
                self.session_open = true;
                debug!("MTP session opened");
                return Ok(());
            }
            return Err(SessionError::Mtp(rc));
        }
        Err(SessionError::Connection(ConnectionError::NoResponse))
    }

    /// Close the MTP session
    pub async fn close(&mut self) -> Result<(), SessionError> {
        if !self.session_open {
            return Ok(());
        }
        let txn = self.next_transaction_id();
        let frame = PtpIpFrame::cmd_request(
            OpCode::CloseSession as u16,
            txn,
            DataPhase::NoData as u32,
            &[],
        );
        let _ = self.conn.send_recv(frame).await;
        self.session_open = false;
        debug!("MTP session closed");
        Ok(())
    }

    /// Execute an MTP operation that returns data
    pub async fn execute_data(
        &mut self,
        opcode: OpCode,
        params: &[u32],
    ) -> Result<MtpResponse, SessionError> {
        self.execute_inner(opcode, params, DataPhase::DataIn).await
    }

    /// Execute an MTP operation with no data phase
    pub async fn execute(
        &mut self,
        opcode: OpCode,
        params: &[u32],
    ) -> Result<MtpResponse, SessionError> {
        self.execute_inner(opcode, params, DataPhase::NoData).await
    }

    async fn execute_inner(
        &mut self,
        opcode: OpCode,
        params: &[u32],
        data_phase: DataPhase,
    ) -> Result<MtpResponse, SessionError> {
        if !self.session_open && opcode != OpCode::OpenSession {
            // GetDeviceInfo can be called without a session
            if opcode != OpCode::GetDeviceInfo {
                return Err(SessionError::SessionNotOpen);
            }
        }

        let txn = self.next_transaction_id();
        debug!("exec MTP op={opcode:?} txn={txn} params={params:?}");

        let frame = PtpIpFrame::cmd_request(opcode as u16, txn, data_phase as u32, params);
        self.conn.send(frame).await?;

        let mut data = Vec::new();

        // Read frames until we get a CmdResponse
        loop {
            let frame = self.conn.recv().await?;
            match PacketType::from_u32(frame.packet_type) {
                Some(PacketType::StartData) => {
                    if let Some(header) = frame.parse_start_data() {
                        trace!(
                            "data start: txn={} total_length={}",
                            header.transaction_id, header.total_length
                        );
                        if header.total_length > 0 && header.total_length < u64::MAX {
                            data.reserve(header.total_length as usize);
                        }
                    }
                }
                Some(PacketType::Data) => {
                    trace!("data chunk: {} bytes", frame.payload.len());
                    data.extend_from_slice(&frame.payload);
                }
                Some(PacketType::EndData) => {
                    trace!("data end: {} bytes", frame.payload.len());
                    data.extend_from_slice(&frame.payload);
                }
                Some(PacketType::CmdResponse) => {
                    if let Some((code, resp_txn, resp_params)) = frame.parse_cmd_response() {
                        let rc = ResponseCode::from_u16(code);
                        debug!("response: {rc:?} txn={resp_txn}");
                        return Ok(MtpResponse {
                            code: rc,
                            transaction_id: resp_txn,
                            params: resp_params,
                            data,
                        });
                    }
                    return Err(SessionError::Connection(ConnectionError::NoResponse));
                }
                _ => {
                    trace!("ignoring packet type 0x{:04x}", frame.packet_type);
                }
            }
        }
    }

    /// Execute a partial object download — used for chunked transfers with progress
    pub async fn get_partial_object(
        &mut self,
        handle: u32,
        offset: u32,
        max_bytes: u32,
    ) -> Result<MtpResponse, SessionError> {
        self.execute_data(OpCode::GetPartialObject, &[handle, offset, max_bytes])
            .await
    }

    pub fn is_open(&self) -> bool {
        self.session_open
    }

    pub fn connection(&self) -> &PtpIpConnection {
        &self.conn
    }
}
