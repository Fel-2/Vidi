//! Top-level menu screens: mode select, YouTube, Twitch and PeerTube menus.

use crate::app::{
    App, AppEvent, ListContext, ListScreen, Screen, SearchContext, SearchInputScreen,
};
use crate::models::{ItemData, ListItem, Platform, SubFeedLoadMore};
use crate::ui::{PEERTUBE_MENU_ITEMS, TWITCH_MENU_ITEMS, YOUTUBE_MENU_ITEMS};
use crate::{config, peertube, player, twitch, youtube};
use crossterm::event::{self, KeyCode};

pub(super) const YOUTUBE_FEED_TITLE: &str = "Subscription Feed";
pub(super) const PEERTUBE_FEED_TITLE: &str = "PeerTube Feed";

pub(super) async fn handle_mode_select(app: &mut App, key: event::KeyEvent, selected: usize) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            let new = selected.saturating_sub(1);
            *app.current_screen_mut() = Screen::ModeSelect { selected: new };
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let new = (selected + 1).min(2);
            *app.current_screen_mut() = Screen::ModeSelect { selected: new };
        }
        KeyCode::Enter => match selected {
            0 => app.push_screen(Screen::YoutubeMenu { selected: 0 }),
            1 => app.push_screen(Screen::TwitchMenu { selected: 0 }),
            2 => open_peertube(app),
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

pub(super) async fn handle_youtube_menu(app: &mut App, key: event::KeyEvent, selected: usize) {
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
                        let _ = tx.send(AppEvent::VideoResults {
                            items,
                            context: ListContext::VideoActions,
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
                app.set_error(
                    "No YouTube subscriptions found. Add URLs to ~/.config/vidi/subscriptions",
                );
                return;
            }

            let make_load_more = |subs: Vec<String>| {
                Some(SubFeedLoadMore {
                    subs,
                    next_playlist_end: 20,
                    label: "── Load More ──".to_string(),
                    platform: Platform::Youtube,
                })
            };

            // Show the cache instantly even when stale; a stale cache kicks
            // off a background refresh that swaps the list in place.
            let cached = youtube::load_feed_cache_with_age();
            if let Some((items, _)) = cached.clone() {
                let _ = app.tx.send(AppEvent::SubFeedResults {
                    items,
                    load_more: make_load_more(subs.clone()),
                    title: YOUTUBE_FEED_TITLE.to_string(),
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

            spawn_feed_fetch(app.tx.clone(), subs, had_cache, had_cache);
        }
        "Channels" => {
            let tx = app.tx.clone();
            app.loading = Some("Loading channels…".to_string());
            tokio::spawn(async move {
                match youtube::fetch_channels().await {
                    Ok(channels) => {
                        let _ = tx.send(AppEvent::ChannelList {
                            channels,
                            context: ListContext::SelectChannelToBrowse,
                            title: "Channels".to_string(),
                        });
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
            let ls = App::make_video_list("Recent", videos, ListContext::VideoActions);
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
            let ls = App::make_video_list("Saved Videos", videos, ListContext::VideoActions);
            app.push_screen(Screen::List(ls));
        }
        "Queue" => {
            if app.queue.is_empty() {
                app.set_info("Queue is empty — press Tab on a video to queue it.");
                return;
            }
            app.push_screen(Screen::List(super::list::build_queue_screen(app)));
        }
        "Edit Config" => {
            let path = config::youtube_config_file();
            let editor = app.config.youtube.editor.clone();
            let _ = player::launch_external(&[&editor, &path.to_string_lossy()]).await;
            if let Ok(cfg) = config::load_config() {
                app.config = cfg;
            }
        }
        "Miscellaneous" => {
            let misc_items = [
                "Explore Channels",
                "Explore Playlists",
                "Import Subscriptions",
                "Search History",
                "Edit Search History",
                "Edit Custom Playlists",
                "Clear Search History",
                "Back",
            ];
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

/// Fetch the subscription feed in the background.
/// `in_place` – swap the visible feed list instead of pushing a new screen.
/// `silent_errors` – drop failures so a visible list isn't replaced by an error.
pub(super) fn spawn_feed_fetch(
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    subs: Vec<String>,
    in_place: bool,
    silent_errors: bool,
) {
    tokio::spawn(async move {
        match youtube::fetch_subscription_feed(subs.clone(), 5, 8, None).await {
            Ok(items) => {
                youtube::save_feed_cache(&items);
                let load_more = Some(SubFeedLoadMore {
                    subs,
                    next_playlist_end: 20,
                    label: "── Load More ──".to_string(),
                    platform: Platform::Youtube,
                });
                let title = YOUTUBE_FEED_TITLE.to_string();
                if in_place {
                    let _ = tx.send(AppEvent::SubFeedRefreshed {
                        items,
                        load_more,
                        title,
                    });
                } else {
                    let _ = tx.send(AppEvent::SubFeedResults {
                        items,
                        load_more,
                        title,
                    });
                }
            }
            Err(e) => {
                if !silent_errors {
                    let _ = tx.send(AppEvent::Error(e.to_string()));
                }
            }
        }
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Twitch menu
// ─────────────────────────────────────────────────────────────────────────────

pub(super) async fn handle_twitch_menu(app: &mut App, key: event::KeyEvent, selected: usize) {
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
                app.set_error(
                    "No Twitch subscriptions found. Add usernames to ~/.config/vidi/twitch_subs",
                );
                return;
            }
            let tx = app.tx.clone();
            let client_id = app.config.twitch.client_id.clone();
            app.loading = Some("Checking subscriptions…".to_string());
            tokio::spawn(async move {
                match twitch::fetch_subscriptions(&client_id).await {
                    Ok(streams) => {
                        let _ = tx.send(AppEvent::TwitchSubsResults(streams));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Error(e.to_string()));
                    }
                }
            });
        }
        "Top Streams" => {
            let tx = app.tx.clone();
            let client_id = app.config.twitch.client_id.clone();
            app.loading = Some("Loading top streams…".to_string());
            tokio::spawn(async move {
                match twitch::fetch_top_streams(&client_id, 40).await {
                    Ok(streams) => {
                        let _ = tx.send(AppEvent::TwitchTopStreams(streams));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Error(e.to_string()));
                    }
                }
            });
        }
        "Browse Categories" => {
            let tx = app.tx.clone();
            let client_id = app.config.twitch.client_id.clone();
            app.loading = Some("Loading categories…".to_string());
            tokio::spawn(async move {
                match twitch::fetch_top_games(&client_id, 40).await {
                    Ok(games) => {
                        let _ = tx.send(AppEvent::TwitchGamesResults(games));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Error(e.to_string()));
                    }
                }
            });
        }
        "Watch VODs" => {
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
// PeerTube menu
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn open_peertube(app: &mut App) {
    if app.config.peertube.instance.trim().is_empty() {
        app.push_screen(Screen::SearchInput(instance_input_screen(
            config::DEFAULT_PEERTUBE_INSTANCE,
        )));
    } else {
        app.push_screen(Screen::PeertubeMenu { selected: 0 });
    }
}

pub(super) fn instance_input_screen(prefill: &str) -> SearchInputScreen {
    SearchInputScreen {
        prompt: "PeerTube instance (↵ to accept)".to_string(),
        input: prefill.to_string(),
        context: SearchContext::PeertubeInstance,
    }
}

pub(super) async fn handle_peertube_menu(app: &mut App, key: event::KeyEvent, selected: usize) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            let new = selected.saturating_sub(1);
            *app.current_screen_mut() = Screen::PeertubeMenu { selected: new };
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let new = (selected + 1).min(PEERTUBE_MENU_ITEMS.len() - 1);
            *app.current_screen_mut() = Screen::PeertubeMenu { selected: new };
        }
        KeyCode::Enter => {
            peertube_menu_action(app, selected).await;
        }
        KeyCode::Esc => {
            app.pop_screen();
        }
        _ => {}
    }
}

async fn peertube_menu_action(app: &mut App, selected: usize) {
    let instance = app.config.peertube.instance.clone();
    let limit = app.config.youtube.no_of_search_results as u32;

    match PEERTUBE_MENU_ITEMS[selected] {
        "Trending" | "Recently Added" => {
            let trending = PEERTUBE_MENU_ITEMS[selected] == "Trending";
            let title = if trending {
                "Trending"
            } else {
                "Recently Added"
            };
            let tx = app.tx.clone();
            app.loading = Some(format!("Fetching {}…", title.to_lowercase()));
            tokio::spawn(async move {
                let result = if trending {
                    peertube::fetch_trending(&instance, limit).await
                } else {
                    peertube::fetch_recent(&instance, limit).await
                };
                match result {
                    Ok(items) => {
                        let _ = tx.send(AppEvent::VideoResults {
                            items,
                            context: ListContext::VideoActions,
                            title: format!("PeerTube {}", title),
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
                prompt: "PeerTube Search".to_string(),
                input: String::new(),
                context: SearchContext::PeertubeSearch,
            }));
        }
        "Subscription Feed" => {
            let subs = peertube::load_subs();
            if subs.is_empty() {
                app.set_error(
                    "No PeerTube subscriptions. Add channel@instance lines to ~/.config/vidi/peertube_subs",
                );
                return;
            }

            let cached = peertube::load_feed_cache_with_age();
            if let Some((items, _)) = cached.clone() {
                let _ = app.tx.send(AppEvent::SubFeedResults {
                    items,
                    load_more: peertube_load_more(subs.clone(), 20),
                    title: PEERTUBE_FEED_TITLE.to_string(),
                });
            }
            if matches!(cached, Some((_, true))) {
                return;
            }
            let had_cache = cached.is_some();
            if !had_cache {
                app.loading = Some("Fetching PeerTube feed…".to_string());
            }
            spawn_peertube_feed_fetch(app.tx.clone(), subs, 10, had_cache, had_cache);
        }
        "Subscribed Channels" => {
            let subs = peertube::load_subs();
            if subs.is_empty() {
                app.set_error("No PeerTube subscriptions yet.");
                return;
            }
            let tx = app.tx.clone();
            app.loading = Some("Loading channels…".to_string());
            tokio::spawn(async move {
                let mut channels = Vec::new();
                for handle in subs {
                    match peertube::fetch_channel_meta(&handle).await {
                        Ok(ch) => channels.push(ch),
                        Err(_) => channels.push(crate::models::Channel {
                            name: handle.clone(),
                            url: peertube::channel_url(&handle),
                            avatar: None,
                        }),
                    }
                }
                let _ = tx.send(AppEvent::ChannelList {
                    channels,
                    context: ListContext::SelectPeertubeChannel,
                    title: "PeerTube Channels".to_string(),
                });
            });
        }
        "Explore Channels" => {
            app.push_screen(Screen::SearchInput(SearchInputScreen {
                prompt: "Explore PeerTube Channels".to_string(),
                input: String::new(),
                context: SearchContext::PeertubeExploreChannels,
            }));
        }
        "Edit Subs" => {
            let path = config::peertube_subs_file();
            let editor = app.config.youtube.editor.clone();
            let _ = player::launch_external(&[&editor, &path.to_string_lossy()]).await;
        }
        "Change Instance" => {
            let current = if instance.is_empty() {
                config::DEFAULT_PEERTUBE_INSTANCE.to_string()
            } else {
                instance
            };
            app.push_screen(Screen::SearchInput(instance_input_screen(&current)));
        }
        _ => {}
    }
}

pub(super) fn peertube_load_more(subs: Vec<String>, next: u32) -> Option<SubFeedLoadMore> {
    Some(SubFeedLoadMore {
        subs,
        next_playlist_end: next,
        label: "── Load More ──".to_string(),
        platform: Platform::Peertube,
    })
}

pub(super) fn spawn_peertube_feed_fetch(
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    subs: Vec<String>,
    per_channel: u32,
    in_place: bool,
    silent_errors: bool,
) {
    tokio::spawn(async move {
        match peertube::fetch_subscription_feed(subs.clone(), per_channel, 8).await {
            Ok(items) => {
                peertube::save_feed_cache(&items);
                let load_more = peertube_load_more(subs, per_channel * 2);
                let title = PEERTUBE_FEED_TITLE.to_string();
                if in_place {
                    let _ = tx.send(AppEvent::SubFeedRefreshed {
                        items,
                        load_more,
                        title,
                    });
                } else {
                    let _ = tx.send(AppEvent::SubFeedResults {
                        items,
                        load_more,
                        title,
                    });
                }
            }
            Err(e) => {
                if !silent_errors {
                    let _ = tx.send(AppEvent::Error(e.to_string()));
                }
            }
        }
    });
}
