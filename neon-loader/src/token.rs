//! Resolve the one-time Nexon launch token from the running game's command line.
//!
//! The launcher starts the client as:
//!   MapleStory.exe "" -nxl NP12:us:0:<n>:<token>
//! The token is one-time and sensitive (docs/launching.md: never log it). This is
//! the macOS/Linux counterpart to `neon-probe/hook-session.c`, which captures the
//! same token from the launcher's `CreateProcessW` into `NeonSessionToken` shared
//! memory on the Wine/Linux path. Read it on demand with `neon-loader --print-token`.

use std::process::Command;

/// The launch token of the running MapleStory client, if one is found.
pub fn game_token() -> Option<String> {
    #[cfg(target_os = "linux")]
    if let Some(t) = from_proc() {
        return Some(t);
    }
    from_ps()
}

#[cfg(target_os = "linux")]
fn from_proc() -> Option<String> {
    let needle = b"MapleStory.exe";
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let cmd = match std::fs::read(entry.path().join("cmdline")) {
            Ok(c) if !c.is_empty() => c,
            _ => continue,
        };
        if cmd.windows(needle.len()).any(|w| w == needle) {
            if let Some(t) = extract(&String::from_utf8_lossy(&cmd)) {
                return Some(t);
            }
        }
    }
    None
}

fn from_ps() -> Option<String> {
    let out = Command::new("ps").arg("-axo").arg("command=").output().ok()?;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if line.contains("MapleStory.exe") {
            if let Some(t) = extract(line) {
                return Some(t);
            }
        }
    }
    None
}

/// Pull the non-space argument after `-nxl` out of a command line.
fn extract(s: &str) -> Option<String> {
    let idx = s.find("-nxl ")?;
    let tok = s[idx + 5..].split_whitespace().next()?;
    (!tok.is_empty()).then(|| tok.to_string())
}

#[cfg(test)]
mod tests {
    use super::extract;

    #[test]
    fn parses_launch_token() {
        let cmd = r#"C:\...\MapleStory.exe "" -nxl NP12:us:0:197951320:ABCDEF012345"#;
        assert_eq!(extract(cmd).as_deref(), Some("NP12:us:0:197951320:ABCDEF012345"));
    }

    #[test]
    fn none_without_token() {
        assert_eq!(extract("MapleStory.exe"), None);
    }
}
