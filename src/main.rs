#[macro_use]
extern crate log;
extern crate simplelog;

mod proto;
mod connection;
mod message;
mod commands;
mod relay;
mod wraith;

use clap::Parser;
use crate::wraith::Config;
use crate::connection::TcpConnection;
use log::{error, info};
use std::net::Ipv4Addr;

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