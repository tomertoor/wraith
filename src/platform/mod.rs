//! Platform-specific code.

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(not(target_os = "linux"))]
pub mod stub {
    pub fn set_if_up(_: &str) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "TUN only supported on Linux",
        ))
    }

    pub fn set_if_addr(_: &str, _: &str, _: u8) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "TUN only supported on Linux",
        ))
    }
}

#[cfg(not(target_os = "linux"))]
pub use stub::*;
