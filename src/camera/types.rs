use crate::protocol::mtp::{self, ObjectFormat, StorageType};

/// Camera device information from GetDeviceInfo
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub standard_version: u16,
    pub vendor_extension_id: u32,
    pub vendor_extension_version: u16,
    pub vendor_extension_desc: String,
    pub functional_mode: u16,
    pub operations_supported: Vec<u16>,
    pub events_supported: Vec<u16>,
    pub device_properties_supported: Vec<u16>,
    pub capture_formats: Vec<u16>,
    pub image_formats: Vec<u16>,
    pub manufacturer: String,
    pub model: String,
    pub device_version: String,
    pub serial_number: String,
}

impl DeviceInfo {
    /// Parse from MTP GetDeviceInfo response data
    pub fn parse(data: &[u8]) -> Option<Self> {
        let mut off = 0;
        let standard_version = mtp::read_u16_le(data, &mut off);
        let vendor_extension_id = mtp::read_u32_le(data, &mut off);
        let vendor_extension_version = mtp::read_u16_le(data, &mut off);
        let vendor_extension_desc = mtp::read_mtp_string(data, &mut off);
        let functional_mode = mtp::read_u16_le(data, &mut off);
        let operations_supported = mtp::read_mtp_u16_array(data, &mut off);
        let events_supported = mtp::read_mtp_u16_array(data, &mut off);
        let device_properties_supported = mtp::read_mtp_u16_array(data, &mut off);
        let capture_formats = mtp::read_mtp_u16_array(data, &mut off);
        let image_formats = mtp::read_mtp_u16_array(data, &mut off);
        let manufacturer = mtp::read_mtp_string(data, &mut off);
        let model = mtp::read_mtp_string(data, &mut off);
        let device_version = mtp::read_mtp_string(data, &mut off);
        let serial_number = mtp::read_mtp_string(data, &mut off);

        Some(Self {
            standard_version,
            vendor_extension_id,
            vendor_extension_version,
            vendor_extension_desc,
            functional_mode,
            operations_supported,
            events_supported,
            device_properties_supported,
            capture_formats,
            image_formats,
            manufacturer,
            model,
            device_version,
            serial_number,
        })
    }

    pub fn supports_partial_object(&self) -> bool {
        self.operations_supported
            .contains(&(mtp::OpCode::GetPartialObject as u16))
    }

    pub fn display_name(&self) -> String {
        if self.model.is_empty() {
            format!("{} Camera", self.manufacturer)
        } else {
            format!("{} {}", self.manufacturer, self.model)
        }
    }
}

/// Storage information from GetStorageInfo
#[derive(Debug, Clone)]
pub struct StorageInfo {
    pub storage_id: u32,
    pub storage_type: u16,
    pub filesystem_type: u16,
    pub access_capability: u16,
    pub max_capacity: u64,
    pub free_space: u64,
    pub free_objects: u32,
    pub description: String,
    pub volume_label: String,
}

impl StorageInfo {
    /// Parse from MTP GetStorageInfo response data
    pub fn parse(storage_id: u32, data: &[u8]) -> Option<Self> {
        let mut off = 0;
        let storage_type = mtp::read_u16_le(data, &mut off);
        let filesystem_type = mtp::read_u16_le(data, &mut off);
        let access_capability = mtp::read_u16_le(data, &mut off);
        let max_capacity = {
            let lo = mtp::read_u32_le(data, &mut off) as u64;
            let hi = mtp::read_u32_le(data, &mut off) as u64;
            (hi << 32) | lo
        };
        let free_space = {
            let lo = mtp::read_u32_le(data, &mut off) as u64;
            let hi = mtp::read_u32_le(data, &mut off) as u64;
            (hi << 32) | lo
        };
        let free_objects = mtp::read_u32_le(data, &mut off);
        let description = mtp::read_mtp_string(data, &mut off);
        let volume_label = mtp::read_mtp_string(data, &mut off);

        Some(Self {
            storage_id,
            storage_type,
            filesystem_type,
            access_capability,
            max_capacity,
            free_space,
            free_objects,
            description,
            volume_label,
        })
    }

    pub fn display_name(&self) -> String {
        if !self.volume_label.is_empty() {
            self.volume_label.clone()
        } else if !self.description.is_empty() {
            self.description.clone()
        } else {
            format!("Storage 0x{:08X}", self.storage_id)
        }
    }

    pub fn is_removable(&self) -> bool {
        self.storage_type == StorageType::RemovableRam as u16
            || self.storage_type == StorageType::RemovableRom as u16
    }
}

/// Object (file/folder) information from GetObjectInfo
#[derive(Debug, Clone)]
pub struct ObjectInfo {
    pub handle: u32,
    pub storage_id: u32,
    pub format: ObjectFormat,
    pub compressed_size: u32,
    pub thumb_format: u16,
    pub thumb_compressed_size: u32,
    pub thumb_width: u32,
    pub thumb_height: u32,
    pub image_width: u32,
    pub image_height: u32,
    pub image_bit_depth: u32,
    pub parent_object: u32,
    pub association_type: u16,
    pub association_desc: u32,
    pub sequence_number: u32,
    pub filename: String,
    pub capture_date: String,
    pub modification_date: String,
    pub keywords: String,
}

impl ObjectInfo {
    /// Parse from MTP GetObjectInfo response data
    pub fn parse(handle: u32, data: &[u8]) -> Option<Self> {
        let mut off = 0;
        let storage_id = mtp::read_u32_le(data, &mut off);
        let format_code = mtp::read_u16_le(data, &mut off);
        let _protection_status = mtp::read_u16_le(data, &mut off);
        let compressed_size = mtp::read_u32_le(data, &mut off);
        let thumb_format = mtp::read_u16_le(data, &mut off);
        let thumb_compressed_size = mtp::read_u32_le(data, &mut off);
        let thumb_width = mtp::read_u32_le(data, &mut off);
        let thumb_height = mtp::read_u32_le(data, &mut off);
        let image_width = mtp::read_u32_le(data, &mut off);
        let image_height = mtp::read_u32_le(data, &mut off);
        let image_bit_depth = mtp::read_u32_le(data, &mut off);
        let parent_object = mtp::read_u32_le(data, &mut off);
        let association_type = mtp::read_u16_le(data, &mut off);
        let association_desc = mtp::read_u32_le(data, &mut off);
        let sequence_number = mtp::read_u32_le(data, &mut off);
        let filename = mtp::read_mtp_string(data, &mut off);
        let capture_date = mtp::read_mtp_string(data, &mut off);
        let modification_date = mtp::read_mtp_string(data, &mut off);
        let keywords = mtp::read_mtp_string(data, &mut off);

        Some(Self {
            handle,
            storage_id,
            format: ObjectFormat::from_u16(format_code),
            compressed_size,
            thumb_format,
            thumb_compressed_size,
            thumb_width,
            thumb_height,
            image_width,
            image_height,
            image_bit_depth,
            parent_object,
            association_type,
            association_desc,
            sequence_number,
            filename,
            capture_date,
            modification_date,
            keywords,
        })
    }

    pub fn is_folder(&self) -> bool {
        self.format == ObjectFormat::Association
    }

    pub fn is_image(&self) -> bool {
        self.format.is_image()
    }

    pub fn is_video(&self) -> bool {
        self.format.is_video()
    }

    /// Human-readable file size
    pub fn size_display(&self) -> String {
        let size = self.compressed_size as f64;
        if size < 1024.0 {
            format!("{} B", self.compressed_size)
        } else if size < 1024.0 * 1024.0 {
            format!("{:.1} KB", size / 1024.0)
        } else if size < 1024.0 * 1024.0 * 1024.0 {
            format!("{:.1} MB", size / (1024.0 * 1024.0))
        } else {
            format!("{:.2} GB", size / (1024.0 * 1024.0 * 1024.0))
        }
    }
}

/// Info about a discovered and identified camera
#[derive(Debug, Clone)]
pub struct CameraInfo {
    pub ip: std::net::Ipv4Addr,
    pub port: u16,
    pub device_info: Option<DeviceInfo>,
}

impl CameraInfo {
    pub fn display_name(&self) -> String {
        if let Some(ref info) = self.device_info {
            info.display_name()
        } else {
            format!("Camera at {}", self.ip)
        }
    }
}
