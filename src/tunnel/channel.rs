//! Canonical channel ID constants used across agent, relay, and tunnel modules.

pub use self::ids::TUNNEL_DATA;
pub use self::ids::TUNNEL_CONTROL;
pub use self::ids::RELAY_DATA;
pub use self::ids::RELAY_CONTROL;

mod ids {
    /// Tunnel data channel — used for IP packet forwarding.
    pub const TUNNEL_DATA: u32 = 0;
    /// Tunnel control channel — used for tunnel open/close/stats.
    pub const TUNNEL_CONTROL: u32 = 1;
    /// Relay data channel — used for port-forward / socat relay traffic.
    pub const RELAY_DATA: u32 = 3;
    /// Relay control channel — used for RelayOpen/RelayClose messages.
    pub const RELAY_CONTROL: u32 = 5;
}
