use std::process::Command;

/// WiFi connection manager using nmcli.
/// Restores original WiFi on Drop, even if the program panics.
pub struct WifiManager {
    original_ssid: Option<String>,
    connected_to_camera: bool,
}

impl Default for WifiManager {
    fn default() -> Self {
        Self::new()
    }
}

impl WifiManager {
    pub fn new() -> Self {
        Self {
            original_ssid: None,
            connected_to_camera: false,
        }
    }

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

    pub fn connect_to_camera(&mut self, camera_ssid: &str, password: Option<&str>) -> bool {
        self.original_ssid = Self::current_ssid();

        let con_name = "ptpull-camera";

        let _ = Command::new("nmcli")
            .args(["connection", "delete", con_name])
            .output();

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

        let ifname = Self::wifi_interface().unwrap_or_else(|| "wlan0".to_string());
        add_args.extend(["ifname", &ifname]);

        if let Some(pw) = password {
            add_args.extend(["wifi-sec.key-mgmt", "wpa-psk", "wifi-sec.psk", pw]);
        }

        let add_output = Command::new("nmcli").args(&add_args).output();
        match add_output {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                eprintln!(
                    "nmcli add failed: {}",
                    String::from_utf8_lossy(&o.stderr).trim()
                );
                return false;
            }
            Err(e) => {
                eprintln!("nmcli error: {e}");
                return false;
            }
        }

        let up_output = Command::new("nmcli")
            .args(["connection", "up", con_name])
            .output();

        match up_output {
            Ok(o) if o.status.success() => {
                self.connected_to_camera = true;
                std::thread::sleep(std::time::Duration::from_secs(2));
                true
            }
            Ok(o) => {
                eprintln!(
                    "nmcli up failed: {}",
                    String::from_utf8_lossy(&o.stderr).trim()
                );
                let _ = Command::new("nmcli")
                    .args(["connection", "delete", con_name])
                    .output();
                false
            }
            Err(e) => {
                eprintln!("nmcli error: {e}");
                false
            }
        }
    }

    pub fn restore_wifi(&mut self) -> bool {
        let _ = Command::new("nmcli")
            .args(["connection", "delete", "ptpull-camera"])
            .output();

        if let Some(ref ssid) = self.original_ssid {
            let success = Command::new("nmcli")
                .args(["connection", "up", ssid])
                .output()
                .is_ok_and(|o| o.status.success())
                || Command::new("nmcli")
                    .args(["dev", "wifi", "connect", ssid])
                    .output()
                    .is_ok_and(|o| o.status.success());

            self.connected_to_camera = !success;
            success
        } else {
            false
        }
    }

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
