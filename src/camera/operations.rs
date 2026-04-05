use std::path::Path;

use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info};

use crate::camera::types::{CameraInfo, DeviceInfo, ObjectInfo, StorageInfo};
use crate::protocol::mtp::{self, OpCode, ResponseCode};
use crate::transport::connection::PtpIpConnection;
use crate::transport::session::{MtpSession, SessionError};

/// Chunk size for partial object downloads (1 MB)
const DOWNLOAD_CHUNK_SIZE: u32 = 1024 * 1024;

/// High-level camera operations
pub struct Camera {
    session: MtpSession,
    pub info: CameraInfo,
    pub device_info: Option<DeviceInfo>,
}

/// Progress callback for downloads
pub type ProgressFn = Box<dyn Fn(u64, u64) + Send>;

impl Camera {
    /// Connect to a camera and open an MTP session
    pub async fn connect(info: CameraInfo) -> Result<Self, SessionError> {
        let conn = PtpIpConnection::connect(info.ip, info.port).await?;
        let mut session = MtpSession::new(conn);
        session.open().await?;

        let mut camera = Self {
            session,
            info,
            device_info: None,
        };

        // Fetch device info
        match camera.get_device_info().await {
            Ok(di) => {
                info!("connected to: {}", di.display_name());
                camera.info.device_info = Some(di.clone());
                camera.device_info = Some(di);
            }
            Err(e) => {
                debug!("failed to get device info: {e}");
            }
        }

        Ok(camera)
    }

    /// Get device info
    pub async fn get_device_info(&mut self) -> Result<DeviceInfo, SessionError> {
        let resp = self
            .session
            .execute_data(OpCode::GetDeviceInfo, &[])
            .await?;
        if !resp.code.is_ok() {
            return Err(SessionError::Mtp(resp.code));
        }
        DeviceInfo::parse(&resp.data).ok_or(SessionError::Mtp(ResponseCode::GeneralError))
    }

    /// List available storage IDs
    pub async fn get_storage_ids(&mut self) -> Result<Vec<u32>, SessionError> {
        let resp = self
            .session
            .execute_data(OpCode::GetStorageIDs, &[])
            .await?;
        if !resp.code.is_ok() {
            return Err(SessionError::Mtp(resp.code));
        }
        let mut offset = 0;
        Ok(mtp::read_mtp_u32_array(&resp.data, &mut offset))
    }

    /// Get storage info for a specific storage ID
    pub async fn get_storage_info(&mut self, storage_id: u32) -> Result<StorageInfo, SessionError> {
        let resp = self
            .session
            .execute_data(OpCode::GetStorageInfo, &[storage_id])
            .await?;
        if !resp.code.is_ok() {
            return Err(SessionError::Mtp(resp.code));
        }
        StorageInfo::parse(storage_id, &resp.data)
            .ok_or(SessionError::Mtp(ResponseCode::GeneralError))
    }

    /// List all storages with their info
    pub async fn list_storages(&mut self) -> Result<Vec<StorageInfo>, SessionError> {
        let ids = self.get_storage_ids().await?;
        let mut storages = Vec::with_capacity(ids.len());
        for id in ids {
            match self.get_storage_info(id).await {
                Ok(info) => storages.push(info),
                Err(e) => debug!("skipping storage 0x{id:08X}: {e}"),
            }
        }
        Ok(storages)
    }

    /// Get object handles for a storage (all objects, all formats)
    pub async fn get_object_handles(&mut self, storage_id: u32) -> Result<Vec<u32>, SessionError> {
        // params: storage_id, format (0 = all), parent (0xFFFFFFFF = all)
        let resp = self
            .session
            .execute_data(OpCode::GetObjectHandles, &[storage_id, 0, 0xFFFFFFFF])
            .await?;
        if !resp.code.is_ok() {
            return Err(SessionError::Mtp(resp.code));
        }
        let mut offset = 0;
        Ok(mtp::read_mtp_u32_array(&resp.data, &mut offset))
    }

    /// Get info about a specific object
    pub async fn get_object_info(&mut self, handle: u32) -> Result<ObjectInfo, SessionError> {
        let resp = self
            .session
            .execute_data(OpCode::GetObjectInfo, &[handle])
            .await?;
        if !resp.code.is_ok() {
            return Err(SessionError::Mtp(resp.code));
        }
        ObjectInfo::parse(handle, &resp.data).ok_or(SessionError::Mtp(ResponseCode::GeneralError))
    }

    /// List all objects in a storage with their info
    pub async fn list_objects(&mut self, storage_id: u32) -> Result<Vec<ObjectInfo>, SessionError> {
        let handles = self.get_object_handles(storage_id).await?;
        info!(
            "found {} objects in storage 0x{storage_id:08X}",
            handles.len()
        );

        let mut objects = Vec::with_capacity(handles.len());
        for handle in handles {
            match self.get_object_info(handle).await {
                Ok(info) => objects.push(info),
                Err(e) => debug!("skipping object 0x{handle:08X}: {e}"),
            }
        }
        Ok(objects)
    }

    /// Download an object to disk using chunked partial object transfer
    pub async fn download_object(
        &mut self,
        obj: &ObjectInfo,
        dest_dir: &Path,
        progress: Option<ProgressFn>,
    ) -> Result<std::path::PathBuf, SessionError> {
        let dest_path = dest_dir.join(&obj.filename);
        let total_size = obj.compressed_size as u64;

        info!(
            "downloading {} ({}) to {}",
            obj.filename,
            obj.size_display(),
            dest_path.display()
        );

        let mut file = fs::File::create(&dest_path)
            .await
            .map_err(|e| SessionError::Connection(e.into()))?;

        let use_partial = self
            .device_info
            .as_ref()
            .is_some_and(|di| di.supports_partial_object());

        if use_partial && total_size > DOWNLOAD_CHUNK_SIZE as u64 {
            // Chunked download with GetPartialObject
            let mut downloaded: u64 = 0;
            while downloaded < total_size {
                let remaining = total_size - downloaded;
                let chunk_size = remaining.min(DOWNLOAD_CHUNK_SIZE as u64) as u32;

                let resp = self
                    .session
                    .get_partial_object(obj.handle, downloaded as u32, chunk_size)
                    .await?;

                if !resp.code.is_ok() {
                    return Err(SessionError::Mtp(resp.code));
                }

                file.write_all(&resp.data)
                    .await
                    .map_err(|e| SessionError::Connection(e.into()))?;

                downloaded += resp.data.len() as u64;

                if let Some(ref cb) = progress {
                    cb(downloaded, total_size);
                }
            }
        } else {
            // Single GetObject transfer
            let resp = self
                .session
                .execute_data(OpCode::GetObject, &[obj.handle])
                .await?;

            if !resp.code.is_ok() {
                return Err(SessionError::Mtp(resp.code));
            }

            file.write_all(&resp.data)
                .await
                .map_err(|e| SessionError::Connection(e.into()))?;

            if let Some(ref cb) = progress {
                cb(resp.data.len() as u64, total_size);
            }
        }

        file.flush()
            .await
            .map_err(|e| SessionError::Connection(e.into()))?;

        info!("downloaded {}", obj.filename);
        Ok(dest_path)
    }

    /// Close the session and disconnect
    pub async fn disconnect(mut self) -> Result<(), SessionError> {
        self.session.close().await
    }
}
