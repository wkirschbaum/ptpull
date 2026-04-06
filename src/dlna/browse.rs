use std::path::Path;

use reqwest::Client;
use tokio::fs;
use tokio::io::{AsyncWriteExt, BufWriter};

use super::discovery::DlnaDevice;

#[derive(Debug, Clone)]
pub struct DlnaItem {
    pub id: String,
    pub parent_id: String,
    pub title: String,
    pub is_container: bool,
    pub resources: Vec<DlnaResource>,
    pub date: String,
    pub upnp_class: String,
}

#[derive(Debug, Clone)]
pub struct DlnaResource {
    pub url: String,
    pub protocol_info: String,
    pub size: u64,
    pub resolution: String,
}

impl DlnaItem {
    pub fn best_resource(&self) -> Option<&DlnaResource> {
        self.resources.iter().max_by_key(|r| r.size)
    }

    pub fn filename(&self) -> String {
        if let Some(res) = self.best_resource()
            && let Some(path) = res.url.split('?').next()
            && let Some(name) = path.rsplit('/').next()
            && !name.is_empty()
            && name.contains('.')
        {
            return name.to_string();
        }
        self.title.clone()
    }

    pub fn size_display(&self) -> String {
        if let Some(res) = self.best_resource() {
            format_bytes(res.size)
        } else {
            "?".to_string()
        }
    }

    pub fn date_folder(&self) -> String {
        if self.date.is_empty() {
            return "unknown".to_string();
        }
        let date_part = self.date.split('T').next().unwrap_or(&self.date);
        let parts: Vec<&str> = date_part.split('-').collect();
        if parts.len() == 3 {
            format!(
                "{}-{:02}-{:02}",
                parts[0],
                parts[1].parse::<u32>().unwrap_or(1),
                parts[2].parse::<u32>().unwrap_or(1),
            )
        } else {
            date_part.to_string()
        }
    }
}

pub struct DlnaBrowser {
    client: Client,
    device: DlnaDevice,
}

struct BrowseResult {
    items: Vec<DlnaItem>,
    number_returned: u32,
    total_matches: u32,
}

impl DlnaBrowser {
    pub fn new(device: DlnaDevice) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .tcp_keepalive(std::time::Duration::from_secs(30))
                .tcp_nodelay(true)
                .pool_max_idle_per_host(2)
                .build()
                .expect("build reqwest client"),
            device,
        }
    }

    async fn browse_page(
        &self,
        object_id: &str,
        start_index: u32,
        count: u32,
    ) -> anyhow::Result<BrowseResult> {
        let soap_body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"
            s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:Browse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1">
      <ObjectID>{object_id}</ObjectID>
      <BrowseFlag>BrowseDirectChildren</BrowseFlag>
      <Filter>*</Filter>
      <StartingIndex>{start_index}</StartingIndex>
      <RequestedCount>{count}</RequestedCount>
      <SortCriteria></SortCriteria>
    </u:Browse>
  </s:Body>
</s:Envelope>"#
        );

        let resp = self
            .client
            .post(&self.device.content_directory_control_url)
            .header("Content-Type", r#"text/xml; charset="utf-8""#)
            .header(
                "SOAPAction",
                r#""urn:schemas-upnp-org:service:ContentDirectory:1#Browse""#,
            )
            .body(soap_body)
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;

        if !status.is_success() {
            anyhow::bail!("DLNA Browse failed: HTTP {status}");
        }

        parse_browse_response(&body)
    }

    async fn browse_all(&self, object_id: &str) -> anyhow::Result<Vec<DlnaItem>> {
        let mut all_items = Vec::new();
        let mut start_index: u32 = 0;
        let page_size: u32 = 100;

        loop {
            let result = self.browse_page(object_id, start_index, page_size).await?;
            let count = result.number_returned;
            all_items.extend(result.items);

            if count == 0
                || (result.total_matches > 0 && start_index + count >= result.total_matches)
            {
                break;
            }
            start_index += count;
        }

        Ok(all_items)
    }

    pub async fn list_all_files(&self) -> anyhow::Result<Vec<DlnaItem>> {
        let mut all_files = Vec::new();
        let mut containers_to_visit = vec!["0".to_string()];

        while let Some(container_id) = containers_to_visit.pop() {
            let items = self.browse_all(&container_id).await?;
            for item in items {
                if item.is_container {
                    containers_to_visit.push(item.id.clone());
                } else {
                    all_files.push(item);
                }
            }
        }

        Ok(all_files)
    }

    /// Returns None if skipped (already exists with same size)
    pub async fn download(
        &self,
        item: &DlnaItem,
        dest_dir: &Path,
    ) -> anyhow::Result<Option<std::path::PathBuf>> {
        let resource = item
            .best_resource()
            .ok_or_else(|| anyhow::anyhow!("no downloadable resource for {}", item.title))?;

        let filename = item.filename();
        let mut dest_path = dest_dir.join(&filename);

        // Skip if file exists with same size
        if let Ok(meta) = tokio::fs::metadata(&dest_path).await {
            if resource.size > 0 && meta.len() == resource.size {
                return Ok(None); // skip
            }
            // Same name but different size — add suffix
            let stem = dest_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let ext = dest_path
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            let mut n = 1;
            loop {
                dest_path = dest_dir.join(format!("{stem}_{n}{ext}"));
                if tokio::fs::metadata(&dest_path).await.is_err() {
                    break;
                }
                n += 1;
            }
        }

        let resp = self.client.get(&resource.url).send().await?;
        let file = fs::File::create(&dest_path).await?;
        let mut writer = BufWriter::with_capacity(256 * 1024, file);

        let mut stream = resp.bytes_stream();
        use futures_util::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            writer.write_all(&chunk).await?;
        }

        writer.flush().await?;

        Ok(Some(dest_path))
    }
}

fn parse_browse_response(xml: &str) -> anyhow::Result<BrowseResult> {
    let doc = roxmltree::Document::parse(xml)?;

    let result_text = doc
        .descendants()
        .find(|n| n.has_tag_name("Result"))
        .and_then(|n| n.text())
        .unwrap_or("");

    let number_returned: u32 = doc
        .descendants()
        .find(|n| n.has_tag_name("NumberReturned"))
        .and_then(|n| n.text())
        .and_then(|t| t.parse().ok())
        .unwrap_or(0);

    let total_matches: u32 = doc
        .descendants()
        .find(|n| n.has_tag_name("TotalMatches"))
        .and_then(|n| n.text())
        .and_then(|t| t.parse().ok())
        .unwrap_or(0);

    if result_text.is_empty() {
        return Ok(BrowseResult {
            items: Vec::new(),
            number_returned,
            total_matches,
        });
    }

    let items = parse_didl_lite(result_text)?;

    Ok(BrowseResult {
        items,
        number_returned,
        total_matches,
    })
}

fn parse_didl_lite(xml: &str) -> anyhow::Result<Vec<DlnaItem>> {
    let doc = roxmltree::Document::parse(xml)?;
    let mut items = Vec::new();

    let dc_ns = "http://purl.org/dc/elements/1.1/";
    let upnp_ns = "urn:schemas-upnp-org:metadata-1-0/upnp/";

    for node in doc.root().children().flat_map(|n| n.children()) {
        let is_container = node.tag_name().name() == "container";
        let is_item = node.tag_name().name() == "item";

        if !is_container && !is_item {
            continue;
        }

        let id = node.attribute("id").unwrap_or("").to_string();
        let parent_id = node.attribute("parentID").unwrap_or("").to_string();

        let title = node
            .descendants()
            .find(|n| n.has_tag_name((dc_ns, "title")))
            .and_then(|n| n.text())
            .unwrap_or("")
            .to_string();

        let date = node
            .descendants()
            .find(|n| n.has_tag_name((dc_ns, "date")))
            .and_then(|n| n.text())
            .unwrap_or("")
            .to_string();

        let upnp_class = node
            .descendants()
            .find(|n| n.has_tag_name((upnp_ns, "class")))
            .and_then(|n| n.text())
            .unwrap_or("")
            .to_string();

        let mut resources = Vec::new();
        for res_node in node.children().filter(|n| n.tag_name().name() == "res") {
            let url = res_node.text().unwrap_or("").trim().to_string();
            let protocol_info = res_node.attribute("protocolInfo").unwrap_or("").to_string();
            let size: u64 = res_node
                .attribute("size")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let resolution = res_node.attribute("resolution").unwrap_or("").to_string();

            if !url.is_empty() {
                resources.push(DlnaResource {
                    url,
                    protocol_info,
                    size,
                    resolution,
                });
            }
        }

        items.push(DlnaItem {
            id,
            parent_id,
            title,
            is_container,
            resources,
            date,
            upnp_class,
        });
    }

    Ok(items)
}

pub fn format_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    if b < 1024.0 {
        format!("{bytes} B")
    } else if b < 1024.0 * 1024.0 {
        format!("{:.1} KB", b / 1024.0)
    } else if b < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", b / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", b / (1024.0 * 1024.0 * 1024.0))
    }
}
