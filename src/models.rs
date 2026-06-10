use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// YouTube models
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Video {
    pub id: String,
    pub title: String,
    pub url: String,
    pub channel: String,
    pub channel_url: String,
    pub upload_date: String,
    pub duration_string: String,
    pub view_count: Option<u64>,
    pub thumbnail: String,
    pub playlist_url: Option<String>,
    pub playlist_title: Option<String>,
    pub description: Option<String>,
    /// Unix timestamp from yt-dlp — more accurate than upload_date for sorting.
    pub timestamp: Option<i64>,
    /// True when the entry is a YouTube Short (hidden unless SHOW_SHORTS).
    #[serde(default)]
    pub is_short: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Channel {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomPlaylist {
    pub name: String,
    #[serde(rename = "playlistUrl")]
    pub playlist_url: String,
    #[serde(rename = "playlistWatchUrl")]
    pub playlist_watch_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SavedVideos {
    pub entries: Vec<Video>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecentVideos {
    pub entries: Vec<Video>,
}

// ---------------------------------------------------------------------------
// Twitch models
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct TwitchStream {
    pub login: String,
    pub title: String,
    pub game: String,
    pub viewers: u64,
    pub is_live: bool,
}

#[derive(Debug, Clone, Default)]
pub struct TwitchVod {
    pub id: String,
    pub title: String,
    pub duration: String,
    pub upload_date: String,
    pub thumbnail: String,
    pub url: String,
}

// ---------------------------------------------------------------------------
// List item data variants
// ---------------------------------------------------------------------------

/// Config for the "Load More" button on the subscription feed list.
#[derive(Debug, Clone)]
pub struct SubFeedLoadMore {
    pub subs: Vec<String>,
    /// playlist-end value to use on the *next* fetch (escalates each click).
    pub next_playlist_end: u32,
    pub label: String,
}

/// Config for the "Load More" button on channel tab / playlist lists.
#[derive(Debug, Clone)]
pub struct ChannelTabLoadMore {
    /// Full URL for the tab (e.g. `https://…/@channel/videos`).
    pub url: String,
    /// The ListContext to use for the results.
    pub context: super::app::ListContext,
    /// Title shown on the list screen.
    pub title: String,
    /// How many entries were already fetched (next fetch starts after these).
    pub current_playlist_end: u32,
    /// How many more to fetch per click.
    pub page_size: u32,
    pub label: String,
}

#[derive(Debug, Clone)]
pub enum ItemData {
    YoutubeVideo(Video),
    TwitchStream(TwitchStream),
    TwitchVod(TwitchVod),
    Channel(Channel),
    CustomPlaylist(CustomPlaylist),
    Text(String),
}

#[derive(Debug, Clone)]
pub struct ListItem {
    pub display: String,
    pub data: ItemData,
}
