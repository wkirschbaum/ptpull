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

## Tested cameras

- Sony DSC-RX10M4 (RX10 IV)

Should work with any camera exposing a UPnP/DLNA ContentDirectory service.

## License

MIT
