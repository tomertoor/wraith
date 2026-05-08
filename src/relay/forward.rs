use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::time::Duration;

use super::{RelayConfig, Transport};

use crate::relay::RELAY_BUFFER_SIZE;

pub(crate) async fn relay_bidirectional<R1, W1, R2, W2>(
    mut a_read: R1,
    mut a_write: W1,
    mut b_read: R2,
    mut b_write: W2,
    active: Arc<AtomicBool>,
) -> Result<()>
where
    R1: AsyncRead + Unpin,
    W1: AsyncWrite + Unpin,
    R2: AsyncRead + Unpin,
    W2: AsyncWrite + Unpin,
{
    let active_a = Arc::clone(&active);
    let active_b = Arc::clone(&active);

    let _ = tokio::join! {
        async move {
            let mut buf = [0u8; RELAY_BUFFER_SIZE];
            while active_a.load(Ordering::SeqCst) {
                match a_read.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if b_write.write_all(&buf[..n]).await.is_err() { break; }
                    }
                }
            }
        },
        async move {
            let mut buf = [0u8; RELAY_BUFFER_SIZE];
            while active_b.load(Ordering::SeqCst) {
                match b_read.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if a_write.write_all(&buf[..n]).await.is_err() { break; }
                    }
                }
            }
        },
    };
    Ok(())
}

pub(crate) async fn relay_stream_to_datagram(
    forward_addr: &str,
    inbound: TcpStream,
    active: Arc<AtomicBool>,
) -> Result<()> {
    let outbound = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
    let (ri, wi) = inbound.into_split();
    tokio::pin!(ri);
    tokio::pin!(wi);

    let active_in = Arc::clone(&active);
    let active_out = Arc::clone(&active);
    let forward_addr_owned = forward_addr.to_string();
    let outbound_in = Arc::clone(&outbound);
    let outbound_out = Arc::clone(&outbound);

    let _ = tokio::join! {
        async move {
            let mut buf = [0u8; RELAY_BUFFER_SIZE];
            loop {
                if !active_in.load(Ordering::SeqCst) { break; }
                match tokio::time::timeout(Duration::from_secs(1), ri.as_mut().read(&mut buf)).await {
                    Ok(Ok(0)) | Ok(Err(_)) => break,
                    Ok(Ok(n)) => {
                        if outbound_in.send_to(&buf[..n], &forward_addr_owned).await.is_err() { break; }
                    }
                    Err(_) => continue,
                }
            }
        },
        async move {
            let mut resp = [0u8; RELAY_BUFFER_SIZE];
            loop {
                if !active_out.load(Ordering::SeqCst) { break; }
                match tokio::time::timeout(Duration::from_secs(2), outbound_out.recv_from(&mut resp)).await {
                    Ok(Ok((m, _))) => {
                        if wi.as_mut().write_all(&resp[..m]).await.is_err() { break; }
                    }
                    Ok(Err(_)) => break,
                    Err(_) => continue,
                }
            }
        },
    };
    Ok(())
}

pub(crate) async fn relay_connection(config: &RelayConfig, inbound: TcpStream, active: Arc<AtomicBool>) -> Result<()> {
    let forward_addr = format!("{}:{}", config.forward.host, config.forward.port);

    match config.forward.protocol {
        Transport::Tcp => {
            let outbound = TcpStream::connect(&forward_addr).await?;
            let (a_read, a_write) = tokio::io::split(inbound);
            let (b_read, b_write) = tokio::io::split(outbound);
            relay_bidirectional(b_read, b_write, a_read, a_write, active).await?;
        }
        Transport::Udp => {
            relay_stream_to_datagram(&forward_addr, inbound, active).await?;
        }
    }
    Ok(())
}
