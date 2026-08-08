use anyhow::Result;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    sync::mpsc,
};

use crate::app::AppEvent;

const IRC_SERVER: &str = "irc.chat.twitch.tv";
const IRC_PORT: u16 = 6667;
const NICK: &str = "justinfan12345";
const PASS: &str = "SCHMOOPIIE";

/// Spawn a background task that connects to Twitch IRC and forwards chat
/// messages through the given sender. Reconnects automatically until the
/// receiver is dropped (i.e. the user leaves the chat screen).
pub fn spawn_chat_task(channel: String, tx: mpsc::UnboundedSender<AppEvent>) {
    tokio::spawn(async move {
        let mut backoff = 1u64;
        loop {
            match run_chat(&channel, tx.clone()).await {
                Ok(()) => {
                    // Connection closed cleanly; try to reconnect after a beat.
                }
                Err(e) => {
                    if tx
                        .send(AppEvent::ChatError(format!("Chat: {}", e)))
                        .is_err()
                    {
                        return; // receiver gone — stop.
                    }
                }
            }
            // Stop if nobody is listening anymore.
            if tx.is_closed() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
            backoff = (backoff * 2).min(30);
        }
    });
}

async fn run_chat(channel: &str, tx: mpsc::UnboundedSender<AppEvent>) -> Result<()> {
    let addr = format!("{}:{}", IRC_SERVER, IRC_PORT);
    let stream = TcpStream::connect(&addr).await?;
    let (reader, mut writer) = stream.into_split();

    // Request message tags so we get real colours and badges, then auth + join.
    writer
        .write_all(b"CAP REQ :twitch.tv/tags twitch.tv/commands\r\n")
        .await?;
    writer
        .write_all(format!("PASS {}\r\n", PASS).as_bytes())
        .await?;
    writer
        .write_all(format!("NICK {}\r\n", NICK).as_bytes())
        .await?;
    writer
        .write_all(format!("JOIN #{}\r\n", channel.to_lowercase()).as_bytes())
        .await?;

    let _ = tx.send(AppEvent::ChatConnected);

    let mut lines = BufReader::new(reader).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.starts_with("PING") {
            writer.write_all(b"PONG :tmi.twitch.tv\r\n").await?;
            continue;
        }

        if let Some(msg) = parse_privmsg(&line) {
            if tx
                .send(AppEvent::ChatMessage {
                    user: msg.user,
                    text: msg.text,
                    color: msg.color,
                    badges: msg.badges,
                })
                .is_err()
            {
                return Ok(()); // receiver gone
            }
        }
    }

    Ok(())
}

struct IrcMsg {
    user: String,
    text: String,
    color: (u8, u8, u8),
    badges: String,
}

/// Parse an IRC line that may carry IRCv3 `@tags`, e.g.
/// `@color=#FF0000;display-name=Foo;badges=moderator/1 :foo!foo@host PRIVMSG #ch :hi`
fn parse_privmsg(line: &str) -> Option<IrcMsg> {
    if !line.contains("PRIVMSG") {
        return None;
    }

    // Split optional tag block (leading '@...') from the IRC message proper.
    let (tags, rest) = if let Some(stripped) = line.strip_prefix('@') {
        match stripped.split_once(' ') {
            Some((t, r)) => (parse_tags(t), r),
            None => (Tags::default(), stripped),
        }
    } else {
        (Tags::default(), line)
    };

    let parts: Vec<&str> = rest.splitn(2, "PRIVMSG").collect();
    if parts.len() < 2 {
        return None;
    }

    // Login from the ":nick!..." prefix.
    let login = parts[0]
        .trim_start_matches(':')
        .split('!')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if login.is_empty() {
        return None;
    }

    // Message text after the first ':' following the channel name.
    let text = parts[1]
        .split_once(':')
        .map(|x| x.1)
        .unwrap_or("")
        .trim()
        .to_string();

    let user = if tags.display_name.is_empty() {
        login.clone()
    } else {
        tags.display_name
    };
    let color = tags.color.unwrap_or_else(|| fallback_color(&login));

    Some(IrcMsg {
        user,
        text,
        color,
        badges: tags.badges,
    })
}

#[derive(Default)]
struct Tags {
    color: Option<(u8, u8, u8)>,
    display_name: String,
    badges: String,
}

/// Parse the `key=value;key=value` IRC tag block into the fields we care about.
fn parse_tags(raw: &str) -> Tags {
    let mut tags = Tags::default();
    for kv in raw.split(';') {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        match k {
            "color" => tags.color = parse_hex_color(v),
            "display-name" => tags.display_name = v.to_string(),
            "badges" => tags.badges = badge_glyphs(v),
            _ => {}
        }
    }
    tags
}

/// `#RRGGBB` → (r, g, b). Returns None for empty/malformed values.
fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some((r, g, b))
}

/// Map the IRC `badges` tag (e.g. `broadcaster/1,subscriber/12`) to glyphs.
fn badge_glyphs(raw: &str) -> String {
    let mut out = String::new();
    for badge in raw.split(',') {
        let name = badge.split('/').next().unwrap_or("");
        let glyph = match name {
            "broadcaster" => "📹",
            "moderator" => "🗡",
            "vip" => "💎",
            "subscriber" | "founder" => "⭐",
            "premium" | "turbo" => "⚡",
            _ => continue,
        };
        out.push_str(glyph);
    }
    out
}

/// Deterministic fallback colour for users who haven't set one. Picks from the
/// set of Twitch default name colours based on a simple username hash.
fn fallback_color(username: &str) -> (u8, u8, u8) {
    const PALETTE: &[(u8, u8, u8)] = &[
        (255, 0, 0),     // Red
        (0, 0, 255),     // Blue
        (0, 128, 0),     // Green
        (178, 34, 34),   // FireBrick
        (255, 127, 80),  // Coral
        (154, 205, 50),  // YellowGreen
        (255, 69, 0),    // OrangeRed
        (46, 139, 87),   // SeaGreen
        (218, 165, 32),  // GoldenRod
        (210, 105, 30),  // Chocolate
        (95, 158, 160),  // CadetBlue
        (30, 144, 255),  // DodgerBlue
        (255, 105, 180), // HotPink
        (138, 43, 226),  // BlueViolet
        (0, 255, 127),   // SpringGreen
    ];
    let hash: usize = username.bytes().fold(0usize, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(b as usize)
    });
    PALETTE[hash % PALETTE.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_privmsg_normal() {
        let line = ":nick!nick@nick.tmi.twitch.tv PRIVMSG #channel :hello world";
        let msg = parse_privmsg(line).unwrap();
        assert_eq!(msg.user, "nick");
        assert_eq!(msg.text, "hello world");
        assert!(msg.badges.is_empty());
    }

    #[test]
    fn parse_privmsg_with_colon_in_message() {
        let line = ":user!user@user.tmi.twitch.tv PRIVMSG #ch :check this: cool";
        let msg = parse_privmsg(line).unwrap();
        assert_eq!(msg.user, "user");
        assert_eq!(msg.text, "check this: cool");
    }

    #[test]
    fn parse_privmsg_with_tags() {
        let line = "@color=#FF0000;display-name=CoolUser;badges=moderator/1,subscriber/6 :cooluser!cooluser@host PRIVMSG #ch :hey";
        let msg = parse_privmsg(line).unwrap();
        assert_eq!(msg.user, "CoolUser");
        assert_eq!(msg.text, "hey");
        assert_eq!(msg.color, (255, 0, 0));
        assert_eq!(msg.badges, "🗡⭐");
    }

    #[test]
    fn parse_privmsg_empty_color_falls_back() {
        let line = "@color=;display-name=Bob :bob!bob@host PRIVMSG #ch :yo";
        let msg = parse_privmsg(line).unwrap();
        assert_eq!(msg.user, "Bob");
        assert_eq!(msg.color, fallback_color("bob"));
    }

    #[test]
    fn parse_privmsg_ignores_non_privmsg() {
        assert!(parse_privmsg("PING :tmi.twitch.tv").is_none());
        assert!(parse_privmsg(":tmi.twitch.tv 001 justinfan12345 :Welcome").is_none());
    }

    #[test]
    fn parse_privmsg_empty_prefix() {
        let line = ": PRIVMSG #ch :test";
        assert!(parse_privmsg(line).is_none());
    }

    #[test]
    fn parse_hex_color_valid_and_invalid() {
        assert_eq!(parse_hex_color("#00FF00"), Some((0, 255, 0)));
        assert_eq!(parse_hex_color(""), None);
        assert_eq!(parse_hex_color("#fff"), None);
        assert_eq!(parse_hex_color("nothex"), None);
    }

    #[test]
    fn fallback_color_deterministic() {
        assert_eq!(fallback_color("testuser"), fallback_color("testuser"));
    }
}
