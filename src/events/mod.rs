//! Event handling: async app events, keyboard/mouse input, and per-screen
//! action handlers (one submodule per screen group).

mod actions;
mod chat_screen;
mod list;
mod menus;
mod search;

use crate::app::{
    App, AppEvent, ChatMessage, ListContext, ListScreen, MessageKind, PreviewEntry, Screen,
};
use crate::models::ItemData;
use crate::models::ListItem;
use crate::{config, preview};
use crossterm::event::{self, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

pub fn handle_app_event(app: &mut App, event: AppEvent) {
    let show_shorts = app.config.youtube.show_shorts;
    match event {
        AppEvent::YoutubeResults {
            items,
            context,
            title,
            channel_load_more,
        } => {
            app.loading = None;
            let items = crate::youtube::apply_shorts_filter(items, show_shorts);
            let mut ls = App::make_video_list(title, items, context);
            ls.channel_load_more = channel_load_more;
            preview::trigger_preview_for_selected(app, &ls);
            app.push_screen(Screen::List(ls));
        }

        AppEvent::TwitchSearchResults(streams) => {
            app.loading = None;
            let ls =
                App::make_stream_list("Twitch Search", streams, ListContext::TwitchStreamActions);
            app.push_screen(Screen::List(ls));
        }

        AppEvent::TwitchSubsResults(streams) => {
            app.loading = None;
            let ls = App::make_stream_list(
                "Live Subscriptions",
                streams,
                ListContext::TwitchStreamActions,
            );
            app.push_screen(Screen::List(ls));
        }

        AppEvent::TwitchVodsResults(vods) => {
            app.loading = None;
            let ls = App::make_vod_list("VODs", vods, ListContext::TwitchVodActions);
            app.push_screen(Screen::List(ls));
        }

        AppEvent::TwitchTopStreams(streams) => {
            app.loading = None;
            let ls =
                App::make_stream_list("Top Streams", streams, ListContext::TwitchStreamActions);
            app.push_screen(Screen::List(ls));
        }

        AppEvent::TwitchGamesResults(games) => {
            app.loading = None;
            let ls = App::make_game_list("Categories", games, ListContext::SelectGameForStreams);
            app.push_screen(Screen::List(ls));
        }

        AppEvent::ChannelList(channels) => {
            app.loading = None;
            let items: Vec<ListItem> = channels
                .into_iter()
                .map(|ch| {
                    let display = ch.name.clone();
                    ListItem {
                        display,
                        data: ItemData::Channel(ch),
                    }
                })
                .collect();
            let ls = ListScreen::new("Channels", items, ListContext::SelectChannelToBrowse);
            app.push_screen(Screen::List(ls));
        }

        AppEvent::CustomPlaylistResults(playlists) => {
            app.loading = None;
            let items: Vec<ListItem> = playlists
                .into_iter()
                .map(|pl| {
                    let display = pl.name.clone();
                    ListItem {
                        display,
                        data: ItemData::CustomPlaylist(pl),
                    }
                })
                .collect();
            let ls = ListScreen::new(
                "Custom Playlists",
                items,
                ListContext::CustomPlaylistActions,
            );
            app.push_screen(Screen::List(ls));
        }

        AppEvent::ChatMessage {
            user,
            text,
            color,
            badges,
        } => {
            if let Screen::TwitchChat(ref mut cs) = app.current_screen_mut() {
                let ts = chrono_now();
                if cs.messages.len() >= 1000 {
                    cs.messages.pop_front();
                }
                cs.messages.push_back(ChatMessage {
                    timestamp: ts,
                    user,
                    text,
                    color,
                    badges,
                });
            }
        }

        AppEvent::ChatConnected => {
            if let Screen::TwitchChat(ref mut cs) = app.current_screen_mut() {
                cs.connected = true;
                cs.status = "Connected".to_string();
            }
        }

        AppEvent::ChatError(e) => {
            {
                if let Screen::TwitchChat(ref mut cs) = app.current_screen_mut() {
                    cs.connected = false;
                    cs.status = format!("Error: {}", e);
                }
            }
            app.set_error(e);
        }

        AppEvent::Error(e) => {
            app.loading = None;
            app.set_error(e);
        }

        AppEvent::StatusMessage(msg) => {
            app.set_info(msg);
        }

        AppEvent::DownloadStarted(msg) => {
            app.set_info(msg);
        }

        AppEvent::SubFeedResults { items, load_more } => {
            app.loading = None;
            let items = crate::youtube::apply_shorts_filter(items, show_shorts);
            let ls = App::make_video_list_with_load_more(
                "Subscription Feed",
                items,
                ListContext::YoutubeVideoActions,
                load_more,
            );
            preview::trigger_preview_for_selected(app, &ls);
            app.push_screen(Screen::List(ls));
        }

        AppEvent::SubFeedRefreshed { items, load_more } => {
            let items = crate::youtube::apply_shorts_filter(items, show_shorts);
            // Only swap the list if the user is still on the subscription feed.
            if let Screen::List(ls) = app.current_screen() {
                if ls.title == "Subscription Feed" {
                    let selected = ls.selected;
                    let scroll_offset = ls.scroll_offset;
                    let filter = ls.filter.clone();
                    let mut new_ls = App::make_video_list_with_load_more(
                        "Subscription Feed",
                        items,
                        ListContext::YoutubeVideoActions,
                        load_more,
                    );
                    new_ls.selected = selected.min(new_ls.total_rows().saturating_sub(1));
                    new_ls.scroll_offset = scroll_offset.min(new_ls.selected);
                    new_ls.filter = filter;
                    preview::trigger_preview_for_selected(app, &new_ls);
                    *app.current_screen_mut() = Screen::List(new_ls);
                    app.set_info("Feed refreshed.");
                }
            }
        }

        AppEvent::SubFeedMoreResults {
            new_items,
            existing_items,
            load_more,
        } => {
            app.loading = None;
            // Merge: existing + new, dedup by id, sort by date desc.
            let new_items = crate::youtube::apply_shorts_filter(new_items, show_shorts);
            let mut all = existing_items;
            for v in new_items {
                if !all.iter().any(|e: &crate::models::Video| e.id == v.id) {
                    all.push(v);
                }
            }
            all.sort_by(|a, b| match (b.timestamp, a.timestamp) {
                (Some(bt), Some(at)) => bt.cmp(&at),
                (Some(_), None) => std::cmp::Ordering::Greater,
                (None, Some(_)) => std::cmp::Ordering::Less,
                _ => b.upload_date.cmp(&a.upload_date),
            });
            let ls = App::make_video_list_with_load_more(
                "Subscription Feed",
                all,
                ListContext::YoutubeVideoActions,
                load_more,
            );
            preview::trigger_preview_for_selected(app, &ls);
            // Replace the current screen (still the sub-feed list).
            if matches!(app.current_screen(), Screen::List(_)) {
                *app.current_screen_mut() = Screen::List(ls);
            } else {
                app.push_screen(Screen::List(ls));
            }
        }

        AppEvent::ChannelTabMoreResults {
            new_items,
            existing_items,
            channel_load_more,
            title,
            context,
        } => {
            app.loading = None;
            // The user can only reach a Shorts tab when SHOW_SHORTS is on, so
            // this filter never empties an explicitly opened Shorts list.
            let new_items = crate::youtube::apply_shorts_filter(new_items, show_shorts);
            let mut all = existing_items;
            for v in new_items {
                if !all.iter().any(|e: &crate::models::Video| e.id == v.id) {
                    all.push(v);
                }
            }
            let mut ls = App::make_video_list(title, all, context);
            ls.channel_load_more = channel_load_more;
            preview::trigger_preview_for_selected(app, &ls);
            if matches!(app.current_screen(), Screen::List(_)) {
                *app.current_screen_mut() = Screen::List(ls);
            } else {
                app.push_screen(Screen::List(ls));
            }
        }

        AppEvent::PreviewReady { video_id } => {
            app.preview_cache
                .insert(video_id.clone(), PreviewEntry { ready: true });
            // If this is the currently displayed video, force kitty refresh.
            if app.kitty_displayed.as_deref() == Some(&video_id) {
                app.kitty_displayed = None;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Key event handler (dispatch by screen)
// ─────────────────────────────────────────────────────────────────────────────

/// Translate mouse input into navigation. Scroll wheel moves the selection up
/// or down, reusing the existing key handlers so behaviour stays consistent.
pub async fn handle_mouse(app: &mut App, m: MouseEvent) {
    let code = match m.kind {
        MouseEventKind::ScrollUp => KeyCode::Up,
        MouseEventKind::ScrollDown => KeyCode::Down,
        _ => return,
    };
    handle_key(app, KeyEvent::new(code, KeyModifiers::NONE)).await;
}

/// Apply user keybinding overrides by mapping a configured character to its
/// canonical navigation key. Skipped on text-entry screens so typing still
/// works. Non-character keys and unmatched characters pass through unchanged.
fn apply_keybindings(key: event::KeyEvent, kb: &config::Keybindings) -> event::KeyEvent {
    let KeyCode::Char(c) = key.code else {
        return key;
    };
    let mapped = if kb.up == Some(c) {
        KeyCode::Up
    } else if kb.down == Some(c) {
        KeyCode::Down
    } else if kb.page_up == Some(c) {
        KeyCode::PageUp
    } else if kb.page_down == Some(c) {
        KeyCode::PageDown
    } else if kb.select == Some(c) {
        KeyCode::Enter
    } else if kb.back == Some(c) {
        KeyCode::Esc
    } else if kb.quit == Some(c) {
        KeyCode::Char('q')
    } else {
        return key;
    };
    KeyEvent::new(mapped, key.modifiers)
}

pub async fn handle_key(app: &mut App, mut key: event::KeyEvent) {
    // Clear status messages on any keypress
    if app.message.is_some() && !matches!(app.message, Some((_, MessageKind::Error))) {
        app.clear_message();
    }

    // Help overlay: any key closes it; `?` opens it anywhere but text entry.
    if app.show_help {
        app.show_help = false;
        return;
    }
    // Text entry: search prompt, or a list with its `/` filter prompt open.
    let text_entry = match app.current_screen() {
        Screen::SearchInput(_) => true,
        Screen::List(ls) => ls.filter_active,
        _ => false,
    };
    if key.code == KeyCode::Char('?') && !text_entry {
        app.show_help = true;
        return;
    }

    // Translate configured keybindings (except on text-entry screens).
    if !text_entry {
        key = apply_keybindings(key, &app.config.keys);
    }

    // Global quit
    if key.code == KeyCode::Char('q')
        && !text_entry
        && !matches!(app.current_screen(), Screen::TwitchChat(_))
        && app.screen_stack.len() <= 1
    {
        app.should_quit = true;
        return;
    }

    // Ctrl-C always quits
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.should_quit = true;
        return;
    }

    match app.current_screen().clone() {
        Screen::ModeSelect { selected } => {
            menus::handle_mode_select(app, key, selected).await;
        }
        Screen::YoutubeMenu { selected } => {
            menus::handle_youtube_menu(app, key, selected).await;
        }
        Screen::TwitchMenu { selected } => {
            menus::handle_twitch_menu(app, key, selected).await;
        }
        Screen::List(ls) => {
            list::handle_list(app, key, ls).await;
        }
        Screen::VideoActions(va) => {
            actions::handle_video_actions(app, key, va).await;
        }
        Screen::QualitySelect(qs) => {
            actions::handle_quality_select(app, key, qs).await;
        }
        Screen::ChannelActions(ca) => {
            actions::handle_channel_actions(app, key, ca).await;
        }
        Screen::TwitchStreamActions(sa) => {
            actions::handle_twitch_stream_actions(app, key, sa).await;
        }
        Screen::TwitchVodActions(va) => {
            actions::handle_twitch_vod_actions(app, key, va).await;
        }
        Screen::SearchInput(si) => {
            search::handle_search_input(app, key, si).await;
        }
        Screen::TwitchChat(_) => {
            chat_screen::handle_chat(app, key);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn chrono_now() -> String {
    // Compute local time by reading the UTC offset from the TZ environment.
    // Falls back to a `date` call on the first invocation to determine the offset.
    use std::sync::OnceLock;
    use std::time::{SystemTime, UNIX_EPOCH};
    static UTC_OFFSET: OnceLock<i64> = OnceLock::new();
    let offset = *UTC_OFFSET.get_or_init(|| {
        // Ask the system for the current UTC offset in seconds.
        std::process::Command::new("date")
            .arg("+%z")
            .output()
            .ok()
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                // Format: +HHMM or -HHMM
                if s.len() >= 5 {
                    let sign: i64 = if s.starts_with('-') { -1 } else { 1 };
                    let hh = s[1..3].parse::<i64>().unwrap_or(0);
                    let mm = s[3..5].parse::<i64>().unwrap_or(0);
                    Some(sign * (hh * 3600 + mm * 60))
                } else {
                    None
                }
            })
            .unwrap_or(0)
    });
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        + offset;
    let day_secs = secs.rem_euclid(86400);
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

pub(crate) fn open_url_in_browser(url: &str) {
    #[cfg(target_os = "linux")]
    let cmd = "xdg-open";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "windows")]
    let cmd = "start";
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let cmd = "xdg-open";

    tokio::process::Command::new(cmd).arg(url).spawn().ok();
}
