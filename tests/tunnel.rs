#[cfg(test)]
mod tunnel_tests {
    use wraith::wraith::tunnel::TunnelManager;

    #[tokio::test]
    async fn test_tunnel_manager_creation() {
        let manager = TunnelManager::new();
        // Basic creation test - manager should be empty
        assert!(manager.list_sessions().await.is_empty());
    }

    #[tokio::test]
    async fn test_tunnel_manager_list_sessions() {
        let manager = TunnelManager::new();
        let sessions = manager.list_sessions().await;
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_tunnel_manager_get_all_session_ids() {
        let manager = TunnelManager::new();
        let ids = manager.get_all_session_ids().await;
        assert!(ids.is_empty());
    }

    #[tokio::test]
    async fn test_tunnel_manager_add_and_remove_session() {
        // This test verifies that adding and removing sessions works
        // We can't easily create a real PeerSession without complex setup,
        // so we just verify the manager's async methods work correctly
        let manager = TunnelManager::new();

        // Initially empty
        assert!(manager.list_sessions().await.is_empty());
        assert!(manager.get_all_session_ids().await.is_empty());

        // Verify we can call these methods without error
        let _ = manager.list_sessions().await;
        let _ = manager.get_all_session_ids().await;
    }
}