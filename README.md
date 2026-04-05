# ptpull

Transfer photos and videos from WiFi-enabled cameras to your computer via PTP/IP.

A modern Rust rewrite of [airmtp](https://github.com/shezi/airmtp) with a terminal UI.

## Features

- SSDP camera discovery on local network
- PTP/IP protocol (Sony, Canon, Nikon WiFi cameras)
- Terminal UI with file browser and download progress
- Chunked downloads via GetPartialObject for reliability
- Keyboard-driven: j/k navigation, space to select, enter to download

## Usage

```bash
# Download to current directory
ptpull

# Download to a specific directory
ptpull /path/to/photos
```

### Controls

| Key | Action |
|-----|--------|
| j/k or arrows | Navigate |
| Enter | Connect to camera / Start download |
| Space | Toggle file selection |
| a | Select/deselect all |
| r | Rescan for cameras |
| q / Esc | Back / Quit |
| Ctrl+C | Force quit |

## Supported Cameras

Any camera that supports MTP over WiFi (PTP-IP), including:
- Nikon (D series, Z series with WiFi)
- Canon (EOS with WiFi)
- Sony (Alpha series with WiFi)

## Building

```bash
cargo build --release
```

## Testing

```bash
# Unit tests (protocol parsing, SSDP, framing)
cargo test --lib

# Integration tests (mock camera server)
cargo test --test integration_test

# All tests
cargo test
```

The integration tests use a mock PTP-IP camera server that simulates the full
connection handshake, file enumeration, and download without needing a physical camera.

## License

MIT
