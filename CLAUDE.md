# CLAUDE.md

## Project Overview

**ptpull** — a Rust CLI tool for downloading photos/videos from WiFi cameras via DLNA/UPnP.

## Target Camera

**Sony DSC-RX10M4 (RX10 IV)**
- WiFi mode: Camera creates its own AP (SSID: `DIRECT-sBC3:DSC-RX10M4`)
- Protocol: DLNA/UPnP ContentDirectory (SOAP/XML over HTTP)
- Camera IP on its AP: `192.168.122.1`
- DLNA port: `64321` (discovered via SSDP NOTIFY)
- Device description: `http://192.168.122.1:64321/dd.xml`
- ContentDirectory control: `/upnp/control/ContentDirectory`
- File downloads: plain HTTP GET to URLs from DIDL-Lite `<res>` elements
- Camera must be in "Send to Smartphone" mode with images selected

## Architecture

```
src/
  lib.rs              # exports dlna + wifi
  main.rs             # CLI entry point, headless DLNA pull
  dlna/
    discovery.rs      # fetch dd.xml, parse control URL
    browse.rs         # SOAP Browse, DIDL-Lite parsing, file download
  wifi.rs             # nmcli WiFi switching (save/restore)
```

## Commands

```bash
cargo build           # Build
cargo test            # Unit tests
cargo clippy          # Lint
cargo fmt             # Format

# Run directly
cargo run -- --dlna http://192.168.122.1:64321 ~/Pictures

# Via shell script (handles WiFi switching)
./pull-photos.sh [dest_dir]
```

## Key Dependencies

- `tokio` — async runtime
- `reqwest` — HTTP client (SOAP requests + file downloads)
- `roxmltree` — XML parsing (dd.xml + DIDL-Lite)
- `serde` / `serde_json` — serialization
- `tracing` — logging to ptpull.log
- `dirs` — home directory expansion
