//! Action menus: video actions, quality select, channel actions, Twitch
//! stream/VOD actions.

use crate::app::{
    quality_options, App, AppEvent, ChannelActionsScreen, ChatScreen, ListContext,
    QualitySelectScreen, Screen, SearchContext, SearchInputScreen, TwitchStreamActionsScreen,
    TwitchVodActionsScreen, VideoActionsScreen,
};
use crate::models::{ChannelTabLoadMore, Video};
use crate::ui::{
    channel_action_items, twitch_stream_action_items, twitch_vod_action_items, VIDEO_ACTION_ITEMS,
};
use crate::{chat, player, twitch, youtube};
use crossterm::event::{self, KeyCode};

use super::open_url_in_browser;

/// Launch mpv for a single video with resume + IPC progress tracking and
/// optional SponsorBlock chapter marking.
async fn watch_video(app: &mut App, video: &Video, quality: &str) {
    let mut args = player::mpv_watch_args(&video.url, &video.title, quality);
    args.extend(player::mpv_sponsorblock_args(
        &app.config.youtube.sponsorblock,
    ));
    if app.config.youtube.watch_progress && !video.id.is_empty() {
        if let Some(start) = crate::progress::resume_position(&video.id) {
            args.push(format!("--start={}", start as u64));
        }
        let socket = crate::progress::socket_path();
        args.extend(crate::progress::mpv_ipc_args(&socket));
        tokio::spawn(crate::progress::track(socket, video.id.clone()));
    }
    let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let _ = player::launch_external(&args_str).await;
    if app.config.youtube.update_recent {
        youtube::add_to_recent(video, app.config.youtube.no_of_recent).ok();
    }
    app.watched_ids.insert(video.id.clone());
}

pub(super) async fn handle_video_actions(
    app: &mut App,
    key: event::KeyEvent,
    mut va: VideoActionsScreen,
) {
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

pub(super) async fn handle_quality_select(
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
            watch_video(app, &video, &quality).await;
            app.pop_screen();
        }
        KeyCode::Esc => {
            app.pop_screen();
        }
        _ => {}
    }
}

async fn video_action_execute(app: &mut App, video: &Video, action: &str) {
    let quality = &app.config.youtube.video_quality.clone();
    let download_dir = app.config.youtube.download_directory.clone();
    let sponsorblock = app.config.youtube.sponsorblock.clone();

    match action {
        "Watch" => {
            watch_video(app, video, quality).await;
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
            let mut args = player::mpv_watch_args(playlist_url, &video.title, quality);
            args.extend(player::mpv_sponsorblock_args(&sponsorblock));
            let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let _ = player::launch_external(&args_str).await;
        }
        "Download" => {
            let url = video.url.clone();
            let dl_dir = download_dir.clone();
            let tx = app.tx.clone();
            let title = video.title.clone();
            let sb = sponsorblock.clone();
            tokio::spawn(async move {
                let mut args = player::ytdlp_download_args(&url, &dl_dir);
                args.extend(player::ytdlp_sponsorblock_args(&sb));
                let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let _ = tx.send(AppEvent::DownloadStarted(format!("Downloading: {}", title)));
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
            let sb = sponsorblock.clone();
            tokio::spawn(async move {
                let mut args = player::ytdlp_download_audio_args(&url, &dl_dir);
                args.extend(player::ytdlp_sponsorblock_args(&sb));
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
            let sb = sponsorblock.clone();
            tokio::spawn(async move {
                let mut args = player::ytdlp_download_args(&playlist_url, &dl_dir);
                args.extend(player::ytdlp_sponsorblock_args(&sb));
                let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let _ = tx.send(AppEvent::DownloadStarted("Downloading all…".to_string()));
                match player::run_background(&args_str).await {
                    Ok(_) => {
                        let _ = tx.send(AppEvent::StatusMessage(
                            "Download all complete.".to_string(),
                        ));
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
            let sb = sponsorblock.clone();
            tokio::spawn(async move {
                let mut args = player::ytdlp_download_audio_args(&playlist_url, &dl_dir);
                args.extend(player::ytdlp_sponsorblock_args(&sb));
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
                        let _ =
                            tx.send(AppEvent::Error(format!("Download all audio failed: {}", e)));
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

pub(super) async fn handle_channel_actions(
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

pub(super) async fn handle_twitch_stream_actions(
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

pub(super) async fn handle_twitch_vod_actions(
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
                                let _ = tx.send(AppEvent::StatusMessage(
                                    "VOD download complete.".to_string(),
                                ));
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
