use std::time::Duration;

use reqwest::Client;

#[derive(Debug, Clone)]
pub struct DlnaDevice {
    pub friendly_name: String,
    pub manufacturer: String,
    pub model_name: String,
    pub base_url: String,
    pub content_directory_control_url: String,
}

impl DlnaDevice {
    pub fn display_name(&self) -> String {
        if !self.friendly_name.is_empty() {
            self.friendly_name.clone()
        } else if !self.model_name.is_empty() {
            self.model_name.clone()
        } else {
            "DLNA Device".to_string()
        }
    }
}

pub async fn discover_dlna(base_url: &str) -> anyhow::Result<DlnaDevice> {
    let client = Client::builder().timeout(Duration::from_secs(10)).build()?;
    let dd_url = format!("{base_url}/dd.xml");
    let body = client.get(&dd_url).send().await?.text().await?;
    parse_device_description(&body, base_url)
}

fn parse_device_description(xml: &str, base_url: &str) -> anyhow::Result<DlnaDevice> {
    let doc = roxmltree::Document::parse(xml)?;

    let mut friendly_name = String::new();
    let mut manufacturer = String::new();
    let mut model_name = String::new();
    let mut content_directory_control_url = String::new();

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

    for node in doc.descendants() {
        if node.has_tag_name("service") {
            let mut is_content_dir = false;
            let mut control_url = String::new();

            for child in node.children() {
                if child.has_tag_name("serviceType")
                    && let Some(text) = child.text()
                    && text.contains("ContentDirectory")
                {
                    is_content_dir = true;
                }
                if child.has_tag_name("controlURL") {
                    control_url = child.text().unwrap_or("").to_string();
                }
            }

            if is_content_dir && !control_url.is_empty() {
                content_directory_control_url = if control_url.starts_with("http") {
                    control_url
                } else {
                    format!("{base_url}{control_url}")
                };
                break;
            }
        }
    }

    if content_directory_control_url.is_empty() {
        anyhow::bail!("ContentDirectory control URL not found in dd.xml");
    }

    Ok(DlnaDevice {
        friendly_name,
        manufacturer,
        model_name,
        base_url: base_url.to_string(),
        content_directory_control_url,
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
    <friendlyName>DSC-RX10M4</friendlyName>
    <manufacturer>Sony Corporation</manufacturer>
    <modelName>SonyImagingDevice</modelName>
    <serviceList>
      <service>
        <serviceType>urn:schemas-upnp-org:service:ContentDirectory:1</serviceType>
        <controlURL>/upnp/control/ContentDirectory</controlURL>
      </service>
    </serviceList>
  </device>
</root>"#;

        let dev = parse_device_description(xml, "http://192.168.122.1:64321").unwrap();
        assert_eq!(dev.friendly_name, "DSC-RX10M4");
        assert_eq!(
            dev.content_directory_control_url,
            "http://192.168.122.1:64321/upnp/control/ContentDirectory"
        );
    }
}
