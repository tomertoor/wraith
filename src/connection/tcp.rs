use crate::connection::connection::Connection;
use crate::connection::framing::{read_frame_len, FramedWriter};
use crate::proto::wraith::WraithMessage;
use log::{error, info};
use prost::Message;
use std::io::{Error, ErrorKind, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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

        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;

        let len = u32::from_be_bytes([len_buf[0], len_buf[1], len_buf[2], len_buf[3]]) as usize;

        if len > 10 * 1024 * 1024 {
            return Err(Error::new(ErrorKind::InvalidData, "message too large"));
        }

        let mut data = vec![0u8; len];
        stream.read_exact(&mut data).await?;

        let msg = WraithMessage::decode(data.as_slice())?;
        Ok(msg)
    }
}

impl Connection for TcpConnection {
    fn init(&mut self) -> Result<()> {
        Ok(())
    }

    fn connect(&mut self) -> Result<()> {
        let addr = format!("{}:{}", self.host, self.port);
        info!("Connecting to {}", addr);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            match TokioTcpStream::connect(&addr).await {
                Ok(stream) => {
                    self.stream = Some(stream);
                    info!("Connected to {}", addr);
                    Ok(())
                }
                Err(e) => {
                    error!("Failed to connect to {}: {}", addr, e);
                    Err(e.into())
                }
            }
        })
    }

    fn listen(&mut self) -> Result<()> {
        let addr = format!("{}:{}", self.host, self.port);
        info!("Listening on {}", addr);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let listener = tokio::net::TcpListener::bind(&addr).await?;
            match listener.accept().await {
                Ok((stream, _)) => {
                    self.stream = Some(stream);
                    info!("Accepted connection");
                    Ok(())
                }
                Err(e) => Err(e.into()),
            }
        })
    }

    fn close(&mut self) -> Result<()> {
        if let Some(stream) = self.stream.take() {
            drop(stream);
        }
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.stream.is_some()
    }
}