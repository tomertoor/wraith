//! Port forwarding module — socat-style TCP/UDP bridging.
//!
//! Provides [`PortForward`] which listens on a local port and pipes
//! traffic to a remote address via the peer Yamux connection.
//!
//! Distinct from a "tunnel" (TUN-based VPN). A forward is a simple
//! port-to-port pipe. A tunnel is a full IP tunnel via a TUN device.

pub mod relay;

pub use relay::SocatRelay;
