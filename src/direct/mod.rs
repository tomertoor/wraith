use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DirectMessage {
    #[serde(rename = "command")]
    Command {
        id: String,
        action: String,
        params: serde_json::Value,
    },
    #[serde(rename = "response")]
    Response {
        id: String,
        result: Option<serde_json::Value>,
        error: Option<String>,
    },
    #[serde(rename = "event")]
    Event {
        action: String,
        data: serde_json::Value,
    },
}

pub async fn run_direct() -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();

    let mut line = String::new();
    while stdin.read_line(&mut line).await? > 0 {
        line.pop(); // remove trailing newline
        let msg: DirectMessage = serde_json::from_str(&line).unwrap_or_else(|_| {
            DirectMessage::Response {
                id: String::new(),
                result: None,
                error: Some("Invalid JSON".to_string()),
            }
        });
        let response = handle_message(msg).await;
        let out = serde_json::to_string(&response)?;
        stdout.write_all(out.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
        line.clear();
    }
    Ok(())
}

async fn handle_message(msg: DirectMessage) -> DirectMessage {
    match msg {
        DirectMessage::Command { id, action, params: _ } => {
            let result = match action.as_str() {
                "node_info" => serde_json::json!({
                    "node_id": uuid::Uuid::new_v4().to_string(),
                    "version": "0.2.0",
                    "tun_capable": true,
                    "supports_relay": true,
                    "supports_nexus": true
                }),
                "listen" => serde_json::json!({ "status": "listening" }),
                "connect" => serde_json::json!({ "status": "connected" }),
                "tunnel_open" => serde_json::json!({ "status": "ok" }),
                "tunnel_close" => serde_json::json!({ "status": "ok" }),
                "relay_open" => serde_json::json!({ "status": "ok" }),
                "relay_close" => serde_json::json!({ "status": "ok" }),
                "status" => serde_json::json!({ "connections": [], "tunnels": [], "relays": [] }),
                _ => serde_json::json!({ "error": "unknown action" }),
            };
            DirectMessage::Response {
                id,
                result: Some(result),
                error: None,
            }
        }
        _ => DirectMessage::Response {
            id: String::new(),
            result: None,
            error: Some("not a command".to_string()),
        },
    }
}
