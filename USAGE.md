# Personal Usage Notes

This is a personal tool for pulling photos from my Sony DSC-RX10M4 camera over WiFi. Use with caution.

## How it works

The `pull-photos.sh` script (gitignored, contains WiFi credentials):

1. Disconnects from your current WiFi
2. Connects to the camera's WiFi access point
3. Downloads all photos via DLNA, organized into date folders
4. Restores your original WiFi connection

Your internet connection will be unavailable during the transfer.

## Setup

1. On the camera: **Menu > Send to Smartphone > Select on This Device** and select the photos to send
2. Run `./pull-photos.sh`
3. Photos land in `~/Pictures/Camera/YYYY-MM-DD/`

## Creating pull-photos.sh

The script is gitignored because it contains WiFi credentials. Create it manually:

```bash
#!/bin/bash
set -e
> ptpull.log

SSID="YOUR-CAMERA-SSID"
PASSWORD="YOUR-CAMERA-PASSWORD"
DEST="${1:-$HOME/Pictures/Camera}"
DLNA_BASE="http://192.168.122.1:64321"

ORIGINAL_WIFI=$(nmcli -t -f active,ssid dev wifi | grep '^yes:' | cut -d: -f2-)
echo "Current WiFi: $ORIGINAL_WIFI"

cleanup() {
    echo ""
    echo "Restoring WiFi to: $ORIGINAL_WIFI"
    nmcli connection delete ptpull-camera 2>/dev/null || true
    if [ -n "$ORIGINAL_WIFI" ]; then
        nmcli connection up "$ORIGINAL_WIFI" 2>/dev/null || \
        nmcli dev wifi connect "$ORIGINAL_WIFI" 2>/dev/null || true
    fi
    echo "Done."
}
trap cleanup EXIT

echo "Connecting to $SSID..."
nmcli connection delete ptpull-camera 2>/dev/null || true
IFACE=$(nmcli -t -f device,type dev | grep ':wifi$' | cut -d: -f1)
nmcli connection add type wifi con-name ptpull-camera \
    ifname "$IFACE" ssid "$SSID" \
    wifi-sec.key-mgmt wpa-psk wifi-sec.psk "$PASSWORD"
nmcli connection up ptpull-camera

echo "Connected! Waiting for network..."
sleep 3
mkdir -p "$DEST"

echo "Pulling photos via DLNA from $DLNA_BASE -> $DEST"
cargo run --quiet -- --dlna "$DLNA_BASE" "$DEST"
```

Make it executable: `chmod +x pull-photos.sh`

## Finding your camera's details

- **SSID**: Shown on camera screen when WiFi is enabled
- **Password**: Shown on camera screen below the SSID
- **DLNA port**: Check `ptpull.log` after a run, or use `socat` to listen for SSDP NOTIFY on the camera network — look for the `LOCATION` header
