//! Generic list screen: navigation, filtering, Load More, item selection.

use crate::app::{
    App, AppEvent, ChannelActionsScreen, ListContext, ListScreen, Screen, SearchContext,
    SearchInputScreen, TwitchStreamActionsScreen, TwitchVodActionsScreen, VideoActionsScreen,
};
use crate::models::{ChannelTabLoadMore, ItemData, ListItem, SubFeedLoadMore, Video};
use crate::{config, player, preview, twitch, youtube};
use crossterm::event::{self, KeyCode};

/// Build the VOD-type chooser (Archives/Highlights/…) for a channel login.
/// Each row displays a friendly label but carries the GQL `BroadcastType`.
pub(super) fn build_vod_type_list(login: &str) -> ListScreen {
    let items: Vec<ListItem> = twitch::VOD_TYPES
        .iter()
        .map(|(label, gql_type)| ListItem {
            display: label.to_string(),
            data: ItemData::Text(gql_type.to_string()),
        })
        .collect();
    ListScreen::new(
        format!("VOD Type — {}", login),
        items,
        ListContext::SelectVodType(login.to_string()),
    )
}

/// Keep `selected` inside the viewport of `visible` rows.
fn clamp_scroll(ls: &mut ListScreen, visible: usize) {
    let max_idx = ls.total_rows().saturating_sub(1);
    ls.selected = ls.selected.min(max_idx);
    if ls.selected < ls.scroll_offset {
        ls.scroll_offset = ls.selected;
    } else if ls.selected >= ls.scroll_offset + visible {
        ls.scroll_offset = ls.selected + 1 - visible;
    }
    let max_offset = ls.total_rows().saturating_sub(visible);
    ls.scroll_offset = ls.scroll_offset.min(max_offset);
}

/// Move the selection to the next row (`forward`) whose display text contains
/// `ls.last_query`, wrapping around. Returns false when there is no match.
fn jump_to_match(ls: &mut ListScreen, forward: bool) -> bool {
    if ls.last_query.is_empty() {
        return false;
    }
    let needle = ls.last_query.to_lowercase();
    let rows: Vec<String> = ls
        .filtered_items()
        .iter()
        .map(|i| i.display.clone())
        .collect();
    if rows.is_empty() {
        return false;
    }
    let len = rows.len();
    for step in 1..=len {
        let idx = if forward {
            (ls.selected + step) % len
        } else {
            (ls.selected + len * 2 - step) % len
        };
        if crate::app::fuzzy_match(&rows[idx], &needle) {
            ls.selected = idx;
            return true;
        }
    }
    false
}

/// Filter prompt editing: characters, word/line deletion, submit and cancel.
fn handle_filter_input(app: &mut App, key: event::KeyEvent, mut ls: ListScreen) {
    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Esc => {
            ls.filter.clear();
            ls.filter_active = false;
        }
        KeyCode::Enter => {
            ls.filter_active = false;
        }
        KeyCode::Backspace => {
            ls.filter.pop();
        }
        KeyCode::Char('w') if ctrl => {
            let trimmed = ls.filter.trim_end();
            let cut = trimmed.rfind(' ').map(|i| i + 1).unwrap_or(0);
            ls.filter.truncate(cut);
        }
        KeyCode::Char('u') if ctrl => {
            ls.filter.clear();
        }
        KeyCode::Char(c) if !ctrl => {
            ls.filter.push(c);
        }
        _ => {
            *app.current_screen_mut() = Screen::List(ls);
            return;
        }
    }
    if !ls.filter.is_empty() {
        ls.last_query = ls.filter.clone();
    }
    ls.selected = 0;
    ls.scroll_offset = 0;
    preview::trigger_preview_for_selected(app, &ls);
    *app.current_screen_mut() = Screen::List(ls);
}

pub(super) async fn handle_list(app: &mut App, key: event::KeyEvent, mut ls: ListScreen) {
    if ls.filter_active {
        handle_filter_input(app, key, ls);
        return;
    }

    let visible = app.list_visible_rows;
    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    let max_idx = ls.total_rows().saturating_sub(1);

    match key.code {
        KeyCode::Esc => {
            app.pop_screen();
        }
        KeyCode::Char('q') => {
            app.pop_screen();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            ls.selected = ls.selected.saturating_sub(1);
            clamp_scroll(&mut ls, visible);
            preview::trigger_preview_for_selected(app, &ls);
            *app.current_screen_mut() = Screen::List(ls);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            ls.selected = (ls.selected + 1).min(max_idx);
            clamp_scroll(&mut ls, visible);
            preview::trigger_preview_for_selected(app, &ls);
            *app.current_screen_mut() = Screen::List(ls);
        }
        KeyCode::PageUp => {
            ls.selected = ls.selected.saturating_sub(visible);
            clamp_scroll(&mut ls, visible);
            preview::trigger_preview_for_selected(app, &ls);
            *app.current_screen_mut() = Screen::List(ls);
        }
        KeyCode::PageDown => {
            ls.selected = (ls.selected + visible).min(max_idx);
            clamp_scroll(&mut ls, visible);
            preview::trigger_preview_for_selected(app, &ls);
            *app.current_screen_mut() = Screen::List(ls);
        }
        KeyCode::Char('u') if ctrl => {
            ls.selected = ls.selected.saturating_sub(visible / 2);
            clamp_scroll(&mut ls, visible);
            preview::trigger_preview_for_selected(app, &ls);
            *app.current_screen_mut() = Screen::List(ls);
        }
        KeyCode::Char('d') if ctrl => {
            ls.selected = (ls.selected + visible / 2).min(max_idx);
            clamp_scroll(&mut ls, visible);
            preview::trigger_preview_for_selected(app, &ls);
            *app.current_screen_mut() = Screen::List(ls);
        }
        KeyCode::Home | KeyCode::Char('g') => {
            ls.selected = 0;
            clamp_scroll(&mut ls, visible);
            preview::trigger_preview_for_selected(app, &ls);
            *app.current_screen_mut() = Screen::List(ls);
        }
        KeyCode::End | KeyCode::Char('G') => {
            ls.selected = max_idx;
            clamp_scroll(&mut ls, visible);
            preview::trigger_preview_for_selected(app, &ls);
            *app.current_screen_mut() = Screen::List(ls);
        }
        KeyCode::Char('n') | KeyCode::Char('N') => {
            let forward = key.code == KeyCode::Char('n');
            if !jump_to_match(&mut ls, forward) && !ls.last_query.is_empty() {
                app.set_info(format!("No match: {}", ls.last_query));
            }
            clamp_scroll(&mut ls, visible);
            preview::trigger_preview_for_selected(app, &ls);
            *app.current_screen_mut() = Screen::List(ls);
        }
        KeyCode::Char('/') => {
            ls.filter_active = true;
            *app.current_screen_mut() = Screen::List(ls);
        }
        KeyCode::Char('r') if ls.title == "Subscription Feed" => {
            let subs = match &ls.load_more {
                Some(lm) => lm.subs.clone(),
                None => youtube::load_subscriptions(),
            };
            if subs.is_empty() {
                app.set_error("No YouTube subscriptions found.");
            } else {
                app.set_info("Refreshing feed…");
                super::menus::spawn_feed_fetch(app.tx.clone(), subs, true, false);
            }
            *app.current_screen_mut() = Screen::List(ls);
        }
        KeyCode::Backspace => {
            ls.filter.clear();
            ls.selected = 0;
            ls.scroll_offset = 0;
            preview::trigger_preview_for_selected(app, &ls);
            *app.current_screen_mut() = Screen::List(ls);
        }
        KeyCode::Char('p') | KeyCode::Char('d') | KeyCode::Char('y') | KeyCode::Char('s') => {
            let item = ls.filtered_items().get(ls.selected).map(|i| (*i).clone());
            *app.current_screen_mut() = Screen::List(ls);
            if let Some(item) = item {
                item_shortcut(app, key.code, item).await;
            }
        }
        KeyCode::Enter => {
            let filtered_len = ls.filtered_items().len();

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
        KeyCode::Tab => {
            let selected_data = ls.filtered_items().get(ls.selected).map(|i| i.data.clone());
            if let Some(ItemData::YoutubeVideo(v)) = selected_data {
                if matches!(ls.context, ListContext::Queue) {
                    app.queue.retain(|q| q.id != v.id);
                    app.set_info(format!("Removed from queue: {}", v.title));
                    *app.current_screen_mut() = Screen::List(build_queue_screen(app));
                    return;
                }
                if app.queue.iter().any(|q| q.id == v.id) {
                    app.set_info(format!("Already queued: {}", v.title));
                } else {
                    app.queue.push(v.clone());
                    app.set_success(format!("Queued ({}): {}", app.queue.len(), v.title));
                }
            }
            *app.current_screen_mut() = Screen::List(ls);
        }
        _ => {}
    }
}

/// Single-key actions on the selected row: play, download, yank URL, save.
async fn item_shortcut(app: &mut App, code: KeyCode, item: ListItem) {
    let copy = |app: &mut App, url: &str| match crate::events::copy_to_clipboard(url) {
        Ok(()) => app.set_success(format!("Copied: {}", url)),
        Err(e) => app.set_error(e.to_string()),
    };

    match item.data {
        ItemData::YoutubeVideo(video) => match code {
            KeyCode::Char('p') => {
                super::actions::video_action_execute(app, &video, "Watch").await;
            }
            KeyCode::Char('d') => {
                super::actions::video_action_execute(app, &video, "Download").await;
            }
            KeyCode::Char('y') => copy(app, &video.url),
            KeyCode::Char('s') => {
                let action = if app.saved_ids.contains(&video.id) {
                    "UnSave"
                } else {
                    "Save"
                };
                super::actions::video_action_execute(app, &video, action).await;
            }
            _ => {}
        },

        ItemData::TwitchStream(stream) => {
            let url = twitch::twitch_stream_url(&stream.login);
            match code {
                KeyCode::Char('p') => {
                    let args = player::streamlink_args(
                        &url,
                        &app.config.twitch.quality,
                        &app.config.twitch.player,
                    );
                    let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                    let _ = player::launch_external(&args_str).await;
                }
                KeyCode::Char('y') => copy(app, &url),
                _ => {}
            }
        }

        ItemData::TwitchVod(vod) => match code {
            KeyCode::Char('p') => {
                let quality = app.config.youtube.video_quality.clone();
                let args = player::mpv_watch_args(&vod.url, &vod.title, &quality);
                let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let _ = player::launch_external(&args_str).await;
            }
            KeyCode::Char('d') => {
                let url = vod.url.clone();
                let dl_dir = app.config.youtube.download_directory.clone();
                let title = vod.title.clone();
                let tx = app.tx.clone();
                tokio::spawn(async move {
                    let args = player::ytdlp_download_args(&url, &dl_dir);
                    let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                    let _ = tx.send(AppEvent::DownloadStarted(format!(
                        "Downloading VOD: {}",
                        title
                    )));
                    match player::run_background(&args_str).await {
                        Ok(_) => {
                            let _ = tx.send(AppEvent::StatusMessage(
                                "VOD download complete.".to_string(),
                            ));
                        }
                        Err(e) => {
                            let _ = tx.send(AppEvent::Error(format!("VOD download failed: {}", e)));
                        }
                    }
                });
            }
            KeyCode::Char('y') => copy(app, &vod.url),
            _ => {}
        },

        ItemData::Channel(channel) if code == KeyCode::Char('y') => copy(app, &channel.url),

        _ => {}
    }
}

/// Queue screen: two virtual command rows followed by the queued videos.
pub(super) fn build_queue_screen(app: &App) -> ListScreen {
    let mut ls = App::make_video_list("Queue", app.queue.clone(), ListContext::Queue);
    let mut items = vec![
        ListItem {
            display: "▶  Play Queue".to_string(),
            data: ItemData::Text("play".to_string()),
        },
        ListItem {
            display: "✖  Clear Queue".to_string(),
            data: ItemData::Text("clear".to_string()),
        },
    ];
    items.append(&mut ls.items);
    ls.items = items;
    ls
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
    app.loading = Some(format!(
        "Loading more (up to {} per channel)…",
        playlist_end
    ));

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

async fn handle_list_item_select(app: &mut App, item: ListItem, context: ListContext) {
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
                app.push_screen(Screen::List(build_vod_type_list(&user)));
            }
        }

        ListContext::SelectVodType(login) => {
            if let ItemData::Text(vod_type) = item.data {
                let tx = app.tx.clone();
                let client_id = app.config.twitch.client_id.clone();
                let login = login.clone();
                app.loading = Some(format!("Fetching VODs for {}…", login));
                tokio::spawn(async move {
                    match twitch::fetch_vods(&client_id, &login, &vod_type).await {
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

        ListContext::SelectGameForStreams => {
            if let ItemData::TwitchGame(game) = item.data {
                let tx = app.tx.clone();
                let client_id = app.config.twitch.client_id.clone();
                let name = game.name.clone();
                app.loading = Some(format!("Loading {}…", name));
                tokio::spawn(async move {
                    match twitch::fetch_game_streams(&client_id, &name, 40).await {
                        Ok(streams) => {
                            let _ = tx.send(AppEvent::TwitchTopStreams(streams));
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
                    "Import Subscriptions" => {
                        app.push_screen(Screen::SearchInput(SearchInputScreen {
                            prompt:
                                "Import subscriptions (path to NewPipe JSON / OPML / Takeout CSV)"
                                    .to_string(),
                            input: String::new(),
                            context: SearchContext::ImportSubscriptions,
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
                        let ls =
                            ListScreen::new("Search History", items, ListContext::SearchHistory);
                        app.push_screen(Screen::List(ls));
                    }
                    "Edit Search History" => {
                        let path = config::youtube_search_history_file();
                        let editor = app.config.youtube.editor.clone();
                        let _ = player::launch_external(&[&editor, &path.to_string_lossy()]).await;
                    }
                    "Edit Custom Playlists" => {
                        let path = config::youtube_custom_playlists_file();
                        let editor = app.config.youtube.editor.clone();
                        let _ = player::launch_external(&[&editor, &path.to_string_lossy()]).await;
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

        ListContext::Queue => match item.data {
            ItemData::Text(cmd) if cmd == "play" => {
                if app.queue.is_empty() {
                    app.set_info("Queue is empty.");
                    return;
                }
                let urls: Vec<String> = app.queue.iter().map(|v| v.url.clone()).collect();
                let mut args = player::mpv_queue_args(&urls, &app.config.youtube.video_quality);
                args.extend(player::mpv_sponsorblock_args(
                    &app.config.youtube.sponsorblock,
                ));
                let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let _ = player::launch_external(&args_str).await;
                if app.config.youtube.update_recent {
                    for v in &app.queue {
                        youtube::add_to_recent(v, app.config.youtube.no_of_recent).ok();
                    }
                }
                let watched: Vec<String> = app.queue.iter().map(|v| v.id.clone()).collect();
                app.watched_ids.extend(watched);
                app.queue.clear();
            }
            ItemData::Text(cmd) if cmd == "clear" => {
                app.queue.clear();
                app.set_success("Queue cleared.");
            }
            ItemData::YoutubeVideo(video) => {
                app.push_screen(Screen::VideoActions(VideoActionsScreen {
                    video,
                    selected: 0,
                }));
            }
            _ => {}
        },

        ListContext::ChannelTab(channel_url) => {
            if let ItemData::YoutubeVideo(video) = item.data {
                app.push_screen(Screen::VideoActions(VideoActionsScreen {
                    video,
                    selected: 0,
                }));
            } else if let ItemData::Text(tab) = item.data {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(n: usize) -> ListScreen {
        let items = (0..n)
            .map(|i| ListItem {
                display: format!("item {}", i),
                data: ItemData::Text(i.to_string()),
            })
            .collect();
        ListScreen::new("t", items, ListContext::Miscellaneous)
    }

    #[test]
    fn clamp_scroll_follows_selection_down() {
        let mut ls = screen(100);
        ls.selected = 25;
        clamp_scroll(&mut ls, 10);
        assert_eq!(ls.scroll_offset, 16);
    }

    #[test]
    fn clamp_scroll_follows_selection_up() {
        let mut ls = screen(100);
        ls.scroll_offset = 40;
        ls.selected = 12;
        clamp_scroll(&mut ls, 10);
        assert_eq!(ls.scroll_offset, 12);
    }

    #[test]
    fn clamp_scroll_never_scrolls_past_the_last_page() {
        let mut ls = screen(30);
        ls.selected = 29;
        ls.scroll_offset = 28;
        clamp_scroll(&mut ls, 10);
        assert_eq!(ls.scroll_offset, 20);
    }

    #[test]
    fn clamp_scroll_short_list_stays_at_top() {
        let mut ls = screen(3);
        ls.selected = 2;
        clamp_scroll(&mut ls, 10);
        assert_eq!(ls.scroll_offset, 0);
    }

    #[test]
    fn clamp_scroll_clamps_selection_to_last_row() {
        let mut ls = screen(5);
        ls.selected = 99;
        clamp_scroll(&mut ls, 10);
        assert_eq!(ls.selected, 4);
    }

    #[test]
    fn jump_to_match_moves_forward_and_wraps() {
        let mut ls = screen(20);
        ls.last_query = "item 1".to_string();
        ls.selected = 0;
        assert!(jump_to_match(&mut ls, true));
        assert_eq!(ls.selected, 1);
        ls.selected = 19;
        assert!(jump_to_match(&mut ls, true));
        assert_eq!(ls.selected, 1);
    }

    #[test]
    fn jump_to_match_moves_backward() {
        let mut ls = screen(20);
        ls.last_query = "ITEM 3".to_string();
        ls.selected = 10;
        assert!(jump_to_match(&mut ls, false));
        assert_eq!(ls.selected, 3);
    }

    #[test]
    fn jump_to_match_without_query_or_match_is_a_noop() {
        let mut ls = screen(5);
        assert!(!jump_to_match(&mut ls, true));
        ls.last_query = "nothing".to_string();
        assert!(!jump_to_match(&mut ls, true));
        assert_eq!(ls.selected, 0);
    }
}
