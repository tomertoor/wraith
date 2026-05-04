#[cfg(test)]
mod codec_tests {
    use wraith::message::codec::MessageCodec;
    use wraith::proto::wraith::{MessageType, WraithMessage};
    use std::collections::HashMap;

    #[test]
    fn test_encode_decode_roundtrip() {
        let original = MessageCodec::create_command(
            "cmd-123".to_string(),
            "create_relay".to_string(),
            HashMap::new(),
            30,
            "target-wraith".to_string(),
        );

        let encoded = MessageCodec::encode(&original);
        let decoded = MessageCodec::decode(&encoded).unwrap();

        assert_eq!(decoded.message_id, original.message_id);
        assert_eq!(decoded.msg_type, original.msg_type);
        assert_eq!(decoded.target_wraith_id, original.target_wraith_id);
    }

    #[test]
    fn test_create_command_result() {
        let result = MessageCodec::create_command_result(
            "cmd-456".to_string(),
            "success".to_string(),
            "relay-id-789".to_string(),
            0,
            100,
            "".to_string(),
        );

        assert_eq!(result.msg_type, MessageType::CommandResult as i32);
        assert!(result.payload.is_some());
    }

    #[test]
    fn test_create_registration() {
        let reg = MessageCodec::create_registration(
            "test-host".to_string(),
            "test-user".to_string(),
            "linux".to_string(),
            "192.168.1.1".to_string(),
        );

        assert_eq!(reg.msg_type, MessageType::Registration as i32);
        assert!(!reg.message_id.is_empty());
        assert!(reg.timestamp > 0);
    }
}

#[cfg(test)]
mod relay_tests {
    use wraith::relay::{Transport, RelayEndpoint, RelayConfig, RelayManager};
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_transport_from_str() {
        assert_eq!(Transport::from_str("tcp"), Transport::Tcp);
        assert_eq!(Transport::from_str("UDP"), Transport::Udp);
        assert_eq!(Transport::from_str("unknown"), Transport::Tcp); // default
        assert_eq!(Transport::from_str(""), Transport::Tcp);
    }

    #[test]
    fn test_transport_display() {
        assert_eq!(Transport::Tcp.to_string(), "tcp");
        assert_eq!(Transport::Udp.to_string(), "udp");
    }

    #[test]
    fn test_relay_endpoint_from_str() {
        let ep = RelayEndpoint::from_str("127.0.0.1", 8080, "tcp");
        assert_eq!(ep.host, "127.0.0.1");
        assert_eq!(ep.port, 8080);
        assert_eq!(ep.protocol, Transport::Tcp);

        let ep_udp = RelayEndpoint::from_str("localhost", 9999, "udp");
        assert_eq!(ep_udp.protocol, Transport::Udp);
    }

    #[test]
    fn test_relay_config_new() {
        let listen = RelayEndpoint::new("0.0.0.0".to_string(), 6666, Transport::Tcp);
        let forward = RelayEndpoint::new("10.0.0.1".to_string(), 443, Transport::Tcp);
        let config = RelayConfig::new(listen.clone(), forward.clone());

        assert_eq!(config.listen.host, "0.0.0.0");
        assert_eq!(config.listen.port, 6666);
        assert_eq!(config.forward.host, "10.0.0.1");
        assert_eq!(config.forward.port, 443);
    }

    #[tokio::test]
    async fn test_relay_manager_create_and_list() {
        let mut manager = RelayManager::new();
        let relay_id = manager.create_relay(RelayConfig::new(
            RelayEndpoint::from_str("127.0.0.1", 16666, "tcp"),
            RelayEndpoint::from_str("127.0.0.1", 17777, "tcp"),
        ));

        assert!(!relay_id.is_empty());
        let relays = manager.list_relays();
        assert_eq!(relays.len(), 1);
        assert_eq!(relays[0].relay_id, relay_id);
    }

    #[tokio::test]
    async fn test_relay_manager_delete() {
        let mut manager = RelayManager::new();
        let relay_id = manager.create_relay(RelayConfig::new(
            RelayEndpoint::from_str("127.0.0.1", 18888, "tcp"),
            RelayEndpoint::from_str("127.0.0.1", 19999, "tcp"),
        ));

        assert!(manager.delete_relay(&relay_id));
        assert!(!manager.delete_relay("non-existent-id")); // double delete returns false
        assert!(manager.list_relays().is_empty());
    }

    #[tokio::test]
    async fn test_relay_manager_multiple_relays() {
        let mut manager = RelayManager::new();

        let id1 = manager.create_relay(RelayConfig::new(
            RelayEndpoint::from_str("127.0.0.1", 20001, "tcp"),
            RelayEndpoint::from_str("127.0.0.1", 20002, "tcp"),
        ));

        let id2 = manager.create_relay(RelayConfig::new(
            RelayEndpoint::from_str("127.0.0.1", 20003, "tcp"),
            RelayEndpoint::from_str("127.0.0.1", 20004, "tcp"),
        ));

        assert_ne!(id1, id2);
        assert_eq!(manager.list_relays().len(), 2);

        manager.delete_relay(&id1);
        assert_eq!(manager.list_relays().len(), 1);

        manager.delete_relay(&id2);
        assert!(manager.list_relays().is_empty());
    }
}

#[cfg(test)]
mod relay_commands_tests {
    use wraith::commands::relay::RelayCommands;
    use wraith::relay::{RelayManager, RelayConfig, RelayEndpoint};
    use wraith::proto::wraith::Command as ProtoCommand;
    use std::sync::{Arc, Mutex};
    use std::collections::HashMap;

    fn make_relay_commands() -> RelayCommands {
        let relay_manager = Arc::new(Mutex::new(RelayManager::new()));
        RelayCommands::new_without_tunnel(relay_manager)
    }

    #[tokio::test]
    async fn test_handle_create_relay_legacy_format() {
        let cmd = make_relay_commands();

        let mut params = HashMap::new();
        params.insert("listen_host".to_string(), "127.0.0.1".to_string());
        params.insert("listen_port".to_string(), "25555".to_string());
        params.insert("forward_host".to_string(), "127.0.0.1".to_string());
        params.insert("forward_port".to_string(), "25556".to_string());
        params.insert("listen_protocol".to_string(), "tcp".to_string());

        let proto_cmd = ProtoCommand {
            command_id: "cmd-1".to_string(),
            action: "create_relay".to_string(),
            params,
            timeout: 30,
        };

        let result = cmd.handle_create_relay(&proto_cmd, "");
        assert_eq!(result.status, "success");
        assert!(!result.output.is_empty()); // relay_id
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_handle_create_relay_hop_format() {
        let cmd = make_relay_commands();

        let mut params = HashMap::new();
        params.insert("hop_0_listen_host".to_string(), "127.0.0.1".to_string());
        params.insert("hop_0_listen_port".to_string(), "26666".to_string());
        params.insert("hop_0_forward_host".to_string(), "127.0.0.1".to_string());
        params.insert("hop_0_forward_port".to_string(), "26667".to_string());
        params.insert("hop_0_protocol".to_string(), "tcp".to_string());

        let proto_cmd = ProtoCommand {
            command_id: "cmd-2".to_string(),
            action: "create_relay".to_string(),
            params,
            timeout: 30,
        };

        let result = cmd.handle_create_relay(&proto_cmd, "");
        assert_eq!(result.status, "success");
        assert!(!result.output.is_empty());
    }

    #[test]
    fn test_handle_create_relay_invalid_missing_hops() {
        let cmd = make_relay_commands();

        // No hops, no legacy format
        let proto_cmd = ProtoCommand {
            command_id: "cmd-3".to_string(),
            action: "create_relay".to_string(),
            params: HashMap::new(),
            timeout: 30,
        };

        let result = cmd.handle_create_relay(&proto_cmd, "");
        assert_eq!(result.status, "error");
        assert!(result.exit_code != 0);
    }

    #[tokio::test]
    async fn test_handle_delete_relay() {
        let relay_manager = Arc::new(Mutex::new(RelayManager::new()));
        let relay_id = {
            let mut m = relay_manager.lock().unwrap();
            m.create_relay(RelayConfig::new(
                RelayEndpoint::from_str("127.0.0.1", 27777, "tcp"),
                RelayEndpoint::from_str("127.0.0.1", 27778, "tcp"),
            ))
        };

        let cmd = RelayCommands::new_without_tunnel(relay_manager);

        let params = HashMap::from([("relay_id".to_string(), relay_id.clone())]);
        let proto_cmd = ProtoCommand {
            command_id: "cmd-del".to_string(),
            action: "delete_relay".to_string(),
            params,
            timeout: 30,
        };

        let result = cmd.handle_delete_relay(&proto_cmd);
        assert_eq!(result.status, "success");
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn test_handle_delete_relay_not_found() {
        let cmd = make_relay_commands();

        let params = HashMap::from([("relay_id".to_string(), "non-existent".to_string())]);
        let proto_cmd = ProtoCommand {
            command_id: "cmd-del-2".to_string(),
            action: "delete_relay".to_string(),
            params,
            timeout: 30,
        };

        let result = cmd.handle_delete_relay(&proto_cmd);
        assert_eq!(result.status, "not_found");
        assert_eq!(result.exit_code, -1);
    }

    #[test]
    fn test_handle_list_relays_empty() {
        let cmd = make_relay_commands();

        let proto_cmd = ProtoCommand {
            command_id: "cmd-list".to_string(),
            action: "list_relays".to_string(),
            params: HashMap::new(),
            timeout: 30,
        };

        let result = cmd.handle_list_relays(&proto_cmd);
        assert_eq!(result.status, "success");
        assert_eq!(result.exit_code, 0);
        // Empty array
        assert_eq!(result.output, "[]");
    }
}

#[cfg(test)]
mod agent_commands_tests {
    use wraith::commands::agent::AgentCommands;
    use wraith::wraith::tunnel::TunnelManager;
    use wraith::wraith::state::WraithState;
    use wraith::proto::wraith::Command as ProtoCommand;
    use std::sync::{Arc, Mutex};
    use std::collections::HashMap;

    fn make_agent_commands() -> AgentCommands {
        AgentCommands::new_without_tunnel()
    }

    fn make_state() -> WraithState {
        WraithState::new()
    }

    #[test]
    fn test_handle_set_id() {
        let cmd = make_agent_commands();
        let state = &mut make_state();
        let original_id = state.wraith_id.clone();

        let mut params = HashMap::new();
        params.insert("wraith_id".to_string(), "my-custom-id".to_string());

        let proto_cmd = ProtoCommand {
            command_id: "cmd-setid".to_string(),
            action: "set_id".to_string(),
            params,
            timeout: 30,
        };

        let result = cmd.handle_set_id(&proto_cmd, state);
        assert_eq!(result.status, "success");
        assert_eq!(result.output, "my-custom-id");
        assert_eq!(state.wraith_id, "my-custom-id");

        // Can reset
        state.set_wraith_id(original_id);
    }

    #[test]
    fn test_handle_set_id_missing_param() {
        let cmd = make_agent_commands();
        let state = &mut make_state();

        let proto_cmd = ProtoCommand {
            command_id: "cmd-setid-2".to_string(),
            action: "set_id".to_string(),
            params: HashMap::new(), // missing wraith_id
            timeout: 30,
        };

        let result = cmd.handle_set_id(&proto_cmd, state);
        assert_eq!(result.status, "error");
        assert!(result.exit_code != 0);
        assert!(result.error.contains("wraith_id"));
    }

    #[test]
    fn test_handle_list_peers_empty() {
        let cmd = make_agent_commands();
        let state = make_state();

        let proto_cmd = ProtoCommand {
            command_id: "cmd-peers".to_string(),
            action: "list_peers".to_string(),
            params: HashMap::new(),
            timeout: 30,
        };

        let result = cmd.handle_list_peers(&proto_cmd, &state);
        assert_eq!(result.status, "success");

        // Should contain empty peers array
        assert!(result.output.contains("\"peers\":[]"));
    }

    #[test]
    fn test_handle_list_peers_with_peers() {
        use wraith::wraith::state::PeerConnection;
        use tokio::sync::mpsc;

        let cmd = make_agent_commands();
        let mut state = make_state();

        let (tx, _rx) = mpsc::channel(10);
        state.add_peer("peer-1".to_string(), "peer-host-1".to_string(), tx);

        let (tx2, _rx2) = mpsc::channel(10);
        state.add_peer("peer-2".to_string(), "peer-host-2".to_string(), tx2);

        let proto_cmd = ProtoCommand {
            command_id: "cmd-peers-2".to_string(),
            action: "list_peers".to_string(),
            params: HashMap::new(),
            timeout: 30,
        };

        let result = cmd.handle_list_peers(&proto_cmd, &state);
        assert_eq!(result.status, "success");
        assert!(result.output.contains("\"peers\":["));
        assert!(result.output.contains("peer-1"));
        assert!(result.output.contains("peer-2"));
    }
}

#[cfg(test)]
mod tunnel_manager_tests {
    use wraith::wraith::tunnel::{TunnelManager, PeerSession};
    use wraith::commands::relay::RelayCommands;
    use wraith::commands::agent::AgentCommands;
    use wraith::relay::RelayManager;
    use std::sync::{Arc, Mutex};

    fn make_tunnel_manager() -> Arc<TunnelManager> {
        let relay_manager = Arc::new(Mutex::new(RelayManager::new()));
        let relay_commands = RelayCommands::new_without_tunnel(relay_manager);
        let agent_commands = AgentCommands::new_without_tunnel();

        Arc::new(TunnelManager::with_commands(relay_commands, agent_commands))
    }

    #[tokio::test]
    async fn test_tunnel_manager_session_lifecycle() {
        let manager = make_tunnel_manager();

        // Initially empty
        assert!(manager.list_sessions().await.is_empty());
        assert!(manager.get_all_session_ids().await.is_empty());

        // Add a session directly via add_session
        // We can't easily create a real PeerSession without network setup,
        // but we can verify the manager's async methods work
        let session_ids = manager.get_all_session_ids().await;
        assert!(session_ids.is_empty());
    }

    #[tokio::test]
    async fn test_tunnel_manager_get_session_nonexistent() {
        let manager = make_tunnel_manager();

        let session = manager.get_session("non-existent").await;
        assert!(session.is_none());
    }

    #[tokio::test]
    async fn test_tunnel_manager_remove_session_empty() {
        let manager = make_tunnel_manager();

        // Removing non-existent session should not panic
        manager.remove_session("non-existent").await;
    }

    #[tokio::test]
    async fn test_tunnel_manager_set_state() {
        use wraith::wraith::state::WraithState;

        let manager = make_tunnel_manager();
        let state = Arc::new(Mutex::new(WraithState::new()));

        manager.set_state(state);
    }
}

#[cfg(test)]
mod integration_tests {
    use wraith::wraith::state::WraithState;
    use wraith::message::codec::MessageCodec;
    use wraith::relay::{RelayManager, RelayConfig, RelayEndpoint, Transport};
    use std::sync::{Arc, Mutex};
    use std::collections::HashMap;

    #[test]
    fn test_state_commands_tracking() {
        let mut state = WraithState::new();
        assert_eq!(state.commands_executed, 0);

        state.increment_commands();
        state.increment_commands();
        state.increment_commands();

        assert_eq!(state.commands_executed, 3);
        assert!(state.last_command_time > 0);
    }

    #[test]
    fn test_pending_response_lifecycle() {
        let state = WraithState::new();
        let msg_id = "pending-test-123".to_string();

        // Initially no pending response
        assert!(state.take_pending_response(&msg_id).is_none());

        // Register a pending response (using a oneshot channel)
        let (tx, rx) = tokio::sync::oneshot::channel();
        state.register_pending_response(msg_id.clone(), tx);

        // Can retrieve it
        let retrieved = state.take_pending_response(&msg_id);
        assert!(retrieved.is_some());

        // After taking, it's gone
        assert!(state.take_pending_response(&msg_id).is_none());
    }

    #[test]
    fn test_message_loop_prevention_tracking() {
        let state = WraithState::new();
        let msg_id = "msg-duplicate-test";

        // First time should not be seen
        assert!(!state.has_seen_message(msg_id));

        // Mark as seen
        state.mark_message_seen(msg_id.to_string());

        // Now should be seen
        assert!(state.has_seen_message(msg_id));

        // Different message should not be seen
        assert!(!state.has_seen_message("different-msg"));
    }

    #[test]
    fn test_relay_endpoint_udp() {
        let ep = RelayEndpoint::from_str("10.0.0.1", 53, "udp");
        assert_eq!(ep.protocol, Transport::Udp);
        assert_eq!(ep.port, 53);
    }

    #[tokio::test]
    async fn test_relay_manager_udp_relay() {
        let mut manager = RelayManager::new();

        let relay_id = manager.create_relay(RelayConfig::new(
            RelayEndpoint::from_str("127.0.0.1", 30001, "udp"),
            RelayEndpoint::from_str("8.8.8.8", 53, "udp"),
        ));

        assert!(!relay_id.is_empty());
        let relays = manager.list_relays();
        assert_eq!(relays.len(), 1);
        assert!(relays[0].protocol.contains("udp"));
    }
}