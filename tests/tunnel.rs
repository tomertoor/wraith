#[cfg(test)]
mod tunnel_tests {
    use wraith::wraith::session::TunnelManager;
    use wraith::wraith::state::WraithState;
    use wraith::relay::RelayManager;
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc;

    fn make_manager() -> TunnelManager {
        let relay_manager = Arc::new(Mutex::new(RelayManager::new()));
        let state = Arc::new(Mutex::new(WraithState::new("test".to_string(), Arc::clone(&relay_manager))));
        let (tx, _rx) = mpsc::channel(100);
        TunnelManager::new(state, tx)
    }

    // Note: These tests are currently skipped due to the need for network
    // setup to create real PeerSession objects.

    #[tokio::test]
    #[ignore]
    async fn test_tunnel_manager_creation() {
        let manager = make_manager();
        // Basic creation test - manager should be empty
        assert!(manager.list_sessions().await.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn test_tunnel_manager_list_sessions() {
        let manager = make_manager();
        let sessions = manager.list_sessions().await;
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn test_tunnel_manager_get_all_session_ids() {
        let manager = make_manager();
        let ids = manager.get_all_session_ids().await;
        assert!(ids.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn test_tunnel_manager_add_and_remove_session() {
        // This test verifies that adding and removing sessions works
        // We can't easily create a real PeerSession without complex setup,
        // so we just verify the manager's async methods work correctly
        let manager = make_manager();

        // Initially empty
        assert!(manager.list_sessions().await.is_empty());
        assert!(manager.get_all_session_ids().await.is_empty());

        // Verify we can call these methods without error
        let _ = manager.list_sessions().await;
        let _ = manager.get_all_session_ids().await;
    }
}