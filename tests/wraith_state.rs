#[cfg(test)]
mod tests {
    use wraith::wraith::state::WraithState;

    #[test]
    fn test_wraith_id_generation() {
        let state = WraithState::new();
        assert!(!state.wraith_id.is_empty());
        // Should be valid UUID format
        assert!(uuid::Uuid::parse_str(&state.wraith_id).is_ok());
    }

    #[test]
    fn test_peer_table_empty() {
        let state = WraithState::new();
        assert!(state.peer_table.is_empty());
    }

    #[test]
    fn test_message_loop_prevention() {
        let state = WraithState::new();
        let msg_id = "test-123";

        assert!(!state.has_seen_message(msg_id));
        state.mark_message_seen(msg_id.to_string());
        assert!(state.has_seen_message(msg_id));
    }

    #[test]
    fn test_set_wraith_id() {
        let mut state = WraithState::new();
        let original_id = state.wraith_id.clone();

        state.set_wraith_id("custom-id".to_string());
        assert_eq!(state.wraith_id, "custom-id");

        // Can change back
        state.set_wraith_id("another-id".to_string());
        assert_eq!(state.wraith_id, "another-id");
    }

    #[test]
    fn test_peer_connection_add_remove() {
        let mut state = WraithState::new();
        use wraith::wraith::state::PeerConnection;
        use tokio::sync::mpsc;

        let (tx, _rx) = mpsc::channel(10);

        // Add peer
        state.add_peer("peer1".to_string(), "peer-hostname".to_string(), tx);
        assert_eq!(state.peer_table.len(), 1);
        assert!(state.peer_table.contains_key("peer1"));

        // Remove peer
        let removed = state.remove_peer("peer1");
        assert!(removed.is_some());
        assert!(state.peer_table.is_empty());
    }
}