//! Wraith — Modular Reverse Tunneling Tool
//!
//! Usage:
//!     wraith c2 <addr>        Connect to Nexus C2 server
//!     wraith listen [addr]    Listen for peer connections
//!     wraith connect <addr>   Connect to a peer
//!     wraith direct           Run as PyWraith subprocess (JSONL stdin/stdout)
//!
//! All tunnel and relay management is done via PyWraith through the network.

mod agent;
mod direct;
mod peer;
mod platform;
mod proto;
mod relay;
mod socat;
mod transport;
mod tunnel;
mod wraith;

use anyhow::Result;
use clap::Parser;
use direct::run_direct;
use peer::{run_connect, run_listen};
use wraith::Config;

// ─── CLI ───────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "wraith")]
#[command(version = "0.2.0")]
#[command(about = "Modular reverse tunneling tool for penetration testing")]
enum Cli {
    /// Connect to a Nexus C2 server.
    C2 {
        /// Nexus C2 server address (e.g., "nexus:8080" or "10.0.0.1:8080").
        addr: String,
    },
    /// Listen for incoming peer connections.
    Listen {
        /// Address to listen on.
        #[arg(default_value = "0.0.0.0:4445")]
        addr: String,
    },
    /// Connect to a peer.
    Connect {
        /// Peer address to connect to.
        addr: String,
    },
    /// Run as subprocess controlled by PyWraith via stdin/stdout JSONL.
    Direct,
}

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    setup_logging("info");

    match cli {
        Cli::C2 { addr } => {
            run_c2(&addr).await?;
        }
        Cli::Listen { addr } => {
            run_listen(&addr, None, "10.8.0.1/24").await?;
        }
        Cli::Connect { addr } => {
            run_connect(&addr, None, "10.8.0.1/24").await?;
        }
        Cli::Direct => {
            run_direct().await?;
        }
    }

    Ok(())
}

fn setup_logging(_level: &str) {
    use simplelog::{ColorChoice, ConfigBuilder, LevelFilter, TermLogger, TerminalMode};

    let _ = TermLogger::init(
        LevelFilter::Info,
        ConfigBuilder::new().build(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    );
}

// ─── C2 ────────────────────────────────────────────────────────────────────

async fn run_c2(c2_addr: &str) -> Result<()> {
    log::info!("Starting Wraith C2 agent → {}", c2_addr);

    let (c2_host, c2_port) = Config::parse_addr(c2_addr);

    let config = Config {
        c2_addr: Some(c2_addr.to_string()),
        tunnel_addr: None,
        relay_host: "127.0.0.1".to_string(),
        relay_port: 4446,
        c2_host,
        c2_port,
        reconnect: true,
        reconnect_delay: 5,
        heartbeat_interval: 30,
        encryption: "chacha20".to_string(),
        log_level: "info".to_string(),
        tunnel_ip: "10.8.0.2".to_string(),
    };

    let mut client = agent::AgentClient::new(config);
    client.connect_with_reconnect().await?;
    client.run_dual().await?;

    Ok(())
}
