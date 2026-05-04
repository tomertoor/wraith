use crate::connection::framing::FramedWriter;
use crate::message::codec::MessageCodec;
use crate::proto::wraith::WraithMessage;
use prost::Message;
use std::io::{Error, ErrorKind, Result};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream as TokioTcpStream;

pub struct TcpConnection {
    host: String,
    port: u16,
    stream: Option<TokioTcpStream>,
    is_server: bool,
}

impl TcpConnection {
    pub fn new(host: String, port: u16, is_server: bool) -> Self {
        Self {
            host,
            port,
            stream: None,
            is_server,
        }
    }

    /// Create a TcpConnection from an existing stream
    pub fn from_stream(stream: TokioTcpStream) -> Self {
        Self {
            host: String::new(),
            port: 0,
            stream: Some(stream),
            is_server: false,
        }
    }

    pub fn is_server(&self) -> bool {
        self.is_server
    }

    pub async fn send_message(&mut self, msg: &WraithMessage) -> Result<()> {
        let stream = self.stream.as_mut().ok_or_else(|| {
            Error::new(ErrorKind::NotConnected, "not connected")
        })?;

        let data = msg.encode_to_vec();
        let framed = FramedWriter::write_frame(&data)?;

        stream.write_all(&framed).await?;
        Ok(())
    }

    pub async fn read_message(&mut self) -> Result<WraithMessage> {
        let stream = self.stream.as_mut().ok_or_else(|| {
            Error::new(ErrorKind::NotConnected, "not connected")
        })?;

        MessageCodec::read_framed_message(stream).await
    }

    pub async fn connect(&mut self) -> Result<()> {
        let addr = format!("{}:{}", self.host, self.port);
        log::info!("Connecting to {}", addr);

        match TokioTcpStream::connect(&addr).await {
            Ok(stream) => {
                self.stream = Some(stream);
                log::info!("Connected to {}", addr);
                Ok(())
            }
            Err(e) => {
                log::error!("Failed to connect to {}: {}", addr, e);
                Err(e.into())
            }
        }
    }

    pub async fn listen(&mut self) -> Result<()> {
        let addr = format!("{}:{}", self.host, self.port);
        log::info!("Listening on {}", addr);

        let listener = tokio::net::TcpListener::bind(&addr).await?;
        match listener.accept().await {
            Ok((stream, _)) => {
                self.stream = Some(stream);
                log::info!("Accepted connection");
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn close(&mut self) -> Result<()> {
        if let Some(stream) = self.stream.take() {
            drop(stream);
        }
        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }
}

