//! Video action menu, quality picker, search input.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::{truncate_str, LAVENDER, TEAL, TEXT, YELLOW};
use crate::app::{QualitySelectScreen, VideoActionsScreen};

const VIDEO_ACTION_DISPLAY: &[&str] = &[
    "👁️  Watch",
    "🎚️  Watch (Select Quality)",
    "▶️  Play All",
    "⬇️  Download",
    "🎵  Download (Audio Only)",
    "📥  Download All",
    "🎶  Download All (Audio Only)",
    "❤️  Save",
    "💔  UnSave",
    "📋  Save Playlist",
    "⏭  Add to Queue",
    "🌐  Open in Browser",
    "←  Back",
];

pub(super) fn render_video_actions(f: &mut Frame, area: Rect, va: &VideoActionsScreen) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(1)])
        .split(area);

    let info = vec![
        Line::from(vec![
            Span::styled(
                "Title    ",
                Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
            ),
            Span::styled(va.video.title.clone(), Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled(
                "Channel  ",
                Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
            ),
            Span::styled(va.video.channel.clone(), Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled(
                "Date     ",
                Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
            ),
            Span::styled(va.video.upload_date.clone(), Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled(
                "Duration ",
                Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {}", va.video.duration_string),
                Style::default().fg(TEXT),
            ),
        ]),
    ];
    let info_block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " 🎬  Video Info ",
            Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(TEAL));
    let info_para = Paragraph::new(info)
        .block(info_block)
        .wrap(Wrap { trim: true });
    f.render_widget(info_para, chunks[0]);

    let display_items = VIDEO_ACTION_DISPLAY;
    let accent = TEAL;

    let list_items: Vec<ListItem> = display_items
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let style = if i == va.selected {
                Style::default()
                    .fg(accent)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(LAVENDER)
            };
            ListItem::new(format!("  {}  ", label)).style(style)
        })
        .collect();

    let action_block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " ⚡  Actions ",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(accent));

    f.render_widget(List::new(list_items).block(action_block), chunks[1]);
}

pub(super) fn render_quality_select(f: &mut Frame, area: Rect, qs: &QualitySelectScreen) {
    let accent = TEAL;
    let list_items: Vec<ListItem> = qs
        .options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            let label = if opt.eq_ignore_ascii_case("best") {
                "Best available".to_string()
            } else {
                format!("{}p", opt)
            };
            let style = if i == qs.selected {
                Style::default()
                    .fg(accent)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(LAVENDER)
            };
            ListItem::new(format!("  {}  ", label)).style(style)
        })
        .collect();

    let title = format!(" 🎚️  Quality — {} ", truncate_str(&qs.video.title, 50));
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            title,
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(accent));

    f.render_widget(List::new(list_items).block(block), area);
}

// ── Search input ──────────────────────────────────────────────────────────────

pub(super) fn render_search_input(f: &mut Frame, area: Rect, prompt: &str, input: &str) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            format!(" 🔍  {} ", prompt),
            Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(YELLOW));

    let display = format!("{}_", input);
    let para = Paragraph::new(Span::styled(display, Style::default().fg(TEXT))).block(block);
    f.render_widget(para, area);
}
