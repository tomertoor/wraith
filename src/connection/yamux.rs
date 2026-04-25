use anyhow::Result;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};
use yamux::{Config, Connection, Mode, Stream};

pub struct YamuxConnection {
    conn: Connection<Compat<TcpStream>>,
    peer_addr: SocketAddr,
}

impl YamuxConnection {
    /// Connect to a remote wraith
    pub async fn connect(addr: &str) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        let peer_addr = stream.peer_addr()?;
        let config = Config::default();
        let conn = Connection::new(stream.compat(), config, Mode::Client);
        Ok(Self { conn, peer_addr })
    }

    /// Accept an incoming Yamux connection (for server mode)
    pub async fn accept(stream: TcpStream) -> Result<Self> {
        let peer_addr = stream.peer_addr()?;
        let config = Config::default();
        let conn = Connection::new(stream.compat(), config, Mode::Server);
        Ok(Self { conn, peer_addr })
    }

    /// Open a new bidirectional stream (async)
    pub async fn open_stream(&mut self) -> Result<Stream> {
        let mut control = self.conn.control();
        control.open_stream().await.map_err(|e| anyhow::anyhow!("{}", e))
    }

    /// Get the next incoming stream (None on EOF)
    pub async fn next_stream(&mut self) -> Option<Result<Stream, yamux::ConnectionError>> {
        match self.conn.next_stream().await {
            Ok(Some(stream)) => Some(Ok(stream)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }

    /// Check if connection is healthy
    /// Uses poll_open_stream on control to check if connection is alive
    pub async fn is_healthy(&mut self) -> bool {
        let mut control = self.conn.control();
        // Try to open a stream - if it completes, connection is healthy
        match control.open_stream().await {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    /// Close the connection
    pub fn close(&mut self) {
        // Yamux Connection doesn't have explicit close, dropping closes
    }
}