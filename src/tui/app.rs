use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use ptpull::camera::types::{CameraInfo, ObjectInfo, StorageInfo};

/// Active screen in the TUI
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Discovery,
    Browser,
    Downloading,
}

/// Download progress for a single file
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub filename: String,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub started_at: Instant,
    pub completed: bool,
    pub error: Option<String>,
}

impl DownloadProgress {
    pub fn fraction(&self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            self.downloaded_bytes as f64 / self.total_bytes as f64
        }
    }

    pub fn speed_bytes_per_sec(&self) -> f64 {
        let elapsed = self.started_at.elapsed().as_secs_f64();
        if elapsed < 0.001 {
            0.0
        } else {
            self.downloaded_bytes as f64 / elapsed
        }
    }
}

/// Application state
pub struct App {
    pub screen: Screen,
    pub running: bool,

    // Discovery
    pub cameras: Vec<CameraInfo>,
    pub discovering: bool,
    pub discovery_error: Option<String>,

    // Browser
    pub selected_camera_idx: usize,
    pub storages: Vec<StorageInfo>,
    pub objects: Vec<ObjectInfo>,
    pub selected_object_idx: usize,
    pub selected_handles: HashSet<u32>,
    pub loading_objects: bool,

    // Download
    pub dest_dir: PathBuf,
    pub downloads: Vec<DownloadProgress>,
    pub download_active: bool,
    pub total_download_started: Option<Instant>,

    // UI state
    pub spinner_tick: usize,
    pub status_message: Option<String>,
}

impl App {
    pub fn new(dest_dir: PathBuf) -> Self {
        Self {
            screen: Screen::Discovery,
            running: true,

            cameras: Vec::new(),
            discovering: false,
            discovery_error: None,

            selected_camera_idx: 0,
            storages: Vec::new(),
            objects: Vec::new(),
            selected_object_idx: 0,
            selected_handles: HashSet::new(),
            loading_objects: false,

            dest_dir,
            downloads: Vec::new(),
            download_active: false,
            total_download_started: None,

            spinner_tick: 0,
            status_message: None,
        }
    }

    pub fn tick(&mut self) {
        self.spinner_tick = self.spinner_tick.wrapping_add(1);
    }

    /// Move selection up in the current list
    pub fn move_up(&mut self) {
        match self.screen {
            Screen::Discovery => {
                if self.selected_camera_idx > 0 {
                    self.selected_camera_idx -= 1;
                }
            }
            Screen::Browser => {
                if self.selected_object_idx > 0 {
                    self.selected_object_idx -= 1;
                }
            }
            Screen::Downloading => {}
        }
    }

    /// Move selection down in the current list
    pub fn move_down(&mut self) {
        match self.screen {
            Screen::Discovery => {
                if !self.cameras.is_empty() && self.selected_camera_idx < self.cameras.len() - 1 {
                    self.selected_camera_idx += 1;
                }
            }
            Screen::Browser => {
                if !self.objects.is_empty() && self.selected_object_idx < self.objects.len() - 1 {
                    self.selected_object_idx += 1;
                }
            }
            Screen::Downloading => {}
        }
    }

    /// Toggle selection of current object
    pub fn toggle_select(&mut self) {
        if self.screen != Screen::Browser || self.objects.is_empty() {
            return;
        }
        let handle = self.objects[self.selected_object_idx].handle;
        if self.selected_handles.contains(&handle) {
            self.selected_handles.remove(&handle);
        } else {
            self.selected_handles.insert(handle);
        }
    }

    /// Select all files (skip folders)
    pub fn select_all(&mut self) {
        for obj in &self.objects {
            if !obj.is_folder() {
                self.selected_handles.insert(obj.handle);
            }
        }
    }

    /// Deselect all
    pub fn deselect_all(&mut self) {
        self.selected_handles.clear();
    }

    /// Get selected objects for download
    pub fn selected_objects(&self) -> Vec<&ObjectInfo> {
        self.objects
            .iter()
            .filter(|o| self.selected_handles.contains(&o.handle))
            .collect()
    }

    /// Total bytes to download
    pub fn total_selected_bytes(&self) -> u64 {
        self.selected_objects()
            .iter()
            .map(|o| o.compressed_size as u64)
            .sum()
    }

    pub fn total_downloaded_bytes(&self) -> u64 {
        self.downloads.iter().map(|d| d.downloaded_bytes).sum()
    }

    pub fn total_download_bytes(&self) -> u64 {
        self.downloads.iter().map(|d| d.total_bytes).sum()
    }

    pub fn all_downloads_complete(&self) -> bool {
        !self.downloads.is_empty()
            && self
                .downloads
                .iter()
                .all(|d| d.completed || d.error.is_some())
    }

    pub fn spinner_char(&self) -> char {
        const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        SPINNER[self.spinner_tick % SPINNER.len()]
    }
}
