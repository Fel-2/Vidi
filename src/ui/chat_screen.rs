//! Twitch chat rendering.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use ratatui::style::Color;

use super::{MAUVE, OVERLAY, SUBTEXT, TEXT};
use crate::app::ChatScreen;

pub(super) fn render_chat(f: &mut Frame, area: Rect, cs: &ChatScreen) {
    let status_suffix = if cs.scroll_offset > 0 {
        format!(" (scrolled {})", cs.scroll_offset)
    } else {
        String::new()
    };

    let header = format!(
        " 💬  #{} | Connected: {}{} ",
        cs.channel, cs.connected, status_suffix
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            header,
            Style::default().fg(MAUVE).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(MAUVE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let height = inner.height as usize;
    let width = inner.width as usize;

    let mut all_lines: Vec<Line> = Vec::new();

    for msg in &cs.messages {
        let (r, g, b) = msg.color;
        let color = Color::Rgb(r, g, b);
        let ts_part = format!("[{}] ", msg.timestamp);
        let badge_part = if msg.badges.is_empty() {
            String::new()
        } else {
            format!("{} ", msg.badges)
        };
        let user_part = format!("{}{}: ", badge_part, msg.user);
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
