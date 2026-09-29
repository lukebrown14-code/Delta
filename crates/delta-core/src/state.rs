//! Tiny on-disk session state: when the user last opened Delta.
//! Port of `delta/core/state.py`.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};

pub const STATE_NAME: &str = ".delta_state.json";

/// How far back a first run looks, so it shows something without replaying history.
pub const FIRST_RUN_WINDOW_DAYS: i64 = 7;

pub fn state_path(db_path: &str) -> PathBuf {
    Path::new(db_path)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(STATE_NAME)
}

/// When the user last opened Delta, or a week ago if that is unknown.
pub fn read_last_seen(db_path: &str) -> DateTime<Utc> {
    let fallback = Utc::now() - Duration::days(FIRST_RUN_WINDOW_DAYS);
    let Ok(raw) = std::fs::read_to_string(state_path(db_path)) else {
        return fallback;
    };
    serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|v| {
            v.get("last_seen")
                .and_then(|s| s.as_str())
                .map(str::to_string)
        })
        .and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|d| d.with_timezone(&Utc))
        })
        .unwrap_or(fallback)
}

/// Record this visit. Never panics — a read-only disk must not stop the app.
pub fn write_last_seen(db_path: &str, when: Option<DateTime<Utc>>) {
    let when = when.unwrap_or_else(Utc::now);
    let path = state_path(db_path);
    let naive = when.naive_utc().format("%Y-%m-%dT%H:%M:%S%.6f").to_string();
    let payload = format!("{{\"last_seen\":\"{naive}+00:00\"}}");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, payload);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_last_seen() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("d.db");
        let when = DateTime::parse_from_rfc3339("2026-09-23T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        write_last_seen(db_path.to_str().unwrap(), Some(when));
        assert_eq!(read_last_seen(db_path.to_str().unwrap()), when);
    }

    #[test]
    fn missing_state_gives_window_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("d.db");
        let seen = read_last_seen(db_path.to_str().unwrap());
        let now = Utc::now();
        assert!(seen < now);
        assert!((now - seen).num_days() >= FIRST_RUN_WINDOW_DAYS - 1);
    }
}
