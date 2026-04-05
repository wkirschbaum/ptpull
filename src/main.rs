use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;

mod tui;

use ptpull::camera::discovery;
use ptpull::camera::operations::Camera;
use tui::app::{App, DownloadProgress, Screen};
use tui::event::{Event, EventReader};
use tui::ui;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing to a file so it doesn't interfere with TUI
    let log_file = std::fs::File::create("ptpull.log").ok();
    if let Some(file) = log_file {
        tracing_subscriber::fmt()
            .with_writer(file)
            .with_ansi(false)
            .init();
    }

    let args: Vec<String> = std::env::args().collect();
    let mut dest_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut manual_ip: Option<std::net::Ipv4Addr> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--ip" | "-i" => {
                i += 1;
                if i < args.len() {
                    manual_ip = Some(args[i].parse().expect("invalid IP address"));
                }
            }
            "--help" | "-h" => {
                eprintln!("Usage: ptpull [OPTIONS] [DEST_DIR]");
                eprintln!();
                eprintln!("Options:");
                eprintln!("  -i, --ip <ADDR>  Connect directly to camera IP (skip discovery)");
                eprintln!("  -h, --help       Show this help");
                std::process::exit(0);
            }
            other => {
                dest_dir = PathBuf::from(other);
            }
        }
        i += 1;
    }

    // Ensure dest dir exists
    tokio::fs::create_dir_all(&dest_dir).await?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, dest_dir, manual_ip).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(ref e) = result {
        eprintln!("Error: {e:?}");
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    dest_dir: PathBuf,
    manual_ip: Option<std::net::Ipv4Addr>,
) -> Result<()> {
    let mut app = App::new(dest_dir);
    let mut events = EventReader::new(Duration::from_millis(200));

    // Channel for async operations to send updates back
    let (action_tx, mut action_rx) = mpsc::unbounded_channel::<Action>();

    if let Some(ip) = manual_ip {
        // Skip discovery, connect directly
        let camera_info = ptpull::camera::types::CameraInfo {
            ip,
            port: ptpull::protocol::ptp_ip::PTP_IP_PORT,
            device_info: None,
        };
        app.status_message = Some(format!("Connecting to {ip}..."));
        start_connect(camera_info, &action_tx);
    } else {
        // Start SSDP discovery
        start_discovery(&action_tx);
        app.discovering = true;
    }

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        // Process async actions
        while let Ok(action) = action_rx.try_recv() {
            handle_action(&mut app, action, &action_tx);
        }

        // Process terminal events
        if let Some(event) = events.next().await {
            match event {
                Event::Key(key) => {
                    if handle_key(&mut app, key.code, key.modifiers, &action_tx) {
                        break;
                    }
                }
                Event::Tick => {
                    app.tick();
                }
                Event::Resize(_, _) => {}
            }
        }

        if !app.running {
            break;
        }
    }

    Ok(())
}

/// Actions sent from async tasks back to the main loop
enum Action {
    CamerasFound(Vec<ptpull::camera::types::CameraInfo>),
    DiscoveryError(String),
    Connected(Box<Camera>),
    ConnectionError(String),
    ObjectsLoaded(Vec<ptpull::camera::types::ObjectInfo>),
    ObjectsError(String),
    DownloadProgress {
        index: usize,
        downloaded: u64,
        total: u64,
    },
    DownloadComplete {
        index: usize,
    },
    DownloadError {
        index: usize,
        error: String,
    },
    AllDownloadsComplete,
}

// Store the camera connection for use across async operations
static CAMERA: std::sync::OnceLock<tokio::sync::Mutex<Option<Camera>>> = std::sync::OnceLock::new();

fn camera_lock() -> &'static tokio::sync::Mutex<Option<Camera>> {
    CAMERA.get_or_init(|| tokio::sync::Mutex::new(None))
}

fn start_discovery(tx: &mpsc::UnboundedSender<Action>) {
    let tx = tx.clone();
    tokio::spawn(async move {
        match discovery::discover().await {
            Ok(cameras) => {
                let _ = tx.send(Action::CamerasFound(cameras));
            }
            Err(e) => {
                let _ = tx.send(Action::DiscoveryError(e.to_string()));
            }
        }
    });
}

fn start_connect(
    camera_info: ptpull::camera::types::CameraInfo,
    tx: &mpsc::UnboundedSender<Action>,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        match Camera::connect(camera_info).await {
            Ok(camera) => {
                let _ = tx.send(Action::Connected(Box::new(camera)));
            }
            Err(e) => {
                let _ = tx.send(Action::ConnectionError(e.to_string()));
            }
        }
    });
}

fn start_load_objects(tx: &mpsc::UnboundedSender<Action>) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let mut guard = camera_lock().lock().await;
        if let Some(ref mut camera) = *guard {
            match camera.list_storages().await {
                Ok(storages) => {
                    let mut all_objects = Vec::new();
                    for storage in &storages {
                        match camera.list_objects(storage.storage_id).await {
                            Ok(objects) => all_objects.extend(objects),
                            Err(e) => {
                                let _ = tx.send(Action::ObjectsError(format!("list objects: {e}")));
                                return;
                            }
                        }
                    }
                    let _ = tx.send(Action::ObjectsLoaded(all_objects));
                }
                Err(e) => {
                    let _ = tx.send(Action::ObjectsError(format!("list storages: {e}")));
                }
            }
        }
    });
}

fn start_downloads(
    objects: Vec<ptpull::camera::types::ObjectInfo>,
    dest_dir: PathBuf,
    tx: &mpsc::UnboundedSender<Action>,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let mut guard = camera_lock().lock().await;
        if let Some(ref mut camera) = *guard {
            for (index, obj) in objects.iter().enumerate() {
                let tx2 = tx.clone();
                let idx = index;
                let progress_fn: ptpull::camera::operations::ProgressFn =
                    Box::new(move |downloaded, total| {
                        let _ = tx2.send(Action::DownloadProgress {
                            index: idx,
                            downloaded,
                            total,
                        });
                    });

                match camera
                    .download_object(obj, &dest_dir, Some(progress_fn))
                    .await
                {
                    Ok(_) => {
                        let _ = tx.send(Action::DownloadComplete { index });
                    }
                    Err(e) => {
                        let _ = tx.send(Action::DownloadError {
                            index,
                            error: e.to_string(),
                        });
                    }
                }
            }
        }
        let _ = tx.send(Action::AllDownloadsComplete);
    });
}

fn handle_action(app: &mut App, action: Action, tx: &mpsc::UnboundedSender<Action>) {
    match action {
        Action::CamerasFound(cameras) => {
            app.cameras = cameras;
            app.discovering = false;
            app.discovery_error = None;
            if app.cameras.len() == 1 {
                app.status_message = Some("Found 1 camera. Press Enter to connect.".into());
            } else {
                app.status_message = Some(format!("Found {} cameras.", app.cameras.len()));
            }
        }
        Action::DiscoveryError(e) => {
            app.discovering = false;
            app.discovery_error = Some(e);
        }
        Action::Connected(camera) => {
            app.status_message = Some(format!("Connected to {}", camera.info.display_name()));
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                *camera_lock().lock().await = Some(*camera);
            });
            app.screen = Screen::Browser;
            app.loading_objects = true;
            start_load_objects(tx);
        }
        Action::ConnectionError(e) => {
            app.status_message = Some(format!("Connection failed: {e}"));
        }
        Action::ObjectsLoaded(objects) => {
            app.objects = objects;
            app.loading_objects = false;
            app.status_message = Some(format!("Loaded {} files", app.objects.len()));
        }
        Action::ObjectsError(e) => {
            app.loading_objects = false;
            app.status_message = Some(format!("Error: {e}"));
        }
        Action::DownloadProgress {
            index,
            downloaded,
            total,
        } => {
            if let Some(dl) = app.downloads.get_mut(index) {
                dl.downloaded_bytes = downloaded;
                dl.total_bytes = total;
            }
        }
        Action::DownloadComplete { index } => {
            if let Some(dl) = app.downloads.get_mut(index) {
                dl.completed = true;
                dl.downloaded_bytes = dl.total_bytes;
            }
        }
        Action::DownloadError { index, error } => {
            if let Some(dl) = app.downloads.get_mut(index) {
                dl.error = Some(error);
            }
        }
        Action::AllDownloadsComplete => {
            app.download_active = false;
            app.status_message = Some("All downloads complete!".into());
        }
    }
}

fn handle_key(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
    tx: &mpsc::UnboundedSender<Action>,
) -> bool {
    // Ctrl+C always quits
    if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
        app.running = false;
        return true;
    }

    match app.screen {
        Screen::Discovery => match code {
            KeyCode::Char('q') => {
                app.running = false;
                return true;
            }
            KeyCode::Char('r') => {
                app.discovering = true;
                app.cameras.clear();
                start_discovery(tx);
            }
            KeyCode::Up | KeyCode::Char('k') => app.move_up(),
            KeyCode::Down | KeyCode::Char('j') => app.move_down(),
            KeyCode::Enter => {
                if !app.cameras.is_empty() {
                    let camera_info = app.cameras[app.selected_camera_idx].clone();
                    app.status_message = Some(format!("Connecting to {}...", camera_info.ip));
                    start_connect(camera_info, tx);
                }
            }
            _ => {}
        },
        Screen::Browser => match code {
            KeyCode::Char('q') | KeyCode::Esc => {
                app.screen = Screen::Discovery;
                app.objects.clear();
                app.selected_handles.clear();
                app.selected_object_idx = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => app.move_up(),
            KeyCode::Down | KeyCode::Char('j') => app.move_down(),
            KeyCode::Char(' ') => app.toggle_select(),
            KeyCode::Char('a') => {
                if app.selected_handles.len()
                    == app.objects.iter().filter(|o| !o.is_folder()).count()
                {
                    app.deselect_all();
                } else {
                    app.select_all();
                }
            }
            KeyCode::Enter => {
                if !app.selected_handles.is_empty() {
                    let objects: Vec<_> = app.selected_objects().into_iter().cloned().collect();
                    app.downloads = objects
                        .iter()
                        .map(|o| DownloadProgress {
                            filename: o.filename.clone(),
                            total_bytes: o.compressed_size as u64,
                            downloaded_bytes: 0,
                            started_at: Instant::now(),
                            completed: false,
                            error: None,
                        })
                        .collect();
                    app.screen = Screen::Downloading;
                    app.download_active = true;
                    app.total_download_started = Some(Instant::now());
                    start_downloads(objects, app.dest_dir.clone(), tx);
                }
            }
            _ => {}
        },
        Screen::Downloading => match code {
            KeyCode::Char('q') | KeyCode::Esc => {
                if !app.download_active {
                    app.screen = Screen::Browser;
                    app.downloads.clear();
                }
            }
            _ => {}
        },
    }

    false
}
