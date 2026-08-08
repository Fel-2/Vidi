use crate::config::Config;
use crate::models::{
    Channel, ChannelTabLoadMore, CustomPlaylist, ItemData, ListItem, SubFeedLoadMore, TwitchGame,
    TwitchStream, TwitchVod, Video,
};
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// App-level events (from async tasks → main loop)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum AppEvent {
    YoutubeResults {
        items: Vec<Video>,
        context: ListContext,
        title: String,
        channel_load_more: Option<ChannelTabLoadMore>,
    },
    TwitchSearchResults(Vec<TwitchStream>),
    TwitchSubsResults(Vec<TwitchStream>),
    TwitchVodsResults(Vec<TwitchVod>),
    TwitchTopStreams(Vec<TwitchStream>),
    TwitchGamesResults(Vec<TwitchGame>),
    ChannelList(Vec<Channel>),
    CustomPlaylistResults(Vec<CustomPlaylist>),
    ChatMessage {
        user: String,
        text: String,
        /// Username display colour as RGB (from IRC tags, or a hashed fallback).
        color: (u8, u8, u8),
        /// Rendered badge glyphs (broadcaster/mod/vip/sub), empty if none.
        badges: String,
    },
    ChatConnected,
    ChatError(String),
    Error(String),
    StatusMessage(String),
    DownloadStarted(String),
    /// Subscription feed first-load results, with optional Load More config.
    SubFeedResults {
        items: Vec<Video>,
        load_more: Option<SubFeedLoadMore>,
    },
    /// Background refresh of the subscription feed finished — update the list
    /// in place if the user is still looking at it.
    SubFeedRefreshed {
        items: Vec<Video>,
        load_more: Option<SubFeedLoadMore>,
    },
    /// Subscription feed "Load More" results — merges with the existing list.
    SubFeedMoreResults {
        new_items: Vec<Video>,
        existing_items: Vec<Video>,
        load_more: Option<SubFeedLoadMore>,
    },
    /// Channel tab "Load More" results — merges with the existing list.
    ChannelTabMoreResults {
        new_items: Vec<Video>,
        existing_items: Vec<Video>,
        channel_load_more: Option<ChannelTabLoadMore>,
        title: String,
        context: ListContext,
    },
    /// Thumbnail downloaded to disk and ready to display.
    PreviewReady {
        video_id: String,
    },
}

// ---------------------------------------------------------------------------
// What to do when an item is selected from a list
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum ListContext {
    YoutubeVideoActions,
    TwitchStreamActions,
    TwitchVodActions,
    SelectChannelForVods,
    /// Twitch VOD-type chooser for a given channel login (Archives/Highlights/…).
    SelectVodType(String),
    SelectChannelToBrowse,
    /// Twitch category list — selecting a game opens its live streams.
    SelectGameForStreams,
    CustomPlaylistActions,
    SearchHistory,
    Miscellaneous,
    ChannelTab(String), // channel url
    /// Play-next queue screen (virtual play/clear rows + queued videos).
    Queue,
}

// ---------------------------------------------------------------------------
// Screen definitions
// ---------------------------------------------------------------------------

/// Cached thumbnail state for a video.
#[derive(Debug, Clone)]
pub struct PreviewEntry {
    pub ready: bool, // image has been downloaded to disk
}

#[derive(Debug, Clone)]
pub struct ListScreen {
    pub title: String,
    pub items: Vec<ListItem>,
    pub filter: String,
    pub filter_active: bool,
    pub last_query: String,
    pub selected: usize,
    pub context: ListContext,
    pub scroll_offset: usize,
    /// When set, a "Load More" button appears at the bottom of the list.
    pub load_more: Option<SubFeedLoadMore>,
    /// Channel tab "Load More" (for Videos, Shorts, Streams, Playlists tabs).
    pub channel_load_more: Option<ChannelTabLoadMore>,
}

impl ListScreen {
    pub fn new(title: impl Into<String>, items: Vec<ListItem>, context: ListContext) -> Self {
        Self {
            title: title.into(),
            items,
            filter: String::new(),
            filter_active: false,
            last_query: String::new(),
            selected: 0,
            context,
            scroll_offset: 0,
            load_more: None,
            channel_load_more: None,
        }
    }

    /// Total navigable rows = filtered items + 1 if any Load More is present.
    pub fn total_rows(&self) -> usize {
        let has_load_more = self.load_more.is_some() || self.channel_load_more.is_some();
        self.filtered_items().len() + if has_load_more { 1 } else { 0 }
    }

    pub fn filtered_items(&self) -> Vec<&ListItem> {
        if self.filter.is_empty() {
            self.items.iter().collect()
        } else {
            let f = self.filter.to_lowercase();
            self.items
                .iter()
                .filter(|i| fuzzy_match(&i.display, &f))
                .collect()
        }
    }
}

/// Match `needle` (already lowercased) against `haystack` as a substring, or
/// failing that as a subsequence, so "rst pod" still finds "Rust Podcast".
pub fn fuzzy_match(haystack: &str, needle: &str) -> bool {
    let hay = haystack.to_lowercase();
    if hay.contains(needle) {
        return true;
    }
    let mut chars = hay.chars();
    needle
        .chars()
        .filter(|c| !c.is_whitespace())
        .all(|c| chars.any(|h| h == c))
}

#[derive(Debug, Clone)]
pub struct SearchInputScreen {
    pub prompt: String,
    pub input: String,
    pub context: SearchContext,
}

#[derive(Debug, Clone)]
pub enum SearchContext {
    YoutubeSearch,
    TwitchSearch,
    ExploreChannels,
    ExplorePlaylists,
    ChannelSearch(String), // channel url
    /// Prompt for a file path to import subscriptions from.
    ImportSubscriptions,
}

#[derive(Debug, Clone)]
pub struct ChatScreen {
    pub channel: String,
    pub messages: std::collections::VecDeque<ChatMessage>,
    pub scroll_offset: usize,
    pub connected: bool,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub timestamp: String,
    pub user: String,
    pub text: String,
    pub color: (u8, u8, u8),
    pub badges: String,
}

#[derive(Debug, Clone)]
pub struct VideoActionsScreen {
    pub video: Video,
    pub selected: usize,
}

#[derive(Debug, Clone)]
pub struct QualitySelectScreen {
    pub video: Video,
    pub options: Vec<String>,
    pub selected: usize,
}

#[derive(Debug, Clone)]
pub struct ChannelActionsScreen {
    pub channel: Channel,
    pub selected: usize,
    pub subscribed: bool,
}

#[derive(Debug, Clone)]
pub struct TwitchStreamActionsScreen {
    pub stream: TwitchStream,
    pub selected: usize,
}

#[derive(Debug, Clone)]
pub struct TwitchVodActionsScreen {
    pub vod: TwitchVod,
    pub selected: usize,
}

#[derive(Debug, Clone)]
pub enum Screen {
    ModeSelect { selected: usize },
    YoutubeMenu { selected: usize },
    TwitchMenu { selected: usize },
    List(ListScreen),
    VideoActions(VideoActionsScreen),
    QualitySelect(QualitySelectScreen),
    ChannelActions(ChannelActionsScreen),
    TwitchStreamActions(TwitchStreamActionsScreen),
    TwitchVodActions(TwitchVodActionsScreen),
    SearchInput(SearchInputScreen),
    TwitchChat(ChatScreen),
}

// ---------------------------------------------------------------------------
// Terminal graphics protocol
// ---------------------------------------------------------------------------

/// Which inline-image protocol the host terminal supports, detected at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsProtocol {
    Kitty,
    ITerm2,
    None,
}

impl GraphicsProtocol {
    pub fn detect() -> Self {
        use std::env::var;
        let term = var("TERM").unwrap_or_default();
        let term_program = var("TERM_PROGRAM").unwrap_or_default();

        if var("KITTY_WINDOW_ID").is_ok()
            || term.contains("kitty")
            || term.contains("ghostty")
            || term_program == "ghostty"
        {
            return GraphicsProtocol::Kitty;
        }
        if term_program == "iTerm.app" || term_program == "WezTerm" {
            return GraphicsProtocol::ITerm2;
        }
        GraphicsProtocol::None
    }
}

// ---------------------------------------------------------------------------
// Message kind
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum MessageKind {
    Info,
    Error,
    Success,
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

pub struct App {
    pub screen_stack: Vec<Screen>,
    pub loading: Option<String>,
    pub message: Option<(String, MessageKind)>,
    pub config: Config,
    pub tx: mpsc::UnboundedSender<AppEvent>,
    pub rx: mpsc::UnboundedReceiver<AppEvent>,
    pub should_quit: bool,
    pub saved_ids: std::collections::HashSet<String>,
    pub watched_ids: std::collections::HashSet<String>,
    /// Thumbnail preview cache keyed by video ID.
    pub preview_cache: std::collections::HashMap<String, PreviewEntry>,
    /// video_id of the kitty image currently on screen, if any.
    pub kitty_displayed: Option<String>,
    /// (x, y, w, h) in terminal cells of the thumbnail area (set during render).
    pub preview_thumb_area: Option<(u16, u16, u16, u16)>,
    /// Inline-image protocol supported by the host terminal.
    pub graphics: GraphicsProtocol,
    /// Help overlay visible (toggled with `?`).
    pub show_help: bool,
    /// Play-next queue (Tab on a list enqueues, played via YouTube → Queue).
    pub queue: Vec<crate::models::Video>,
    /// Rows visible in the list body, updated on every render.
    pub list_visible_rows: usize,
}

impl App {
    pub fn new(config: Config) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            screen_stack: vec![Screen::ModeSelect { selected: 0 }],
            loading: None,
            message: None,
            config,
            tx,
            rx,
            should_quit: false,
            saved_ids: std::collections::HashSet::new(),
            watched_ids: std::collections::HashSet::new(),
            preview_cache: std::collections::HashMap::new(),
            kitty_displayed: None,
            preview_thumb_area: None,
            graphics: GraphicsProtocol::detect(),
            show_help: false,
            queue: Vec::new(),
            list_visible_rows: 20,
        }
    }

    pub fn current_screen(&self) -> &Screen {
        self.screen_stack
            .last()
            .expect("screen stack is never empty")
    }

    pub fn current_screen_mut(&mut self) -> &mut Screen {
        self.screen_stack
            .last_mut()
            .expect("screen stack is never empty")
    }

    pub fn push_screen(&mut self, screen: Screen) {
        self.screen_stack.push(screen);
    }

    pub fn pop_screen(&mut self) {
        if self.screen_stack.len() > 1 {
            self.screen_stack.pop();
        }
    }

    pub fn set_error(&mut self, msg: impl Into<String>) {
        self.message = Some((msg.into(), MessageKind::Error));
        self.loading = None;
    }

    pub fn set_info(&mut self, msg: impl Into<String>) {
        self.message = Some((msg.into(), MessageKind::Info));
    }

    pub fn set_success(&mut self, msg: impl Into<String>) {
        self.message = Some((msg.into(), MessageKind::Success));
    }

    pub fn clear_message(&mut self) {
        self.message = None;
    }

    pub fn make_video_list(
        title: impl Into<String>,
        videos: Vec<Video>,
        context: ListContext,
    ) -> ListScreen {
        let items = videos
            .into_iter()
            .map(|v| {
                let date = if let Some(ts) = v.timestamp {
                    crate::ui::relative_time(ts)
                } else if !v.upload_date.is_empty() {
                    // YYYYMMDD → YYYY-MM-DD
                    if v.upload_date.len() == 8 {
                        format!(
                            "{}-{}-{}",
                            &v.upload_date[..4],
                            &v.upload_date[4..6],
                            &v.upload_date[6..8]
                        )
                    } else {
                        v.upload_date.clone()
                    }
                } else {
                    String::new()
                };
                let display = format!(
                    "{:<50} | {:<20} | {:<14} | {}",
                    truncate(&v.title, 50),
                    truncate(&v.channel, 20),
                    date,
                    v.duration_string
                );
                ListItem {
                    display,
                    data: ItemData::YoutubeVideo(v),
                }
            })
            .collect();
        ListScreen::new(title, items, context)
    }

    pub fn make_video_list_with_load_more(
        title: impl Into<String>,
        videos: Vec<Video>,
        context: ListContext,
        load_more: Option<SubFeedLoadMore>,
    ) -> ListScreen {
        let mut ls = Self::make_video_list(title, videos, context);
        ls.load_more = load_more;
        ls
    }

    pub fn make_stream_list(
        title: impl Into<String>,
        streams: Vec<TwitchStream>,
        context: ListContext,
    ) -> ListScreen {
        let items = streams
            .into_iter()
            .map(|s| {
                let status = if s.is_live { "LIVE" } else { "OFF " };
                let meta = if s.is_live && !s.uptime.is_empty() {
                    format!("{:>6} 👁 {:>7} up", s.viewers, s.uptime)
                } else {
                    format!("{:>6} viewers", s.viewers)
                };
                let display = format!(
                    "{} {:<16} | {:<18} | {:<20} | {}",
                    status,
                    meta,
                    truncate(&s.login, 18),
                    truncate(&s.game, 20),
                    truncate(&s.title, 50)
                );
                ListItem {
                    display,
                    data: ItemData::TwitchStream(s),
                }
            })
            .collect();
        ListScreen::new(title, items, context)
    }

    pub fn make_game_list(
        title: impl Into<String>,
        games: Vec<TwitchGame>,
        context: ListContext,
    ) -> ListScreen {
        let items = games
            .into_iter()
            .map(|g| {
                let display = format!("{:>9} 👁  | {}", g.viewers, truncate(&g.name, 60));
                ListItem {
                    display,
                    data: ItemData::TwitchGame(g),
                }
            })
            .collect();
        ListScreen::new(title, items, context)
    }

    pub fn make_vod_list(
        title: impl Into<String>,
        vods: Vec<TwitchVod>,
        context: ListContext,
    ) -> ListScreen {
        let items = vods
            .into_iter()
            .map(|v| {
                let views = if v.view_count > 0 {
                    format!("{:>8} 👁", v.view_count)
                } else {
                    " ".repeat(10)
                };
                let display = format!(
                    "{:<10} | {:>9} | {} | {}",
                    v.upload_date,
                    truncate(&v.duration, 9),
                    views,
                    truncate(&v.title, 70)
                );
                ListItem {
                    display,
                    data: ItemData::TwitchVod(v),
                }
            })
            .collect();
        ListScreen::new(title, items, context)
    }
}

/// Resolution options offered by the quality picker, highest first.
pub fn quality_options() -> Vec<String> {
    ["best", "2160", "1440", "1080", "720", "480", "360", "240"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut result: String = s.chars().take(max.saturating_sub(1)).collect();
        result.push('…');
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_exact_length() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_long_string() {
        assert_eq!(truncate("hello world", 5), "hell…");
    }

    #[test]
    fn truncate_unicode() {
        // 4 chars, truncate to 3 → 2 chars + ellipsis
        assert_eq!(truncate("café", 3), "ca…");
    }

    #[test]
    fn truncate_one() {
        assert_eq!(truncate("hello", 1), "…");
    }

    #[test]
    fn list_screen_filter() {
        let items = vec![
            ListItem {
                display: "Alpha Video".to_string(),
                data: ItemData::Text("a".to_string()),
            },
            ListItem {
                display: "Beta Stream".to_string(),
                data: ItemData::Text("b".to_string()),
            },
            ListItem {
                display: "Alpha Stream".to_string(),
                data: ItemData::Text("c".to_string()),
            },
        ];
        let mut ls = ListScreen::new("Test", items, ListContext::Miscellaneous);
        ls.filter = "alpha".to_string();
        let filtered = ls.filtered_items();
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].display, "Alpha Video");
        assert_eq!(filtered[1].display, "Alpha Stream");
    }

    #[test]
    fn list_screen_filter_matches_subsequences() {
        let items = vec![
            ListItem {
                display: "Rust Podcast Episode 3".to_string(),
                data: ItemData::Text("a".to_string()),
            },
            ListItem {
                display: "Cooking Show".to_string(),
                data: ItemData::Text("b".to_string()),
            },
        ];
        let mut ls = ListScreen::new("Test", items, ListContext::Miscellaneous);
        ls.filter = "rst pod".to_string();
        let filtered = ls.filtered_items();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].display, "Rust Podcast Episode 3");
    }

    #[test]
    fn fuzzy_match_rejects_out_of_order_characters() {
        assert!(fuzzy_match("Rust Podcast", "podcast"));
        assert!(!fuzzy_match("Rust Podcast", "podcastx"));
        assert!(!fuzzy_match("Rust Podcast", "tsur podcast"));
    }

    #[test]
    fn list_screen_empty_filter_returns_all() {
        let items = vec![
            ListItem {
                display: "A".to_string(),
                data: ItemData::Text("a".to_string()),
            },
            ListItem {
                display: "B".to_string(),
                data: ItemData::Text("b".to_string()),
            },
        ];
        let ls = ListScreen::new("Test", items, ListContext::Miscellaneous);
        assert_eq!(ls.filtered_items().len(), 2);
    }

    #[test]
    fn list_screen_total_rows_with_load_more() {
        let items = vec![ListItem {
            display: "A".to_string(),
            data: ItemData::Text("a".to_string()),
        }];
        let mut ls = ListScreen::new("Test", items, ListContext::Miscellaneous);
        assert_eq!(ls.total_rows(), 1);
        ls.load_more = Some(crate::models::SubFeedLoadMore {
            subs: vec![],
            next_playlist_end: 20,
            label: "Load More".to_string(),
        });
        assert_eq!(ls.total_rows(), 2);
    }
}
