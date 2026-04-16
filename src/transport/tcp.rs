//! TCP transport for Wraith.

use async_trait::async_trait;
use std::io::Result;
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("connection failed: {0}")]
    Connect(String),
    #[error("bind failed: {0}")]
    Bind(String),
    #[error("accept failed: {0}")]
    Accept(String),
}

/// TCP transport implementation.
pub struct TcpTransport;

impl TcpTransport {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TcpTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Transport for TcpTransport {
    async fn connect(&self, addr: &str) -> Result<TcpStream> {
        TcpStream::connect(addr)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("connect: {}", e)))
    }

    async fn listen(&self, addr: &str) -> Result<TcpListener> {
        TcpListener::bind(addr)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("bind: {}", e)))
    }
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn connect(&self, addr: &str) -> Result<TcpStream>;
    async fn listen(&self, addr: &str) -> Result<TcpListener>;
}
