use crate::message::codec::MessageCodec;
use crate::proto::wraith::WraithMessage;
use anyhow::Result;
use log::{debug, info, warn};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};
use tokio_util::sync::CancellationToken;
use yamux::{Config, Connection, Mode};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::io::{AsyncRead, AsyncReadExt};

#[derive(Clone)]
pub struct PeerSession {
    pub wraith_id: String,
    pub hostname: String,
    pub connection: Arc<tokio::sync::Mutex<yamux::Connection<Compat<TcpStream>>>>,
    pub command_tx: mpsc::Sender<WraithMessage>,
    cancel_token: CancellationToken,
}

impl PeerSession {
    pub fn new(
        wraith_id: String,
        hostname: String,
        connection: Arc<tokio::sync::Mutex<yamux::Connection<Compat<TcpStream>>>>,
        command_tx: mpsc::Sender<WraithMessage>,
    ) -> Self {
        Self {
            wraith_id,
            hostname,
            connection,
            command_tx,
            cancel_token: CancellationToken::new(),
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
        Ok(Self::new(wraith_id, hostname, Arc::new(tokio::sync::Mutex::new(connection)), command_tx))
    }

    /// Read a WraithMessage from a Yamux stream
    pub async fn read_message<R>(stream: &mut R) -> Result<Option<WraithMessage>>
    where
        R: AsyncRead + Unpin,
    {
        let mut length_buf = [0u8; 4];

        match stream.read_exact(&mut length_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(None);
            }
            Err(e) => {
                return Err(anyhow::anyhow!("read error: {}", e));
            }
        }

        let len = u32::from_be_bytes(length_buf) as usize;

        let mut data = vec![0u8; len];
        stream.read_exact(&mut data).await?;

        let msg = MessageCodec::decode(&data)?;
        Ok(Some(msg))
    }

    /// Write a WraithMessage to a Yamux stream
    pub async fn write_message<W>(
        writer: &mut W,
        msg: &WraithMessage,
    ) -> Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        let data = MessageCodec::encode(msg);
        debug!("write_message: encoding complete, {} bytes", data.len());

        let len = data.len() as u32;

        writer.write_all(&len.to_be_bytes()).await?;
        debug!("write_message: wrote length prefix");

        writer.write_all(&data).await?;
        debug!("write_message: wrote data payload");

        writer.flush().await?;
        debug!("write_message: flush complete");

        Ok(())
    }

    /// Send a command through the session's command channel
    pub async fn send_command(&self, msg: WraithMessage) -> Result<()> {
        self.command_tx.send(msg).await.map_err(|e| anyhow::anyhow!("send error: {}", e))?;
        Ok(())
    }

    /// Returns a reference to this session's cancellation token.
    /// Cancel the token to signal writer tasks to terminate.
    pub fn cancel_token(&self) -> &CancellationToken {
        &self.cancel_token
    }

    /// Spawn a background writer task that forwards messages from `rx` to `write_half`.
    /// The task is cancelled automatically when this session's cancellation token is fired.
    pub fn spawn_writer<W>(&self, write_half: W, mut rx: mpsc::Receiver<WraithMessage>)
    where
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let cancel = self.cancel_token.clone();
        let wraith_id = self.wraith_id.clone();
        tokio::spawn(async move {
            let mut writer = write_half;
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("Writer task cancelled for {}", wraith_id);
                }
                _ = async {
                    while let Some(msg) = rx.recv().await {
                        if let Err(e) = Self::write_message(&mut writer, &msg).await {
                            warn!("Failed to forward message: {}", e);
                            break;
                        }
                    }
                    info!("Writer task finished for {}", wraith_id);
                } => {}
            }
        });
    }
}
