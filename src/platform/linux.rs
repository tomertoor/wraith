//! Linux-specific platform code — TUN interface configuration via netlink.

use std::io;
use std::process::Command;

/// Set a network interface UP.
pub fn set_if_up(iface: &str) -> io::Result<()> {
    let output = Command::new("ip")
        .args(["link", "set", iface, "up"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            String::from_utf8_lossy(&output.stderr),
        ));
    }
    Ok(())
}

/// Set a network interface DOWN.
pub fn set_if_down(iface: &str) -> io::Result<()> {
    let output = Command::new("ip")
        .args(["link", "set", iface, "down"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            String::from_utf8_lossy(&output.stderr),
        ));
    }
    Ok(())
}

/// Assign an IP address and netmask to an interface.
pub fn set_if_addr(iface: &str, ip: &str, prefix_len: u8) -> io::Result<()> {
    let cidr = format!("{}/{}", ip, prefix_len);
    let output = Command::new("ip")
        .args(["addr", "add", &cidr, "dev", iface])
        .output()?;
    if !output.status.success() {
        // If address already exists, that's okay
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("File exists") {
            return Err(io::Error::new(io::ErrorKind::Other, stderr));
        }
    }
    Ok(())
}

/// Add a route through the TUN interface.
pub fn add_route(dst: &str, iface: &str) -> io::Result<()> {
    let output = Command::new("ip")
        .args(["route", "add", dst, "dev", iface])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("File exists") {
            return Err(io::Error::new(io::ErrorKind::Other, stderr));
        }
    }
    Ok(())
}

/// Delete a route.
pub fn del_route(dst: &str) -> io::Result<()> {
    let output = Command::new("ip")
        .args(["route", "del", dst])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("No such file or directory") {
            return Err(io::Error::new(io::ErrorKind::Other, stderr));
        }
    }
    Ok(())
}

/// Delete an IP address from an interface.
pub fn del_if_addr(iface: &str, ip: &str, prefix_len: u8) -> io::Result<()> {
    let cidr = format!("{}/{}", ip, prefix_len);
    let output = Command::new("ip")
        .args(["addr", "del", &cidr, "dev", iface])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("No such file or directory") {
            return Err(io::Error::new(io::ErrorKind::Other, stderr));
        }
    }
    Ok(())
}
