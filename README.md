# ptpull

Transfer photos and videos from WiFi-enabled cameras via DLNA/UPnP.

Inspired by [airmtp](https://github.com/shezi/airmtp). Connects to a camera's WiFi AP, browses its DLNA ContentDirectory, and downloads all files.

## Usage

```bash
# Download from camera at known DLNA endpoint
ptpull --dlna http://192.168.122.1:64321 ~/Photos
```

### With automatic WiFi switching

Create a `pull-photos.sh` script (gitignored) that connects to the camera's WiFi, pulls photos, and restores your original WiFi:

```bash
./pull-photos.sh              # downloads to ~/Pictures
./pull-photos.sh ~/Photos     # custom destination
```

### Options

```
ptpull [OPTIONS] [DEST_DIR]

Options:
  --dlna <URL>       DLNA base URL of the camera
  --password <PW>    WiFi password for camera AP
  --pull             Headless mode (default)
  -h, --help         Show help
```

## Tested Cameras

- Sony DSC-RX10M4 (RX10 IV) — DLNA MediaServer on port 64321

Should work with any camera that exposes a UPnP/DLNA ContentDirectory service over WiFi.

## How it works

1. Connects to the camera's WiFi AP (via `nmcli` in the shell script)
2. Fetches the DLNA device description XML (`/dd.xml`)
3. Browses the ContentDirectory via SOAP to enumerate all photos
4. Downloads each file via plain HTTP GET
5. Restores the original WiFi connection

## Building

```bash
cargo build --release
```

## License

MIT
