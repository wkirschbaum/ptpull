# ptpull

Download photos from WiFi-enabled cameras via DLNA/UPnP.

Connects to a camera's WiFi, browses its media library, and downloads all files organized into date folders. Handles WiFi switching automatically and restores your connection when done, even on Ctrl+C.

## How it works

The camera acts as a WiFi Direct access point and runs a DLNA/UPnP MediaServer. ptpull uses:

1. **SSDP** — discovers the camera's DLNA service (one-time setup)
2. **UPnP Device Description** — fetches `dd.xml` to find the ContentDirectory control URL
3. **SOAP/XML** — sends `Browse` requests to the ContentDirectory service to enumerate files
4. **HTTP GET** — downloads photos from URLs provided in the DIDL-Lite browse results

No proprietary protocols. No app pairing. Just standard UPnP/DLNA over plain HTTP.

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

## Camera setup

### Before each transfer

1. Turn on your camera
2. Go to **Menu > Network** (or wireless icon)
3. Select **Send to Smartphone**
4. Choose **Select on This Device**
5. Select the photos you want to transfer (or select all)
6. The camera will show "Connecting..." and display its **SSID** and **password**
7. Run `ptpull` (or your `pull-photos.sh` script)

The camera's WiFi stays active until you cancel on the camera or it times out.

### First-time setup

You need to find your camera's DLNA URL once:

1. On the camera, start **Send to Smartphone** as above
2. On your computer, connect to the camera's WiFi manually (note the SSID and password)
3. Find the DLNA port using SSDP:
   ```bash
   echo -e 'M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: "ssdp:discover"\r\nMX: 3\r\nST: ssdp:all\r\n\r\n' \
     | socat - UDP4-DATAGRAM:239.255.255.250:1900,so-broadcast,ip-add-membership=239.255.255.250:0.0.0.0
   ```
4. Look for `LOCATION: http://<IP>:<PORT>/dd.xml` in the response
5. The `http://<IP>:<PORT>` part is your `--dlna` URL
6. Create your `pull-photos.sh` script with the SSID, password, and DLNA URL

## WiFi safety

- First Ctrl+C: restores WiFi and exits cleanly
- Second Ctrl+C: force quits immediately
- Panics/crashes: WiFi is restored via Drop handler
- The program always cleans up the temporary `ptpull-camera` network profile

## Performance

Tested with a Sony RX10 IV over WiFi Direct, downloading 14 JPEGs (7-17 MB each, ~156 MB total):

- **~4 MB/s** sustained throughput
- **~2-3 seconds** per file
- **0ms gap** between files (HTTP connection reuse)
- **~40 seconds** total for 14 files

The bottleneck is the camera's WiFi Direct radio, not the software. For comparison:

| Method | Speed | 10 MB photo |
|---|---|---|
| WiFi Direct (ptpull) | ~4 MB/s | ~2.5s |
| USB cable | ~30 MB/s | ~0.3s |
| SD card reader | ~90 MB/s | ~0.1s |

The software side is optimised with TCP_NODELAY, connection keep-alive, 256 KB buffered writes, and HTTP connection pooling. There is zero idle time between file downloads — the next request starts as soon as the previous file finishes writing.

WiFi is the convenient option when you don't want to remove the SD card. For bulk transfers of hundreds of photos, a card reader is faster.

## Tested cameras

- Sony DSC-RX10M4 (RX10 IV)

Should work with any camera exposing a UPnP/DLNA ContentDirectory service.

## License

MIT
