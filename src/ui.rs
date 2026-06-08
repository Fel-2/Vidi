use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::{
    App, ChatScreen, ListScreen, MessageKind, QualitySelectScreen, Screen, VideoActionsScreen,
};
use crate::chat::irc_color_to_ratatui;
use crate::models::ItemData;

// ── Catppuccin Mocha palette ─────────────────────────────────────────────────
const MAUVE:    Color = Color::Rgb(203, 166, 247);
const LAVENDER: Color = Color::Rgb(180, 190, 254);
const BLUE:     Color = Color::Rgb(137, 180, 250);
const TEAL:     Color = Color::Rgb(148, 226, 213);
const GREEN:    Color = Color::Rgb(166, 227, 161);
const YELLOW:   Color = Color::Rgb(249, 226, 175);
const PEACH:    Color = Color::Rgb(250, 179, 135);
const RED:      Color = Color::Rgb(243, 139, 168);
const TEXT:     Color = Color::Rgb(205, 214, 244);
const SUBTEXT:  Color = Color::Rgb(147, 153, 178);
const OVERLAY:  Color = Color::Rgb(108, 112, 134);
const SURFACE:  Color = Color::Rgb(49,  50,  68 );

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ── ASCII art logo ────────────────────────────────────────────────────────────
const YV_ART: &[&str] = &[
    "██╗░░██╗██╗██████╗░██╗",
    "██║░░██║██║██╔══██╗██║",
    "╚██╗██╔╝██║██║░░██║██║",
    "░████╔╝░██║██║░░██║██║",
    "░╚██╔╝░░██║██████╔╝██║",
    "░░╚═╝░░░╚═╝╚═════╝░╚═╝",
];

// ── Public constants (values must not change) ─────────────────────────────────

pub const YOUTUBE_MENU_ITEMS: &[&str] = &[
    "Trending",
    "Search",
    "Subscription Feed",
    "Channels",
    "Custom Playlists",
    "Recent",
    "Saved Videos",
    "Edit Config",
    "Miscellaneous",
];

pub const TWITCH_MENU_ITEMS: &[&str] = &[
    "Search Live",
    "Live Subscriptions",
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
    "Open in Browser",
    "Back",
];

// ── Display labels with emojis (parallel to the constants above) ─────────────

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
    "🌐  Open in Browser",
    "←  Back",
];

const CHANNEL_TABS_DISPLAY: &[&str] =
    &["📹  Videos", "📱  Shorts", "📡  Streams", "📋  Playlists", "🔍  Search"];

const YOUTUBE_MENU_DISPLAY: &[&str] = &[
    "🔥  Trending",
    "🔍  Search",
    "📡  Subscription Feed",
    "📋  Channels",
    "🎯  Custom Playlists",
    "🕐  Recent",
    "❤️  Saved Videos",
    "⚙️  Edit Config",
    "🎲  Miscellaneous",
];

const TWITCH_MENU_DISPLAY: &[&str] = &[
    "🔍  Search Live",
    "💜  Live Subscriptions",
    "🎬  Watch VODs",
    "✏️  Edit Subs",
];

// ── Public helpers ────────────────────────────────────────────────────────────

pub fn channel_action_items(subscribed: bool) -> Vec<String> {
    let mut items: Vec<String> = CHANNEL_TABS.iter().map(|s| s.to_string()).collect();
    if !subscribed {
        items.push("Subscribe".to_string());
    }
    items
}

pub fn twitch_stream_action_items() -> Vec<String> {
    vec![
        "Watch Stream".to_string(),
        "Open Chat".to_string(),
        "Watch + Chat".to_string(),
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
        Span::styled("vidi", Style::default().fg(MAUVE).add_modifier(Modifier::BOLD)),
        Span::raw(" ▐  "),
        Span::styled(format!("{} {}", emoji, name), Style::default().fg(LAVENDER).add_modifier(Modifier::BOLD)),
        Span::raw(" "),
    ]);

    f.render_widget(Paragraph::new(line).block(block), area);
}

fn screen_emoji_and_title(screen: &Screen) -> (&'static str, &'static str) {
    match screen {
        Screen::ModeSelect { .. }      => ("🎬", "Mode Select"),
        Screen::YoutubeMenu { .. }     => ("📺", "YouTube"),
        Screen::TwitchMenu { .. }      => ("🟣", "Twitch"),
        Screen::List(_)                => ("📋", "List"),
        Screen::VideoActions(_)        => ("🎬", "Video Actions"),
        Screen::QualitySelect(_)       => ("🎚️", "Select Quality"),
        Screen::ChannelActions(_)      => ("📋", "Channel"),
        Screen::TwitchStreamActions(_) => ("🟣", "Stream Actions"),
        Screen::TwitchVodActions(_)    => ("🎬", "VOD Actions"),
        Screen::SearchInput(_)         => ("🔍", "Search"),
        Screen::TwitchChat(_)          => ("💬", "Twitch Chat"),
    }
}

// ── Content router ────────────────────────────────────────────────────────────

fn render_content(f: &mut Frame, app: &mut App, area: Rect) {
    let screen = app.current_screen().clone();
    match screen {
        Screen::ModeSelect { selected } => render_mode_select(f, area, selected),
        Screen::YoutubeMenu { selected } => render_youtube_menu(f, area, selected),
        Screen::TwitchMenu { selected } => render_twitch_menu(f, area, selected),
        Screen::List(_) => {
            let ls = match app.current_screen().clone() {
                Screen::List(ls) => ls,
                _ => unreachable!(),
            };
            render_list_screen(f, area, &ls, app);
        }
        Screen::VideoActions(ref va) => render_video_actions(f, area, va),
        Screen::QualitySelect(ref qs) => render_quality_select(f, area, qs),
        Screen::ChannelActions(ref ca) => {
            let mut labels: Vec<String> =
                CHANNEL_TABS_DISPLAY.iter().map(|s| s.to_string()).collect();
            if !ca.subscribed {
                labels.push("➕  Subscribe".to_string());
            }
            render_action_menu_string(
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
            let labels: Vec<String> = vec![
                "📺  Watch Stream".to_string(),
                "💬  Open Chat".to_string(),
                "🎬  Watch + Chat".to_string(),
                "←  Back".to_string(),
            ];
            let title = format!(
                "🟣  Stream: {} | {} | {}",
                sa.stream.login, sa.stream.game, sa.stream.title
            );
            render_action_menu_string(f, area, &title, &labels, sa.selected, MAUVE, MAUVE);
        }
        Screen::TwitchVodActions(ref va) => {
            let labels: Vec<String> = vec![
                "▶️  Watch VOD".to_string(),
                "⬇️  Download".to_string(),
                "🌐  Open in Browser".to_string(),
                "←  Back".to_string(),
            ];
            let title = format!("🎬  VOD: {}", va.vod.title);
            render_action_menu_string(f, area, &title, &labels, va.selected, MAUVE, MAUVE);
        }
        Screen::SearchInput(ref si) => render_search_input(f, area, &si.prompt, &si.input),
        Screen::TwitchChat(ref cs) => render_chat(f, area, cs),
    }
}

// ── Mode select ───────────────────────────────────────────────────────────────

fn render_mode_select(f: &mut Frame, area: Rect, selected: usize) {
    // Center the ASCII art + menu vertically
    let art_height = YV_ART.len() as u16;
    let menu_items = ["📺  YouTube", "🟣  Twitch"];
    let menu_height = menu_items.len() as u16;
    // gap between art and menu
    let gap: u16 = 1;
    let total_inner = art_height + gap + menu_height;
    let v_pad = area.height.saturating_sub(total_inner) / 2;

    // Build lines: top padding + art + gap + menu items
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

fn render_youtube_menu(f: &mut Frame, area: Rect, selected: usize) {
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
        .title(Span::styled(" 📺  YouTube ", Style::default().fg(BLUE).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(BLUE));

    f.render_widget(List::new(items).block(block), area);
}

// ── Twitch menu ───────────────────────────────────────────────────────────────

fn render_twitch_menu(f: &mut Frame, area: Rect, selected: usize) {
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
        .title(Span::styled(" 🟣  Twitch ", Style::default().fg(MAUVE).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(MAUVE));

    f.render_widget(List::new(items).block(block), area);
}

// ── Generic action menu (string labels, configurable accent) ──────────────────

fn render_action_menu_string(
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

// ── List screen ───────────────────────────────────────────────────────────────

fn render_list_screen(f: &mut Frame, area: Rect, ls: &ListScreen, app: &mut App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    // Filter bar
    let filter_block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" 🔍  Filter ", Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(YELLOW));
    let filter_text = if ls.filter.is_empty() {
        Span::styled("type to filter…", Style::default().fg(OVERLAY).add_modifier(Modifier::ITALIC))
    } else {
        Span::styled(format!("{}_", ls.filter), Style::default().fg(TEXT))
    };
    f.render_widget(Paragraph::new(Line::from(filter_text)).block(filter_block), outer[0]);

    // Body: list | preview
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(outer[1]);

    render_item_list(f, body[0], ls, app);
    render_preview_panel(f, body[1], ls, app);
}

fn render_item_list(f: &mut Frame, area: Rect, ls: &ListScreen, app: &App) {
    let filtered = ls.filtered_items();
    let visible_height = area.height.saturating_sub(2) as usize;
    let scroll_start = ls.scroll_offset;

    let mut visible_items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .skip(scroll_start)
        .take(visible_height)
        .map(|(i, item)| {
            let (saved, watched) = match &item.data {
                ItemData::YoutubeVideo(v) => (
                    app.saved_ids.contains(&v.id),
                    app.watched_ids.contains(&v.id),
                ),
                _ => (false, false),
            };
            let prefix = match (saved, watched) {
                (true, true)  => " ❤✓",
                (true, false) => " ❤ ",
                (false, true) => "  ✓",
                (false, false) => "   ",
            };
            let style = if i == ls.selected {
                Style::default()
                    .fg(TEAL)
                    .bg(SURFACE)
                    .add_modifier(Modifier::BOLD)
            } else if watched {
                Style::default().fg(SUBTEXT)
            } else {
                item_style_for_data(&item.data)
            };
            ListItem::new(format!("{}{}", prefix, item.display)).style(style)
        })
        .collect();

    // "Load More" row (sub feed or channel tab)
    let load_more_label = ls.load_more.as_ref().map(|lm| &lm.label)
        .or_else(|| ls.channel_load_more.as_ref().map(|lm| &lm.label));
    if let Some(label) = load_more_label {
        let lm_idx = filtered.len();
        if lm_idx >= scroll_start && visible_items.len() < visible_height {
            let style = if ls.selected == lm_idx {
                Style::default()
                    .fg(YELLOW)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(YELLOW)
            };
            visible_items.push(ListItem::new(format!(" {}", label)).style(style));
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            format!(" 📋  {} ({}) ", ls.title, filtered.len()),
            Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(BLUE));

    f.render_widget(List::new(visible_items).block(block), area);
}

fn render_preview_panel(f: &mut Frame, area: Rect, ls: &ListScreen, app: &mut App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" 🖼  Preview ", Style::default().fg(OVERLAY)))
        .border_style(Style::default().fg(OVERLAY));

    let filtered = ls.filtered_items();
    let selected = filtered.get(ls.selected);

    // Determine cache key and whether this item has a preview at all.
    let cache_key: Option<String> = selected.and_then(|item| match &item.data {
        ItemData::YoutubeVideo(v) => Some(v.id.clone()),
        ItemData::TwitchStream(s) if s.is_live => Some(format!("twitch_{}", s.login)),
        ItemData::TwitchVod(v) if !v.thumbnail.is_empty() => Some(format!("twitchvod_{}", v.id)),
        ItemData::Channel(c) => Some(crate::preview::channel_cache_key(&c.url)),
        _ => None,
    });

    let inner = block.inner(area);
    f.render_widget(block, area);

    let inner_w = inner.width as usize;
    let inner_h = inner.height as usize;
    let thumb_height = (inner_h / 2).clamp(5, 14);

    app.preview_thumb_area = if cache_key.is_some() {
        Some((inner.x, inner.y, inner.width, thumb_height as u16))
    } else {
        None
    };

    // Thumbnail placeholder rows
    let mut lines: Vec<Line> = Vec::new();
    if let Some(ref key) = cache_key {
        let entry = app.preview_cache.get(key);
        let status = match entry {
            None => "  no preview",
            Some(e) if !e.ready => "  loading…",
            Some(_) => "",
        };
        lines.push(Line::from(Span::styled(
            status,
            Style::default().fg(OVERLAY).add_modifier(Modifier::ITALIC),
        )));
        for _ in 1..thumb_height {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(
            "─".repeat(inner_w),
            Style::default().fg(OVERLAY),
        )));
    }

    let label = |s: &str| Span::styled(
        s.to_string(),
        Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
    );

    // Per-item metadata
    if let Some(item) = selected {
        match &item.data {
            ItemData::YoutubeVideo(v) => {
                lines.push(Line::from(vec![
                    label("Title    "),
                    Span::styled(truncate_str(&v.title, inner_w.saturating_sub(9)), Style::default().fg(TEXT)),
                ]));
                lines.push(Line::from(vec![
                    label("Channel  "),
                    Span::styled(truncate_str(&v.channel, inner_w.saturating_sub(9)), Style::default().fg(TEXT)),
                ]));
                let date_str = if let Some(ts) = v.timestamp {
                    relative_time(ts)
                } else if !v.upload_date.is_empty() {
                    format_date(&v.upload_date)
                } else {
                    String::new()
                };
                if !date_str.is_empty() {
                    lines.push(Line::from(vec![
                        label("Date     "),
                        Span::styled(date_str, Style::default().fg(TEXT)),
                    ]));
                }
                if !v.duration_string.is_empty() {
                    lines.push(Line::from(vec![
                        label("Duration "),
                        Span::styled(v.duration_string.clone(), Style::default().fg(TEXT)),
                    ]));
                }
                if let Some(views) = v.view_count {
                    lines.push(Line::from(vec![
                        label("Views    "),
                        Span::styled(format_views(views), Style::default().fg(TEXT)),
                    ]));
                }
                if let Some(ref desc) = v.description {
                    if !desc.is_empty() {
                        lines.push(Line::from(""));
                        lines.push(Line::from(Span::styled(
                            "Description",
                            Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
                        )));
                        let remaining = inner_h.saturating_sub(lines.len()).saturating_sub(1);
                        for (i, dline) in desc.lines().enumerate() {
                            if i >= remaining { break; }
                            lines.push(Line::from(Span::styled(
                                truncate_str(dline, inner_w),
                                Style::default().fg(SUBTEXT),
                            )));
                        }
                    }
                }
            }
            ItemData::TwitchStream(s) => {
                let status_style = if s.is_live {
                    Style::default().fg(GREEN).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(OVERLAY)
                };
                lines.push(Line::from(vec![
                    label("Channel  "),
                    Span::styled(truncate_str(&s.login, inner_w.saturating_sub(9)), Style::default().fg(TEXT)),
                ]));
                lines.push(Line::from(vec![
                    label("Status   "),
                    Span::styled(if s.is_live { "🔴 LIVE" } else { "⚫ Offline" }, status_style),
                ]));
                if !s.game.is_empty() {
                    lines.push(Line::from(vec![
                        label("Game     "),
                        Span::styled(truncate_str(&s.game, inner_w.saturating_sub(9)), Style::default().fg(TEXT)),
                    ]));
                }
                if s.viewers > 0 {
                    lines.push(Line::from(vec![
                        label("Viewers  "),
                        Span::styled(format_views(s.viewers), Style::default().fg(TEXT)),
                    ]));
                }
                if !s.title.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled("Title", Style::default().fg(TEAL).add_modifier(Modifier::BOLD))));
                    lines.push(Line::from(Span::styled(
                        truncate_str(&s.title, inner_w),
                        Style::default().fg(SUBTEXT),
                    )));
                }
            }
            ItemData::TwitchVod(v) => {
                lines.push(Line::from(vec![
                    label("Title    "),
                    Span::styled(truncate_str(&v.title, inner_w.saturating_sub(9)), Style::default().fg(TEXT)),
                ]));
                if !v.duration.is_empty() {
                    lines.push(Line::from(vec![
                        label("Duration "),
                        Span::styled(v.duration.clone(), Style::default().fg(TEXT)),
                    ]));
                }
                if !v.upload_date.is_empty() {
                    lines.push(Line::from(vec![
                        label("Date     "),
                        Span::styled(format_date(&v.upload_date), Style::default().fg(TEXT)),
                    ]));
                }
            }
            ItemData::Channel(c) => {
                lines.push(Line::from(vec![
                    label("Channel  "),
                    Span::styled(truncate_str(&c.name, inner_w.saturating_sub(9)), Style::default().fg(TEXT)),
                ]));
                lines.push(Line::from(vec![
                    label("URL      "),
                    Span::styled(truncate_str(&c.url, inner_w.saturating_sub(9)), Style::default().fg(SUBTEXT)),
                ]));
            }
            _ => {}
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}

// ── Video actions ─────────────────────────────────────────────────────────────

fn render_video_actions(f: &mut Frame, area: Rect, va: &VideoActionsScreen) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(1)])
        .split(area);

    // Video info panel
    let info = vec![
        Line::from(vec![
            Span::styled("Title    ", Style::default().fg(TEAL).add_modifier(Modifier::BOLD)),
            Span::styled(va.video.title.clone(), Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("Channel  ", Style::default().fg(TEAL).add_modifier(Modifier::BOLD)),
            Span::styled(va.video.channel.clone(), Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("Date     ", Style::default().fg(TEAL).add_modifier(Modifier::BOLD)),
            Span::styled(va.video.upload_date.clone(), Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("Duration ", Style::default().fg(TEAL).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {}", va.video.duration_string), Style::default().fg(TEXT)),
        ]),
    ];
    let info_block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" 🎬  Video Info ", Style::default().fg(TEAL).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(TEAL));
    let info_para = Paragraph::new(info).block(info_block).wrap(Wrap { trim: true });
    f.render_widget(info_para, chunks[0]);

    // Actions list
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
        .title(Span::styled(" ⚡  Actions ", Style::default().fg(accent).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(accent));

    f.render_widget(List::new(list_items).block(action_block), chunks[1]);
}

fn render_quality_select(f: &mut Frame, area: Rect, qs: &QualitySelectScreen) {
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
        .title(Span::styled(title, Style::default().fg(accent).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(accent));

    f.render_widget(List::new(list_items).block(block), area);
}

// ── Search input ──────────────────────────────────────────────────────────────

fn render_search_input(f: &mut Frame, area: Rect, prompt: &str, input: &str) {
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

// ── Twitch chat ───────────────────────────────────────────────────────────────

fn render_chat(f: &mut Frame, area: Rect, cs: &ChatScreen) {
    let status_suffix = if cs.scroll_offset > 0 {
        format!(" (scrolled {})", cs.scroll_offset)
    } else {
        String::new()
    };

    let header = format!(" 💬  #{} | Connected: {}{} ", cs.channel, cs.connected, status_suffix);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(header, Style::default().fg(MAUVE).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(MAUVE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let height = inner.height as usize;
    let width = inner.width as usize;

    let mut all_lines: Vec<Line> = Vec::new();

    for msg in &cs.messages {
        let color = irc_color_to_ratatui(msg.color);
        let ts_part = format!("[{}] ", msg.timestamp);
        let user_part = format!("{}: ", msg.user);
        let text_part = msg.text.clone();

        let full_text = format!("{}{}{}", ts_part, user_part, text_part);

        let mut remaining = full_text.as_str();
        let mut first_chunk = true;

        while !remaining.is_empty() {
            let chunk_len = remaining
                .char_indices()
                .take(width)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(remaining.len());
            let (chunk, rest) = remaining.split_at(chunk_len);
            remaining = rest;

            if first_chunk {
                let ts_end = ts_part.len().min(chunk.len());
                let user_start = ts_end;
                let user_end = (ts_end + user_part.len()).min(chunk.len());
                let text_start = user_end;

                let mut spans = Vec::new();
                if ts_end > 0 {
                    spans.push(Span::styled(
                        chunk[..ts_end].to_string(),
                        Style::default().fg(OVERLAY),
                    ));
                }
                if user_end > user_start {
                    spans.push(Span::styled(
                        chunk[user_start..user_end].to_string(),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ));
                }
                if text_start < chunk.len() {
                    spans.push(Span::styled(
                        chunk[text_start..].to_string(),
                        Style::default().fg(TEXT),
                    ));
                }
                all_lines.push(Line::from(spans));
                first_chunk = false;
            } else {
                all_lines.push(Line::from(vec![Span::styled(
                    format!("  {}", chunk),
                    Style::default().fg(SUBTEXT),
                )]));
            }
        }
    }

    let total = all_lines.len();
    let end = total.saturating_sub(cs.scroll_offset);
    let start = end.saturating_sub(height);
    let visible: Vec<Line> = all_lines[start..end].to_vec();

    f.render_widget(Paragraph::new(visible), inner);
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
        .title(Span::styled(" ⏳  Loading ", Style::default().fg(PEACH).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(PEACH));
    let para = Paragraph::new(Span::styled(text, Style::default().fg(PEACH))).block(block);
    f.render_widget(para, popup_area);
}

// ── Status bar ────────────────────────────────────────────────────────────────

fn render_statusbar(f: &mut Frame, app: &App, area: Rect) {
    let (msg_text, msg_style) = if let Some((ref msg, ref kind)) = app.message {
        let style = match kind {
            MessageKind::Error   => Style::default().fg(RED).add_modifier(Modifier::BOLD),
            MessageKind::Success => Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
            MessageKind::Info    => Style::default().fg(TEAL),
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
            "↑↓ navigate   ↵ select   q quit".to_string()
        }
        Screen::List(_) => {
            "↑↓ navigate   type to filter   ↵ select   ⎋ back   q quit".to_string()
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

fn item_style_for_data(data: &ItemData) -> Style {
    match data {
        ItemData::TwitchStream(s) => {
            if s.is_live {
                Style::default().fg(GREEN).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(OVERLAY)
            }
        }
        ItemData::YoutubeVideo(_) => Style::default().fg(TEXT),
        ItemData::TwitchVod(_)    => Style::default().fg(SUBTEXT),
        ItemData::Channel(_)      => Style::default().fg(SUBTEXT),
        ItemData::CustomPlaylist(_) => Style::default().fg(SUBTEXT),
        ItemData::Text(_)         => Style::default().fg(SUBTEXT),
    }
}

fn truncate_str(s: &str, max: usize) -> String {
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

fn format_date(d: &str) -> String {
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
        if m == 1 { "1 minute ago".to_string() } else { format!("{} minutes ago", m) }
    } else if diff < 86400 {
        let h = diff / 3600;
        if h == 1 { "1 hour ago".to_string() } else { format!("{} hours ago", h) }
    } else if diff < 7 * 86400 {
        let d = diff / 86400;
        if d == 1 { "1 day ago".to_string() } else { format!("{} days ago", d) }
    } else if diff < 30 * 86400 {
        let w = diff / (7 * 86400);
        if w == 1 { "1 week ago".to_string() } else { format!("{} weeks ago", w) }
    } else if diff < 365 * 86400 {
        let mo = diff / (30 * 86400);
        if mo == 1 { "1 month ago".to_string() } else { format!("{} months ago", mo) }
    } else {
        let y = diff / (365 * 86400);
        if y == 1 { "1 year ago".to_string() } else { format!("{} years ago", y) }
    }
}

fn format_views(v: u64) -> String {
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
