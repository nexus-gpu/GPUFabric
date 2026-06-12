pub mod asm;
pub mod cmd;
pub mod config;
pub mod device_info;
pub mod mobile_control_stream;
pub mod mobile_tls_policy;
pub mod model_downloader;
#[cfg(not(target_os = "ios"))]
pub mod model_downloader_example;
pub mod network_info;
pub mod nvswitch_check;
pub mod safe_command;
pub mod security_metrics;
pub mod system_info;
pub mod system_info_vulkan;

use std::sync::OnceLock;
use tracing::{debug, Level};

static LOG_ICONS_UTF8: OnceLock<bool> = OnceLock::new();

fn detect_utf8_locale() -> bool {
    for key in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Ok(v) = std::env::var(key) {
            let v = v.trim();
            if v.is_empty() {
                continue;
            }
            let lower = v.to_ascii_lowercase();
            return lower.contains("utf-8") || lower.contains("utf8");
        }
    }

    true
}

pub fn log_icon(unicode: &'static str, ascii: &'static str) -> &'static str {
    let utf8 = *LOG_ICONS_UTF8.get_or_init(detect_utf8_locale);
    if utf8 {
        unicode
    } else {
        ascii
    }
}

pub fn init_logging() {
    // Use DEBUG level for debug builds, INFO for release builds

    let _ = LOG_ICONS_UTF8.get_or_init(detect_utf8_locale);

    #[cfg(not(debug_assertions))]
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_ansi(!cfg!(windows))
        .with_target(false)
        // Debug builds: show thread info, file, and line number
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(false)
        .with_line_number(false)
        .compact()
        .init();

    #[cfg(debug_assertions)]
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_ansi(!cfg!(windows))
        .with_target(false)
        // Debug builds: show thread info, file, and line number
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .compact()
        .init();

    debug!("Logging initialized");
}
