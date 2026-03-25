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

/// Spawn a background task that connects to Twitch IRC and forwards
/// chat messages through the given sender.
pub fn spawn_chat_task(
    channel: String,
    tx: mpsc::UnboundedSender<AppEvent>,
) {
    tokio::spawn(async move {
        if let Err(e) = run_chat(&channel, tx.clone()).await {
            let _ = tx.send(AppEvent::ChatError(format!("Chat error: {}", e)));
        }
    });
}

async fn run_chat(
    channel: &str,
    tx: mpsc::UnboundedSender<AppEvent>,
) -> Result<()> {
    let addr = format!("{}:{}", IRC_SERVER, IRC_PORT);
    let stream = TcpStream::connect(&addr).await?;
    let (reader, mut writer) = stream.into_split();

    // Send auth
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
        // Handle PING
        if line.starts_with("PING") {
            writer.write_all(b"PONG :tmi.twitch.tv\r\n").await?;
            continue;
        }

        // Parse PRIVMSG
        // Format: :nick!nick@nick.tmi.twitch.tv PRIVMSG #channel :message
        if let Some(msg) = parse_privmsg(&line) {
            let color = hash_color(&msg.user);
            let _ = tx.send(AppEvent::ChatMessage {
                user: msg.user,
                text: msg.text,
                color,
            });
        }
    }

    Ok(())
}

struct IrcMsg {
    user: String,
    text: String,
}

fn parse_privmsg(line: &str) -> Option<IrcMsg> {
    // :nick!nick@host PRIVMSG #channel :message
    if !line.contains("PRIVMSG") {
        return None;
    }
    let parts: Vec<&str> = line.splitn(2, "PRIVMSG").collect();
    if parts.len() < 2 {
        return None;
    }
    // Extract user from the prefix ":nick!..."
    let prefix = parts[0];
    let user = prefix
        .trim_start_matches(':')
        .split('!')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if user.is_empty() {
        return None;
    }

    // Extract message after ": " following channel name
    let rest = parts[1];
    let text = rest
        .splitn(2, ':')
        .nth(1)
        .unwrap_or("")
        .trim()
        .to_string();

    Some(IrcMsg { user, text })
}

/// Deterministic color (0-6) based on username hash.
fn hash_color(username: &str) -> u8 {
    let hash: u32 = username.bytes().fold(0u32, |acc, b| acc.wrapping_add(b as u32));
    (hash % 7) as u8
}

/// Map color index (0-6) to a ratatui Color.
pub fn irc_color_to_ratatui(idx: u8) -> ratatui::style::Color {
    use ratatui::style::Color;
    match idx {
        0 => Color::Red,
        1 => Color::Green,
        2 => Color::Yellow,
        3 => Color::Blue,
        4 => Color::Magenta,
        5 => Color::Cyan,
        _ => Color::White,
    }
}
