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
        ItemData::Video(ref v) => trigger_preview(app, v),
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
        ItemData::KickStream(ref s) => {
            // Live: the stream thumbnail. Offline: the channel avatar.
            let image = if s.is_live && !s.thumbnail.is_empty() {
                Some(s.thumbnail.clone())
            } else if !s.avatar.is_empty() {
                Some(s.avatar.clone())
            } else {
                None
            };
            if let Some(url) = image {
                trigger_preview_raw(app, kick_stream_cache_key(&s.slug), url);
            }
        }
        ItemData::KickVod(ref v) if !v.thumbnail.is_empty() => {
            trigger_preview_raw(app, kick_vod_cache_key(&v.id), v.thumbnail.clone());
        }
        ItemData::Channel(ref c) => match c.avatar {
            Some(ref url) => trigger_preview_raw(app, channel_cache_key(&c.url), url.clone()),
            None => trigger_channel_preview(app, &c.url),
        },
        _ => {}
    }
}

/// Cache key (and on-disk PNG filename stem) for a Twitch category box-art preview.
pub fn twitch_game_cache_key(name: &str) -> String {
    format!("twitchgame_{}", sanitize(name))
}

/// Cache key for a Kick live-stream thumbnail.
pub fn kick_stream_cache_key(slug: &str) -> String {
    format!("kick_{}", sanitize(slug))
}

/// Cache key for a Kick VOD thumbnail.
pub fn kick_vod_cache_key(id: &str) -> String {
    format!("kickvod_{}", sanitize(id))
}

/// Lowercase alphanumeric-safe token suitable for a cache filename stem.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// Cache key (and on-disk PNG filename stem) for a channel's avatar preview.
pub fn channel_cache_key(channel_url: &str) -> String {
    let trimmed = channel_url.trim_end_matches('/');
    let host = trimmed
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or_default();
    let name = trimmed.rsplit('/').next().unwrap_or(channel_url);
    let id: String = format!("{}_{}", host, name)
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
    if !crate::player::is_youtube_url(channel_url) {
        return;
    }
    let cache_key = channel_cache_key(channel_url);
    if !should_fetch(app, &cache_key) {
        return;
    }
    app.preview_cache.insert(
        cache_key.clone(),
        PreviewEntry {
            ready: false,
            retry_at: None,
        },
    );
    let tx = app.tx.clone();
    let cache_dir = config::youtube_preview_cache_dir();
    let channel_url = channel_url.to_string();
    tokio::spawn(async move {
        let Some(avatar_url) = crate::youtube::channel_avatar_url(&channel_url).await else {
            let _ = tx.send(AppEvent::PreviewFailed {
                video_id: cache_key,
            });
            return;
        };
        let ready = fetch_thumbnail(&cache_key, &avatar_url, &cache_dir).await;
        let _ = tx.send(if ready {
            AppEvent::PreviewReady {
                video_id: cache_key,
            }
        } else {
            AppEvent::PreviewFailed {
                video_id: cache_key,
            }
        });
    });
}

/// If the video's preview isn't cached yet, spawn an async task to download
/// the thumbnail, then send `AppEvent::PreviewReady`.
pub fn trigger_preview(app: &mut App, video: &Video) {
    if video.thumbnail.is_empty() || !should_fetch(app, &video.id) {
        return;
    }
    // Insert a placeholder so we don't launch duplicate tasks.
    app.preview_cache.insert(
        video.id.clone(),
        PreviewEntry {
            ready: false,
            retry_at: None,
        },
    );
    let tx = app.tx.clone();
    let video_id = video.id.clone();
    let thumbnail_url = video.thumbnail.clone();
    let cache_dir = config::youtube_preview_cache_dir();
    tokio::spawn(async move {
        let ready = fetch_thumbnail(&video_id, &thumbnail_url, &cache_dir).await;
        let _ = tx.send(if ready {
            AppEvent::PreviewReady { video_id }
        } else {
            AppEvent::PreviewFailed { video_id }
        });
    });
}

/// Like `trigger_preview` but takes an arbitrary cache key and URL directly.
pub fn trigger_preview_raw(app: &mut App, cache_key: String, thumbnail_url: String) {
    if !should_fetch(app, &cache_key) {
        return;
    }
    app.preview_cache.insert(
        cache_key.clone(),
        PreviewEntry {
            ready: false,
            retry_at: None,
        },
    );
    let tx = app.tx.clone();
    let cache_dir = config::youtube_preview_cache_dir();
    tokio::spawn(async move {
        let ready = fetch_thumbnail(&cache_key, &thumbnail_url, &cache_dir).await;
        let _ = tx.send(if ready {
            AppEvent::PreviewReady {
                video_id: cache_key,
            }
        } else {
            AppEvent::PreviewFailed {
                video_id: cache_key,
            }
        });
    });
}

const PLACEHOLDER_DIMS: (u32, u32) = (120, 90);

pub const RETRY_BACKOFF: std::time::Duration = std::time::Duration::from_secs(60);

fn should_fetch(app: &App, key: &str) -> bool {
    match app.preview_cache.get(key) {
        None => true,
        Some(e) if e.ready => false,
        Some(e) => match e.retry_at {
            None => false,
            Some(at) => std::time::Instant::now() >= at,
        },
    }
}

fn thumbnail_candidates(url: &str) -> Vec<String> {
    let mut out = vec![url.to_string()];
    let Some((id, _)) = url
        .strip_prefix("https://i.ytimg.com/vi/")
        .and_then(|rest| rest.split_once('/'))
    else {
        return out;
    };
    if id.is_empty() {
        return out;
    }
    for name in [
        "maxresdefault.jpg",
        "hq720.jpg",
        "hqdefault.jpg",
        "mqdefault.jpg",
    ] {
        let candidate = format!("https://i.ytimg.com/vi/{}/{}", id, name);
        if !out.contains(&candidate) {
            out.push(candidate);
        }
    }
    out
}

fn cached_thumbnail_is_real(path: &std::path::Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 24];
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    if f.read_exact(&mut head).is_err() || head[..8] != *b"\x89PNG\r\n\x1a\n" {
        return false;
    }
    let w = u32::from_be_bytes(head[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(head[20..24].try_into().unwrap());
    (w > 0 && h > 0) && (w, h) != PLACEHOLDER_DIMS
}

async fn download_png(url: &str, dest: &std::path::Path) -> bool {
    let Ok(resp) = crate::innertube::http_client().get(url).send().await else {
        return false;
    };
    if !resp.status().is_success() {
        return false;
    }
    let Ok(bytes) = resp.bytes().await else {
        return false;
    };
    let Ok(img) = image::load_from_memory(&bytes) else {
        return false;
    };
    if (img.width(), img.height()) == PLACEHOLDER_DIMS {
        return false;
    }
    let mut png_bytes = Vec::new();
    if img
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .is_err()
    {
        return false;
    }
    tokio::fs::write(dest, &png_bytes).await.is_ok() && dest.exists()
}

async fn fetch_thumbnail(video_id: &str, thumbnail_url: &str, cache_dir: &std::path::Path) -> bool {
    let _ = tokio::fs::create_dir_all(cache_dir).await;
    let img_path = cache_dir.join(format!("{}.png", video_id));
    if cached_thumbnail_is_real(&img_path) {
        return true;
    }
    for url in thumbnail_candidates(thumbnail_url) {
        if download_png(&url, &img_path).await {
            return true;
        }
    }
    false
}

/// Returns the preview cache key for the selected item in the current list screen.
fn selected_video_id(app: &App) -> Option<String> {
    if let Screen::List(ref ls) = app.current_screen() {
        let filtered = ls.filtered_items();
        if let Some(item) = filtered.get(ls.selected) {
            return match &item.data {
                ItemData::Video(v) => Some(v.id.clone()),
                ItemData::TwitchStream(s) if s.is_live => Some(format!("twitch_{}", s.login)),
                ItemData::TwitchVod(v) if !v.thumbnail.is_empty() => {
                    Some(format!("twitchvod_{}", v.id))
                }
                ItemData::TwitchGame(g) if !g.box_art.is_empty() => {
                    Some(twitch_game_cache_key(&g.name))
                }
                ItemData::KickStream(s) => {
                    let image = (s.is_live && !s.thumbnail.is_empty()) || !s.avatar.is_empty();
                    image.then(|| kick_stream_cache_key(&s.slug))
                }
                ItemData::KickVod(v) if !v.thumbnail.is_empty() => Some(kick_vod_cache_key(&v.id)),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_png(name: &str, w: u32, h: u32) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("vidi-preview-test-{}.png", name));
        let img = image::RgbImage::from_pixel(w, h, image::Rgb([20, 40, 60]));
        img.save(&path).unwrap();
        path
    }

    // ── thumbnail_candidates ─────────────────────────────────────────────

    #[test]
    fn candidates_keep_requested_url_first_then_ytimg_fallbacks() {
        let c = thumbnail_candidates("https://i.ytimg.com/vi/dQw4w9WgXcQ/hq720.jpg");
        assert_eq!(c[0], "https://i.ytimg.com/vi/dQw4w9WgXcQ/hq720.jpg");
        assert_eq!(c[1], "https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg");
        assert!(c.contains(&"https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_string()));
        assert!(c.contains(&"https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg".to_string()));
    }

    #[test]
    fn candidates_are_deduplicated() {
        let c = thumbnail_candidates("https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg");
        assert_eq!(c.len(), 4);
        let mut sorted = c.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), c.len());
    }

    #[test]
    fn candidates_for_non_youtube_urls_are_untouched() {
        let url = "https://static-cdn.jtvnw.net/previews-ttv/live_user_x-640x360.jpg";
        assert_eq!(thumbnail_candidates(url), vec![url]);
        assert_eq!(thumbnail_candidates("not a url"), vec!["not a url"]);
    }

    #[test]
    fn candidates_skip_ytimg_urls_without_an_id() {
        assert_eq!(
            thumbnail_candidates("https://i.ytimg.com/vi//hq.jpg").len(),
            1
        );
    }

    // ── should_fetch ─────────────────────────────────────────────────────

    fn app_with(entry: Option<PreviewEntry>) -> App {
        let mut app = App::new(config::Config::default());
        if let Some(e) = entry {
            app.preview_cache.insert("k".to_string(), e);
        }
        app
    }

    #[test]
    fn fetch_is_skipped_while_in_flight_or_ready() {
        assert!(should_fetch(&app_with(None), "k"));
        assert!(!should_fetch(
            &app_with(Some(PreviewEntry {
                ready: false,
                retry_at: None
            })),
            "k"
        ));
        assert!(!should_fetch(
            &app_with(Some(PreviewEntry {
                ready: true,
                retry_at: None
            })),
            "k"
        ));
    }

    #[test]
    fn fetch_is_blocked_until_the_backoff_expires() {
        let cooling = app_with(Some(PreviewEntry {
            ready: false,
            retry_at: Some(std::time::Instant::now() + RETRY_BACKOFF),
        }));
        assert!(!should_fetch(&cooling, "k"));

        let elapsed = app_with(Some(PreviewEntry {
            ready: false,
            retry_at: Some(std::time::Instant::now() - RETRY_BACKOFF),
        }));
        assert!(should_fetch(&elapsed, "k"), "expired backoff must retry");
    }

    // ── cached_thumbnail_is_real ─────────────────────────────────────────

    #[test]
    fn cached_placeholder_is_rejected_so_it_gets_refetched() {
        let p = tmp_png("placeholder", PLACEHOLDER_DIMS.0, PLACEHOLDER_DIMS.1);
        assert!(!cached_thumbnail_is_real(&p));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn cached_real_thumbnail_is_accepted() {
        let p = tmp_png("real", 1280, 720);
        assert!(cached_thumbnail_is_real(&p));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn missing_and_corrupt_cache_entries_are_rejected() {
        assert!(!cached_thumbnail_is_real(
            &std::env::temp_dir().join("vidi-does-not-exist.png")
        ));
        let junk = std::env::temp_dir().join("vidi-preview-test-junk.png");
        std::fs::write(&junk, b"not an image at all, definitely not").unwrap();
        assert!(!cached_thumbnail_is_real(&junk));
        let _ = std::fs::remove_file(&junk);
    }

    #[tokio::test]
    #[ignore = "needs network"]
    async fn fetch_falls_back_past_ytimg_404_to_a_size_that_exists() {
        let dir = std::env::temp_dir().join("vidi-preview-test-net");
        let key = "fallback-probe";
        let dest = dir.join(format!("{}.png", key));
        let _ = std::fs::remove_file(&dest);

        let ok = fetch_thumbnail(
            key,
            "https://i.ytimg.com/vi/jNQXAC9IVRw/maxresdefault.jpg",
            &dir,
        )
        .await;

        assert!(ok, "fallback chain should reach hqdefault");
        assert!(cached_thumbnail_is_real(&dest));
        let decoded = image::open(&dest).unwrap();
        assert_ne!(
            (decoded.width(), decoded.height()),
            PLACEHOLDER_DIMS,
            "placeholder must never be cached"
        );
        let _ = std::fs::remove_file(&dest);
    }

    #[tokio::test]
    #[ignore = "needs network"]
    async fn fetch_reports_failure_instead_of_caching_a_placeholder() {
        let dir = std::env::temp_dir().join("vidi-preview-test-net");
        let key = "no-such-video";
        let dest = dir.join(format!("{}.png", key));
        let _ = std::fs::remove_file(&dest);

        let ok = fetch_thumbnail(
            key,
            "https://i.ytimg.com/vi/aaaaaaaaaaa/maxresdefault.jpg",
            &dir,
        )
        .await;

        assert!(!ok, "a video with no thumbnail must not report success");
        assert!(
            !cached_thumbnail_is_real(&dest),
            "nothing decodable should have been written"
        );
    }

    #[tokio::test]
    #[ignore = "needs network"]
    async fn fetch_recovers_a_stale_cached_placeholder() {
        let dir = std::env::temp_dir().join("vidi-preview-test-net");
        let _ = std::fs::create_dir_all(&dir);
        let key = "stale-placeholder";
        let dest = dir.join(format!("{}.png", key));
        let img = image::RgbImage::from_pixel(
            PLACEHOLDER_DIMS.0,
            PLACEHOLDER_DIMS.1,
            image::Rgb([128, 128, 128]),
        );
        img.save(&dest).unwrap();
        assert!(!cached_thumbnail_is_real(&dest));

        let ok = fetch_thumbnail(
            key,
            "https://i.ytimg.com/vi/jNQXAC9IVRw/maxresdefault.jpg",
            &dir,
        )
        .await;

        assert!(ok);
        assert!(
            cached_thumbnail_is_real(&dest),
            "stale placeholder should have been refetched"
        );
        let _ = std::fs::remove_file(&dest);
    }
}
