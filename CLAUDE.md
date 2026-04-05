# CLAUDE.md

## Project Overview

**ptpull** — a Rust TUI tool for transferring photos/videos from WiFi-enabled cameras.

## Target Camera

**Sony DSC-RX10M4 (RX10 IV)**
- WiFi mode: Camera creates its own AP (SSID: `DIRECT-xxxx:DSC-RX10M4`)
- Protocol: Sony Camera Remote API (JSON-RPC over HTTP), NOT PTP-IP
- Camera IP when connected to its AP: `192.168.122.1:8080`
- API base: `http://192.168.122.1:8080/sony`
- Endpoints: `/sony/camera`, `/sony/avContent`, `/sony/system`
- Must call `setCameraFunction("Contents Transfer")` before browsing files
- File listing via `getContentList` with pagination (100 items per page)
- Downloads are plain HTTP GET to URLs from content list
- "Send to Computer" mode: camera joins existing WiFi, needs receiver service

## Architecture

Two protocol backends:
1. **PTP-IP** (`src/protocol/`, `src/transport/`, `src/camera/`) — for Nikon/Canon cameras
2. **Sony Camera Remote API** (`src/sony/`) — HTTP JSON-RPC for Sony cameras

TUI in `src/tui/` using ratatui with Elm architecture (app state, events, render).

## Commands

```bash
cargo build           # Build
cargo test            # All tests (unit + integration with mock camera)
cargo test --lib      # Unit tests only
cargo clippy          # Lint
cargo fmt             # Format

# Run
cargo run -- [dest_dir]              # With SSDP discovery
cargo run -- --ip 192.168.122.1      # Direct IP connection
```

## Testing

Integration tests use a mock PTP-IP camera server in `tests/mock_camera.rs`.
Sony API testing requires connecting to the camera's WiFi AP.

## Key Dependencies

- `ratatui` / `crossterm` — TUI
- `tokio` — async runtime
- `binrw` — PTP-IP binary protocol parsing
- `reqwest` / `serde_json` — Sony HTTP API
- `roxmltree` — Sony device description XML parsing
- `socket2` — SSDP multicast
