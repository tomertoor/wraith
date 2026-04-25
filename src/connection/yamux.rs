use anyhow::Result;
use std::net::SocketAddr;
use std::pin::Pin;
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
        let mut conn = Pin::new(&mut self.conn);
        futures::future::poll_fn(|cx| conn.poll_new_outbound(cx)).await.map_err(|e| anyhow::anyhow!("{}", e))
    }

    /// Get the next incoming stream (None on EOF)
    pub async fn next_stream(&mut self) -> Option<Result<Stream, yamux::ConnectionError>> {
        let mut conn = Pin::new(&mut self.conn);
        match futures::future::poll_fn(|cx| conn.poll_next_inbound(cx)).await {
            Some(Ok(stream)) => Some(Ok(stream)),
            Some(Err(e)) => Some(Err(e)),
            None => None,
        }
    }

    /// Check if connection is healthy
    pub async fn is_healthy(&mut self) -> bool {
        let mut conn = Pin::new(&mut self.conn);
        futures::future::poll_fn(|cx| conn.poll_new_outbound(cx)).await.is_ok()
    }

    /// Close the connection
    pub fn close(&mut self) {
        // Yamux Connection doesn't have explicit close, dropping closes
    }
}