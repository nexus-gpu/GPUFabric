pub mod asm;
pub mod cmd;
pub mod config;
pub mod device_info;
pub mod model_downloader;
pub mod model_downloader_example;
pub mod network_info;
pub mod system_info;
pub mod system_info_vulkan;

use tracing::{debug, Level};

pub fn init_logging() {
    // Use DEBUG level for debug builds, INFO for release builds

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
