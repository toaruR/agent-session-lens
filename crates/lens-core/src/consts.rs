//! Architecture-v5 section 2: every default value is defined here exactly once.

pub const STALE_SECS: u64 = 600;
pub const RECENT_WRITE_SECS: u64 = 30;
pub const CLOCK_SKEW_MS: u64 = 5000;
pub const REEVAL_SECS: u64 = 5;
pub const MAX_LINE_BYTES: usize = 4194304;
pub const HEAD_SCAN_BYTES: u64 = 524288;
pub const TAIL_SCAN_BYTES: u64 = 8388608;
pub const DEBOUNCE_MS: u64 = 200;
pub const HOT_WINDOW_SECS: u64 = 900;
pub const HOT_POLL_SECS: u64 = 2;
pub const RECONCILE_SECS: u64 = 60;
pub const INITIAL_SCAN_DAYS: u64 = 7;
pub const DISTRO_POLL_SECS: u64 = 60;
pub const HB_INTERVAL_SECS: u64 = 10;
pub const HB_TIMEOUT_SECS: u64 = 30;
pub const HB_MEDIAN_N: usize = 5;
pub const BACKOFF_MAX_SECS: u64 = 60;
pub const PROTOCOL_VERSION: u32 = 1;
pub const RESOLVE_MAX_DEPTH: usize = 32;
pub const TRUNC_TEXT_CHARS: usize = 2000;
pub const TRUNC_OUT_CHARS: usize = 1000;
pub const SK_MIN: usize = 20;
pub const GH_LEN: usize = 36;
pub const AWS_LEN: usize = 16;
pub const R4_WORDS: [&str; 5] = ["TOKEN", "SECRET", "PASSWORD", "API_?KEY", "CSRF"];
pub const SETTINGS_PATH: &str = r"%APPDATA%\agent-session-lens\settings.json";
pub const HELPER_PATH: &str = "/usr/local/bin/lens-wsl-helper";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section2_values_match_design() {
        assert_eq!(STALE_SECS, 600);
        assert_eq!(RECENT_WRITE_SECS, 30);
        assert_eq!(CLOCK_SKEW_MS, 5000);
        assert_eq!(REEVAL_SECS, 5);
        assert_eq!(MAX_LINE_BYTES, 4194304);
        assert_eq!(HEAD_SCAN_BYTES, 524288);
        assert_eq!(TAIL_SCAN_BYTES, 8388608);
        assert_eq!(DEBOUNCE_MS, 200);
        assert_eq!(HOT_WINDOW_SECS, 900);
        assert_eq!(HOT_POLL_SECS, 2);
        assert_eq!(RECONCILE_SECS, 60);
        assert_eq!(INITIAL_SCAN_DAYS, 7);
        assert_eq!(DISTRO_POLL_SECS, 60);
        assert_eq!(HB_INTERVAL_SECS, 10);
        assert_eq!(HB_TIMEOUT_SECS, 30);
        assert_eq!(HB_MEDIAN_N, 5);
        assert_eq!(BACKOFF_MAX_SECS, 60);
        assert_eq!(PROTOCOL_VERSION, 1);
        assert_eq!(RESOLVE_MAX_DEPTH, 32);
        assert_eq!(TRUNC_TEXT_CHARS, 2000);
        assert_eq!(TRUNC_OUT_CHARS, 1000);
        assert_eq!(SK_MIN, 20);
        assert_eq!(GH_LEN, 36);
        assert_eq!(AWS_LEN, 16);
    }

    #[test]
    fn section2_strings_match_design() {
        assert_eq!(R4_WORDS, ["TOKEN", "SECRET", "PASSWORD", "API_?KEY", "CSRF"]);
        assert_eq!(HELPER_PATH, "/usr/local/bin/lens-wsl-helper");
        assert!(SETTINGS_PATH.ends_with(r"agent-session-lens\settings.json"));
    }
}
