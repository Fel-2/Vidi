//! Import YouTube subscriptions from NewPipe JSON exports, OPML feeds
//! (YouTube takeout pre-2021, FreeTube, Invidious) and YouTube Takeout CSV.
//!
//! No XML/CSV dependencies: OPML attributes and CSV columns are simple enough
//! to scan by hand (channel URLs and IDs contain no commas or quotes).

use anyhow::{Context, Result};

/// One imported subscription: canonical channel URL plus display name.
#[derive(Debug, PartialEq)]
pub struct ImportedSub {
    pub url: String,
    pub name: Option<String>,
}

/// Parse a NewPipe `subscriptions.json` export.
pub fn parse_newpipe_json(content: &str) -> Result<Vec<ImportedSub>> {
    let json: serde_json::Value = serde_json::from_str(content).context("not valid JSON")?;
    let subs = json
        .get("subscriptions")
        .and_then(|s| s.as_array())
        .context("no \"subscriptions\" array")?;
    Ok(subs
        .iter()
        .filter_map(|s| {
            let url = s.get("url")?.as_str()?.to_string();
            let name = s
                .get("name")
                .and_then(|n| n.as_str())
                .map(|n| n.to_string());
            Some(ImportedSub { url, name })
        })
        .collect())
}

/// Parse an OPML export: `<outline title="Name" xmlUrl="…channel_id=UC…"/>`.
pub fn parse_opml(content: &str) -> Result<Vec<ImportedSub>> {
    let mut out = Vec::new();
    for chunk in content.split("<outline").skip(1) {
        let tag = chunk.split('>').next().unwrap_or("");
        let Some(xml_url) = attr_value(tag, "xmlUrl") else {
            continue;
        };
        let Some(channel_id) = xml_url
            .split("channel_id=")
            .nth(1)
            .map(|r| r.split('&').next().unwrap_or(r))
        else {
            continue;
        };
        let name = attr_value(tag, "title").or_else(|| attr_value(tag, "text"));
        out.push(ImportedSub {
            url: format!("https://www.youtube.com/channel/{}", channel_id),
            name: name.map(unescape_xml),
        });
    }
    if out.is_empty() {
        anyhow::bail!("no channel outlines found in OPML");
    }
    Ok(out)
}

fn attr_value(tag: &str, attr: &str) -> Option<String> {
    let needle = format!("{}=\"", attr);
    let rest = tag.split(&needle).nth(1)?;
    rest.split('"').next().map(|s| s.to_string())
}

fn unescape_xml(s: String) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

/// Parse a YouTube Takeout `subscriptions.csv`
/// (header: `Channel Id,Channel Url,Channel Title`).
pub fn parse_takeout_csv(content: &str) -> Result<Vec<ImportedSub>> {
    let mut lines = content.lines();
    let header = lines.next().context("empty file")?;
    if !header.to_lowercase().contains("channel id") {
        anyhow::bail!("not a YouTube Takeout subscriptions.csv");
    }
    let out: Vec<ImportedSub> = lines
        .filter_map(|line| {
            let mut cols = line.splitn(3, ',');
            let id = cols.next()?.trim();
            let _url = cols.next()?.trim();
            // Title is the remainder; may contain commas.
            let title = cols.next().map(|t| t.trim().to_string());
            if !id.starts_with("UC") {
                return None;
            }
            Some(ImportedSub {
                url: format!("https://www.youtube.com/channel/{}", id),
                name: title.filter(|t| !t.is_empty()),
            })
        })
        .collect();
    if out.is_empty() {
        anyhow::bail!("no subscriptions found in CSV");
    }
    Ok(out)
}

/// Parse any supported format, trying JSON → OPML → CSV in order.
pub fn parse_any(content: &str) -> Result<Vec<ImportedSub>> {
    parse_newpipe_json(content)
        .or_else(|_| parse_opml(content))
        .or_else(|_| parse_takeout_csv(content))
        .map_err(|_| {
            anyhow::anyhow!("unrecognised format (expected NewPipe JSON, OPML or Takeout CSV)")
        })
}

/// Import subscriptions from `path` into the subscriptions file.
/// Returns (added, skipped-as-duplicate).
pub fn import_file(path: &str) -> Result<(usize, usize)> {
    let expanded = if let Some(rest) = path.strip_prefix("~/") {
        dirs::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| std::path::PathBuf::from(path))
    } else {
        std::path::PathBuf::from(path)
    };
    let content = std::fs::read_to_string(&expanded)
        .with_context(|| format!("cannot read {}", expanded.display()))?;
    let imported = parse_any(&content)?;

    let existing: std::collections::HashSet<String> = crate::youtube::load_subscriptions()
        .into_iter()
        .map(|u| normalize(&u))
        .collect();

    let mut added = 0;
    let mut skipped = 0;
    let mut new_lines = String::new();
    let mut seen_this_import = std::collections::HashSet::new();
    for sub in imported {
        let key = normalize(&sub.url);
        if existing.contains(&key) || !seen_this_import.insert(key) {
            skipped += 1;
            continue;
        }
        match &sub.name {
            Some(name) => new_lines.push_str(&format!("{}  {}\n", sub.url, name)),
            None => new_lines.push_str(&format!("{}\n", sub.url)),
        }
        added += 1;
    }

    if added > 0 {
        let path = crate::config::youtube_subs_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        write!(file, "{}", new_lines)?;
    }

    Ok((added, skipped))
}

fn normalize(url: &str) -> String {
    url.trim()
        .trim_end_matches('/')
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_start_matches("www.")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newpipe_json() {
        let content = r#"{
            "app_version": "0.27.0",
            "subscriptions": [
                {"service_id": 0, "url": "https://www.youtube.com/channel/UC123", "name": "Chan One"},
                {"service_id": 0, "url": "https://www.youtube.com/channel/UC456", "name": "Chan Two"}
            ]
        }"#;
        let subs = parse_newpipe_json(content).unwrap();
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].url, "https://www.youtube.com/channel/UC123");
        assert_eq!(subs[0].name.as_deref(), Some("Chan One"));
    }

    #[test]
    fn opml_outlines() {
        let content = r#"<?xml version="1.0"?>
<opml version="1.1"><body>
  <outline text="Subs">
    <outline text="A &amp; B" title="A &amp; B" type="rss" xmlUrl="https://www.youtube.com/feeds/videos.xml?channel_id=UCaaa"/>
    <outline text="C" title="C" type="rss" xmlUrl="https://www.youtube.com/feeds/videos.xml?channel_id=UCbbb"/>
  </outline>
</body></opml>"#;
        let subs = parse_opml(content).unwrap();
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].url, "https://www.youtube.com/channel/UCaaa");
        assert_eq!(subs[0].name.as_deref(), Some("A & B"));
    }

    #[test]
    fn opml_without_channels_fails() {
        assert!(parse_opml("<opml><body></body></opml>").is_err());
    }

    #[test]
    fn takeout_csv() {
        let content = "Channel Id,Channel Url,Channel Title\n\
                       UC123,http://www.youtube.com/channel/UC123,Some Channel\n\
                       UC456,http://www.youtube.com/channel/UC456,Other, With Comma\n";
        let subs = parse_takeout_csv(content).unwrap();
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].url, "https://www.youtube.com/channel/UC123");
        assert_eq!(subs[0].name.as_deref(), Some("Some Channel"));
        assert_eq!(subs[1].name.as_deref(), Some("Other, With Comma"));
    }

    #[test]
    fn csv_wrong_header_fails() {
        assert!(parse_takeout_csv("foo,bar\n1,2\n").is_err());
    }

    #[test]
    fn parse_any_detects_format() {
        assert!(
            parse_any(r#"{"subscriptions": [{"url": "https://youtube.com/channel/UC1"}]}"#).is_ok()
        );
        assert!(parse_any("garbage").is_err());
    }

    #[test]
    fn normalize_variants_match() {
        assert_eq!(
            normalize("https://www.youtube.com/channel/UC123/"),
            normalize("http://youtube.com/channel/UC123")
        );
    }
}
