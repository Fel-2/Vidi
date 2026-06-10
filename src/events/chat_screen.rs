//! Twitch chat screen scrolling.

use crate::app::{App, Screen};
use crossterm::event::{self, KeyCode};

pub(super) fn handle_chat(app: &mut App, key: event::KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.pop_screen();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if let Screen::TwitchChat(ref mut cs) = app.current_screen_mut() {
                cs.scroll_offset += 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Screen::TwitchChat(ref mut cs) = app.current_screen_mut() {
                if cs.scroll_offset > 0 {
                    cs.scroll_offset -= 1;
                }
            }
        }
        KeyCode::PageUp => {
            if let Screen::TwitchChat(ref mut cs) = app.current_screen_mut() {
                cs.scroll_offset += 10;
            }
        }
        KeyCode::PageDown => {
            if let Screen::TwitchChat(ref mut cs) = app.current_screen_mut() {
                if cs.scroll_offset >= 10 {
                    cs.scroll_offset -= 10;
                } else {
                    cs.scroll_offset = 0;
                }
            }
        }
        _ => {}
    }
}
