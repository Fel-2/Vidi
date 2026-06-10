//! Watch-progress tracking via the mpv JSON IPC socket.
//!
//! When a video is played, mpv is started with `--input-ipc-server` and a
//! background task polls `time-pos`/`duration` every couple of seconds,
//! persisting the position to disk. The next "Watch" of the same video
//! resumes from the stored position with `--start`. Videos watched past 90%
//! are considered finished and their entry is removed.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEntry {
    /// Playback position in seconds.
    pub position: f64,
    /// Total duration in seconds (0 when unknown).
    pub duration: f64,
    /// Unix timestamp of the last update.
    pub updated_at: u64,
}

const FINISHED_FRACTION: f64 = 0.90;
/// Don't bother resuming inside the first 30 seconds.
const MIN_RESUME_SECS: f64 = 30.0;
/// Keep at most this many progress entries (oldest dropped first).
const MAX_ENTRIES: usize = 500;

pub fn progress_file() -> PathBuf {
    crate::config::youtube_cache_dir().join("watch_progress.json")
}

pub fn load_progress() -> HashMap<String, ProgressEntry> {
    std::fs::read_to_string(progress_file())
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default()
}

fn save_progress(map: &HashMap<String, ProgressEntry>) {
    let path = progress_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Ok(json) = serde_json::to_string(map) {
        std::fs::write(path, json).ok();
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Record the current position for a video; removes the entry once the
/// position passes the finished threshold.
pub fn update_entry(video_id: &str, position: f64, duration: f64) {
    let mut map = load_progress();
    let finished = duration > 0.0 && position / duration >= FINISHED_FRACTION;
    if finished {
        map.remove(video_id);
    } else {
        map.insert(
            video_id.to_string(),
            ProgressEntry {
                position,
                duration,
                updated_at: now_unix(),
            },
        );
        if map.len() > MAX_ENTRIES {
            let mut by_age: Vec<(String, u64)> =
                map.iter().map(|(k, v)| (k.clone(), v.updated_at)).collect();
            by_age.sort_by_key(|(_, t)| *t);
            for (k, _) in by_age.iter().take(map.len() - MAX_ENTRIES) {
                map.remove(k);
            }
        }
    }
    save_progress(&map);
}

/// Position (seconds) to resume `video_id` from, if it is worth resuming.
pub fn resume_position(video_id: &str) -> Option<f64> {
    let map = load_progress();
    let entry = map.get(video_id)?;
    if entry.position < MIN_RESUME_SECS {
        return None;
    }
    if entry.duration > 0.0 && entry.position / entry.duration >= FINISHED_FRACTION {
        return None;
    }
    Some(entry.position)
}

/// Fresh, unique IPC socket path for one mpv invocation.
pub fn socket_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    crate::config::youtube_cache_dir().join(format!("mpv-{}-{}.sock", std::process::id(), nanos))
}

/// mpv arguments enabling the IPC server on `socket`.
pub fn mpv_ipc_args(socket: &std::path::Path) -> Vec<String> {
    vec![format!("--input-ipc-server={}", socket.display())]
}

/// Poll the mpv IPC socket until mpv exits, persisting progress as we go.
/// Spawn with `tokio::spawn`; the task ends by itself when the socket closes.
#[cfg(unix)]
pub async fn track(socket: PathBuf, video_id: String) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    // mpv creates the socket shortly after startup; retry for a while.
    let stream = {
        let mut attempt = 0;
        loop {
            match UnixStream::connect(&socket).await {
                Ok(s) => break Some(s),
                Err(_) if attempt < 30 => {
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                Err(_) => break None,
            }
        }
    };
    let Some(stream) = stream else {
        let _ = std::fs::remove_file(&socket);
        return;
    };

    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    let mut position: Option<f64> = None;
    let mut duration: Option<f64> = None;

    'outer: loop {
        let request = "{\"command\":[\"get_property\",\"time-pos\"],\"request_id\":101}\n\
                       {\"command\":[\"get_property\",\"duration\"],\"request_id\":102}\n";
        if write_half.write_all(request.as_bytes()).await.is_err() {
            break;
        }

        // Read replies (interleaved with events) for up to a second.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            let line = tokio::time::timeout_at(deadline, lines.next_line()).await;
            match line {
                Ok(Ok(Some(line))) => {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                        let id = json.get("request_id").and_then(|i| i.as_i64());
                        let data = json.get("data").and_then(|d| d.as_f64());
                        match (id, data) {
                            (Some(101), Some(p)) => position = Some(p),
                            (Some(102), Some(d)) => duration = Some(d),
                            _ => {}
                        }
                    }
                }
                Ok(Ok(None)) | Ok(Err(_)) => break 'outer, // socket closed
                Err(_) => break,                           // poll window over
            }
        }

        if let Some(p) = position {
            update_entry(&video_id, p, duration.unwrap_or(0.0));
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    if let Some(p) = position {
        update_entry(&video_id, p, duration.unwrap_or(0.0));
    }
    let _ = std::fs::remove_file(&socket);
}

#[cfg(not(unix))]
pub async fn track(_socket: PathBuf, _video_id: String) {}

#[cfg(test)]
mod tests {
    use super::*;

    // Single test: the progress file is shared on disk and parallel tests
    // doing load-modify-write cycles would race each other.
    #[test]
    fn progress_file_roundtrip() {
        let id = format!("test-{}", std::process::id());
        update_entry(&id, 120.0, 600.0);
        assert_eq!(resume_position(&id), Some(120.0));
        // Finished → entry removed.
        update_entry(&id, 590.0, 600.0);
        assert_eq!(resume_position(&id), None);

        // Early positions are not worth resuming.
        update_entry(&id, 10.0, 600.0);
        assert_eq!(resume_position(&id), None);
        update_entry(&id, 595.0, 600.0); // cleanup
    }

    #[test]
    fn socket_paths_unique() {
        assert_ne!(socket_path(), socket_path());
    }
}
