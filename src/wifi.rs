use std::process::Command;

use tracing::{debug, info, warn};

/// WiFi connection manager using nmcli
pub struct WifiManager {
    original_ssid: Option<String>,
    connected_to_camera: bool,
}

impl WifiManager {
    pub fn new() -> Self {
        Self {
            original_ssid: None,
            connected_to_camera: false,
        }
    }

    /// Get the currently connected WiFi SSID
    pub fn current_ssid() -> Option<String> {
        let output = Command::new("nmcli")
            .args(["-t", "-f", "active,ssid", "dev", "wifi"])
            .output()
            .ok()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some(ssid) = line.strip_prefix("yes:") {
                let ssid = ssid.trim();
                if !ssid.is_empty() {
                    return Some(ssid.to_string());
                }
            }
        }
        None
    }

    /// Scan for available SSIDs matching a pattern
    pub fn scan_for_camera() -> Vec<String> {
        // Trigger a rescan
        let _ = Command::new("nmcli")
            .args(["dev", "wifi", "rescan"])
            .output();

        // Brief pause for scan results
        std::thread::sleep(std::time::Duration::from_secs(2));

        let output = Command::new("nmcli")
            .args(["-t", "-f", "ssid", "dev", "wifi", "list"])
            .output();

        let output = match output {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout
            .lines()
            .map(|line| {
                // nmcli -t escapes colons as \: — unescape them
                line.trim().replace("\\:", ":")
            })
            .filter(|line| {
                !line.is_empty()
                    && (line.starts_with("DIRECT-")
                        || line.contains("DSC-")
                        || line.contains("ILCE-")
                        || line.contains("NIKON")
                        || line.contains("Canon")
                        || line.contains("EOS"))
            })
            .collect()
    }

    /// Save current SSID and connect to camera's WiFi
    pub fn connect_to_camera(&mut self, camera_ssid: &str, password: Option<&str>) -> bool {
        // Remember current network
        self.original_ssid = Self::current_ssid();
        info!(
            "saving current WiFi: {:?}",
            self.original_ssid.as_deref().unwrap_or("none")
        );

        // Connect to camera
        info!("connecting to camera WiFi: {camera_ssid}");
        let con_name = "ptpull-camera";

        // Remove any stale ptpull-camera profile
        let _ = Command::new("nmcli")
            .args(["connection", "delete", con_name])
            .output();

        // Create a connection profile — nmcli "dev wifi connect" fails
        // on first-time SSIDs without a saved profile, so we create one
        let mut add_args = vec![
            "connection",
            "add",
            "type",
            "wifi",
            "con-name",
            con_name,
            "ssid",
            camera_ssid,
        ];

        // Detect WiFi interface name
        let ifname = Self::wifi_interface().unwrap_or_else(|| "wlan0".to_string());
        add_args.extend(["ifname", &ifname]);

        if let Some(pw) = password {
            add_args.extend(["wifi-sec.key-mgmt", "wpa-psk", "wifi-sec.psk", pw]);
        }

        let add_output = Command::new("nmcli").args(&add_args).output();
        match add_output {
            Ok(o) if o.status.success() => {
                debug!("created connection profile {con_name}");
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                warn!("failed to create connection profile: {stderr}");
                return false;
            }
            Err(e) => {
                warn!("nmcli error: {e}");
                return false;
            }
        }

        // Activate the connection
        let up_output = Command::new("nmcli")
            .args(["connection", "up", con_name])
            .output();

        match up_output {
            Ok(o) if o.status.success() => {
                self.connected_to_camera = true;
                info!("connected to {camera_ssid}");

                // Wait for connection to stabilize
                std::thread::sleep(std::time::Duration::from_secs(2));
                true
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                warn!("failed to activate connection: {stderr}");
                // Clean up the profile we created
                let _ = Command::new("nmcli")
                    .args(["connection", "delete", con_name])
                    .output();
                false
            }
            Err(e) => {
                warn!("nmcli error: {e}");
                false
            }
        }
    }

    /// Reconnect to the original WiFi network
    pub fn restore_wifi(&mut self) -> bool {
        // Clean up the camera connection profile
        let _ = Command::new("nmcli")
            .args(["connection", "delete", "ptpull-camera"])
            .output();

        if let Some(ref ssid) = self.original_ssid {
            info!("restoring WiFi to: {ssid}");
            // Use connection up with the SSID name — works for saved connections
            let output = Command::new("nmcli")
                .args(["connection", "up", ssid])
                .output();

            let success = match output {
                Ok(o) if o.status.success() => true,
                _ => {
                    // Fallback: try dev wifi connect
                    Command::new("nmcli")
                        .args(["dev", "wifi", "connect", ssid])
                        .output()
                        .is_ok_and(|o| o.status.success())
                }
            };

            if success {
                info!("restored WiFi to {ssid}");
                self.connected_to_camera = false;
                true
            } else {
                warn!("failed to restore WiFi to {ssid}");
                false
            }
        } else {
            debug!("no original SSID to restore");
            false
        }
    }

    /// Detect the WiFi interface name
    fn wifi_interface() -> Option<String> {
        let output = Command::new("nmcli")
            .args(["-t", "-f", "device,type", "dev"])
            .output()
            .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.ends_with(":wifi") {
                return line.strip_suffix(":wifi").map(|s| s.to_string());
            }
        }
        None
    }

    /// Check if nmcli is available
    pub fn is_available() -> bool {
        Command::new("nmcli")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }
}

impl Drop for WifiManager {
    fn drop(&mut self) {
        if self.connected_to_camera {
            self.restore_wifi();
        }
    }
}
