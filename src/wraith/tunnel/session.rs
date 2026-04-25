use crate::message::codec::MessageCodec;
use crate::proto::wraith::WraithMessage;
use anyhow::Result;
use futures::io::{AsyncReadExt, AsyncWriteExt};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};
use yamux::{Config, Connection, Mode, Stream};

#[derive(Clone)]
pub struct PeerSession {
    pub wraith_id: String,
    pub hostname: String,
    pub connection: Arc<tokio::sync::Mutex<Connection<Compat<TcpStream>>>>,
    pub command_tx: mpsc::Sender<WraithMessage>,
}

impl PeerSession {
    pub fn new(
        wraith_id: String,
        hostname: String,
        connection: Connection<Compat<TcpStream>>,
        command_tx: mpsc::Sender<WraithMessage>,
    ) -> Self {
        Self {
            wraith_id,
            hostname,
            connection: Arc::new(tokio::sync::Mutex::new(connection)),
            command_tx,
        }
    }

    /// Create a new peer session from a raw TcpStream
    pub async fn from_tcpstream(
        wraith_id: String,
        hostname: String,
        stream: TcpStream,
        command_tx: mpsc::Sender<WraithMessage>,
    ) -> Result<Self> {
        let config = Config::default();
        let connection = Connection::new(stream.compat(), config, Mode::Server);
        Ok(Self::new(wraith_id, hostname, connection, command_tx))
    }

    /// Read a WraithMessage from a Yamux stream
    pub async fn read_message(stream: &mut Stream) -> Result<Option<WraithMessage>> {
        let mut length_buf = [0u8; 4];
        match stream.read_exact(&mut length_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(anyhow::anyhow!("read error: {}", e)),
        }
        let len = u32::from_be_bytes(length_buf) as usize;

        let mut data = vec![0u8; len];
        stream.read_exact(&mut data).await?;

        let msg = MessageCodec::decode(&data)?;
        Ok(Some(msg))
    }

    /// Write a WraithMessage to a Yamux stream
    pub async fn write_message(stream: &mut Stream, msg: &WraithMessage) -> Result<()> {
        let data = MessageCodec::encode(msg);
        let len = data.len() as u32;
        stream.write_all(&len.to_be_bytes()).await?;
        stream.write_all(&data).await?;
        stream.flush().await?;
        Ok(())
    }

    /// Send a command through the session's command channel
    pub async fn send_command(&self, msg: WraithMessage) -> Result<()> {
        self.command_tx.send(msg).await.map_err(|e| anyhow::anyhow!("send error: {}", e))?;
        Ok(())
    }
}