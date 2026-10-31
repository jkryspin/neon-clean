use std::path::{Path, PathBuf};

pub const APP_DIR: &str = r"C:\Nexon\Library\maplestorycw\appdata";
pub const LAUNCHER: &str = r"C:\Program Files (x86)\Nexon\Nexon Launcher\nexon_launcher.exe";
pub const PLAY_URL: &str = "nxl://launch/59813";
pub const MANIFEST: &str = r"C:\Nexon\Library\maplestorycw\patchdata\59813.manifest.hash";
#[allow(dead_code)]
pub const PATCH_LOG: &str = r"C:\Neon\logs\patch-launch";

pub const CLEAN_HASH: &str = "1942A7B31BE7C3E582D00561C6DC41A5EA163DF8EE65554859DA7A3339F18F65";
pub const PATCHED_HASH: &str = "0E9169D9FC0E322451AD13DF16647B8D62F7C6683D23DEADC17B47C5C2EEFAF4";
#[allow(dead_code)]
pub const MOCK_GRAPP: (&str, &str) = (
    "grap64.dll",
    "D33642B922B0FF4F5A7B1E3F860CCB05E967FFA30B60CEC4D6BB8BCF39E6A6A8",
);
#[allow(dead_code)]
pub const MOCK_NMCO: (&str, &str) = (
    "nmcogame64.dll",
    "E10C368B08D7C7077E78B085703AD969C1C2089C092C235581284C4A8B9C3E56",
);

pub fn root() -> PathBuf {
    if let Ok(root) = std::env::var("NEON_ROOT") {
        return PathBuf::from(root);
    }
    let mut dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    while let Some(d) = dir {
        if d.join("apps")
            .join("loader")
            .join("payload")
            .join("probe")
            .exists()
            || d.join("tools").join("ngs-patch").exists()
        {
            return d;
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn payload(name: &str) -> PathBuf {
    root()
        .join("apps")
        .join("loader")
        .join("payload")
        .join(name)
}

pub fn patch_script() -> PathBuf {
    root().join("tools").join("ngs-patch").join("patch.py")
}

pub fn ce_exe() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().unwrap_or(Path::new("."));
    let sibling = dir.join("neon-ce.exe");
    if sibling.exists() {
        return sibling;
    }
    root().join("target").join("debug").join("neon-ce.exe")
}

pub fn python() -> String {
    for candidate in [
        r"C:\Users\John\AppData\Local\Programs\Python\Python312\python.exe",
        "python",
        "py",
    ] {
        if candidate.starts_with(r"C:\") {
            if Path::new(candidate).exists() {
                return candidate.to_string();
            }
        } else if std::process::Command::new(candidate)
            .arg("--version")
            .output()
            .is_ok()
        {
            return candidate.to_string();
        }
    }
    "python".into()
}
