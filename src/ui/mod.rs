//! Rendering: one submodule per screen group, shared palette and helpers here.

mod actions;
mod chat_screen;
mod list;
mod menus;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::{App, MessageKind, Screen};
use crate::models::ItemData;

// ── Catppuccin Mocha palette ─────────────────────────────────────────────────
pub(crate) const MAUVE: Color = Color::Rgb(203, 166, 247);
pub(crate) const LAVENDER: Color = Color::Rgb(180, 190, 254);
pub(crate) const BLUE: Color = Color::Rgb(137, 180, 250);
pub(crate) const TEAL: Color = Color::Rgb(148, 226, 213);
pub(crate) const GREEN: Color = Color::Rgb(166, 227, 161);
pub(crate) const YELLOW: Color = Color::Rgb(249, 226, 175);
pub(crate) const PEACH: Color = Color::Rgb(250, 179, 135);
pub(crate) const RED: Color = Color::Rgb(243, 139, 168);
pub(crate) const TEXT: Color = Color::Rgb(205, 214, 244);
pub(crate) const SUBTEXT: Color = Color::Rgb(147, 153, 178);
pub(crate) const OVERLAY: Color = Color::Rgb(108, 112, 134);
pub(crate) const SURFACE: Color = Color::Rgb(49, 50, 68);

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ── Public constants (values must not change) ─────────────────────────────────

pub const YOUTUBE_MENU_ITEMS: &[&str] = &[
    "Trending",
    "Search",
    "Subscription Feed",
    "Channels",
    "Custom Playlists",
    "Recent",
    "Saved Videos",
    "Queue",
    "Edit Config",
    "Miscellaneous",
];

pub const TWITCH_MENU_ITEMS: &[&str] = &[
    "Search Live",
    "Live Subscriptions",
    "Top Streams",
    "Browse Categories",
    "Watch VODs",
    "Edit Subs",
];

pub const CHANNEL_TABS: &[&str] = &["Videos", "Shorts", "Streams", "Playlists", "Search"];

pub const VIDEO_ACTION_ITEMS: &[&str] = &[
    "Watch",
    "Watch (Select Quality)",
    "Play All",
    "Download",
    "Download (Audio Only)",
    "Download All",
    "Download All (Audio Only)",
    "Save",
    "UnSave",
    "Save Playlist",
    "Add to Queue",
    "Open in Browser",
    "Back",
];

pub(crate) const CHANNEL_TABS_DISPLAY: &[&str] = &[
    "📹  Videos",
    "📱  Shorts",
    "📡  Streams",
    "📋  Playlists",
    "🔍  Search",
];

// ── Public helpers ────────────────────────────────────────────────────────────

pub fn channel_action_items(subscribed: bool, show_shorts: bool) -> Vec<String> {
    let mut items: Vec<String> = CHANNEL_TABS
        .iter()
        .filter(|t| show_shorts || **t != "Shorts")
        .map(|s| s.to_string())
        .collect();
    if !subscribed {
        items.push("Subscribe".to_string());
    }
    items
}

pub fn twitch_stream_action_items(followed: bool) -> Vec<String> {
    vec![
        "Watch Stream".to_string(),
        "Open Chat".to_string(),
        "Watch + Chat".to_string(),
        "Watch VODs".to_string(),
        if followed {
            "Unfollow".to_string()
        } else {
            "Follow".to_string()
        },
        "Back".to_string(),
    ]
}

pub fn twitch_vod_action_items() -> Vec<String> {
    vec![
        "Watch VOD".to_string(),
        "Download".to_string(),
        "Open in Browser".to_string(),
        "Back".to_string(),
    ]
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn render(f: &mut Frame, app: &mut App) {
    let size = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(size);

    render_titlebar(f, app, chunks[0]);
    render_content(f, app, chunks[1]);
    render_statusbar(f, app, chunks[2]);

    if let Some(ref msg) = app.loading.clone() {
        render_loading(f, size, msg);
    }

    if app.show_help {
        render_help(f, size, &app.config.keys);
    }
}

// ── Help overlay ──────────────────────────────────────────────────────────────

fn render_help(f: &mut Frame, area: Rect, keys: &crate::config::Keybindings) {
    let override_of = |c: Option<char>| c.map(|c| format!(" / {}", c)).unwrap_or_default();
    let rows: Vec<(String, &str)> = vec![
        (format!("↑ / k{}", override_of(keys.up)), "move up"),
        (format!("↓ / j{}", override_of(keys.down)), "move down"),
        (format!("PgUp{}", override_of(keys.page_up)), "page up"),
        (format!("PgDn{}", override_of(keys.page_down)), "page down"),
        (format!("↵{}", override_of(keys.select)), "select"),
        (format!("⎋{}", override_of(keys.back)), "back"),
        (format!("q{}", override_of(keys.quit)), "back / quit"),
        ("Ctrl-C".to_string(), "quit"),
        ("type".to_string(), "filter current list"),
        ("⌫".to_string(), "delete filter character"),
        ("⇥".to_string(), "queue video (in Queue: remove)"),
        ("?".to_string(), "this help"),
    ];
    let extra = [
        "",
        "Lists: ❤ saved   ✓ watched",
        "Search prefixes: :today :week :month :year :live …",
        "Keys are configurable in vidi.conf (KEY_UP, KEY_DOWN, …).",
    ];

    let height = (rows.len() + extra.len() + 2) as u16;
    let width = 60u16.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(area.x + x, area.y + y, width, height.min(area.height));

    f.render_widget(Clear, popup);

    let mut lines: Vec<Line> = rows
        .iter()
        .map(|(key, what)| {
            Line::from(vec![
                Span::styled(
                    format!(" {:<14}", key),
                    Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
                ),
                Span::styled(what.to_string(), Style::default().fg(TEXT)),
            ])
        })
        .collect();
    for line in extra {
        lines.push(Line::from(Span::styled(
            format!(" {}", line),
            Style::default().fg(SUBTEXT),
        )));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " ❓  Help — any key to close ",
            Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(YELLOW));
    f.render_widget(Paragraph::new(lines).block(block), popup);
}

// ── Title bar ─────────────────────────────────────────────────────────────────

fn render_titlebar(f: &mut Frame, app: &App, area: Rect) {
    let screen = app.current_screen();
    let (emoji, name) = screen_emoji_and_title(screen);

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(OVERLAY));

    let line = Line::from(vec![
        Span::raw(" ▌ "),
        Span::styled(
            "vidi",
            Style::default().fg(MAUVE).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ▐  "),
        Span::styled(
            format!("{} {}", emoji, name),
            Style::default().fg(LAVENDER).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ]);

    f.render_widget(Paragraph::new(line).block(block), area);
}

fn screen_emoji_and_title(screen: &Screen) -> (&'static str, &'static str) {
    match screen {
        Screen::ModeSelect { .. } => ("🎬", "Mode Select"),
        Screen::YoutubeMenu { .. } => ("📺", "YouTube"),
        Screen::TwitchMenu { .. } => ("🟣", "Twitch"),
        Screen::List(_) => ("📋", "List"),
        Screen::VideoActions(_) => ("🎬", "Video Actions"),
        Screen::QualitySelect(_) => ("🎚️", "Select Quality"),
        Screen::ChannelActions(_) => ("📋", "Channel"),
        Screen::TwitchStreamActions(_) => ("🟣", "Stream Actions"),
        Screen::TwitchVodActions(_) => ("🎬", "VOD Actions"),
        Screen::SearchInput(_) => ("🔍", "Search"),
        Screen::TwitchChat(_) => ("💬", "Twitch Chat"),
    }
}

// ── Content router ────────────────────────────────────────────────────────────

fn render_content(f: &mut Frame, app: &mut App, area: Rect) {
    let screen = app.current_screen().clone();
    match screen {
        Screen::ModeSelect { selected } => menus::render_mode_select(f, area, selected),
        Screen::YoutubeMenu { selected } => menus::render_youtube_menu(f, area, selected),
        Screen::TwitchMenu { selected } => menus::render_twitch_menu(f, area, selected),
        Screen::List(_) => {
            let ls = match app.current_screen().clone() {
                Screen::List(ls) => ls,
                _ => unreachable!(),
            };
            list::render_list_screen(f, area, &ls, app);
        }
        Screen::VideoActions(ref va) => actions::render_video_actions(f, area, va),
        Screen::QualitySelect(ref qs) => actions::render_quality_select(f, area, qs),
        Screen::ChannelActions(ref ca) => {
            // Keep in lockstep with channel_action_items (same Shorts gate).
            let mut labels: Vec<String> = CHANNEL_TABS_DISPLAY
                .iter()
                .filter(|l| app.config.youtube.show_shorts || !l.ends_with("Shorts"))
                .map(|s| s.to_string())
                .collect();
            if !ca.subscribed {
                labels.push("➕  Subscribe".to_string());
            }
            menus::render_action_menu_string(
                f,
                area,
                &format!("📋  Channel: {}", ca.channel.name),
                &labels,
                ca.selected,
                TEAL,
                BLUE,
            );
        }
        Screen::TwitchStreamActions(ref sa) => {
            let follow_label = if crate::twitch::is_followed(&sa.stream.login) {
                "💔  Unfollow"
            } else {
                "➕  Follow"
            };
            let labels: Vec<String> = vec![
                "📺  Watch Stream".to_string(),
                "💬  Open Chat".to_string(),
                "🎬  Watch + Chat".to_string(),
                "🎞  Watch VODs".to_string(),
                follow_label.to_string(),
                "←  Back".to_string(),
            ];
            let title = format!(
                "🟣  Stream: {} | {} | {}",
                sa.stream.login, sa.stream.game, sa.stream.title
            );
            menus::render_action_menu_string(f, area, &title, &labels, sa.selected, MAUVE, MAUVE);
        }
        Screen::TwitchVodActions(ref va) => {
            let labels: Vec<String> = vec![
                "▶️  Watch VOD".to_string(),
                "⬇️  Download".to_string(),
                "🌐  Open in Browser".to_string(),
                "←  Back".to_string(),
            ];
            let title = format!("🎬  VOD: {}", va.vod.title);
            menus::render_action_menu_string(f, area, &title, &labels, va.selected, MAUVE, MAUVE);
        }
        Screen::SearchInput(ref si) => actions::render_search_input(f, area, &si.prompt, &si.input),
        Screen::TwitchChat(ref cs) => chat_screen::render_chat(f, area, cs),
    }
}

// ── Loading overlay ───────────────────────────────────────────────────────────

fn render_loading(f: &mut Frame, area: Rect, msg: &str) {
    let spinner_idx = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_millis()
        / 100) as usize
        % SPINNER_FRAMES.len();
    let spinner = SPINNER_FRAMES[spinner_idx];

    let popup_width = 50u16.min(area.width.saturating_sub(4));
    let popup_height = 5u16;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect::new(
        area.x + popup_x,
        area.y + popup_y,
        popup_width,
        popup_height,
    );

    f.render_widget(Clear, popup_area);

    let text = format!(" {} {} ", spinner, msg);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " ⏳  Loading ",
            Style::default().fg(PEACH).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(PEACH));
    let para = Paragraph::new(Span::styled(text, Style::default().fg(PEACH))).block(block);
    f.render_widget(para, popup_area);
}

// ── Status bar ────────────────────────────────────────────────────────────────

fn render_statusbar(f: &mut Frame, app: &App, area: Rect) {
    let (msg_text, msg_style) = if let Some((ref msg, ref kind)) = app.message {
        let style = match kind {
            MessageKind::Error => Style::default().fg(RED).add_modifier(Modifier::BOLD),
            MessageKind::Success => Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
            MessageKind::Info => Style::default().fg(TEAL),
        };
        (msg.clone(), style)
    } else {
        let hints = keybind_hints(app.current_screen());
        (hints, Style::default().fg(OVERLAY))
    };

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(OVERLAY));

    f.render_widget(Paragraph::new(msg_text).style(msg_style).block(block), area);
}

fn keybind_hints(screen: &Screen) -> String {
    match screen {
        Screen::ModeSelect { .. } | Screen::YoutubeMenu { .. } | Screen::TwitchMenu { .. } => {
            "↑↓ navigate   ↵ select   ? help   q quit".to_string()
        }
        Screen::List(_) => {
            "↑↓ navigate   type to filter   ↵ select   ⇥ queue   ⎋ back   ? help   q quit"
                .to_string()
        }
        Screen::VideoActions(_)
        | Screen::QualitySelect(_)
        | Screen::ChannelActions(_)
        | Screen::TwitchStreamActions(_)
        | Screen::TwitchVodActions(_) => "↑↓ navigate   ↵ select   ⎋ back".to_string(),
        Screen::SearchInput(_) => "Type query   ↵ submit   ⎋ cancel".to_string(),
        Screen::TwitchChat(_) => "↑↓ scroll   ⎋/q exit".to_string(),
    }
}

// ── Utilities ─────────────────────────────────────────────────────────────────

pub(crate) fn item_style_for_data(data: &ItemData) -> Style {
    match data {
        ItemData::TwitchStream(s) => {
            if s.is_live {
                Style::default().fg(GREEN).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(OVERLAY)
            }
        }
        ItemData::YoutubeVideo(_) => Style::default().fg(TEXT),
        ItemData::TwitchVod(_) => Style::default().fg(SUBTEXT),
        ItemData::TwitchGame(_) => Style::default().fg(MAUVE),
        ItemData::Channel(_) => Style::default().fg(SUBTEXT),
        ItemData::CustomPlaylist(_) => Style::default().fg(SUBTEXT),
        ItemData::Text(_) => Style::default().fg(SUBTEXT),
    }
}

pub(crate) fn truncate_str(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut r: String = s.chars().take(max.saturating_sub(1)).collect();
        r.push('…');
        r
    }
}

pub(crate) fn format_date(d: &str) -> String {
    if d.len() == 8 {
        format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..8])
    } else {
        d.to_string()
    }
}

pub fn relative_time(timestamp: i64) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let diff = now - timestamp;
    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        let m = diff / 60;
        if m == 1 {
            "1 minute ago".to_string()
        } else {
            format!("{} minutes ago", m)
        }
    } else if diff < 86400 {
        let h = diff / 3600;
        if h == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{} hours ago", h)
        }
    } else if diff < 7 * 86400 {
        let d = diff / 86400;
        if d == 1 {
            "1 day ago".to_string()
        } else {
            format!("{} days ago", d)
        }
    } else if diff < 30 * 86400 {
        let w = diff / (7 * 86400);
        if w == 1 {
            "1 week ago".to_string()
        } else {
            format!("{} weeks ago", w)
        }
    } else if diff < 365 * 86400 {
        let mo = diff / (30 * 86400);
        if mo == 1 {
            "1 month ago".to_string()
        } else {
            format!("{} months ago", mo)
        }
    } else {
        let y = diff / (365 * 86400);
        if y == 1 {
            "1 year ago".to_string()
        } else {
            format!("{} years ago", y)
        }
    }
}

pub(crate) fn format_views(v: u64) -> String {
    if v >= 1_000_000_000 {
        format!("{:.1}B", v as f64 / 1_000_000_000.0)
    } else if v >= 1_000_000 {
        format!("{:.1}M", v as f64 / 1_000_000.0)
    } else if v >= 1_000 {
        format!("{:.1}K", v as f64 / 1_000.0)
    } else {
        v.to_string()
    }
}
