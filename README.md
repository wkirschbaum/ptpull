# ptpull

Download photos from WiFi-enabled cameras via DLNA/UPnP.

Connects to a camera's WiFi, browses its media library, and downloads all files organized into date folders. Handles WiFi switching automatically and restores your connection when done, even on Ctrl+C.

## Install

```bash
cargo install --path .
```

## Usage

### Direct (already on camera WiFi)

```bash
ptpull --dlna http://192.168.122.1:64321 ~/Pictures/Camera
```

### With automatic WiFi switching

```bash
ptpull --ssid "DIRECT-xxxx:YourCamera" --password "YourPassword" \
       --dlna http://192.168.122.1:64321 ~/Pictures/Camera
```

This will:
1. Save your current WiFi connection
2. Connect to the camera's WiFi AP
3. Download all photos into `~/Pictures/Camera/YYYY-MM-DD/`
4. Skip files already downloaded (same name + size)
5. Restore your original WiFi when done

### Options

```
--dlna <URL>        DLNA base URL of the camera (required)
--ssid <SSID>       Camera WiFi SSID (enables auto WiFi switching)
-p, --password <PW> Camera WiFi password
-h, --help          Show help
```

### Convenience script

Create a `pull-photos.sh` with your camera's credentials (add to `.gitignore`):

```bash
#!/bin/bash
ptpull --ssid "DIRECT-xxxx:YourCamera" --password "secret" \
       --dlna http://192.168.122.1:64321 ~/Pictures/Camera
```

## Finding your camera's details

1. On your camera, enable WiFi / "Send to Smartphone" mode
2. Note the **SSID** and **password** shown on the camera screen
3. Connect to the camera WiFi manually once
4. Find the DLNA port with SSDP:
   ```bash
   echo -e 'M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: "ssdp:discover"\r\nMX: 3\r\nST: ssdp:all\r\n\r\n' \
     | socat - UDP4-DATAGRAM:239.255.255.250:1900,so-broadcast,ip-add-membership=239.255.255.250:0.0.0.0
   ```
5. Look for `LOCATION: http://<IP>:<PORT>/dd.xml` in the response — the `http://<IP>:<PORT>` part is your `--dlna` URL

## WiFi safety

- First Ctrl+C: restores WiFi and exits cleanly
- Second Ctrl+C: force quits immediately
- Panics/crashes: WiFi is restored via Drop handler
- The program always cleans up the temporary `ptpull-camera` network profile

## Tested cameras

- Sony DSC-RX10M4 (RX10 IV)

Should work with any camera exposing a UPnP/DLNA ContentDirectory service.

## License

MIT
