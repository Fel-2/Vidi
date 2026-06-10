//! Event handling: async app events, keyboard/mouse input, and per-screen
//! action handlers.

use crate::app::{
    quality_options, App, AppEvent, ChannelActionsScreen, ChatMessage, ChatScreen, ListContext,
    ListScreen, MessageKind, PreviewEntry, QualitySelectScreen, Screen, SearchContext,
    SearchInputScreen, TwitchStreamActionsScreen, TwitchVodActionsScreen, VideoActionsScreen,
};
use crate::models::{ChannelTabLoadMore, ItemData, ListItem, SubFeedLoadMore, Video};
use crate::ui::{
    channel_action_items, twitch_stream_action_items, twitch_vod_action_items, TWITCH_MENU_ITEMS,
    VIDEO_ACTION_ITEMS, YOUTUBE_MENU_ITEMS,
};
use crate::{chat, config, player, preview, twitch, youtube};
use crossterm::event::{self, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

pub fn handle_app_event(app: &mut App, event: AppEvent) {
    match event {
        AppEvent::YoutubeResults { items, context, title, channel_load_more } => {
            app.loading = None;
            let mut ls = App::make_video_list(title, items, context);
            ls.channel_load_more = channel_load_more;
            preview::trigger_preview_for_selected(app, &ls);
            app.push_screen(Screen::List(ls));
        }

        AppEvent::TwitchSearchResults(streams) => {
            app.loading = None;
            let ls = App::make_stream_list("Twitch Search", streams, ListContext::TwitchStreamActions);
            app.push_screen(Screen::List(ls));
        }

        AppEvent::TwitchSubsResults(streams) => {
            app.loading = None;
            let ls = App::make_stream_list("Live Subscriptions", streams, ListContext::TwitchStreamActions);
            app.push_screen(Screen::List(ls));
        }

        AppEvent::TwitchVodsResults(vods) => {
            app.loading = None;
            let ls = App::make_vod_list("VODs", vods, ListContext::TwitchVodActions);
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
            let ls = ListScreen::new("Custom Playlists", items, ListContext::CustomPlaylistActions);
            app.push_screen(Screen::List(ls));
        }

        AppEvent::ChatMessage { user, text, color } => {
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

        AppEvent::SubFeedMoreResults { new_items, existing_items, load_more } => {
            app.loading = None;
            // Merge: existing + new, dedup by id, sort by date desc.
            let mut all = existing_items;
            for v in new_items {
                if !all.iter().any(|e: &Video| e.id == v.id) {
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

        AppEvent::ChannelTabMoreResults { new_items, existing_items, channel_load_more, title, context } => {
            app.loading = None;
            let mut all = existing_items;
            for v in new_items {
                if !all.iter().any(|e: &Video| e.id == v.id) {
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
            app.preview_cache.insert(video_id.clone(), PreviewEntry { ready: true });
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
    if app.message.is_some()
        && !matches!(app.message, Some((_, MessageKind::Error)))
    {
        app.clear_message();
    }

    // Translate configured keybindings (except on text-entry screens).
    if !matches!(app.current_screen(), Screen::SearchInput(_)) {
        key = apply_keybindings(key, &app.config.keys);
    }

    // Global quit
    if key.code == KeyCode::Char('q')
        && !matches!(app.current_screen(), Screen::SearchInput(_))
        && !matches!(app.current_screen(), Screen::TwitchChat(_))
        && app.screen_stack.len() <= 1 {
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
            handle_mode_select(app, key, selected).await;
        }
        Screen::YoutubeMenu { selected } => {
            handle_youtube_menu(app, key, selected).await;
        }
        Screen::TwitchMenu { selected } => {
            handle_twitch_menu(app, key, selected).await;
        }
        Screen::List(ls) => {
            handle_list(app, key, ls).await;
        }
        Screen::VideoActions(va) => {
            handle_video_actions(app, key, va).await;
        }
        Screen::QualitySelect(qs) => {
            handle_quality_select(app, key, qs).await;
        }
        Screen::ChannelActions(ca) => {
            handle_channel_actions(app, key, ca).await;
        }
        Screen::TwitchStreamActions(sa) => {
            handle_twitch_stream_actions(app, key, sa).await;
        }
        Screen::TwitchVodActions(va) => {
            handle_twitch_vod_actions(app, key, va).await;
        }
        Screen::SearchInput(si) => {
            handle_search_input(app, key, si).await;
        }
        Screen::TwitchChat(_) => {
            handle_chat(app, key);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Mode select
// ─────────────────────────────────────────────────────────────────────────────

async fn handle_mode_select(app: &mut App, key: event::KeyEvent, selected: usize) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            let new = selected.saturating_sub(1);
            *app.current_screen_mut() = Screen::ModeSelect { selected: new };
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let new = (selected + 1).min(1);
            *app.current_screen_mut() = Screen::ModeSelect { selected: new };
        }
        KeyCode::Enter => match selected {
            0 => app.push_screen(Screen::YoutubeMenu { selected: 0 }),
            1 => app.push_screen(Screen::TwitchMenu { selected: 0 }),
            _ => {}
        },
        KeyCode::Esc | KeyCode::Char('q') => {
            app.should_quit = true;
        }
        _ => {}
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// YouTube menu
// ─────────────────────────────────────────────────────────────────────────────

async fn handle_youtube_menu(app: &mut App, key: event::KeyEvent, selected: usize) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            let new = selected.saturating_sub(1);
            *app.current_screen_mut() = Screen::YoutubeMenu { selected: new };
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let new = (selected + 1).min(YOUTUBE_MENU_ITEMS.len() - 1);
            *app.current_screen_mut() = Screen::YoutubeMenu { selected: new };
        }
        KeyCode::Enter => {
            youtube_menu_action(app, selected).await;
        }
        KeyCode::Esc => {
            app.pop_screen();
        }
        _ => {}
    }
}

async fn youtube_menu_action(app: &mut App, selected: usize) {
    match YOUTUBE_MENU_ITEMS[selected] {
        "Trending" => {
            let limit = app.config.youtube.no_of_search_results as u32;
            let tx = app.tx.clone();
            app.loading = Some("Fetching trending videos…".to_string());
            tokio::spawn(async move {
                match youtube::fetch_trending(limit).await {
                    Ok(items) => {
                        let _ = tx.send(AppEvent::YoutubeResults {
                            items,
                            context: ListContext::YoutubeVideoActions,
                            title: "Trending".to_string(),
                            channel_load_more: None,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Error(e.to_string()));
                    }
                }
            });
        }
        "Search" => {
            app.push_screen(Screen::SearchInput(SearchInputScreen {
                prompt: "YouTube Search".to_string(),
                input: String::new(),
                context: SearchContext::YoutubeSearch,
            }));
        }
        "Subscription Feed" => {
            let subs = youtube::load_subscriptions();
            if subs.is_empty() {
                app.set_error("No YouTube subscriptions found. Add URLs to ~/.config/vidi/subscriptions");
                return;
            }

            let make_load_more = |subs: Vec<String>| {
                Some(SubFeedLoadMore {
                    subs,
                    next_playlist_end: 20,
                    label: "── Load More ──".to_string(),
                })
            };

            // Show the cache instantly even when stale; a stale cache kicks
            // off a background refresh that swaps the list in place.
            let cached = youtube::load_feed_cache_with_age();
            if let Some((items, _)) = cached.clone() {
                let _ = app.tx.send(AppEvent::SubFeedResults {
                    items,
                    load_more: make_load_more(subs.clone()),
                });
            }

            let fresh = matches!(cached, Some((_, true)));
            if fresh {
                return;
            }
            let had_cache = cached.is_some();
            if !had_cache {
                app.loading = Some("Fetching subscription feed…".to_string());
            }

            let tx = app.tx.clone();
            let subs_clone = subs.clone();
            tokio::spawn(async move {
                match youtube::fetch_subscription_feed(subs_clone.clone(), 5, 8, None).await {
                    Ok(items) => {
                        youtube::save_feed_cache(&items);
                        let load_more = Some(SubFeedLoadMore {
                            subs: subs_clone,
                            next_playlist_end: 20,
                            label: "── Load More ──".to_string(),
                        });
                        if had_cache {
                            let _ = tx.send(AppEvent::SubFeedRefreshed { items, load_more });
                        } else {
                            let _ = tx.send(AppEvent::SubFeedResults { items, load_more });
                        }
                    }
                    Err(e) => {
                        // A failed background refresh shouldn't nuke the visible list.
                        if !had_cache {
                            let _ = tx.send(AppEvent::Error(e.to_string()));
                        }
                    }
                }
            });
        }
        "Channels" => {
            let tx = app.tx.clone();
            app.loading = Some("Loading channels…".to_string());
            tokio::spawn(async move {
                match youtube::fetch_channels().await {
                    Ok(channels) => {
                        let _ = tx.send(AppEvent::ChannelList(channels));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Error(e.to_string()));
                    }
                }
            });
        }
        "Custom Playlists" => {
            let playlists = youtube::load_custom_playlists();
            if playlists.is_empty() {
                app.set_error("No custom playlists found. Add them to custom_playlists.json");
                return;
            }
            let _ = app.tx.send(AppEvent::CustomPlaylistResults(playlists));
        }
        "Recent" => {
            let recent = youtube::load_recent();
            let mut videos = recent.entries;
            videos.reverse();
            if videos.is_empty() {
                app.set_error("No recent videos.");
                return;
            }
            let ls = App::make_video_list("Recent", videos, ListContext::YoutubeVideoActions);
            app.push_screen(Screen::List(ls));
        }
        "Saved Videos" => {
            let saved = youtube::load_saved();
            let mut videos = saved.entries;
            if videos.is_empty() {
                app.set_error("No saved videos.");
                return;
            }
            videos.reverse();
            let ls = App::make_video_list("Saved Videos", videos, ListContext::YoutubeVideoActions);
            app.push_screen(Screen::List(ls));
        }
        "Edit Config" => {
            let path = config::youtube_config_file();
            let editor = app.config.youtube.editor.clone();
            let _ = player::launch_external(&[&editor, &path.to_string_lossy()]).await;
            // Reload config
            if let Ok(cfg) = config::load_config() {
                app.config = cfg;
            }
        }
        "Miscellaneous" => {
            let misc_items = ["Explore Channels",
                "Explore Playlists",
                "Search History",
                "Edit Search History",
                "Edit Custom Playlists",
                "Clear Search History",
                "Back"];
            let list_items: Vec<ListItem> = misc_items
                .iter()
                .map(|s| ListItem {
                    display: s.to_string(),
                    data: ItemData::Text(s.to_string()),
                })
                .collect();
            let ls = ListScreen::new("Miscellaneous", list_items, ListContext::Miscellaneous);
            app.push_screen(Screen::List(ls));
        }
        _ => {}
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Twitch menu
// ─────────────────────────────────────────────────────────────────────────────

async fn handle_twitch_menu(app: &mut App, key: event::KeyEvent, selected: usize) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            let new = selected.saturating_sub(1);
            *app.current_screen_mut() = Screen::TwitchMenu { selected: new };
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let new = (selected + 1).min(TWITCH_MENU_ITEMS.len() - 1);
            *app.current_screen_mut() = Screen::TwitchMenu { selected: new };
        }
        KeyCode::Enter => {
            twitch_menu_action(app, selected).await;
        }
        KeyCode::Esc => {
            app.pop_screen();
        }
        _ => {}
    }
}

async fn twitch_menu_action(app: &mut App, selected: usize) {
    match TWITCH_MENU_ITEMS[selected] {
        "Search Live" => {
            app.push_screen(Screen::SearchInput(SearchInputScreen {
                prompt: "Twitch Search".to_string(),
                input: String::new(),
                context: SearchContext::TwitchSearch,
            }));
        }
        "Live Subscriptions" => {
            let subs = twitch::load_twitch_subs();
            if subs.is_empty() {
                app.set_error("No Twitch subscriptions found. Add usernames to ~/.config/vidi/twitch_subs");
                return;
            }
            let tx = app.tx.clone();
            app.loading = Some("Checking subscriptions…".to_string());
            tokio::spawn(async move {
                match twitch::check_subs_parallel().await {
                    Ok(streams) => {
                        let _ = tx.send(AppEvent::TwitchSubsResults(streams));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Error(e.to_string()));
                    }
                }
            });
        }
        "Watch VODs" => {
            // Show channel selection list first
            let subs = twitch::load_twitch_subs();
            if subs.is_empty() {
                app.set_error("No Twitch subscriptions found.");
                return;
            }
            let items: Vec<ListItem> = subs
                .into_iter()
                .map(|u| ListItem {
                    display: u.clone(),
                    data: ItemData::Text(u),
                })
                .collect();
            let ls = ListScreen::new(
                "Select Channel for VODs",
                items,
                ListContext::SelectChannelForVods,
            );
            app.push_screen(Screen::List(ls));
        }
        "Edit Subs" => {
            let path = config::twitch_subs_file();
            let editor = app.config.twitch.editor.clone();
            let _ = player::launch_external(&[&editor, &path.to_string_lossy()]).await;
        }
        _ => {}
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Generic list screen
// ─────────────────────────────────────────────────────────────────────────────

async fn handle_list(app: &mut App, key: event::KeyEvent, mut ls: ListScreen) {
    match key.code {
        KeyCode::Esc => {
            app.pop_screen();
        }
        KeyCode::Char('q') => {
            app.pop_screen();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if ls.selected > 0 {
                ls.selected -= 1;
                if ls.selected < ls.scroll_offset {
                    ls.scroll_offset = ls.selected;
                }
                preview::trigger_preview_for_selected(app, &ls);
            }
            *app.current_screen_mut() = Screen::List(ls);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let max_idx = ls.total_rows().saturating_sub(1);
            if ls.selected < max_idx {
                ls.selected += 1;
                let visible = crossterm::terminal::size()
                    .map(|(_, h)| (h as usize).saturating_sub(11))
                    .unwrap_or(20);
                if ls.selected >= ls.scroll_offset + visible {
                    ls.scroll_offset = ls.selected.saturating_sub(visible - 1);
                }
                preview::trigger_preview_for_selected(app, &ls);
            }
            *app.current_screen_mut() = Screen::List(ls);
        }
        KeyCode::PageUp => {
            let page = 10;
            ls.selected = ls.selected.saturating_sub(page);
            ls.scroll_offset = ls.scroll_offset.saturating_sub(page);
            preview::trigger_preview_for_selected(app, &ls);
            *app.current_screen_mut() = Screen::List(ls);
        }
        KeyCode::PageDown => {
            let page = 10;
            let max_idx = ls.total_rows().saturating_sub(1);
            ls.selected = (ls.selected + page).min(max_idx);
            ls.scroll_offset = ls.scroll_offset.saturating_add(page).min(max_idx);
            preview::trigger_preview_for_selected(app, &ls);
            *app.current_screen_mut() = Screen::List(ls);
        }
        KeyCode::Backspace => {
            ls.filter.pop();
            ls.selected = 0;
            ls.scroll_offset = 0;
            preview::trigger_preview_for_selected(app, &ls);
            *app.current_screen_mut() = Screen::List(ls);
        }
        KeyCode::Enter => {
            let filtered_len = ls.filtered_items().len();

            // "Load More" virtual row selected?
            if ls.selected == filtered_len {
                if let Some(ref lm) = ls.load_more.clone() {
                    execute_load_more(app, &ls, lm);
                } else if let Some(ref clm) = ls.channel_load_more.clone() {
                    execute_channel_load_more(app, &ls, clm);
                }
                return; // keep the current screen
            }

            if filtered_len == 0 {
                return;
            }
            let selected_idx = ls.selected.min(filtered_len - 1);
            let filtered = ls.filtered_items();
            let item = filtered[selected_idx].clone();
            drop(filtered);
            app.pop_screen();
            handle_list_item_select(app, item, ls.context.clone()).await;
        }
        KeyCode::Char(c) => {
            ls.filter.push(c);
            ls.selected = 0;
            ls.scroll_offset = 0;
            preview::trigger_preview_for_selected(app, &ls);
            *app.current_screen_mut() = Screen::List(ls);
        }
        _ => {}
    }
}

/// Spawn the next subscription-feed fetch when the user presses "Load More".
fn execute_load_more(app: &mut App, ls: &ListScreen, lm: &SubFeedLoadMore) {
    let subs = lm.subs.clone();
    let playlist_end = lm.next_playlist_end;
    // Collect existing videos so we can merge on arrival.
    let existing: Vec<Video> = ls
        .items
        .iter()
        .filter_map(|i| {
            if let ItemData::YoutubeVideo(v) = &i.data {
                Some(v.clone())
            } else {
                None
            }
        })
        .collect();
    let tx = app.tx.clone();
    app.loading = Some(format!("Loading more (up to {} per channel)…", playlist_end));

    // Determine the next Load More config (escalate: 30 → 100 → none).
    let next_lm = if playlist_end < 100 {
        Some(SubFeedLoadMore {
            subs: subs.clone(),
            next_playlist_end: 50,
            label: "── Load More ──".to_string(),
        })
    } else {
        None // no further pages
    };

    tokio::spawn(async move {
        match youtube::fetch_subscription_feed(subs, playlist_end, 8, None).await {
            Ok(new_items) => {
                let _ = tx.send(AppEvent::SubFeedMoreResults {
                    new_items,
                    existing_items: existing,
                    load_more: next_lm,
                });
            }
            Err(e) => {
                let _ = tx.send(AppEvent::Error(e.to_string()));
            }
        }
    });
}

fn execute_channel_load_more(app: &mut App, ls: &ListScreen, clm: &ChannelTabLoadMore) {
    let existing: Vec<Video> = ls
        .items
        .iter()
        .filter_map(|i| {
            if let ItemData::YoutubeVideo(v) = &i.data {
                Some(v.clone())
            } else {
                None
            }
        })
        .collect();
    let new_end = clm.current_playlist_end + clm.page_size;
    let url = clm.url.clone();
    let context = clm.context.clone();
    let title = clm.title.clone();
    let page_size = clm.page_size;
    let tx = app.tx.clone();
    app.loading = Some(format!("Loading more (up to {})…", new_end));

    let next_clm = Some(ChannelTabLoadMore {
        url: clm.url.clone(),
        context: clm.context.clone(),
        title: clm.title.clone(),
        current_playlist_end: new_end,
        page_size,
        label: "── Load More ──".to_string(),
    });

    tokio::spawn(async move {
        match youtube::fetch_playlist(&url, new_end).await {
            Ok(new_items) => {
                let _ = tx.send(AppEvent::ChannelTabMoreResults {
                    new_items,
                    existing_items: existing,
                    channel_load_more: next_clm,
                    title,
                    context,
                });
            }
            Err(e) => {
                let _ = tx.send(AppEvent::Error(e.to_string()));
            }
        }
    });
}

async fn handle_list_item_select(
    app: &mut App,
    item: ListItem,
    context: ListContext,
) {
    match context {
        ListContext::YoutubeVideoActions => {
            if let ItemData::YoutubeVideo(video) = item.data {
                app.push_screen(Screen::VideoActions(VideoActionsScreen {
                    video,
                    selected: 0,
                }));
            }
        }

        ListContext::TwitchStreamActions => {
            if let ItemData::TwitchStream(stream) = item.data {
                app.push_screen(Screen::TwitchStreamActions(TwitchStreamActionsScreen {
                    stream,
                    selected: 0,
                }));
            }
        }

        ListContext::TwitchVodActions => {
            if let ItemData::TwitchVod(vod) = item.data {
                app.push_screen(Screen::TwitchVodActions(TwitchVodActionsScreen {
                    vod,
                    selected: 0,
                }));
            }
        }

        ListContext::SelectChannelForVods => {
            if let ItemData::Text(user) = item.data {
                let tx = app.tx.clone();
                let user_clone = user.clone();
                app.loading = Some(format!("Fetching VODs for {}…", user));
                tokio::spawn(async move {
                    match twitch::fetch_vods(&user_clone).await {
                        Ok(vods) => {
                            let _ = tx.send(AppEvent::TwitchVodsResults(vods));
                        }
                        Err(e) => {
                            let _ = tx.send(AppEvent::Error(e.to_string()));
                        }
                    }
                });
            }
        }

        ListContext::SelectChannelToBrowse => {
            if let ItemData::Channel(ch) = item.data {
                let subs = youtube::load_subscriptions();
                let subscribed = subs.iter().any(|s| s.trim() == ch.url.trim());
                app.push_screen(Screen::ChannelActions(ChannelActionsScreen {
                    channel: ch,
                    selected: 0,
                    subscribed,
                }));
            }
        }

        ListContext::CustomPlaylistActions => {
            if let ItemData::CustomPlaylist(pl) = item.data {
                // Fetch playlist videos
                let tx = app.tx.clone();
                let url = pl.playlist_url.clone();
                let name = pl.name.clone();
                let limit = app.config.youtube.no_of_search_results as u32;
                app.loading = Some(format!("Loading playlist: {}…", name));
                tokio::spawn(async move {
                    match youtube::fetch_playlist(&url, limit).await {
                        Ok(items) => {
                            let _ = tx.send(AppEvent::YoutubeResults {
                                items,
                                context: ListContext::YoutubeVideoActions,
                                title: name,
                                channel_load_more: None,
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(AppEvent::Error(e.to_string()));
                        }
                    }
                });
            }
        }

        ListContext::SearchHistory => {
            if let ItemData::Text(query) = item.data {
                let tx = app.tx.clone();
                let limit = app.config.youtube.no_of_search_results as u32;
                app.loading = Some(format!("Searching: {}…", query));
                let query_clone = query.clone();
                tokio::spawn(async move {
                    let (sp, q) = youtube::parse_search_filter(&query_clone);
                    match youtube::fetch_search(&q, &sp, limit).await {
                        Ok(items) => {
                            let _ = tx.send(AppEvent::YoutubeResults {
                                items,
                                context: ListContext::YoutubeVideoActions,
                                title: format!("Search: {}", query_clone),
                                channel_load_more: None,
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(AppEvent::Error(e.to_string()));
                        }
                    }
                });
            }
        }

        ListContext::Miscellaneous => {
            if let ItemData::Text(action) = item.data {
                match action.as_str() {
                    "Explore Channels" => {
                        app.push_screen(Screen::SearchInput(SearchInputScreen {
                            prompt: "Explore Channels".to_string(),
                            input: String::new(),
                            context: SearchContext::ExploreChannels,
                        }));
                    }
                    "Explore Playlists" => {
                        app.push_screen(Screen::SearchInput(SearchInputScreen {
                            prompt: "Explore Playlists".to_string(),
                            input: String::new(),
                            context: SearchContext::ExplorePlaylists,
                        }));
                    }
                    "Search History" => {
                        let history = youtube::load_search_history();
                        if history.is_empty() {
                            app.set_info("No search history.");
                            return;
                        }
                        let items: Vec<ListItem> = history
                            .into_iter()
                            .rev()
                            .map(|q| ListItem {
                                display: q.clone(),
                                data: ItemData::Text(q),
                            })
                            .collect();
                        let ls = ListScreen::new(
                            "Search History",
                            items,
                            ListContext::SearchHistory,
                        );
                        app.push_screen(Screen::List(ls));
                    }
                    "Edit Search History" => {
                        let path = config::youtube_search_history_file();
                        let editor = app.config.youtube.editor.clone();
                        let _ = player::launch_external(&[&editor, &path.to_string_lossy()])
                            .await;
                    }
                    "Edit Custom Playlists" => {
                        let path = config::youtube_custom_playlists_file();
                        let editor = app.config.youtube.editor.clone();
                        let _ = player::launch_external(&[&editor, &path.to_string_lossy()])
                            .await;
                    }
                    "Clear Search History" => {
                        let path = config::youtube_search_history_file();
                        std::fs::write(path, "").ok();
                        app.set_success("Search history cleared.");
                    }
                    "Back" => {}
                    _ => {}
                }
            }
        }

        ListContext::ChannelTab(channel_url) => {
            // Item is a video
            if let ItemData::YoutubeVideo(video) = item.data {
                app.push_screen(Screen::VideoActions(VideoActionsScreen {
                    video,
                    selected: 0,
                }));
            } else if let ItemData::Text(tab) = item.data {
                // Tab selection
                let tab_path = match tab.as_str() {
                    "Videos" => "/videos",
                    "Shorts" => "/shorts",
                    "Streams" => "/streams",
                    "Playlists" => "/playlists",
                    "Search" => {
                        app.push_screen(Screen::SearchInput(SearchInputScreen {
                            prompt: "Search channel".to_string(),
                            input: String::new(),
                            context: SearchContext::ChannelSearch(channel_url),
                        }));
                        return;
                    }
                    _ => return,
                };
                let tx = app.tx.clone();
                let url = format!("{}{}", channel_url.trim_end_matches('/'), tab_path);
                let limit = app.config.youtube.no_of_search_results as u32;
                let tab_name = tab.clone();
                let channel_url_for_clm = channel_url.clone();
                app.loading = Some(format!("Loading {}…", tab));
                tokio::spawn(async move {
                    match youtube::fetch_playlist(&url, limit).await {
                        Ok(items) => {
                            let clm = Some(ChannelTabLoadMore {
                                url: url.clone(),
                                context: ListContext::ChannelTab(channel_url.clone()),
                                title: tab_name.clone(),
                                current_playlist_end: limit,
                                page_size: limit,
                                label: "── Load More ──".to_string(),
                            });
                            let _ = tx.send(AppEvent::YoutubeResults {
                                items,
                                context: ListContext::ChannelTab(channel_url_for_clm),
                                title: tab_name,
                                channel_load_more: clm,
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(AppEvent::Error(e.to_string()));
                        }
                    }
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Video actions
// ─────────────────────────────────────────────────────────────────────────────

async fn handle_video_actions(app: &mut App, key: event::KeyEvent, mut va: VideoActionsScreen) {
    let actions: &[&str] = VIDEO_ACTION_ITEMS;

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            if va.selected > 0 {
                va.selected -= 1;
            }
            *app.current_screen_mut() = Screen::VideoActions(va);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if va.selected + 1 < actions.len() {
                va.selected += 1;
            }
            *app.current_screen_mut() = Screen::VideoActions(va);
        }
        KeyCode::Enter => {
            let action = actions[va.selected];
            video_action_execute(app, &va.video.clone(), action).await;
        }
        KeyCode::Esc => {
            app.pop_screen();
        }
        _ => {}
    }
}

async fn handle_quality_select(
    app: &mut App,
    key: event::KeyEvent,
    mut qs: QualitySelectScreen,
) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            if qs.selected > 0 {
                qs.selected -= 1;
            }
            *app.current_screen_mut() = Screen::QualitySelect(qs);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if qs.selected + 1 < qs.options.len() {
                qs.selected += 1;
            }
            *app.current_screen_mut() = Screen::QualitySelect(qs);
        }
        KeyCode::Enter => {
            let quality = qs.options[qs.selected].clone();
            let video = qs.video.clone();
            let update_recent = app.config.youtube.update_recent;
            let no_of_recent = app.config.youtube.no_of_recent;
            let args = player::mpv_watch_args(&video.url, &video.title, &quality);
            let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let _ = player::launch_external(&args_str).await;
            if update_recent {
                youtube::add_to_recent(&video, no_of_recent).ok();
            }
            app.watched_ids.insert(video.id.clone());
            app.pop_screen();
        }
        KeyCode::Esc => {
            app.pop_screen();
        }
        _ => {}
    }
}

async fn video_action_execute(
    app: &mut App,
    video: &Video,
    action: &str,
) {
    let quality = &app.config.youtube.video_quality.clone();
    let update_recent = app.config.youtube.update_recent;
    let no_of_recent = app.config.youtube.no_of_recent;
    let download_dir = app.config.youtube.download_directory.clone();

    match action {
        "Watch" => {
            let args = player::mpv_watch_args(&video.url, &video.title, quality);
            let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let _ = player::launch_external(&args_str).await;
            if update_recent {
                youtube::add_to_recent(video, no_of_recent).ok();
            }
            app.watched_ids.insert(video.id.clone());
        }
        "Watch (Select Quality)" => {
            app.push_screen(Screen::QualitySelect(QualitySelectScreen {
                video: video.clone(),
                options: quality_options(),
                selected: 0,
            }));
        }
        "Play All" => {
            let playlist_url = video.playlist_url.as_deref().unwrap_or(&video.url);
            let args = player::mpv_watch_args(playlist_url, &video.title, quality);
            let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let _ = player::launch_external(&args_str).await;
        }
        "Download" => {
            let url = video.url.clone();
            let dl_dir = download_dir.clone();
            let tx = app.tx.clone();
            let title = video.title.clone();
            tokio::spawn(async move {
                let args = player::ytdlp_download_args(&url, &dl_dir);
                let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let _ = tx.send(AppEvent::DownloadStarted(format!(
                    "Downloading: {}",
                    title
                )));
                match player::run_background(&args_str).await {
                    Ok(_) => {
                        let _ = tx.send(AppEvent::StatusMessage(format!(
                            "Download complete: {}",
                            title
                        )));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Error(format!(
                            "Download failed: {} — {}",
                            title, e
                        )));
                    }
                }
            });
        }
        "Download (Audio Only)" => {
            let url = video.url.clone();
            let dl_dir = download_dir.clone();
            let tx = app.tx.clone();
            let title = video.title.clone();
            tokio::spawn(async move {
                let args = player::ytdlp_download_audio_args(&url, &dl_dir);
                let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let _ = tx.send(AppEvent::DownloadStarted(format!(
                    "Downloading audio: {}",
                    title
                )));
                match player::run_background(&args_str).await {
                    Ok(_) => {
                        let _ = tx.send(AppEvent::StatusMessage(format!(
                            "Audio download complete: {}",
                            title
                        )));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Error(format!(
                            "Audio download failed: {} — {}",
                            title, e
                        )));
                    }
                }
            });
        }
        "Download All" => {
            let playlist_url = video
                .playlist_url
                .clone()
                .unwrap_or_else(|| video.url.clone());
            let dl_dir = download_dir.clone();
            let tx = app.tx.clone();
            tokio::spawn(async move {
                let args = player::ytdlp_download_args(&playlist_url, &dl_dir);
                let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let _ = tx.send(AppEvent::DownloadStarted("Downloading all…".to_string()));
                match player::run_background(&args_str).await {
                    Ok(_) => {
                        let _ = tx.send(AppEvent::StatusMessage("Download all complete.".to_string()));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Error(format!("Download all failed: {}", e)));
                    }
                }
            });
        }
        "Download All (Audio Only)" => {
            let playlist_url = video
                .playlist_url
                .clone()
                .unwrap_or_else(|| video.url.clone());
            let dl_dir = download_dir.clone();
            let tx = app.tx.clone();
            tokio::spawn(async move {
                let args = player::ytdlp_download_audio_args(&playlist_url, &dl_dir);
                let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let _ = tx.send(AppEvent::DownloadStarted(
                    "Downloading all audio…".to_string(),
                ));
                match player::run_background(&args_str).await {
                    Ok(_) => {
                        let _ = tx.send(AppEvent::StatusMessage(
                            "Download all audio complete.".to_string(),
                        ));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Error(format!(
                            "Download all audio failed: {}",
                            e
                        )));
                    }
                }
            });
        }
        "Save" => {
            youtube::save_video(video).ok();
            app.saved_ids.insert(video.id.clone());
            app.set_success(format!("Saved: {}", video.title));
        }
        "UnSave" => {
            youtube::unsave_video(&video.id).ok();
            app.saved_ids.remove(&video.id);
            app.set_success(format!("Removed from saved: {}", video.title));
        }
        "Save Playlist" => {
            youtube::save_playlist_as_custom(video).ok();
            app.set_success("Playlist saved to custom playlists.");
        }
        "Open in Browser" => {
            let url = video.url.clone();
            open_url_in_browser(&url);
            app.set_info(format!("Opening: {}", url));
        }
        "Back" => {
            app.pop_screen();
        }
        _ => {}
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Channel actions (tab selection)
// ─────────────────────────────────────────────────────────────────────────────

async fn handle_channel_actions(
    app: &mut App,
    key: event::KeyEvent,
    mut ca: ChannelActionsScreen,
) {
    let items = channel_action_items(ca.subscribed);
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            if ca.selected > 0 {
                ca.selected -= 1;
            }
            *app.current_screen_mut() = Screen::ChannelActions(ca);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if ca.selected + 1 < items.len() {
                ca.selected += 1;
            }
            *app.current_screen_mut() = Screen::ChannelActions(ca);
        }
        KeyCode::Enter => {
            let tab = &items[ca.selected];
            let channel_url = ca.channel.url.clone();

            if tab == "Subscribe" {
                match youtube::subscribe_channel(&channel_url) {
                    Ok(()) => {
                        ca.subscribed = true;
                        app.set_success(format!("Subscribed to {}", ca.channel.name));
                        *app.current_screen_mut() = Screen::ChannelActions(ca);
                    }
                    Err(e) => app.set_error(format!("Failed to subscribe: {e}")),
                }
                return;
            }

            if tab == "Search" {
                app.push_screen(Screen::SearchInput(SearchInputScreen {
                    prompt: format!("Search {}", ca.channel.name),
                    input: String::new(),
                    context: SearchContext::ChannelSearch(channel_url),
                }));
                return;
            }

            let tab_path = match tab.as_str() {
                "Videos" => "/videos",
                "Shorts" => "/shorts",
                "Streams" => "/streams",
                "Playlists" => "/playlists",
                _ => return,
            };

            let tx = app.tx.clone();
            let url = format!("{}{}", channel_url.trim_end_matches('/'), tab_path);
            let limit = app.config.youtube.no_of_search_results as u32;
            let tab_name = tab.clone();
            let channel_url_clone = channel_url.clone();
            app.loading = Some(format!("Loading {}…", tab));

            tokio::spawn(async move {
                match youtube::fetch_playlist(&url, limit).await {
                    Ok(items) => {
                        let clm = Some(ChannelTabLoadMore {
                            url: url.clone(),
                            context: ListContext::ChannelTab(channel_url_clone.clone()),
                            title: tab_name.clone(),
                            current_playlist_end: limit,
                            page_size: limit,
                            label: "── Load More ──".to_string(),
                        });
                        let _ = tx.send(AppEvent::YoutubeResults {
                            items,
                            context: ListContext::ChannelTab(channel_url_clone),
                            title: tab_name,
                            channel_load_more: clm,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Error(e.to_string()));
                    }
                }
            });
        }
        KeyCode::Esc => {
            app.pop_screen();
        }
        _ => {}
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Twitch stream actions
// ─────────────────────────────────────────────────────────────────────────────

async fn handle_twitch_stream_actions(
    app: &mut App,
    key: event::KeyEvent,
    mut sa: TwitchStreamActionsScreen,
) {
    let items = twitch_stream_action_items();
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            if sa.selected > 0 {
                sa.selected -= 1;
            }
            *app.current_screen_mut() = Screen::TwitchStreamActions(sa);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if sa.selected + 1 < items.len() {
                sa.selected += 1;
            }
            *app.current_screen_mut() = Screen::TwitchStreamActions(sa);
        }
        KeyCode::Enter => {
            let action = &items[sa.selected];
            let stream_url = twitch::twitch_stream_url(&sa.stream.login);
            let quality = app.config.twitch.quality.clone();
            let player_bin = app.config.twitch.player.clone();

            match action.as_str() {
                "Watch Stream" => {
                    let args = player::streamlink_args(&stream_url, &quality, &player_bin);
                    let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                    let _ = player::launch_external(&args_str).await;
                }
                "Open Chat" => {
                    let channel = sa.stream.login.clone();
                    let tx = app.tx.clone();
                    let chat_screen = ChatScreen {
                        channel: channel.clone(),
                        messages: std::collections::VecDeque::new(),
                        scroll_offset: 0,
                        connected: false,
                        status: "Connecting…".to_string(),
                    };
                    app.pop_screen();
                    app.push_screen(Screen::TwitchChat(chat_screen));
                    chat::spawn_chat_task(channel, tx);
                }
                "Watch + Chat" => {
                    // Launch stream detached, open chat in TUI
                    let args = player::streamlink_args(&stream_url, &quality, &player_bin);
                    let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                    player::spawn_detached(&args_str).ok();

                    let channel = sa.stream.login.clone();
                    let tx = app.tx.clone();
                    let chat_screen = ChatScreen {
                        channel: channel.clone(),
                        messages: std::collections::VecDeque::new(),
                        scroll_offset: 0,
                        connected: false,
                        status: "Connecting…".to_string(),
                    };
                    app.pop_screen();
                    app.push_screen(Screen::TwitchChat(chat_screen));
                    chat::spawn_chat_task(channel, tx);
                }
                "Back" => {
                    app.pop_screen();
                }
                _ => {}
            }
        }
        KeyCode::Esc => {
            app.pop_screen();
        }
        _ => {}
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Twitch VOD actions
// ─────────────────────────────────────────────────────────────────────────────

async fn handle_twitch_vod_actions(
    app: &mut App,
    key: event::KeyEvent,
    mut va: TwitchVodActionsScreen,
) {
    let items = twitch_vod_action_items();
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            if va.selected > 0 {
                va.selected -= 1;
            }
            *app.current_screen_mut() = Screen::TwitchVodActions(va);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if va.selected + 1 < items.len() {
                va.selected += 1;
            }
            *app.current_screen_mut() = Screen::TwitchVodActions(va);
        }
        KeyCode::Enter => {
            let action = &items[va.selected];
            match action.as_str() {
                "Watch VOD" => {
                    let quality = app.config.youtube.video_quality.clone();
                    let args = player::mpv_watch_args(&va.vod.url, &va.vod.title, &quality);
                    let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                    let _ = player::launch_external(&args_str).await;
                }
                "Download" => {
                    let url = va.vod.url.clone();
                    let dl_dir = app.config.youtube.download_directory.clone();
                    let tx = app.tx.clone();
                    let title = va.vod.title.clone();
                    tokio::spawn(async move {
                        let args = player::ytdlp_download_args(&url, &dl_dir);
                        let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                        let _ = tx.send(AppEvent::DownloadStarted(format!(
                            "Downloading VOD: {}",
                            title
                        )));
                        match player::run_background(&args_str).await {
                            Ok(_) => {
                                let _ = tx.send(AppEvent::StatusMessage("VOD download complete.".to_string()));
                            }
                            Err(e) => {
                                let _ = tx.send(AppEvent::Error(format!(
                                    "VOD download failed: {} — {}",
                                    title, e
                                )));
                            }
                        }
                    });
                }
                "Open in Browser" => {
                    let url = va.vod.url.clone();
                    tokio::process::Command::new("xdg-open")
                        .arg(&url)
                        .spawn()
                        .ok();
                }
                "Back" => {
                    app.pop_screen();
                }
                _ => {}
            }
        }
        KeyCode::Esc => {
            app.pop_screen();
        }
        _ => {}
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Search input
// ─────────────────────────────────────────────────────────────────────────────

async fn handle_search_input(app: &mut App, key: event::KeyEvent, mut si: SearchInputScreen) {
    match key.code {
        KeyCode::Esc => {
            app.pop_screen();
        }
        KeyCode::Enter => {
            if si.input.is_empty() {
                return;
            }
            let input = si.input.clone();
            let ctx = si.context.clone();
            app.pop_screen();
            execute_search(app, input, ctx).await;
        }
        KeyCode::Backspace => {
            si.input.pop();
            *app.current_screen_mut() = Screen::SearchInput(si);
        }
        KeyCode::Char(c) => {
            si.input.push(c);
            *app.current_screen_mut() = Screen::SearchInput(si);
        }
        _ => {}
    }
}

async fn execute_search(app: &mut App, input: String, ctx: SearchContext) {
    match ctx {
        SearchContext::YoutubeSearch => {
            // Handle !N history recall
            let query = if let Some(n_str) = input.strip_prefix('!') {
                if let Ok(n) = n_str.trim().parse::<usize>() {
                    let history = youtube::load_search_history();
                    let rev: Vec<String> = history.into_iter().rev().collect();
                    rev.get(n.saturating_sub(1))
                        .cloned()
                        .unwrap_or(input.clone())
                } else {
                    input.clone()
                }
            } else {
                input.clone()
            };

            // Save to history
            if app.config.youtube.search_history {
                youtube::append_search_history(&query).ok();
            }

            let tx = app.tx.clone();
            let limit = app.config.youtube.no_of_search_results as u32;
            let q = query.clone();
            app.loading = Some(format!("Searching: {}…", query));
            tokio::spawn(async move {
                let (sp, query_term) = youtube::parse_search_filter(&q);
                match youtube::fetch_search(&query_term, &sp, limit).await {
                    Ok(items) => {
                        let _ = tx.send(AppEvent::YoutubeResults {
                            items,
                            context: ListContext::YoutubeVideoActions,
                            title: format!("Search: {}", q),
                            channel_load_more: None,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Error(e.to_string()));
                    }
                }
            });
        }

        SearchContext::TwitchSearch => {
            let tx = app.tx.clone();
            let q = input.clone();
            let client_id = app.config.twitch.client_id.clone();
            app.loading = Some(format!("Searching Twitch: {}…", input));
            tokio::spawn(async move {
                match twitch::search_twitch(&q, &client_id).await {
                    Ok(streams) => {
                        let _ = tx.send(AppEvent::TwitchSearchResults(streams));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Error(e.to_string()));
                    }
                }
            });
        }

        SearchContext::ExploreChannels => {
            let tx = app.tx.clone();
            let query = input.clone();
            app.loading = Some(format!("Searching channels: {}…", input));
            tokio::spawn(async move {
                match youtube::search_channels(&query, 20).await {
                    Ok(channels) => {
                        let _ = tx.send(AppEvent::ChannelList(channels));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Error(e.to_string()));
                    }
                }
            });
        }

        SearchContext::ExplorePlaylists => {
            let tx = app.tx.clone();
            let limit = app.config.youtube.no_of_search_results as u32;
            let q = input.clone();
            app.loading = Some(format!("Exploring playlists: {}…", input));
            tokio::spawn(async move {
                let url = format!(
                    "https://www.youtube.com/results?search_query={}&sp=EgIQAw%253D%253D",
                    youtube::urlencoding_simple(&q)
                );
                match youtube::fetch_playlist(&url, limit).await {
                    Ok(items) => {
                        let _ = tx.send(AppEvent::YoutubeResults {
                            items,
                            context: ListContext::YoutubeVideoActions,
                            title: format!("Playlists: {}", q),
                            channel_load_more: None,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Error(e.to_string()));
                    }
                }
            });
        }

        SearchContext::ChannelSearch(channel_url) => {
            let tx = app.tx.clone();
            let limit = app.config.youtube.no_of_search_results as u32;
            let url = format!(
                "{}/search?query={}",
                channel_url.trim_end_matches('/'),
                youtube::urlencoding_simple(&input)
            );
            let channel_url_clone = channel_url.clone();
            let title = format!("Channel Search: {}", input);
            app.loading = Some("Searching channel…".to_string());
            tokio::spawn(async move {
                match youtube::fetch_playlist(&url, limit).await {
                    Ok(items) => {
                        let _ = tx.send(AppEvent::YoutubeResults {
                            items,
                            context: ListContext::ChannelTab(channel_url_clone),
                            title,
                            channel_load_more: None,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Error(e.to_string()));
                    }
                }
            });
        }

    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Chat
// ─────────────────────────────────────────────────────────────────────────────

fn handle_chat(app: &mut App, key: event::KeyEvent) {
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

fn open_url_in_browser(url: &str) {
    #[cfg(target_os = "linux")]
    let cmd = "xdg-open";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "windows")]
    let cmd = "start";
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let cmd = "xdg-open";

    tokio::process::Command::new(cmd)
        .arg(url)
        .spawn()
        .ok();
}
