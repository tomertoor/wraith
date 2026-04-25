#[macro_use]
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

    /// C2 host
    #[arg(long, default_value = "127.0.0.1")]
    c2_host: Ipv4Addr,

    /// C2 port
    #[arg(short = 'p', long, default_value_t = 4444)]
    c2_port: u16,

    /// Listen mode (server)
    #[arg(short = 'l', long)]
    listen: bool,

    /// Agent mode: listen for peer wraith connections (format: HOST:PORT)
    #[arg(long, value_name = "HOST:PORT")]
    agent_listen: Option<String>,

    /// Agent mode: connect to C2 at HOST:PORT, and optionally connect to peer at --peer HOST:PORT
    #[arg(long, value_name = "HOST:PORT")]
    agent_connect: Option<String>,

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

fn main() {
    let args = Args::parse();

    setup_logging(args.debug, &args.log_file).unwrap();

    // Handle agent mode
    if let Some(addr) = &args.agent_listen {
        let (host, port) = parse_host_port(addr);
        info!("Agent mode: listening for peers on {}:{}", host, port);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut wraith = wraith::Wraith::new_agent_listener(host, port);
        rt.block_on(wraith.run_agent_listener());
        return;
    }

    if let Some(addr) = &args.agent_connect {
        let (host, port) = parse_host_port(addr);
        let peer_addr = args.peer.clone();
        info!("Agent mode: connecting to C2 at {}:{}", host, port);
        if let Some(ref peer) = peer_addr {
            info!("Agent mode: connecting to peer at {}", peer);
        }
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut wraith = wraith::Wraith::new_agent_connect(host, port, peer_addr);
        rt.block_on(wraith.run_agent_connect());
        return;
    }

    let host = args.c2_host.to_string();
    let port = args.c2_port;
    let is_server = args.listen;

    info!("Starting wraith...");
    info!("Host: {}:{}, Mode: {}", host, port, if is_server { "listen" } else { "connect" });

    let mut wraith = wraith::Wraith::new(host, port, is_server);

    let rt = tokio::runtime::Runtime::new().unwrap();

    loop {
        match rt.block_on(wraith.run()) {
            Ok(_) => info!("Connection closed gracefully"),
            Err(e) => {
                error!("Error: {}", e);
            }
        }
    }
}