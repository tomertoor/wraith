//! Wraith — Modular Reverse Tunneling Tool
//!
//! Usage:
//!     wraith agent <c2_addr>          Run as agent, connect to C2 server (PyWraith)
//!     wraith listen [addr]           Listen for peer connections
//!     wraith connect <addr>          Connect to a peer
//!     wraith forward <lport> <rhost:rport>  One-shot TCP port forward
//!
//! Tunnel and relay management is done via PyWraith through the C2 channel.

mod agent;
mod peer;
mod platform;
mod proto;
mod relay;
mod tserver;
mod transport;
mod tunnel;
mod wraith;

use anyhow::Result;
use clap::Parser;
use peer::{run_connect, run_listen};
use relay::SocatRelay;
use wraith::Config;

// ─── CLI ───────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "wraith")]
#[command(version = "0.2.0")]
#[command(about = "Modular reverse tunneling tool for penetration testing")]
enum Cli {
    /// Run as agent, connecting to a C2 server (PyWraith).
    Agent {
        /// C2 server address (e.g., "localhost:9000").
        c2_addr: String,
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
    /// One-shot TCP port forward: listen on lport and bridge to rhost:rport.
    Forward {
        /// Local port to listen on.
        lport: u16,
        /// Remote host and port (e.g., "192.168.1.1:80").
        rhost: String,
        /// Remote port.
        rport: u16,
    },
}

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    setup_logging("info");

    match cli {
        Cli::Agent { c2_addr } => {
            run_agent(&c2_addr).await?;
        }
        Cli::Listen { addr } => {
            run_listen(&addr, None, "10.8.0.1/24").await?;
        }
        Cli::Connect { addr } => {
            run_connect(&addr, None, "10.8.0.1/24").await?;
        }
        Cli::Forward { lport, rhost, rport } => {
            run_forward(lport, &rhost, rport).await?;
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

// ─── Agent ────────────────────────────────────────────────────────────────────

async fn run_agent(c2_addr: &str) -> Result<()> {
    log::info!("Starting Wraith agent, connecting to C2 at {}", c2_addr);

    let (c2_host, c2_port) = Config::parse_addr(c2_addr);

    // Load defaults then override from CLI
    let base = Config::from_args();

    let config = Config {
        c2_addr: Some(c2_addr.to_string()),
        tunnel_addr: None,
        tunnel_host: base.tunnel_host,
        tunnel_port: base.tunnel_port,
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

// ─── Forward ──────────────────────────────────────────────────────────────────

async fn run_forward(lport: u16, rhost: &str, rport: u16) -> Result<()> {
    let local_addr = format!("0.0.0.0:{}", lport);
    let remote_addr = format!("{}:{}", rhost, rport);
    let forward_id = format!("fwd-{}", uuid::Uuid::new_v4());

    log::info!("Starting forward {}: {} -> {}", forward_id, local_addr, remote_addr);

    let mut socat = SocatRelay::new();
    socat.start_relay(&forward_id, "tcp", &local_addr, &remote_addr).await?;

    // Keep running — the forward tasks run in background
    tokio::signal::ctrl_c().await?;

    Ok(())
}
