//! Thumbnail downloading and inline-image rendering (kitty / iTerm2 protocols).

use crate::app::{App, AppEvent, GraphicsProtocol, ListScreen, PreviewEntry, Screen};
use crate::config;
use crate::models::{ItemData, Video};

/// Trigger a thumbnail preview for whatever item is currently selected in `ls`.
pub fn trigger_preview_for_selected(app: &mut App, ls: &ListScreen) {
    let filtered = ls.filtered_items();
    if ls.selected >= filtered.len() {
        return;
    }
    match &filtered[ls.selected].data {
        ItemData::YoutubeVideo(ref v) => trigger_preview(app, v),
        ItemData::TwitchStream(ref s) if s.is_live => {
            let cache_key = format!("twitch_{}", s.login);
            let url = format!(
                "https://static-cdn.jtvnw.net/previews-ttv/live_user_{}-640x360.jpg",
                s.login
            );
            trigger_preview_raw(app, cache_key, url);
        }
        ItemData::TwitchVod(ref v) if !v.thumbnail.is_empty() => {
            let cache_key = format!("twitchvod_{}", v.id);
            trigger_preview_raw(app, cache_key, v.thumbnail.clone());
        }
        ItemData::TwitchGame(ref g) if !g.box_art.is_empty() => {
            trigger_preview_raw(app, twitch_game_cache_key(&g.name), g.box_art.clone());
        }
        ItemData::Channel(ref c) => trigger_channel_preview(app, &c.url),
        _ => {}
    }
}

/// Cache key (and on-disk PNG filename stem) for a Twitch category box-art preview.
pub fn twitch_game_cache_key(name: &str) -> String {
    let id: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("twitchgame_{}", id)
}

/// Cache key (and on-disk PNG filename stem) for a channel's avatar preview.
pub fn channel_cache_key(channel_url: &str) -> String {
    let id: String = channel_url
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(channel_url)
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '@' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    format!("channel_{}", id)
}

/// Resolve (lazily, via yt-dlp the first time) and download a channel's avatar.
/// The avatar URL is disk-cached, so only the first hover spawns a yt-dlp; later
/// hovers are a plain HTTP fetch like any other thumbnail.
fn trigger_channel_preview(app: &mut App, channel_url: &str) {
    let cache_key = channel_cache_key(channel_url);
    if app.preview_cache.contains_key(&cache_key) {
        return;
    }
    app.preview_cache
        .insert(cache_key.clone(), PreviewEntry { ready: false });
    let tx = app.tx.clone();
    let cache_dir = config::youtube_preview_cache_dir();
    let channel_url = channel_url.to_string();
    tokio::spawn(async move {
        let Some(avatar_url) = crate::youtube::channel_avatar_url(&channel_url).await else {
            return;
        };
        let ready = fetch_thumbnail(&cache_key, &avatar_url, &cache_dir).await;
        if ready {
            let _ = tx.send(AppEvent::PreviewReady {
                video_id: cache_key,
            });
        }
    });
}

/// If the video's preview isn't cached yet, spawn an async task to download
/// the thumbnail, then send `AppEvent::PreviewReady`.
pub fn trigger_preview(app: &mut App, video: &Video) {
    if video.thumbnail.is_empty() || app.preview_cache.contains_key(&video.id) {
        return;
    }
    // Insert a placeholder so we don't launch duplicate tasks.
    app.preview_cache
        .insert(video.id.clone(), PreviewEntry { ready: false });
    let tx = app.tx.clone();
    let video_id = video.id.clone();
    let thumbnail_url = video.thumbnail.clone();
    let cache_dir = config::youtube_preview_cache_dir();
    tokio::spawn(async move {
        let ready = fetch_thumbnail(&video_id, &thumbnail_url, &cache_dir).await;
        if ready {
            let _ = tx.send(AppEvent::PreviewReady { video_id });
        }
    });
}

/// Like `trigger_preview` but takes an arbitrary cache key and URL directly.
pub fn trigger_preview_raw(app: &mut App, cache_key: String, thumbnail_url: String) {
    if app.preview_cache.contains_key(&cache_key) {
        return;
    }
    app.preview_cache
        .insert(cache_key.clone(), PreviewEntry { ready: false });
    let tx = app.tx.clone();
    let cache_dir = config::youtube_preview_cache_dir();
    tokio::spawn(async move {
        let ready = fetch_thumbnail(&cache_key, &thumbnail_url, &cache_dir).await;
        if ready {
            let _ = tx.send(AppEvent::PreviewReady {
                video_id: cache_key,
            });
        }
    });
}

async fn fetch_thumbnail(video_id: &str, thumbnail_url: &str, cache_dir: &std::path::Path) -> bool {
    let _ = tokio::fs::create_dir_all(cache_dir).await;
    let img_path = cache_dir.join(format!("{}.png", video_id));
    if img_path.exists() {
        return true;
    }
    if let Ok(resp) = reqwest::get(thumbnail_url).await {
        if let Ok(bytes) = resp.bytes().await {
            // Decode whatever format YouTube returns and re-encode as PNG.
            if let Ok(img) = image::load_from_memory(&bytes) {
                let mut png_bytes = Vec::new();
                if img
                    .write_to(
                        &mut std::io::Cursor::new(&mut png_bytes),
                        image::ImageFormat::Png,
                    )
                    .is_ok()
                {
                    let _ = tokio::fs::write(&img_path, &png_bytes).await;
                }
            }
        }
    }
    img_path.exists()
}

/// Returns the preview cache key for the selected item in the current list screen.
fn selected_video_id(app: &App) -> Option<String> {
    if let Screen::List(ref ls) = app.current_screen() {
        let filtered = ls.filtered_items();
        if let Some(item) = filtered.get(ls.selected) {
            return match &item.data {
                ItemData::YoutubeVideo(v) => Some(v.id.clone()),
                ItemData::TwitchStream(s) if s.is_live => Some(format!("twitch_{}", s.login)),
                ItemData::TwitchVod(v) if !v.thumbnail.is_empty() => {
                    Some(format!("twitchvod_{}", v.id))
                }
                ItemData::TwitchGame(g) if !g.box_art.is_empty() => {
                    Some(twitch_game_cache_key(&g.name))
                }
                ItemData::Channel(c) => Some(channel_cache_key(&c.url)),
                _ => None,
            };
        }
    }
    None
}

/// Compute (c, r) cell dimensions for a PNG that fit within (max_c, max_r) without stretching.
/// Assumes standard 8×16px terminal cells (so 1 row is twice as tall as 1 col in pixels).
fn png_aspect_fit(png_bytes: &[u8], max_c: u16, max_r: u16) -> (u16, u16) {
    // Read width/height from the PNG IHDR chunk (bytes 16–23).
    let (img_w, img_h) = if png_bytes.len() >= 24 && png_bytes[0..8] == *b"\x89PNG\r\n\x1a\n" {
        let w = u32::from_be_bytes([png_bytes[16], png_bytes[17], png_bytes[18], png_bytes[19]]);
        let h = u32::from_be_bytes([png_bytes[20], png_bytes[21], png_bytes[22], png_bytes[23]]);
        if w > 0 && h > 0 {
            (w as f32, h as f32)
        } else {
            (16.0, 9.0)
        }
    } else {
        (16.0, 9.0) // fallback: assume 16:9
    };

    // cols_per_row: how many columns equal one row in pixel height (cell = 8px wide, 16px tall).
    let cols_per_row = (img_w / img_h) * 2.0;

    let c_from_r = (max_r as f32 * cols_per_row).round() as u16;
    let r_from_c = (max_c as f32 / cols_per_row).round() as u16;

    if c_from_r <= max_c {
        // Height-limited: image fits within max_c columns.
        (c_from_r.max(1), max_r.max(1))
    } else {
        // Width-limited: constrain to max_c columns.
        (max_c.max(1), r_from_c.max(1))
    }
}

/// After ratatui draws a frame, overlay the selected thumbnail using whichever
/// inline-image protocol the terminal supports (kitty or iTerm2).
pub fn kitty_update_display(app: &mut App) {
    if app.graphics == GraphicsProtocol::None {
        return;
    }

    let selected_id = selected_video_id(app);

    let Some(ref video_id) = selected_id else {
        if app.kitty_displayed.is_some() {
            clear_preview(app);
            app.kitty_displayed = None;
        }
        return;
    };

    if app.kitty_displayed.as_deref() == Some(video_id.as_str()) {
        return;
    }

    clear_preview(app);
    app.kitty_displayed = None;

    let Some(entry) = app.preview_cache.get(video_id) else {
        return;
    };
    if !entry.ready {
        return;
    }

    let Some((tx, ty, tw, th)) = app.preview_thumb_area else {
        return;
    };

    let img_path = config::youtube_preview_cache_dir().join(format!("{}.png", video_id));
    if !img_path.exists() {
        return;
    }

    let Ok(png_bytes) = std::fs::read(&img_path) else {
        return;
    };

    // Terminal cells are ~8px wide × 16px tall, so 1 cell-row = 2 cell-columns in pixel height.
    let (display_c, display_r) = png_aspect_fit(&png_bytes, tw, th);

    match app.graphics {
        GraphicsProtocol::Kitty => emit_kitty_image(&png_bytes, tx, ty, display_c, display_r),
        GraphicsProtocol::ITerm2 => emit_iterm2_image(&png_bytes, tx, ty, display_c, display_r),
        GraphicsProtocol::None => return,
    }

    app.kitty_displayed = Some(video_id.clone());
}

/// Emit a thumbnail via the kitty graphics protocol at the given cell position.
fn emit_kitty_image(png_bytes: &[u8], tx: u16, ty: u16, display_c: u16, display_r: u16) {
    use base64::Engine;
    use std::io::Write;

    let data_b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes);
    let b64_bytes = data_b64.as_bytes();

    let mut stdout = std::io::stdout();
    // Move cursor to the thumbnail area (1-indexed row/col).
    let _ = write!(stdout, "\x1b[{};{}H", ty + 1, tx + 1);

    // Send in 4096-byte base64 chunks (kitty protocol limit per APC).
    const CHUNK: usize = 4096;
    let num_chunks = (b64_bytes.len() + CHUNK - 1).max(1) / CHUNK;
    for (i, chunk) in b64_bytes.chunks(CHUNK).enumerate() {
        let m = if i + 1 < num_chunks { 1 } else { 0 };
        let s = std::str::from_utf8(chunk).unwrap_or("");
        if i == 0 {
            let _ = write!(
                stdout,
                "\x1b_Ga=T,i=1,t=d,f=100,c={},r={},q=2,m={};{}\x1b\\",
                display_c, display_r, m, s
            );
        } else {
            let _ = write!(stdout, "\x1b_Gm={};{}\x1b\\", m, s);
        }
    }
    let _ = stdout.flush();
}

/// Emit a thumbnail via the iTerm2 inline-image protocol (also WezTerm).
fn emit_iterm2_image(png_bytes: &[u8], tx: u16, ty: u16, display_c: u16, display_r: u16) {
    use base64::Engine;
    use std::io::Write;

    let data_b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes);

    let mut stdout = std::io::stdout();
    // Move cursor to the thumbnail area (1-indexed row/col).
    let _ = write!(stdout, "\x1b[{};{}H", ty + 1, tx + 1);
    // width/height as bare cell counts; preserveAspectRatio keeps it undistorted.
    let _ = write!(
        stdout,
        "\x1b]1337;File=inline=1;width={};height={};preserveAspectRatio=1;size={}:{}\x07",
        display_c,
        display_r,
        png_bytes.len(),
        data_b64
    );
    let _ = stdout.flush();
}

/// Clear the previously displayed thumbnail for the active protocol.
fn clear_preview(app: &App) {
    use std::io::Write;

    let mut stdout = std::io::stdout();
    match app.graphics {
        GraphicsProtocol::Kitty => {
            // Delete all kitty images visible on screen.
            let _ = write!(stdout, "\x1b_Ga=d,d=a,q=2\x1b\\");
        }
        GraphicsProtocol::ITerm2 => {
            // iTerm2 has no delete-by-id: overwrite the image cells with spaces.
            if let Some((tx, ty, tw, th)) = app.preview_thumb_area {
                let blank = " ".repeat(tw as usize);
                for row in 0..th {
                    let _ = write!(stdout, "\x1b[{};{}H{}", ty + row + 1, tx + 1, blank);
                }
            }
        }
        GraphicsProtocol::None => {}
    }
    let _ = stdout.flush();
}
