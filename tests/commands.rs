#[cfg(test)]
mod command_tests {
    use wraith::wraith::state::WraithState;

    #[test]
    fn test_state_increment_commands() {
        let mut state = WraithState::new();
        assert_eq!(state.commands_executed, 0);
        assert_eq!(state.last_command_time, 0);

        state.increment_commands();

        assert_eq!(state.commands_executed, 1);
        assert!(state.last_command_time > 0);
    }

    #[test]
    fn test_state_set_connected() {
        let mut state = WraithState::new();
        assert!(!state.connected);

        state.set_connected(true);
        assert!(state.connected);

        state.set_connected(false);
        assert!(!state.connected);
    }

    #[test]
    fn test_state_system_info() {
        let state = WraithState::new();
        // System info should be populated
        assert!(!state.hostname.is_empty());
        assert!(!state.username.is_empty());
        assert!(!state.os.is_empty());
        // IP address defaults to 0.0.0.0
        assert_eq!(state.ip_address, "0.0.0.0");
    }

    #[test]
    fn test_state_new_with_relay_manager() {
        use wraith::relay::RelayManager;
        use std::sync::{Arc, Mutex};

        let relay_manager = Arc::new(Mutex::new(RelayManager::new()));
        let state = WraithState::new_with_relay_manager("test-wraith-id".to_string(), relay_manager.clone());

        // State should be initialized with the provided relay manager
        assert!(state.relay_manager.lock().is_ok());
    }
}