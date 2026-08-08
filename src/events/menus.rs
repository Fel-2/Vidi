//! Top-level menu screens: mode select, YouTube menu, Twitch menu.

use crate::app::{
    App, AppEvent, ListContext, ListScreen, Screen, SearchContext, SearchInputScreen,
};
use crate::models::{ItemData, ListItem, SubFeedLoadMore};
use crate::ui::{TWITCH_MENU_ITEMS, YOUTUBE_MENU_ITEMS};
use crate::{config, player, twitch, youtube};
use crossterm::event::{self, KeyCode};

pub(super) async fn handle_mode_select(app: &mut App, key: event::KeyEvent, selected: usize) {
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
