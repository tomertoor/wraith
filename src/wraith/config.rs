use std::net::Ipv4Addr;

pub struct Config {
    pub ip_address: Ipv4Addr,
    pub port: u16,
    pub is_listen: bool,
}

impl Config {
    pub fn new(ip_address: Ipv4Addr, port: u16, is_listen: bool) -> Self {
        Self {
            ip_address,
            port,
            is_listen,
        }
    }

    pub fn default() -> Self {
        Self {
            ip_address: Ipv4Addr::new(127, 0, 0, 1),
            port: 4444,
            is_listen: false,
        }
    }
}