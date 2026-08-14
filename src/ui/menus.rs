//! Menu screens: mode select (logo), YouTube/Twitch menus, generic action menu.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use super::{BLUE, LAVENDER, MAUVE, PEACH, SUBTEXT, TEAL};

// ── ASCII art logo ────────────────────────────────────────────────────────────
const YV_ART: &[&str] = &[
    "██╗░░██╗██╗██████╗░██╗",
    "██║░░██║██║██╔══██╗██║",
    "╚██╗██╔╝██║██║░░██║██║",
    "░████╔╝░██║██║░░██║██║",
    "░╚██╔╝░░██║██████╔╝██║",
    "░░╚═╝░░░╚═╝╚═════╝░╚═╝",
];

const YOUTUBE_MENU_DISPLAY: &[&str] = &[
    "🔥  Trending",
    "🔍  Search",
    "📡  Subscription Feed",
    "📋  Channels",
    "🎯  Custom Playlists",
    "🕐  Recent",
    "❤️  Saved Videos",
    "⏭  Queue",
    "⚙️  Edit Config",
    "🎲  Miscellaneous",
];

const TWITCH_MENU_DISPLAY: &[&str] = &[
    "🔍  Search Live",
    "💜  Live Subscriptions",
    "🔥  Top Streams",
    "🗂  Browse Categories",
    "🎬  Watch VODs",
    "✏️  Edit Subs",
];

const PEERTUBE_MENU_DISPLAY: &[&str] = &[
    "🔥  Trending",
    "🆕  Recently Added",
    "🔍  Search",
    "📡  Subscription Feed",
    "📋  Subscribed Channels",
    "🔎  Explore Channels",
    "✏️  Edit Subs",
    "🌐  Change Instance",
];

// ── Mode select ───────────────────────────────────────────────────────────────

pub(super) fn render_mode_select(f: &mut Frame, area: Rect, selected: usize) {
    let art_height = YV_ART.len() as u16;
    let menu_items = ["📺  YouTube", "🟣  Twitch", "🐙  PeerTube"];
    let menu_height = menu_items.len() as u16;
    let gap: u16 = 1;
    let total_inner = art_height + 1 + gap + menu_height;
    let v_pad = area.height.saturating_sub(total_inner) / 2;

    let mut lines: Vec<Line> = Vec::new();
    for _ in 0..v_pad {
        lines.push(Line::from(""));
    }
    for art_line in YV_ART {
        lines.push(Line::from(Span::styled(
            *art_line,
            Style::default().fg(MAUVE).add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::from(Span::styled(
        format!("v{}", crate::update::current_version()),
        Style::default().fg(SUBTEXT),
    )));
    for _ in 0..gap {
        lines.push(Line::from(""));
    }

    for (i, label) in menu_items.iter().enumerate() {
        let style = if i == selected {
            Style::default()
                .fg(TEAL)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default().fg(SUBTEXT)
        };
        let padding = " ".repeat(2);
        lines.push(Line::from(Span::styled(
            format!("{}{}  ", padding, label),
            style,
        )));
    }

    let para = Paragraph::new(lines).alignment(Alignment::Center);
    f.render_widget(para, area);
}

// ── YouTube menu ──────────────────────────────────────────────────────────────

pub(super) fn render_youtube_menu(f: &mut Frame, area: Rect, selected: usize) {
    let items: Vec<ListItem> = YOUTUBE_MENU_DISPLAY
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let style = if i == selected {
                Style::default()
                    .fg(TEAL)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(BLUE)
            };
            ListItem::new(format!("  {}  ", label)).style(style)
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " 📺  YouTube ",
            Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(BLUE));

    f.render_widget(List::new(items).block(block), area);
}

// ── Twitch menu ───────────────────────────────────────────────────────────────

pub(super) fn render_twitch_menu(f: &mut Frame, area: Rect, selected: usize) {
    let items: Vec<ListItem> = TWITCH_MENU_DISPLAY
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let style = if i == selected {
                Style::default()
                    .fg(MAUVE)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(LAVENDER)
            };
            ListItem::new(format!("  {}  ", label)).style(style)
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " 🟣  Twitch ",
            Style::default().fg(MAUVE).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(MAUVE));

    f.render_widget(List::new(items).block(block), area);
}

// ── PeerTube menu ─────────────────────────────────────────────────────────────

pub(super) fn render_peertube_menu(f: &mut Frame, area: Rect, selected: usize) {
    let items: Vec<ListItem> = PEERTUBE_MENU_DISPLAY
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let style = if i == selected {
                Style::default()
                    .fg(PEACH)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(LAVENDER)
            };
            ListItem::new(format!("  {}  ", label)).style(style)
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " 🐙  PeerTube ",
            Style::default().fg(PEACH).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(PEACH));

    f.render_widget(List::new(items).block(block), area);
}

// ── Generic action menu (string labels, configurable accent) ──────────────────

pub(super) fn render_action_menu_string(
    f: &mut Frame,
    area: Rect,
    title: &str,
    items: &[String],
    selected: usize,
    accent: Color,
    border: Color,
) {
    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let style = if i == selected {
                Style::default()
                    .fg(accent)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(LAVENDER)
            };
            ListItem::new(format!("  {}  ", label)).style(style)
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", title))
        .border_style(Style::default().fg(border));

    f.render_widget(List::new(list_items).block(block), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn mode_select_shows_version() {
        let mut terminal = Terminal::new(TestBackend::new(60, 24)).unwrap();
        terminal
            .draw(|f| render_mode_select(f, f.area(), 0))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains(&format!("v{}", crate::update::current_version())));
    }
}
