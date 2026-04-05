use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Result;
use ptpull::dlna::browse::{DlnaBrowser, format_bytes};
use ptpull::dlna::discovery;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut dest_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut dlna_url: Option<String> = None;
    let mut ssid: Option<String> = None;
    let mut password: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--dlna" => {
                i += 1;
                if i < args.len() {
                    dlna_url = Some(args[i].clone());
                }
            }
            "--ssid" => {
                i += 1;
                if i < args.len() {
                    ssid = Some(args[i].clone());
                }
            }
            "--password" | "-p" => {
                i += 1;
                if i < args.len() {
                    password = Some(args[i].clone());
                }
            }
            "--help" | "-h" => {
                eprintln!("Usage: ptpull --dlna <URL> [OPTIONS] [DEST_DIR]");
                eprintln!();
                eprintln!("Download photos from a camera via DLNA/UPnP.");
                eprintln!("Files are organized into date subfolders (YYYY-MM-DD).");
                eprintln!();
                eprintln!("Options:");
                eprintln!("  --dlna <URL>        DLNA base URL (e.g. http://192.168.122.1:64321)");
                eprintln!("  --ssid <SSID>       Connect to camera WiFi AP before pulling");
                eprintln!("  -p, --password <PW> WiFi password for the camera AP");
                eprintln!("  -h, --help          Show this help");
                std::process::exit(0);
            }
            other => {
                let path = if let Some(rest) = other.strip_prefix("~/") {
                    dirs::home_dir()
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join(rest)
                } else if other == "~" {
                    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
                } else {
                    PathBuf::from(other)
                };
                dest_dir = path;
            }
        }
        i += 1;
    }

    let url = dlna_url.unwrap_or_else(|| {
        eprintln!("Error: --dlna <URL> is required.");
        eprintln!("Example: ptpull --dlna http://192.168.122.1:64321 ~/Photos");
        std::process::exit(1);
    });

    // WiFi switching — Drop restores even on panic
    let mut wifi = None;
    if let Some(ref camera_ssid) = ssid {
        use ptpull::wifi::WifiManager;

        if !WifiManager::is_available() {
            eprintln!("Error: nmcli not found.");
            std::process::exit(1);
        }

        let mut wm = WifiManager::new();
        eprintln!("Connecting to {camera_ssid}...");
        if !wm.connect_to_camera(camera_ssid, password.as_deref()) {
            eprintln!("Failed to connect to {camera_ssid}");
            std::process::exit(1);
        }
        eprintln!("Connected.");
        wifi = Some(wm);
    }

    tokio::fs::create_dir_all(&dest_dir).await?;

    let result = run_pull(&url, &dest_dir).await;

    // Explicit restore (Drop is the safety net)
    if let Some(mut wm) = wifi {
        eprintln!("Restoring WiFi...");
        wm.restore_wifi();
    }

    result
}

async fn run_pull(base_url: &str, dest_dir: &Path) -> Result<()> {
    eprintln!("Connecting to {base_url}...");

    let device = discovery::discover_dlna(base_url).await?;
    eprintln!("Found: {}", device.display_name());

    let browser = DlnaBrowser::new(device);
    let files = browser.list_all_files().await?;
    eprintln!("Found {} files", files.len());

    if files.is_empty() {
        eprintln!("No files to download.");
        return Ok(());
    }

    let total_bytes: u64 = files
        .iter()
        .filter_map(|f| f.best_resource())
        .map(|r| r.size)
        .sum();
    let total_count = files.len();
    let started = Instant::now();
    let mut downloaded_bytes: u64 = 0;
    let mut downloaded_count: u64 = 0;
    let mut skipped_count: u64 = 0;

    for (idx, item) in files.iter().enumerate() {
        let name = item.filename();
        let date_folder = item.date_folder();

        let file_dest = dest_dir.join(&date_folder);
        tokio::fs::create_dir_all(&file_dest).await?;

        match browser.download(item, &file_dest).await {
            Ok(Some(_)) => {
                downloaded_count += 1;
                downloaded_bytes += item.best_resource().map(|r| r.size).unwrap_or(0);
            }
            Ok(None) => {
                skipped_count += 1;
            }
            Err(e) => {
                // Print error on its own line, then resume progress
                eprintln!("\rERROR {date_folder}/{name}: {e}                    ");
            }
        }

        // Single updating status line
        let elapsed = started.elapsed().as_secs_f64();
        let speed = if elapsed > 0.1 {
            downloaded_bytes as f64 / elapsed
        } else {
            0.0
        };
        let remaining = total_bytes.saturating_sub(downloaded_bytes);
        let eta = if speed > 0.0 {
            remaining as f64 / speed
        } else {
            0.0
        };
        eprint!(
            "\r[{}/{}] {}/{} {}/s ETA {:.0}s | {} done, {} skip   ",
            idx + 1,
            total_count,
            format_bytes(downloaded_bytes),
            format_bytes(total_bytes),
            format_bytes(speed as u64),
            eta,
            downloaded_count,
            skipped_count,
        );
    }

    let elapsed = started.elapsed().as_secs_f64();
    let speed = if elapsed > 0.1 {
        downloaded_bytes as f64 / elapsed
    } else {
        0.0
    };
    eprintln!();
    eprintln!(
        "Done! {} downloaded, {} skipped, {} in {:.0}s ({}/s)",
        downloaded_count,
        skipped_count,
        format_bytes(downloaded_bytes),
        elapsed,
        format_bytes(speed as u64),
    );

    Ok(())
}
