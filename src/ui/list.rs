//! List screen: filter bar, item list, preview/metadata panel.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use super::{
    format_date, format_views, item_style_for_data, relative_time, truncate_str, BLUE, GREEN,
    OVERLAY, SUBTEXT, SURFACE, TEAL, TEXT, YELLOW,
};
use crate::app::{App, ListScreen};
use crate::models::ItemData;

pub(super) fn render_list_screen(f: &mut Frame, area: Rect, ls: &ListScreen, app: &mut App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    // Filter bar
    let filter_block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " 🔍  Filter ",
            Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(YELLOW));
    let filter_text = if ls.filter.is_empty() {
        Span::styled(
            "type to filter…",
            Style::default().fg(OVERLAY).add_modifier(Modifier::ITALIC),
        )
    } else {
        Span::styled(format!("{}_", ls.filter), Style::default().fg(TEXT))
    };
    f.render_widget(
        Paragraph::new(Line::from(filter_text)).block(filter_block),
        outer[0],
    );

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
                (true, true) => " ❤✓",
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
    let load_more_label = ls
        .load_more
        .as_ref()
        .map(|lm| &lm.label)
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
        ItemData::TwitchGame(g) if !g.box_art.is_empty() => {
            Some(crate::preview::twitch_game_cache_key(&g.name))
        }
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

    let label = |s: &str| {
        Span::styled(
            s.to_string(),
            Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
        )
    };

    // Per-item metadata
    if let Some(item) = selected {
        match &item.data {
            ItemData::YoutubeVideo(v) => {
                lines.push(Line::from(vec![
                    label("Title    "),
                    Span::styled(
                        truncate_str(&v.title, inner_w.saturating_sub(9)),
                        Style::default().fg(TEXT),
                    ),
                ]));
                lines.push(Line::from(vec![
                    label("Channel  "),
                    Span::styled(
                        truncate_str(&v.channel, inner_w.saturating_sub(9)),
                        Style::default().fg(TEXT),
                    ),
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
                            if i >= remaining {
                                break;
                            }
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
                    Span::styled(
                        truncate_str(&s.login, inner_w.saturating_sub(9)),
                        Style::default().fg(TEXT),
                    ),
                ]));
                lines.push(Line::from(vec![
                    label("Status   "),
                    Span::styled(
                        if s.is_live {
                            "🔴 LIVE"
                        } else {
                            "⚫ Offline"
                        },
                        status_style,
                    ),
                ]));
                if !s.game.is_empty() {
                    lines.push(Line::from(vec![
                        label("Game     "),
                        Span::styled(
                            truncate_str(&s.game, inner_w.saturating_sub(9)),
                            Style::default().fg(TEXT),
                        ),
                    ]));
                }
                if s.viewers > 0 {
                    lines.push(Line::from(vec![
                        label("Viewers  "),
                        Span::styled(format_views(s.viewers), Style::default().fg(TEXT)),
                    ]));
                }
                if !s.uptime.is_empty() {
                    lines.push(Line::from(vec![
                        label("Uptime   "),
                        Span::styled(s.uptime.clone(), Style::default().fg(TEXT)),
                    ]));
                }
                if !s.title.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "Title",
                        Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
                    )));
                    lines.push(Line::from(Span::styled(
                        truncate_str(&s.title, inner_w),
                        Style::default().fg(SUBTEXT),
                    )));
                }
            }
            ItemData::TwitchVod(v) => {
                lines.push(Line::from(vec![
                    label("Title    "),
                    Span::styled(
                        truncate_str(&v.title, inner_w.saturating_sub(9)),
                        Style::default().fg(TEXT),
                    ),
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
                if !v.game.is_empty() {
                    lines.push(Line::from(vec![
                        label("Game     "),
                        Span::styled(
                            truncate_str(&v.game, inner_w.saturating_sub(9)),
                            Style::default().fg(TEXT),
                        ),
                    ]));
                }
                if v.view_count > 0 {
                    lines.push(Line::from(vec![
                        label("Views    "),
                        Span::styled(format_views(v.view_count), Style::default().fg(TEXT)),
                    ]));
                }
            }
            ItemData::Channel(c) => {
                lines.push(Line::from(vec![
                    label("Channel  "),
                    Span::styled(
                        truncate_str(&c.name, inner_w.saturating_sub(9)),
                        Style::default().fg(TEXT),
                    ),
                ]));
                lines.push(Line::from(vec![
                    label("URL      "),
                    Span::styled(
                        truncate_str(&c.url, inner_w.saturating_sub(9)),
                        Style::default().fg(SUBTEXT),
                    ),
                ]));
            }
            ItemData::TwitchGame(g) => {
                lines.push(Line::from(vec![
                    label("Category "),
                    Span::styled(
                        truncate_str(&g.name, inner_w.saturating_sub(9)),
                        Style::default().fg(TEXT),
                    ),
                ]));
                lines.push(Line::from(vec![
                    label("Viewers  "),
                    Span::styled(format_views(g.viewers), Style::default().fg(TEXT)),
                ]));
            }
            _ => {}
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}
