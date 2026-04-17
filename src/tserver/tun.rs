//! TUN interface management for the tunnel server side (requires root).

use std::io;
use std::net::Ipv4Addr;
use tokio_tun::TunBuilder;

/// A TUN device wrapper using tokio-tun.
pub struct TunDevice {
    pub name: String,
    tun: tokio_tun::Tun,
    /// Stored IP and prefix for automatic cleanup on drop.
    ip: String,
    prefix_len: u8,
}

impl TunDevice {
    /// Create and open a TUN interface named `name`, configuring IP and bringing it up.
    pub async fn open(name: &str, ip: &str, prefix_len: u8) -> io::Result<Self> {
        let tun = TunBuilder::new()
            .name(name)
            .tap(false)
            .packet_info(true)
            .mtu(1500)
            .address(ip.parse().unwrap_or(Ipv4Addr::new(10, 8, 0, 1)))
            .destination(Ipv4Addr::new(10, 8, 0, 2))
            .netmask(Ipv4Addr::new(255, 255, 255, 0))
            .up()
            .try_build()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let tun_name = tun.name().to_string();

        Ok(Self {
            name: tun_name,
            tun,
            ip: ip.to_string(),
            prefix_len,
        })
    }

    /// Configure the interface with IP and netmask prefix.
    pub fn configure(&self, ip: &str, prefix_len: u8) -> io::Result<()> {
        crate::platform::set_if_addr(&self.name, ip, prefix_len)?;
        crate::platform::set_if_up(&self.name)?;
        Ok(())
    }

    /// Add a route through this interface.
    pub fn add_route(&self, dst: &str) -> io::Result<()> {
        crate::platform::add_route(dst, &self.name)
    }

    /// Remove routes and bring interface down — called automatically on drop.
    pub fn cleanup(&self) -> io::Result<()> {
        let _ = crate::platform::del_route(&self.ip);
        crate::platform::del_if_addr(&self.name, &self.ip, self.prefix_len)?;
        crate::platform::set_if_down(&self.name)?;
        Ok(())
    }

    /// Read an IP packet from the TUN device.
    pub async fn read_packet(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        use tokio::io::AsyncReadExt;
        self.tun.read(buf).await
    }

    /// Write an IP packet to the TUN device.
    pub async fn write_packet(&mut self, buf: &[u8]) -> io::Result<usize> {
        use tokio::io::AsyncWriteExt;
        self.tun.write_all(buf).await?;
        Ok(buf.len())
    }

    /// Get the underlying TUN device for async I/O.
    pub fn inner(&self) -> &tokio_tun::Tun {
        &self.tun
    }

    /// Get a mutable reference to the TUN device.
    pub fn inner_mut(&mut self) -> &mut tokio_tun::Tun {
        &mut self.tun
    }
}

impl Drop for TunDevice {
    fn drop(&mut self) {
        // Attempt cleanup; ignore errors since we may be in an error path.
        if let Err(e) = self.cleanup() {
            log::warn!("TUN device {} cleanup error: {}", self.name, e);
        }
    }
}
