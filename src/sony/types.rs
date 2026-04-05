use serde::{Deserialize, Serialize};

/// Sony JSON-RPC request
#[derive(Debug, Serialize)]
pub struct JsonRpcRequest {
    pub method: String,
    pub params: serde_json::Value,
    pub id: u32,
    pub version: String,
}

/// Sony JSON-RPC response
#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse {
    pub id: u32,
    pub result: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
}

/// Service endpoints discovered from device description XML
#[derive(Debug, Clone)]
pub struct SonyDeviceInfo {
    pub friendly_name: String,
    pub manufacturer: String,
    pub model_name: String,
    pub base_url: String,
    pub services: Vec<SonyService>,
}

#[derive(Debug, Clone)]
pub struct SonyService {
    pub service_type: String,
    pub action_url: String,
}

impl SonyDeviceInfo {
    pub fn endpoint(&self, service: &str) -> Option<String> {
        self.services
            .iter()
            .find(|s| s.service_type == service)
            .map(|s| format!("{}/{}", s.action_url, service))
    }

    pub fn camera_endpoint(&self) -> Option<String> {
        self.endpoint("camera")
    }

    pub fn av_content_endpoint(&self) -> Option<String> {
        self.endpoint("avContent")
    }

    pub fn system_endpoint(&self) -> Option<String> {
        self.endpoint("system")
    }

    pub fn display_name(&self) -> String {
        if !self.friendly_name.is_empty() {
            self.friendly_name.clone()
        } else if !self.model_name.is_empty() {
            self.model_name.clone()
        } else {
            "Sony Camera".to_string()
        }
    }
}

/// Content item from getContentList
#[derive(Debug, Clone)]
pub struct ContentItem {
    pub uri: String,
    pub content_kind: String,
    pub title: String,
    pub created_time: String,
    pub folder_no: String,
    pub file_no: String,
    pub originals: Vec<ContentOriginal>,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ContentOriginal {
    pub file_name: String,
    pub url: String,
    pub still_object: Option<String>, // "jpeg", "raw"
}

impl ContentItem {
    pub fn is_directory(&self) -> bool {
        self.content_kind == "directory"
    }

    pub fn is_still(&self) -> bool {
        self.content_kind == "still"
    }

    pub fn is_video(&self) -> bool {
        self.content_kind.starts_with("movie")
    }

    pub fn best_original(&self) -> Option<&ContentOriginal> {
        // Prefer JPEG for stills, first available for everything else
        if self.is_still() {
            self.originals
                .iter()
                .find(|o| o.still_object.as_deref() == Some("jpeg"))
                .or(self.originals.first())
        } else {
            self.originals.first()
        }
    }

    pub fn display_name(&self) -> String {
        if let Some(orig) = self.originals.first() {
            orig.file_name.clone()
        } else {
            self.title.clone()
        }
    }

    /// Parse a content item from Sony JSON response
    pub fn from_json(val: &serde_json::Value) -> Option<Self> {
        let uri = val.get("uri")?.as_str()?.to_string();
        let content_kind = val
            .get("contentKind")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let title = val
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let created_time = val
            .get("createdTime")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let folder_no = val
            .get("folderNo")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let file_no = val
            .get("fileNo")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut originals = Vec::new();
        if let Some(content) = val.get("content") {
            if let Some(orig_arr) = content.get("original").and_then(|v| v.as_array()) {
                for orig in orig_arr {
                    let file_name = orig
                        .get("fileName")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let url = orig
                        .get("url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let still_object = orig
                        .get("stillObject")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    if !url.is_empty() {
                        originals.push(ContentOriginal {
                            file_name,
                            url,
                            still_object,
                        });
                    }
                }
            }
        }

        let thumbnail_url = val
            .get("thumbnailUrl")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Some(Self {
            uri,
            content_kind,
            title,
            created_time,
            folder_no,
            file_no,
            originals,
            thumbnail_url,
        })
    }
}
