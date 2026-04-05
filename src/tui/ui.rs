use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph, Row, Table};

use crate::tui::app::{App, Screen};

pub fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(0),    // main content
            Constraint::Length(1), // status bar
        ])
        .split(frame.area());

    draw_header(frame, chunks[0], app);
    draw_status_bar(frame, chunks[2], app);

    match app.screen {
        Screen::Discovery => draw_discovery(frame, chunks[1], app),
        Screen::Browser => draw_browser(frame, chunks[1], app),
        Screen::Downloading => draw_downloading(frame, chunks[1], app),
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let title = match app.screen {
        Screen::Discovery => " ptpull - Camera Discovery ",
        Screen::Browser => " ptpull - File Browser ",
        Screen::Downloading => " ptpull - Downloading ",
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    frame.render_widget(block, area);
}

fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let help = match app.screen {
        Screen::Discovery => "r: rescan | enter: connect | q: quit",
        Screen::Browser => "j/k: navigate | space: select | a: all | enter: download | q: back",
        Screen::Downloading => "q: back (when done)",
    };

    let status = if let Some(ref msg) = app.status_message {
        format!("{msg}  |  {help}")
    } else {
        help.to_string()
    };

    let bar = Paragraph::new(Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(status, Style::default().fg(Color::DarkGray)),
    ]));
    frame.render_widget(bar, area);
}

fn draw_discovery(frame: &mut Frame, area: Rect, app: &App) {
    if app.discovering {
        let spinner = app.spinner_char();
        let text = Paragraph::new(format!(
            " {spinner} Searching for cameras on the network..."
        ))
        .block(Block::default().borders(Borders::ALL).title(" Cameras "));
        frame.render_widget(text, area);
        return;
    }

    if app.cameras.is_empty() {
        let msg = if let Some(ref err) = app.discovery_error {
            format!(" No cameras found. Error: {err}\n\n Press 'r' to retry.")
        } else {
            " No cameras found.\n\n Make sure your camera's WiFi is enabled.\n Press 'r' to scan again.".to_string()
        };
        let text =
            Paragraph::new(msg).block(Block::default().borders(Borders::ALL).title(" Cameras "));
        frame.render_widget(text, area);
        return;
    }

    let items: Vec<ListItem> = app
        .cameras
        .iter()
        .enumerate()
        .map(|(i, cam)| {
            let style = if i == app.selected_camera_idx {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let name = cam.display_name();
            ListItem::new(format!("  {name}  ({})", cam.ip)).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Cameras ({}) ", app.cameras.len())),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    let mut state = ListState::default();
    state.select(Some(app.selected_camera_idx));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_browser(frame: &mut Frame, area: Rect, app: &App) {
    if app.loading_objects {
        let spinner = app.spinner_char();
        let text = Paragraph::new(format!(" {spinner} Loading files from camera..."))
            .block(Block::default().borders(Borders::ALL).title(" Files "));
        frame.render_widget(text, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    // File table
    let header = Row::new(vec!["", "Filename", "Size", "Type", "Date"]).style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .objects
        .iter()
        .enumerate()
        .map(|(i, obj)| {
            let selected = if app.selected_handles.contains(&obj.handle) {
                "[x]"
            } else {
                "[ ]"
            };
            let type_str = if obj.is_folder() {
                "DIR"
            } else if obj.is_image() {
                "IMG"
            } else if obj.is_video() {
                "VID"
            } else {
                "   "
            };
            let style = if i == app.selected_object_idx {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            Row::new(vec![
                selected.to_string(),
                obj.filename.clone(),
                obj.size_display(),
                type_str.to_string(),
                obj.capture_date.clone(),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Length(3),
        Constraint::Min(20),
        Constraint::Length(10),
        Constraint::Length(4),
        Constraint::Length(20),
    ];

    let table = Table::new(rows, widths).header(header).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Files ({}) ", app.objects.len())),
    );

    frame.render_widget(table, chunks[0]);

    // Selection info
    let selected_count = app.selected_handles.len();
    let selected_size = app.total_selected_bytes();
    let size_str = format_bytes(selected_size);
    let info = Paragraph::new(format!(
        " {selected_count} selected ({size_str})  |  Destination: {}",
        app.dest_dir.display()
    ))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(info, chunks[1]);
}

fn draw_downloading(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(0)])
        .split(area);

    // Overall progress
    let total = app.total_download_bytes();
    let downloaded = app.total_downloaded_bytes();
    let ratio = if total == 0 {
        0.0
    } else {
        (downloaded as f64 / total as f64).min(1.0)
    };
    let completed = app.downloads.iter().filter(|d| d.completed).count();
    let total_count = app.downloads.len();

    let overall = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(format!(
            " Overall: {completed}/{total_count} files  ({} / {}) ",
            format_bytes(downloaded),
            format_bytes(total),
        )))
        .gauge_style(Style::default().fg(Color::Green))
        .ratio(ratio);
    frame.render_widget(overall, chunks[0]);

    // Per-file progress
    let items: Vec<ListItem> = app
        .downloads
        .iter()
        .map(|dl| {
            let status = if dl.completed {
                "done".to_string()
            } else if let Some(ref err) = dl.error {
                format!("ERR: {err}")
            } else {
                let pct = (dl.fraction() * 100.0) as u8;
                let speed = format_bytes(dl.speed_bytes_per_sec() as u64);
                format!("{pct}% ({speed}/s)")
            };

            let style = if dl.completed {
                Style::default().fg(Color::Green)
            } else if dl.error.is_some() {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Yellow)
            };

            ListItem::new(format!("  {} - {status}", dl.filename)).style(style)
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(" Downloads "));
    frame.render_widget(list, chunks[1]);
}

fn format_bytes(bytes: u64) -> String {
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
