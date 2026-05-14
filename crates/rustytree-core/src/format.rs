//! Display helpers used by both the binary's UI code and (eventually) any
//! alternative front-end. Pure functions, easy to unit-test in isolation.

use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local};
use humansize::{BINARY, format_size};

/// Format a byte count as a human-readable IEC string (`1.5 KiB`, `3.2 MiB`).
pub fn bytes(n: u64) -> String {
    format_size(n, BINARY)
}

/// Placeholder rendered in cells whose underlying value is `None` —
/// e.g. the Allocated column on Windows until `GetCompressedFileSize`
/// lands. Centralised so the CLI and GUI can't drift on capitalisation
/// or wording.
pub const NA_PLACEHOLDER: &str = "N/A";

/// Format an optional byte count: `Some(n)` falls through to [`bytes`],
/// `None` becomes [`NA_PLACEHOLDER`]. Used for columns whose value
/// isn't reliably computable on every platform — most commonly the
/// Allocated column on Windows, which can't yet derive a real on-disk
/// size from a `std::fs::Metadata`.
pub fn bytes_opt(n: Option<u64>) -> String {
    match n {
        Some(b) => bytes(b),
        None => NA_PLACEHOLDER.to_string(),
    }
}

/// Format a fraction in `[0.0, 1.0]` as a percent with one decimal place.
/// Out-of-range values are clamped.
pub fn percent(fraction: f32) -> String {
    let clamped = fraction.clamp(0.0, 1.0);
    format!("{:.1}%", clamped * 100.0)
}

/// Format a [`SystemTime`] as `YYYY-MM-DD HH:MM`. Unavailable -> empty string.
pub fn mtime(t: Option<SystemTime>) -> String {
    match t {
        Some(t) => DateTime::<Local>::from(t)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        None => String::new(),
    }
}

/// Format a [`Duration`] as `1m 23.4s` / `1.2s` / `420ms`.
pub fn elapsed(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs >= 60.0 {
        let m = (secs / 60.0).floor();
        let s = secs - m * 60.0;
        format!("{m:.0}m {s:.1}s")
    } else if secs >= 1.0 {
        format!("{secs:.1}s")
    } else {
        format!("{}ms", d.as_millis())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_uses_iec_units() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1024), "1 KiB");
        assert_eq!(bytes(1024 * 1024), "1 MiB");
    }

    #[test]
    fn bytes_opt_renders_some_like_bytes_and_none_as_placeholder() {
        assert_eq!(bytes_opt(Some(1024)), "1 KiB");
        assert_eq!(bytes_opt(Some(0)), "0 B");
        assert_eq!(bytes_opt(None), NA_PLACEHOLDER);
        assert_eq!(NA_PLACEHOLDER, "N/A");
    }

    #[test]
    fn percent_clamps_and_rounds() {
        assert_eq!(percent(0.0), "0.0%");
        assert_eq!(percent(0.5), "50.0%");
        assert_eq!(percent(1.0), "100.0%");
        assert_eq!(percent(-1.0), "0.0%");
        assert_eq!(percent(2.0), "100.0%");
    }

    #[test]
    fn mtime_none_is_empty_string() {
        assert_eq!(mtime(None), "");
    }

    #[test]
    fn mtime_some_renders_local_string() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(0);
        let s = mtime(Some(t));
        // We don't assert the exact local-tz value, but format must be 16 chars
        // ("YYYY-MM-DD HH:MM").
        assert_eq!(s.len(), 16, "got {s:?}");
    }

    #[test]
    fn elapsed_uses_appropriate_units() {
        assert_eq!(elapsed(Duration::from_millis(200)), "200ms");
        assert_eq!(elapsed(Duration::from_secs_f64(1.25)), "1.2s");
        assert_eq!(elapsed(Duration::from_secs_f64(75.0)), "1m 15.0s");
    }
}
