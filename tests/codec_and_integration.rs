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
        assert_eq!(result.status, "error");
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
    use wraith::wraith::session::TunnelManager;
    use wraith::wraith::state::WraithState;
    use wraith::proto::wraith::Command as ProtoCommand;
    use std::sync::{Arc, Mutex};
    use std::collections::HashMap;

    fn make_agent_commands() -> AgentCommands {
        AgentCommands::new_without_tunnel()
    }

    fn make_state() -> WraithState {
        WraithState::default()
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
    use wraith::wraith::session::{TunnelManager, PeerSession};
    use wraith::commands::relay::RelayCommands;
    use wraith::commands::agent::AgentCommands;
    use wraith::wraith::state::WraithState;
    use wraith::relay::RelayManager;
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc;

    fn make_tunnel_manager() -> Arc<TunnelManager> {
        let relay_manager = Arc::new(Mutex::new(RelayManager::new()));
        let state = Arc::new(Mutex::new(WraithState::new("test".to_string(), Arc::clone(&relay_manager))));
        let (tx, _rx) = mpsc::channel(100);
        let manager = Arc::new(TunnelManager::new(state, tx));
        let relay_commands = RelayCommands::new(Arc::clone(&relay_manager), Arc::clone(&manager));
        let agent_commands = AgentCommands::new(Arc::clone(&manager));
        manager.set_commands(relay_commands, agent_commands);
        manager
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
    async fn test_tunnel_manager_new_with_state() {
        // TunnelManager::new now requires state and peer event channel at construction time
        let relay_manager = Arc::new(Mutex::new(RelayManager::new()));
        let state = Arc::new(Mutex::new(WraithState::new("test-state".to_string(), relay_manager)));
        let (tx, _rx) = mpsc::channel(100);
        let manager = TunnelManager::new(state, tx);

        // Verify basic functionality works
        assert!(manager.list_sessions().await.is_empty());
    }
}

#[cfg(test)]
mod integration_tests {
    use wraith::wraith::state::WraithState;
    use wraith::message::codec::MessageCodec;
    use wraith::relay::{RelayManager, RelayConfig, RelayEndpoint, Transport};
    use std::sync::{Arc, Mutex};
    use std::collections::HashMap;

    fn make_state() -> WraithState {
        WraithState::default()
    }

    #[test]
    fn test_state_commands_tracking() {
        let mut state = make_state();
        assert_eq!(state.commands_executed, 0);

        state.increment_commands();
        state.increment_commands();
        state.increment_commands();

        assert_eq!(state.commands_executed, 3);
        assert!(state.last_command_time > 0);
    }

    #[test]
    fn test_pending_response_lifecycle() {
        let state = make_state();
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
        let state = make_state();
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

#[cfg(test)]
mod peer_routing_tests {
    use wraith::wraith::session::{TunnelManager, PeerEvent};
    use wraith::wraith::state::WraithState;
    use wraith::commands::relay::RelayCommands;
    use wraith::commands::agent::AgentCommands;
    use wraith::relay::RelayManager;
    use wraith::message::codec::MessageCodec;
    use std::sync::{Arc, Mutex};
    use std::collections::HashMap;
    use tokio::sync::mpsc;

    /// Create a fully initialized TunnelManager with the given wraith_id.
    /// Processes PeerEvent updates in the background so peer_table stays in sync.
    fn make_wraith(wraith_id: &str) -> (Arc<TunnelManager>, Arc<Mutex<WraithState>>) {
        let relay_manager = Arc::new(Mutex::new(RelayManager::new()));
        let state = Arc::new(Mutex::new(WraithState::new(
            wraith_id.to_string(),
            Arc::clone(&relay_manager),
        )));
        let (peer_event_tx, mut peer_event_rx) = mpsc::channel::<PeerEvent>(100);
        let manager = Arc::new(TunnelManager::new(Arc::clone(&state), peer_event_tx));

        let relay_cmds = RelayCommands::new(Arc::clone(&relay_manager), Arc::clone(&manager));
        let agent_cmds = AgentCommands::new(Arc::clone(&manager));
        manager.set_commands(relay_cmds, agent_cmds);

        // Background task: process PeerEvent → state.peer_table
        let state_events = Arc::clone(&state);
        tokio::spawn(async move {
            while let Some(event) = peer_event_rx.recv().await {
                match event {
                    PeerEvent::Added { wraith_id, hostname, sender } => {
                        state_events.lock().expect("state lock").add_peer(wraith_id, hostname, sender);
                    }
                    PeerEvent::Removed { wraith_id } => {
                        state_events.lock().expect("state lock").remove_peer(&wraith_id);
                    }
                }
            }
        });

        (manager, state)
    }

    /// End-to-end test: C2 → Wraith A (ID "2") → Wraith B (ID "5")
    ///
    /// Sets up two wraith instances connected via TCP+Yamux. Wraith A (client, ID "2")
    /// connects to Wraith B (server, ID "5"). After registration exchange, Wraith A
    /// knows about peer "5" and Wraith B knows about peer "2".
    ///
    /// Verifies that a list_relays command targeted at "5" gets routed from A to B
    /// and the response comes back.
    #[tokio::test]
    async fn test_peer_routing_list_relays_through_chain() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let peer_addr = listener.local_addr().unwrap();

        // Create Wraith A (ID "2") and Wraith B (ID "5")
        let (manager_a, state_a) = make_wraith("2");
        let (manager_b, state_b) = make_wraith("5");

        // Wraith B accepts a peer connection
        let manager_b_clone = Arc::clone(&manager_b);
        let state_b_clone = Arc::clone(&state_b);
        let accept_handle = tokio::spawn(async move {
            let (stream, _addr) = listener.accept().await.unwrap();
            TunnelManager::handle_peer_connection(stream, state_b_clone, manager_b_clone)
                .await
                .unwrap();
        });

        // Wraith A connects to Wraith B, sending its local ID "2"
        manager_a
            .clone()
            .connect_to_peer(
                format!("127.0.0.1:{}", peer_addr.port()),
                "2".to_string(),       // local_wraith_id
                "host-a".to_string(),  // local_hostname
                "linux".to_string(),   // local_os
            )
            .await
            .unwrap();

        // Give time for peer event processing
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        // Verify Wraith A now knows about peer "5" (Wraith B's ID)
        {
            let sa = state_a.lock().expect("state lock");
            assert!(sa.peer_table.contains_key("5"),
                "Wraith A should have peer '5' in peer_table, got: {:?}",
                sa.peer_table.keys().collect::<Vec<_>>());
        }

        // Verify Wraith B knows about peer "2" (Wraith A's ID)
        {
            let sb = state_b.lock().expect("state lock");
            assert!(sb.peer_table.contains_key("2"),
                "Wraith B should have peer '2' in peer_table, got: {:?}",
                sb.peer_table.keys().collect::<Vec<_>>());
        }

        // Route a list_relays command from Wraith A targeting Wraith B (ID "5")
        let cmd = MessageCodec::create_command(
            "cmd-list-relays-1".to_string(),
            "list_relays".to_string(),
            HashMap::new(),
            30,
            "5".to_string(), // target_wraith_id = Wraith B
        );

        let response = manager_a.route_message(cmd).await;

        assert!(response.is_some(), "Should receive a response from Wraith B");

        let resp = response.unwrap();
        assert_eq!(resp.msg_type, wraith::proto::wraith::MessageType::CommandResult as i32);

        if let Some(wraith::proto::wraith::wraith_message::Payload::Result(result)) = &resp.payload {
            assert_eq!(result.status, "success", "list_relays should succeed on Wraith B, got: {}", result.error);
            assert_eq!(result.output, "[]", "Wraith B should have no relays");
        } else {
            panic!("Expected CommandResult payload, got {:?}", resp.payload);
        }

        accept_handle.abort();
    }

    /// Test that a command targeted at the local wraith is dispatched locally
    /// without being forwarded to any peer.
    #[tokio::test]
    async fn test_local_dispatch_no_forward() {
        let (manager_a, state_a) = make_wraith("2");

        // Create a list_relays command targeted at wraith "2" itself
        let cmd = MessageCodec::create_command(
            "cmd-local-1".to_string(),
            "list_relays".to_string(),
            HashMap::new(),
            30,
            "2".to_string(), // target = self
        );

        let response = manager_a.route_message(cmd).await;
        assert!(response.is_some());

        let resp = response.unwrap();
        if let Some(wraith::proto::wraith::wraith_message::Payload::Result(result)) = &resp.payload {
            assert_eq!(result.status, "success");
        } else {
            panic!("Expected CommandResult payload");
        }
    }

    /// Test that a relay created on Wraith A doesn't appear when listing relays
    /// on Wraith B through peer routing.
    #[tokio::test]
    async fn test_peer_routing_relay_isolation() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let peer_addr = listener.local_addr().unwrap();

        let (manager_a, state_a) = make_wraith("2");
        let (manager_b, state_b) = make_wraith("5");

        // Create a relay on Wraith A directly (not through routing)
        {
            let rm = state_a.lock().expect("state lock").relay_manager.clone();
            let relay_id = rm.lock().unwrap().create_relay(
                wraith::relay::RelayConfig::new(
                    wraith::relay::RelayEndpoint::from_str("127.0.0.1", 40001, "tcp"),
                    wraith::relay::RelayEndpoint::from_str("127.0.0.1", 40002, "tcp"),
                ),
            );
            assert!(!relay_id.is_empty());
        }

        // Wraith B accepts peer
        let manager_b_clone = Arc::clone(&manager_b);
        let state_b_clone = Arc::clone(&state_b);
        let accept_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            TunnelManager::handle_peer_connection(stream, state_b_clone, manager_b_clone)
                .await
                .unwrap();
        });

        // Connect A → B (A sends local ID "2")
        manager_a.clone()
            .connect_to_peer(
                format!("127.0.0.1:{}", peer_addr.port()),
                "2".to_string(),
                "host-a".to_string(),
                "linux".to_string(),
            )
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        // List relays on Wraith B (ID "5") via routing — should be empty
        let cmd = MessageCodec::create_command(
            "cmd-isolation-1".to_string(),
            "list_relays".to_string(),
            HashMap::new(),
            30,
            "5".to_string(), // target Wraith B
        );

        let response = manager_a.route_message(cmd).await;
        assert!(response.is_some());

        let resp = response.unwrap();
        if let Some(wraith::proto::wraith::wraith_message::Payload::Result(result)) = &resp.payload {
            assert_eq!(result.status, "success");
            assert_eq!(result.output, "[]", "Wraith B should have no relays (relay on A shouldn't leak)");
        } else {
            panic!("Expected CommandResult payload");
        }

        accept_handle.abort();
    }
}