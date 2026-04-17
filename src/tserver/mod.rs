//! Tunnel server implementation.
//!
//! The tunnel server (tserver) accepts agent connections via Yamux and:
//! - Manages TUN network interfaces for VPN tunnel traffic
//! - Forwards tunnel IP packets to/from the TUN device
//! - Handles tunnel open/close with agents
//!
//! Distinct from a "relay" (port forwarding). The tserver does NOT do
//! port forwarding itself. Port forwarding is handled by the [`crate::relay`] module.

pub mod server;
pub mod tun;
pub mod tunnel_handler;

pub use server::TunnelServer;
pub use tun::TunDevice;
pub use tunnel_handler::TunnelHandler;
