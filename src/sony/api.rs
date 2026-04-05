use std::path::Path;

use reqwest::Client;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info};

use super::types::{ContentItem, JsonRpcRequest, JsonRpcResponse, SonyDeviceInfo};

/// Sony Camera Remote API client
pub struct SonyCamera {
    client: Client,
    pub device: SonyDeviceInfo,
    request_id: u32,
}

pub type ProgressFn = Box<dyn Fn(u64, u64) + Send>;

#[derive(Debug, thiserror::Error)]
pub enum SonyError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API error: {0}")]
    Api(String),
    #[error("missing endpoint: {0}")]
    MissingEndpoint(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl SonyCamera {
    pub fn new(device: SonyDeviceInfo) -> Self {
        Self {
            client: Client::new(),
            device,
            request_id: 0,
        }
    }

    fn next_id(&mut self) -> u32 {
        self.request_id += 1;
        self.request_id
    }

    async fn call(
        &mut self,
        endpoint: &str,
        method: &str,
        params: serde_json::Value,
        version: &str,
    ) -> Result<serde_json::Value, SonyError> {
        let id = self.next_id();
        let req = JsonRpcRequest {
            method: method.to_string(),
            params,
            id,
            version: version.to_string(),
        };

        debug!("Sony API call: {method} -> {endpoint}");
        let resp: JsonRpcResponse = self
            .client
            .post(endpoint)
            .json(&req)
            .send()
            .await?
            .json()
            .await?;

        if let Some(err) = resp.error {
            return Err(SonyError::Api(format!("{err}")));
        }

        resp.result
            .ok_or_else(|| SonyError::Api("no result".into()))
    }

    fn camera_url(&self) -> Result<String, SonyError> {
        self.device
            .camera_endpoint()
            .ok_or_else(|| SonyError::MissingEndpoint("camera".into()))
    }

    fn av_content_url(&self) -> Result<String, SonyError> {
        self.device
            .av_content_endpoint()
            .ok_or_else(|| SonyError::MissingEndpoint("avContent".into()))
    }

    /// Switch camera to Contents Transfer mode
    pub async fn set_contents_transfer_mode(&mut self) -> Result<(), SonyError> {
        let url = self.camera_url()?;
        self.call(
            &url,
            "setCameraFunction",
            serde_json::json!(["Contents Transfer"]),
            "1.0",
        )
        .await?;
        info!("switched to Contents Transfer mode");
        Ok(())
    }

    /// Get available storage schemes
    pub async fn get_scheme_list(&mut self) -> Result<Vec<String>, SonyError> {
        let url = self.av_content_url()?;
        let result = self
            .call(&url, "getSchemeList", serde_json::json!([]), "1.0")
            .await?;

        let mut schemes = Vec::new();
        if let Some(arr) = result
            .as_array()
            .and_then(|a| a.first())
            .and_then(|v| v.as_array())
        {
            for item in arr {
                if let Some(scheme) = item.get("scheme").and_then(|v| v.as_str()) {
                    schemes.push(scheme.to_string());
                }
            }
        }
        debug!("schemes: {schemes:?}");
        Ok(schemes)
    }

    /// Get storage sources for a scheme
    pub async fn get_source_list(&mut self, scheme: &str) -> Result<Vec<String>, SonyError> {
        let url = self.av_content_url()?;
        let result = self
            .call(
                &url,
                "getSourceList",
                serde_json::json!([{"scheme": scheme}]),
                "1.0",
            )
            .await?;

        let mut sources = Vec::new();
        if let Some(arr) = result
            .as_array()
            .and_then(|a| a.first())
            .and_then(|v| v.as_array())
        {
            for item in arr {
                if let Some(source) = item.get("source").and_then(|v| v.as_str()) {
                    sources.push(source.to_string());
                }
            }
        }
        debug!("sources: {sources:?}");
        Ok(sources)
    }

    /// Get total content count for a source
    pub async fn get_content_count(&mut self, uri: &str) -> Result<u32, SonyError> {
        let url = self.av_content_url()?;
        let result = self
            .call(
                &url,
                "getContentCount",
                serde_json::json!([{
                    "uri": uri,
                    "type": ["still", "movie_mp4", "movie_xavcs"],
                    "view": "flat"
                }]),
                "1.2",
            )
            .await?;

        let count = result
            .as_array()
            .and_then(|a| a.first())
            .and_then(|v| v.get("count"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        debug!("content count: {count}");
        Ok(count)
    }

    /// List all content on a source (handles pagination)
    pub async fn list_all_content(&mut self, uri: &str) -> Result<Vec<ContentItem>, SonyError> {
        let url = self.av_content_url()?;
        let mut all_items = Vec::new();
        let mut start_idx = 0u32;
        let page_size = 100u32;

        loop {
            let result = self
                .call(
                    &url,
                    "getContentList",
                    serde_json::json!([{
                        "uri": uri,
                        "stIdx": start_idx,
                        "cnt": page_size,
                        "view": "flat",
                        "type": ["still", "movie_mp4", "movie_xavcs"],
                        "sort": "ascending"
                    }]),
                    "1.3",
                )
                .await?;

            let items: Vec<ContentItem> = result
                .as_array()
                .and_then(|a| a.first())
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(ContentItem::from_json).collect())
                .unwrap_or_default();

            let count = items.len() as u32;
            all_items.extend(items);

            if count < page_size {
                break;
            }
            start_idx += count;
        }

        info!("found {} content items", all_items.len());
        Ok(all_items)
    }

    /// Download a content item to disk
    pub async fn download(
        &self,
        item: &ContentItem,
        dest_dir: &Path,
        progress: Option<ProgressFn>,
    ) -> Result<std::path::PathBuf, SonyError> {
        let original = item
            .best_original()
            .ok_or_else(|| SonyError::Api("no downloadable original".into()))?;

        let dest_path = dest_dir.join(&original.file_name);
        info!(
            "downloading {} to {}",
            original.file_name,
            dest_path.display()
        );

        let resp = self.client.get(&original.url).send().await?;
        let total_size = resp.content_length().unwrap_or(0);
        let mut file = fs::File::create(&dest_path).await?;
        let mut downloaded: u64 = 0;

        let mut stream = resp.bytes_stream();
        use futures_util::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            if let Some(ref cb) = progress {
                cb(downloaded, total_size);
            }
        }

        file.flush().await?;
        info!("downloaded {}", original.file_name);
        Ok(dest_path)
    }

    /// Get available API methods (for debugging)
    pub async fn get_available_apis(&mut self) -> Result<Vec<String>, SonyError> {
        let url = self.camera_url()?;
        let result = self
            .call(&url, "getAvailableApiList", serde_json::json!([]), "1.0")
            .await?;

        let apis: Vec<String> = result
            .as_array()
            .and_then(|a| a.first())
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        Ok(apis)
    }
}
