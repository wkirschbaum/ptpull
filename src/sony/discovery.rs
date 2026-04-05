use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tracing::{debug, info, warn};

use super::types::SonyDeviceInfo;

const SSDP_MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const SSDP_PORT: u16 = 1900;
const SONY_SEARCH_TARGET: &str = "urn:schemas-sony-com:service:ScalarWebAPI:1";

/// Discover Sony cameras via SSDP and fetch their device description
pub async fn discover_sony(timeout: Duration) -> anyhow::Result<Vec<SonyDeviceInfo>> {
    let locations = ssdp_search(timeout).await?;
    let mut devices = Vec::new();

    let client = reqwest::Client::new();
    for location in locations {
        match fetch_device_description(&client, &location).await {
            Ok(info) => {
                info!(
                    "found Sony camera: {} at {}",
                    info.display_name(),
                    info.base_url
                );
                devices.push(info);
            }
            Err(e) => warn!("failed to fetch device description from {location}: {e}"),
        }
    }

    Ok(devices)
}

/// Also try well-known Sony camera IPs directly (skip SSDP)
pub async fn try_known_ips() -> anyhow::Result<Option<SonyDeviceInfo>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()?;

    let known_urls = [
        "http://192.168.122.1:8080/sony/dd.xml",
        "http://10.0.0.1:10000/sony/dd.xml",
    ];

    for url in &known_urls {
        debug!("trying known Sony URL: {url}");
        match fetch_device_description(&client, url).await {
            Ok(info) => {
                info!("found Sony camera at known IP: {}", info.display_name());
                return Ok(Some(info));
            }
            Err(_) => continue,
        }
    }

    Ok(None)
}

async fn ssdp_search(timeout: Duration) -> anyhow::Result<Vec<String>> {
    let msearch = format!(
        "M-SEARCH * HTTP/1.1\r\n\
         HOST: {SSDP_MULTICAST_ADDR}:{SSDP_PORT}\r\n\
         MAN: \"ssdp:discover\"\r\n\
         MX: 3\r\n\
         ST: {SONY_SEARCH_TARGET}\r\n\
         \r\n"
    );

    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0).into())?;

    let socket = UdpSocket::from_std(socket.into())?;
    let multicast_addr = SocketAddrV4::new(SSDP_MULTICAST_ADDR, SSDP_PORT);

    socket.send_to(msearch.as_bytes(), multicast_addr).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    socket.send_to(msearch.as_bytes(), multicast_addr).await?;

    debug!("sent Sony SSDP M-SEARCH");

    let mut locations = Vec::new();
    let mut buf = vec![0u8; 4096];
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        let result = tokio::time::timeout_at(deadline, socket.recv_from(&mut buf)).await;
        match result {
            Ok(Ok((len, addr))) => {
                let response = String::from_utf8_lossy(&buf[..len]);
                debug!("SSDP response from {addr}");
                if let Some(location) = parse_location(&response) {
                    if !locations.contains(&location) {
                        locations.push(location);
                    }
                }
            }
            Ok(Err(e)) => {
                warn!("SSDP recv error: {e}");
                break;
            }
            Err(_) => break, // timeout
        }
    }

    Ok(locations)
}

fn parse_location(response: &str) -> Option<String> {
    for line in response.lines() {
        let line = line.trim();
        if let Some(val) = line
            .strip_prefix("LOCATION:")
            .or_else(|| line.strip_prefix("location:"))
        {
            return Some(val.trim().to_string());
        }
    }
    None
}

/// Fetch and parse Sony device description XML
pub async fn fetch_device_description(
    client: &reqwest::Client,
    url: &str,
) -> anyhow::Result<SonyDeviceInfo> {
    let body = client.get(url).send().await?.text().await?;
    parse_device_description(&body, url)
}

fn parse_device_description(xml: &str, source_url: &str) -> anyhow::Result<SonyDeviceInfo> {
    let doc = roxmltree::Document::parse(xml)?;

    let mut friendly_name = String::new();
    let mut manufacturer = String::new();
    let mut model_name = String::new();
    let mut services = Vec::new();

    // Find device element
    for node in doc.descendants() {
        if node.has_tag_name("friendlyName") {
            friendly_name = node.text().unwrap_or("").to_string();
        }
        if node.has_tag_name("manufacturer") {
            manufacturer = node.text().unwrap_or("").to_string();
        }
        if node.has_tag_name("modelName") {
            model_name = node.text().unwrap_or("").to_string();
        }
    }

    // Find Sony ScalarWebAPI services
    for node in doc.descendants() {
        if node.tag_name().name().contains("X_ScalarWebAPI_Service")
            && !node
                .tag_name()
                .name()
                .contains("X_ScalarWebAPI_ServiceList")
        {
            let mut service_type = String::new();
            let mut action_url = String::new();

            for child in node.children() {
                let tag = child.tag_name().name();
                if tag.contains("ServiceType") {
                    service_type = child.text().unwrap_or("").to_string();
                }
                if tag.contains("ActionList_URL") {
                    action_url = child.text().unwrap_or("").to_string();
                }
            }

            if !service_type.is_empty() && !action_url.is_empty() {
                services.push(super::types::SonyService {
                    service_type,
                    action_url,
                });
            }
        }
    }

    // Derive base URL from source
    let base_url = if let Some(s) = services.first() {
        s.action_url.clone()
    } else {
        // Fall back to deriving from the XML URL
        source_url
            .rsplit_once('/')
            .map(|(base, _)| base.to_string())
            .unwrap_or_else(|| source_url.to_string())
    };

    Ok(SonyDeviceInfo {
        friendly_name,
        manufacturer,
        model_name,
        base_url,
        services,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_device_description() {
        let xml = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <friendlyName>RX10M4</friendlyName>
    <manufacturer>Sony Corporation</manufacturer>
    <modelName>DSC-RX10M4</modelName>
    <av:X_ScalarWebAPI_DeviceInfo xmlns:av="urn:schemas-sony-com:av">
      <av:X_ScalarWebAPI_ServiceList>
        <av:X_ScalarWebAPI_Service>
          <av:X_ScalarWebAPI_ServiceType>camera</av:X_ScalarWebAPI_ServiceType>
          <av:X_ScalarWebAPI_ActionList_URL>http://192.168.122.1:8080/sony</av:X_ScalarWebAPI_ActionList_URL>
        </av:X_ScalarWebAPI_Service>
        <av:X_ScalarWebAPI_Service>
          <av:X_ScalarWebAPI_ServiceType>avContent</av:X_ScalarWebAPI_ServiceType>
          <av:X_ScalarWebAPI_ActionList_URL>http://192.168.122.1:8080/sony</av:X_ScalarWebAPI_ActionList_URL>
        </av:X_ScalarWebAPI_Service>
      </av:X_ScalarWebAPI_ServiceList>
    </av:X_ScalarWebAPI_DeviceInfo>
  </device>
</root>"#;

        let info = parse_device_description(xml, "http://192.168.122.1:8080/sony/dd.xml").unwrap();
        assert_eq!(info.friendly_name, "RX10M4");
        assert_eq!(info.manufacturer, "Sony Corporation");
        assert_eq!(info.model_name, "DSC-RX10M4");
        assert_eq!(info.services.len(), 2);
        assert_eq!(
            info.camera_endpoint().unwrap(),
            "http://192.168.122.1:8080/sony/camera"
        );
        assert_eq!(
            info.av_content_endpoint().unwrap(),
            "http://192.168.122.1:8080/sony/avContent"
        );
    }
}
