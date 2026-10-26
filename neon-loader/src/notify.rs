//! Discord webhook alerts. The webhook URL is a secret: it is read from the environment variable
//! `NEON_DISCORD_WEBHOOK` or a file (`--discord-webhook-file`), never written to logs or status files.
//! Sending runs on a throwaway thread (`curl`), so a slow network can never stall the bot.

use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    LevelUp,
    Death,
    LowHp,
    Stranger,
    LieDetector,
    Stopped,
    Lag,
    Disconnect,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::LevelUp => "level up",
            Kind::Death => "death",
            Kind::LowHp => "low HP",
            Kind::Stranger => "stranger nearby",
            Kind::LieDetector => "LIE DETECTOR",
            Kind::Stopped => "stopped",
            Kind::Lag => "stream lag",
            Kind::Disconnect => "DISCONNECTED?",
        }
    }

    /// Minimum gap between two alerts of this kind.
    fn cooldown(self) -> Duration {
        match self {
            Kind::LevelUp | Kind::Stopped => Duration::ZERO,
            Kind::LieDetector | Kind::Death | Kind::Disconnect => Duration::from_secs(20),
            Kind::LowHp | Kind::Stranger | Kind::Lag => Duration::from_secs(120),
        }
    }

    pub fn parse(name: &str) -> Option<Kind> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "level" | "levelup" => Kind::LevelUp,
            "death" | "dead" => Kind::Death,
            "lowhp" | "flee" | "hp" => Kind::LowHp,
            "stranger" | "strangers" => Kind::Stranger,
            "lie" | "liedetector" | "ld" => Kind::LieDetector,
            "stop" | "stopped" => Kind::Stopped,
            "lag" => Kind::Lag,
            "disconnect" | "connection" => Kind::Disconnect,
            _ => return None,
        })
    }
}

/// Which alert (if any) a bot notice should raise.
pub fn classify(msg: &str) -> Option<Kind> {
    let m = msg.to_ascii_lowercase();
    if m.contains("lie detector") {
        Some(Kind::LieDetector)
    } else if m.contains("no data from the game server") {
        Some(Kind::Disconnect)
    } else if m.contains("level up") {
        Some(Kind::LevelUp)
    } else if m.contains("character is dead") {
        Some(Kind::Death)
    } else if m.contains("stopping to avoid dying") {
        Some(Kind::LowHp)
    } else if m.contains("stranger within") {
        Some(Kind::Stranger)
    } else if m.contains("plan finished") || m.contains("reached target level") || m.contains("session limit") || m.contains("stop requested") {
        Some(Kind::Stopped)
    } else if m.contains("stream lag") {
        Some(Kind::Lag)
    } else {
        None
    }
}

/// Minimal JSON string escaping for the webhook body.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

pub fn valid_webhook(url: &str) -> bool {
    let u = url.trim();
    (u.starts_with("https://discord.com/api/webhooks/") || u.starts_with("https://discordapp.com/api/webhooks/")) && !u.contains(char::is_whitespace)
}

type Sender = Box<dyn Fn(&str, &str) + Send>;

pub struct Notifier {
    url: Option<String>,
    enabled: Vec<Kind>,
    last: HashMap<Kind, Instant>,
    session: String,
    send: Sender,
}

impl Notifier {
    pub fn new(url: Option<String>, enabled: Vec<Kind>, session: &str) -> Self {
        Self::with_sender(url, enabled, session, Box::new(|url, body| {
            let (url, body) = (url.to_string(), body.to_string());
            std::thread::spawn(move || {
                let _ = std::process::Command::new("curl")
                    .args(["-s", "-m", "8", "-o", "/dev/null", "-H", "Content-Type: application/json", "-d", &body, &url])
                    .status();
            });
        }))
    }

    pub fn with_sender(url: Option<String>, enabled: Vec<Kind>, session: &str, send: Sender) -> Self {
        let url = url.filter(|u| valid_webhook(u));
        Self { url, enabled, last: HashMap::new(), session: session.to_string(), send }
    }

    pub fn active(&self) -> bool {
        self.url.is_some()
    }

    /// Raise an alert for a bot notice if it is one we report. Returns true when something was sent.
    pub fn notice(&mut self, msg: &str, level: Option<u16>) -> bool {
        let Some(kind) = classify(msg) else { return false };
        self.send_kind(kind, msg, level)
    }

    pub fn send_kind(&mut self, kind: Kind, msg: &str, level: Option<u16>) -> bool {
        let Some(url) = self.url.clone() else { return false };
        if !self.enabled.contains(&kind) {
            return false;
        }
        if let Some(t) = self.last.get(&kind) {
            if t.elapsed() < kind.cooldown() {
                return false;
            }
        }
        self.last.insert(kind, Instant::now());
        let lv = level.map(|l| format!(" (Lv {l})")).unwrap_or_default();
        let text = format!("[neon:{}] **{}**{} {}", self.session, kind.label(), lv, msg);
        let body = format!("{{\"content\":\"{}\"}}", json_escape(&text));
        (self.send)(&url, &body);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    const URL: &str = "https://discord.com/api/webhooks/123/abc_DEF";

    fn capture(enabled: Vec<Kind>) -> (Notifier, Arc<Mutex<Vec<(String, String)>>>) {
        let log = Arc::new(Mutex::new(vec![]));
        let l2 = log.clone();
        let n = Notifier::with_sender(Some(URL.into()), enabled, "main", Box::new(move |u, b| l2.lock().unwrap().push((u.into(), b.into()))));
        (n, log)
    }

    #[test]
    fn classifies_the_notices_the_bot_emits() {
        assert_eq!(classify("LEVEL UP -> 8"), Some(Kind::LevelUp));
        assert_eq!(classify("paused: character is dead"), Some(Kind::Death));
        assert_eq!(classify("HP 45%: stopping to avoid dying"), Some(Kind::LowHp));
        assert_eq!(classify("paused: stranger within 120px"), Some(Kind::Stranger));
        assert_eq!(classify("paused: lie detector active"), Some(Kind::LieDetector));
        assert_eq!(classify("paused: no data from the game server for 15 s (disconnected?)"), Some(Kind::Disconnect));
        assert_eq!(classify("plan finished"), Some(Kind::Stopped));
        assert_eq!(classify("map changed to 5"), None);
    }

    #[test]
    fn sends_once_per_cooldown_and_only_enabled_kinds() {
        let (mut n, log) = capture(vec![Kind::LevelUp, Kind::LowHp]);
        assert!(n.notice("LEVEL UP -> 8", Some(8)));
        assert!(n.notice("LEVEL UP -> 9", Some(9)), "level ups are never throttled");
        assert!(n.notice("HP 40%: stopping to avoid dying", Some(9)));
        assert!(!n.notice("HP 41%: stopping to avoid dying", Some(9)), "low-HP spam is rate limited");
        assert!(!n.notice("paused: character is dead", Some(9)), "death not enabled");
        let sent = log.lock().unwrap();
        assert_eq!(sent.len(), 3);
        assert!(sent[0].1.contains("Lv 8") && sent[0].1.contains("level up") && sent[0].1.contains("[neon:main]"), "{}", sent[0].1);
    }

    #[test]
    fn no_url_or_a_non_discord_url_means_silence() {
        let hits = Arc::new(Mutex::new(0));
        for url in [None, Some("http://evil.example/hook".to_string()), Some("https://discord.com/api/webhooks/1/x y".to_string())] {
            let h = hits.clone();
            let mut n = Notifier::with_sender(url, vec![Kind::LevelUp], "main", Box::new(move |_, _| *h.lock().unwrap() += 1));
            assert!(!n.active());
            assert!(!n.notice("LEVEL UP -> 8", Some(8)));
        }
        assert_eq!(*hits.lock().unwrap(), 0);
    }

    #[test]
    fn message_text_is_json_safe() {
        assert_eq!(json_escape("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
        let (mut n, log) = capture(vec![Kind::Stopped]);
        n.notice("plan finished: \"daily\"\nbye", None);
        let body = log.lock().unwrap()[0].1.clone();
        let v: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
        assert!(v["content"].as_str().unwrap().contains("\"daily\""));
    }

    #[test]
    fn kind_names_parse() {
        assert_eq!(Kind::parse("LEVEL"), Some(Kind::LevelUp));
        assert_eq!(Kind::parse("ld"), Some(Kind::LieDetector));
        assert_eq!(Kind::parse("nope"), None);
    }
}
