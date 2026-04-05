use std::time::Duration;

use tracing::info;

use crate::camera::types::CameraInfo;
use crate::protocol::ssdp;

/// Default SSDP discovery timeout
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);

/// Discover cameras on the local network
pub async fn discover() -> anyhow::Result<Vec<CameraInfo>> {
    discover_with_timeout(DISCOVERY_TIMEOUT).await
}

/// Discover cameras with a custom timeout
pub async fn discover_with_timeout(timeout: Duration) -> anyhow::Result<Vec<CameraInfo>> {
    let discovered = ssdp::discover_cameras(timeout).await?;

    let cameras: Vec<CameraInfo> = discovered
        .into_iter()
        .map(|d| {
            info!("found camera: {}:{} ({})", d.ip, d.port, d.server);
            CameraInfo {
                ip: d.ip,
                port: d.port,
                device_info: None, // populated after connecting
            }
        })
        .collect();

    Ok(cameras)
}
