use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tracing::{debug, info, warn};

const SSDP_MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const SSDP_PORT: u16 = 1900;

/// Known camera SSDP service types
const SEARCH_TARGETS: &[&str] = &[
    "urn:microsoft-com:service:MtpNullService:1", // Sony, generic MTP
    "urn:schemas-canon-com:service:ICPO-SmartPhoneEOSSystemService:1", // Canon
];

/// A camera discovered via SSDP
#[derive(Debug, Clone)]
pub struct DiscoveredCamera {
    pub ip: Ipv4Addr,
    pub port: u16,
    pub location: String,
    pub server: String,
    pub usn: String,
}

/// Discover cameras on the local network via SSDP M-SEARCH
pub async fn discover_cameras(timeout: Duration) -> anyhow::Result<Vec<DiscoveredCamera>> {
    let mut cameras = Vec::new();

    for target in SEARCH_TARGETS {
        match search_for_target(target, timeout).await {
            Ok(found) => cameras.extend(found),
            Err(e) => warn!("SSDP search for {target} failed: {e}"),
        }
    }

    // Deduplicate by IP
    cameras.sort_by(|a, b| a.ip.cmp(&b.ip));
    cameras.dedup_by(|a, b| a.ip == b.ip);

    info!("discovered {} camera(s)", cameras.len());
    Ok(cameras)
}

async fn search_for_target(
    target: &str,
    timeout: Duration,
) -> anyhow::Result<Vec<DiscoveredCamera>> {
    let msearch = format!(
        "M-SEARCH * HTTP/1.1\r\n\
         HOST: {SSDP_MULTICAST_ADDR}:{SSDP_PORT}\r\n\
         MAN: \"ssdp:discover\"\r\n\
         MX: 3\r\n\
         ST: {target}\r\n\
         \r\n"
    );

    // Create socket with SO_REUSEADDR
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0).into())?;

    let socket = UdpSocket::from_std(socket.into())?;
    let multicast_addr = SocketAddrV4::new(SSDP_MULTICAST_ADDR, SSDP_PORT);

    // Send M-SEARCH twice for reliability
    socket.send_to(msearch.as_bytes(), multicast_addr).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    socket.send_to(msearch.as_bytes(), multicast_addr).await?;

    debug!("sent M-SEARCH for {target}");

    let mut cameras = Vec::new();
    let mut buf = vec![0u8; 4096];

    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let result = tokio::time::timeout_at(deadline, socket.recv_from(&mut buf)).await;
        match result {
            Ok(Ok((len, addr))) => {
                let response = String::from_utf8_lossy(&buf[..len]);
                debug!("SSDP response from {addr}: {response}");
                if let Some(camera) = parse_ssdp_response(&response) {
                    info!("found camera at {}:{}", camera.ip, camera.port);
                    cameras.push(camera);
                }
            }
            Ok(Err(e)) => {
                warn!("SSDP recv error: {e}");
                break;
            }
            Err(_) => break, // timeout
        }
    }

    Ok(cameras)
}

fn parse_ssdp_response(response: &str) -> Option<DiscoveredCamera> {
    if !response.starts_with("HTTP/1.1 200") {
        return None;
    }

    let mut location = None;
    let mut server = String::new();
    let mut usn = String::new();

    for line in response.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("LOCATION:") {
            location = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("location:") {
            location = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("SERVER:") {
            server = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("server:") {
            server = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("USN:") {
            usn = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("usn:") {
            usn = val.trim().to_string();
        }
    }

    let location_str = location?;

    // Parse IP from location URL like http://192.168.1.100:15740/...
    let (ip, port) = parse_location_url(&location_str)?;

    Some(DiscoveredCamera {
        ip,
        port,
        location: location_str,
        server,
        usn,
    })
}

fn parse_location_url(url: &str) -> Option<(Ipv4Addr, u16)> {
    let stripped = url.strip_prefix("http://")?;
    let host_part = stripped.split('/').next()?;

    if let Some((host, port_str)) = host_part.split_once(':') {
        let ip: Ipv4Addr = host.parse().ok()?;
        let port: u16 = port_str.parse().ok()?;
        Some((ip, port))
    } else {
        let ip: Ipv4Addr = host_part.parse().ok()?;
        Some((ip, super::ptp_ip::PTP_IP_PORT))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ssdp_response() {
        let response = "HTTP/1.1 200 OK\r\n\
            LOCATION: http://192.168.1.100:15740/upnp.xml\r\n\
            SERVER: Camera/1.0\r\n\
            USN: uuid:test-camera\r\n\
            \r\n";

        let camera = parse_ssdp_response(response).unwrap();
        assert_eq!(camera.ip, Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(camera.port, 15740);
        assert_eq!(camera.server, "Camera/1.0");
    }

    #[test]
    fn test_parse_location_url() {
        let (ip, port) = parse_location_url("http://10.0.0.1:15740/desc.xml").unwrap();
        assert_eq!(ip, Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(port, 15740);
    }

    #[test]
    fn test_parse_location_no_port() {
        let (ip, port) = parse_location_url("http://10.0.0.1/desc.xml").unwrap();
        assert_eq!(ip, Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(port, 15740); // default PTP-IP port
    }
}
