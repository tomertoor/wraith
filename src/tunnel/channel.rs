//! Canonical channel ID constants used across agent, relay, tunnel server, and forward modules.

pub use self::ids::TUNNEL_DATA;
pub use self::ids::TUNNEL_CONTROL;
pub use self::ids::FORWARD_DATA;
pub use self::ids::FORWARD_CONTROL;

mod ids {
    /// Tunnel data channel — used for IP packet forwarding.
    pub const TUNNEL_DATA: u32 = 0;
    /// Tunnel control channel — used for tunnel open/close/stats.
    pub const TUNNEL_CONTROL: u32 = 1;
    /// Forward data channel — used for port-forward relay traffic.
    pub const FORWARD_DATA: u32 = 3;
    /// Forward control channel — used for RelayOpen/RelayClose messages.
    pub const FORWARD_CONTROL: u32 = 5;
}
