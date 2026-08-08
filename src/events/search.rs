//! Search input screen and search execution per context.

use crate::app::{App, AppEvent, ListContext, Screen, SearchContext, SearchInputScreen};
use crate::youtube;
use crossterm::event::{self, KeyCode};

pub(super) async fn handle_search_input(
    app: &mut App,
    key: event::KeyEvent,
    mut si: SearchInputScreen,
) {
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
                match crate::twitch::search_twitch(&q, &client_id).await {
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

        SearchContext::ImportSubscriptions => match crate::subs_import::import_file(input.trim()) {
            Ok((added, skipped)) => {
                app.set_success(format!(
                    "Imported {} subscription(s), {} already present.",
                    added, skipped
                ));
            }
            Err(e) => app.set_error(format!("Import failed: {}", e)),
        },

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
