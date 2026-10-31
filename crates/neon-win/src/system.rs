//! Machine-level state the launcher reports before and after injection.

use std::path::Path;

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct AcState {
    pub grap_mocked: bool,
    pub nmco_mocked: bool,
    pub grap_loaded: bool,
    pub nmco_loaded: bool,
    pub blackcat_service: String,
    pub ngs_service: String,
}

pub fn ac_state(app: &Path, loaded: &[String]) -> AcState {
    let mocked = |name: &str| {
        let real = app.join(format!("{name}.orig"));
        let cur = app.join(name);
        real.exists() && cur.exists() && file_differs(&real, &cur)
    };
    AcState {
        grap_mocked: mocked("grap64.dll"),
        nmco_mocked: mocked("nmcogame64.dll"),
        grap_loaded: loaded.iter().any(|m| m.eq_ignore_ascii_case("grap64.dll")),
        nmco_loaded: loaded
            .iter()
            .any(|m| m.eq_ignore_ascii_case("nmcogame64.dll")),
        blackcat_service: service_state("BlackCat1"),
        ngs_service: service_state("NGS"),
    }
}

fn file_differs(a: &Path, b: &Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(a), Ok(b)) => a != b,
        _ => true,
    }
}

fn service_state(name: &str) -> String {
    use std::process::Command;
    let out = Command::new("sc").args(["query", name]).output();
    match out {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            let state = text
                .lines()
                .find(|l| l.contains("STATE"))
                .map(|l| l.trim().to_string())
                .unwrap_or_else(|| "unknown".into());
            state
        }
        Err(_) => "unknown".into(),
    }
}
