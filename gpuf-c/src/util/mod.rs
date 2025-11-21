
pub mod cmd;
pub mod asm;
pub mod config;
pub mod network_info;
pub mod system_info;
pub mod system_info_vulkan;
pub mod device_info;
pub mod model_downloader;
pub mod model_downloader_example;


use tracing::{Level,debug};

pub fn init_logging() {
    tracing_subscriber::fmt()
    .with_max_level(Level::DEBUG )
    .with_ansi(!cfg!(windows)) 
    .init();
    debug!("Logging initialized");
}
