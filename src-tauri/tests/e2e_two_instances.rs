use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use xent_lib::cert::TlsCertificate;
use xent_lib::db::{ChatMessage, Database};
use xent_lib::discovery::DiscoveryManager;
use xent_lib::server::{
    start_server, DeletePayload, EditPayload, ReactionPayload, ReadPayload, SendMessagePayload,
};
use xent_lib::settings::SettingsState;
use xent_lib::transfer::{
    cancel_file_transfer_core, pause_file_transfer_core, respond_file_transfer_core,
    resume_file_transfer_core, send_file_request_core, TransferManager, TransferStatus,
};

struct TestNode {
    dir: std::path::PathBuf,
    settings: Arc<SettingsState>,
    db: Arc<Database>,
    transfer_mgr: Arc<TransferManager>,
    discovery_mgr: Arc<DiscoveryManager>,
    port: u16,
    device_id: String,
    alias: String,
}

impl TestNode {
    async fn spawn(name: &str, requested_port: u16) -> Self {
        let dir = std::env::temp_dir().join(format!("xent_e2e_{}_{}", name, Uuid::new_v4()));
        let download_dir = dir.join("downloads");
        std::fs::create_dir_all(&download_dir).unwrap();

        let settings_path = dir.join("settings.json");
        let settings = Arc::new(SettingsState::open_at_path(
            settings_path,
            download_dir.to_string_lossy().to_string(),
            Some(requested_port),
            requested_port,
            name.to_string(),
        ));
        settings.update_alias(name.to_string()).unwrap();

        let db = Arc::new(Database::open_path(&dir.join("chat.db")).unwrap());
        let cert = TlsCertificate::generate().unwrap();
        let transfer_mgr = Arc::new(TransferManager::new(None, Some(db.clone())));

        let mut cfg = settings.get_settings();
        let bound_port = start_server(
            cfg.clone(),
            cert.clone(),
            db.clone(),
            transfer_mgr.clone(),
            None,
            Some(settings.clone()),
        )
        .await
        .expect("Failed to start test HTTPS server");

        if bound_port != cfg.port {
            cfg.port = bound_port;
            settings.update_settings(cfg.clone()).unwrap();
        }

        let discovery_mgr = Arc::new(DiscoveryManager::new(
            cfg.clone(),
            cert.fingerprint,
            None,
            Some(settings.clone()),
            Some(db.clone()),
        ));

        Self {
            dir,
            settings,
            db,
            transfer_mgr,
            discovery_mgr,
            port: bound_port,
            device_id: cfg.device_id,
            alias: name.to_string(),
        }
    }
}

impl Drop for TestNode {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn test_client() -> reqwest::Client {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
}

#[tokio::test]
async fn e2e_two_instances_messaging_and_file_transfers() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let node_a = TestNode::spawn("NodeA", 54411).await;
    let node_b = TestNode::spawn("NodeB", 54431).await;

    // 1. Discover B from A via add_peer_by_ip (also tests /api/info + /api/discovery/hello)
    let peer_b = node_a
        .discovery_mgr
        .add_peer_by_ip(format!("127.0.0.1:{}", node_b.port))
        .await
        .expect("NodeA should connect to NodeB by IP");
    assert_eq!(peer_b.device_id, node_b.device_id);
    assert!(peer_b.latency_ms.is_some());

    // Register A on B symmetrically as /api/discovery/hello does via AppHandle in full app
    node_b
        .discovery_mgr
        .add_peer_by_ip(format!("127.0.0.1:{}", node_a.port))
        .await
        .expect("NodeB should connect to NodeA by IP");

    let client = test_client();
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    // 2. Message A -> B + Read Receipt + Edit + Reaction + Delete
    let msg_id = Uuid::new_v4().to_string();
    let out_msg = ChatMessage {
        id: msg_id.clone(),
        thread_peer_id: node_b.device_id.clone(),
        sender_id: node_a.device_id.clone(),
        sender_name: node_a.alias.clone(),
        receiver_id: node_b.device_id.clone(),
        receiver_name: node_b.alias.clone(),
        content: "Hello \"NodeB\" 'quote' & test\nLine 2".to_string(),
        status: "Delivered, 12:00 PM".to_string(),
        created_at: now_ms,
        is_incoming: false,
        is_read: true,
        quote: Some("Prev: hi".to_string()),
        is_edited: false,
        is_deleted: false,
        reactions: None,
    };
    node_a.db.save_message(&out_msg).unwrap();

    let payload = SendMessagePayload {
        id: msg_id.clone(),
        sender_id: node_a.device_id.clone(),
        sender_name: node_a.alias.clone(),
        receiver_id: node_b.device_id.clone(),
        receiver_name: node_b.alias.clone(),
        content: out_msg.content.clone(),
        created_at: now_ms,
        quote: out_msg.quote.clone(),
    };
    let resp = client
        .post(format!("https://127.0.0.1:{}/api/message", node_b.port))
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // Duplicate retry should be ignored idempotently without resetting is_read
    let b_msgs = node_b.db.get_messages(&node_a.device_id, 50).unwrap();
    assert_eq!(b_msgs.len(), 1);
    assert_eq!(b_msgs[0].content, out_msg.content);
    assert_eq!(b_msgs[0].quote.as_deref(), Some("Prev: hi"));

    // B marks thread read -> sends B's device_id to A -> A persists Read receipt
    node_b.db.mark_as_read(&node_a.device_id).unwrap();
    let read_resp = client
        .post(format!("https://127.0.0.1:{}/api/message/read", node_a.port))
        .json(&ReadPayload {
            peer_id: node_b.device_id.clone(),
        })
        .send()
        .await
        .unwrap();
    assert!(read_resp.status().is_success());
    let a_msg_after_read = node_a.db.get_message_by_id(&msg_id).unwrap().unwrap();
    assert!(
        a_msg_after_read.status.starts_with("Read"),
        "Expected Read status, got: {}",
        a_msg_after_read.status
    );

    // Edit sync A -> B
    let edit_resp = client
        .post(format!("https://127.0.0.1:{}/api/message/edit", node_b.port))
        .json(&EditPayload {
            message_id: msg_id.clone(),
            new_content: "Edited hello!".to_string(),
        })
        .send()
        .await
        .unwrap();
    assert!(edit_resp.status().is_success());
    let b_edited = node_b.db.get_message_by_id(&msg_id).unwrap().unwrap();
    assert!(b_edited.is_edited);
    assert_eq!(b_edited.content, "Edited hello!");

    // Reaction sync A -> B
    let react_resp = client
        .post(format!("https://127.0.0.1:{}/api/message/reaction", node_b.port))
        .json(&ReactionPayload {
            message_id: msg_id.clone(),
            user_name: node_a.alias.clone(),
            emoji: "🔥".to_string(),
        })
        .send()
        .await
        .unwrap();
    assert!(react_resp.status().is_success());
    let b_reacted = node_b.db.get_message_by_id(&msg_id).unwrap().unwrap();
    assert!(b_reacted.reactions.unwrap().contains("🔥"));

    // Delete sync A -> B
    let del_resp = client
        .post(format!("https://127.0.0.1:{}/api/message/delete", node_b.port))
        .json(&DeletePayload {
            message_id: msg_id.clone(),
        })
        .send()
        .await
        .unwrap();
    assert!(del_resp.status().is_success());
    let b_deleted = node_b.db.get_message_by_id(&msg_id).unwrap().unwrap();
    assert!(b_deleted.is_deleted);

    // 3. Large Multi-Chunk File Transfer (1.4 MB) with Manual Accept + Pause/Resume
    let src_file = node_a.dir.join("test & sample (1).bin");
    let data: Vec<u8> = (0..(1_400_000)).map(|i| ((i * 31) % 251) as u8).collect();
    std::fs::write(&src_file, &data).unwrap();

    let info = send_file_request_core(
        &node_a.transfer_mgr,
        &node_a.discovery_mgr,
        &node_a.settings,
        Some(&node_a.db),
        node_b.device_id.clone(),
        src_file.to_string_lossy().to_string(),
    )
    .await
    .expect("send_file_request_core should succeed");
    assert_eq!(info.status, TransferStatus::Pending);

    // Node B accepts -> Node A starts upload automatically
    respond_file_transfer_core(&node_b.transfer_mgr, info.transfer_id.clone(), true)
        .await
        .expect("respond_file_transfer_core should succeed");

    // Wait for completion (up to 15s)
    let mut completed = false;
    for _ in 0..150 {
        let a_done = node_a
            .transfer_mgr
            .get_transfer(&info.transfer_id)
            .map(|t| t.status == TransferStatus::Completed)
            .unwrap_or(false);
        let b_done = node_b
            .transfer_mgr
            .get_transfer(&info.transfer_id)
            .map(|t| t.status == TransferStatus::Completed)
            .unwrap_or(false);
        if a_done && b_done {
            completed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(completed, "1.4 MB multi-chunk transfer must complete on both sides");

    let b_transfer = node_b.transfer_mgr.get_transfer(&info.transfer_id).unwrap();
    let recv_path = std::path::PathBuf::from(b_transfer.file_path.unwrap());
    assert_eq!(std::fs::read(&recv_path).unwrap(), data, "Transferred file bytes must match identically");

    // Verify transfer history direction flags in DB (even if alias later changes!)
    let a_hist = node_a.db.get_transfers(20).unwrap();
    let b_hist = node_b.db.get_transfers(20).unwrap();
    assert!(!a_hist[0].is_incoming, "Sender DB must record is_incoming=false");
    assert!(b_hist[0].is_incoming, "Receiver DB must record is_incoming=true");

    // 3b. Message sent AFTER file transfer must appear AFTER file transfer in DB timeline
    tokio::time::sleep(Duration::from_millis(15)).await;
    let after_file_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let after_msg_id = Uuid::new_v4().to_string();
    let after_msg = ChatMessage {
        id: after_msg_id.clone(),
        thread_peer_id: node_b.device_id.clone(),
        sender_id: node_a.device_id.clone(),
        sender_name: node_a.alias.clone(),
        receiver_id: node_b.device_id.clone(),
        receiver_name: node_b.alias.clone(),
        content: "After file SMS".to_string(),
        status: "Delivered".to_string(),
        created_at: after_file_ts,
        is_incoming: false,
        is_read: true,
        quote: None,
        is_edited: false,
        is_deleted: false,
        reactions: None,
    };
    node_a.db.save_message(&after_msg).unwrap();
    let timeline = node_a.db.get_timeline(&node_b.device_id, 50).unwrap();
    let file_pos = timeline
        .iter()
        .position(|item| matches!(item, xent_lib::db::TimelineItem::Transfer(t) if t.id == info.transfer_id))
        .expect("Timeline must contain transfer");
    let after_pos = timeline
        .iter()
        .position(|item| matches!(item, xent_lib::db::TimelineItem::Message(m) if m.id == after_msg_id))
        .expect("Timeline must contain message sent after file");
    assert!(
        after_pos > file_pos,
        "Message sent after file must appear AFTER file in timeline (file_pos={}, after_pos={})",
        file_pos,
        after_pos
    );

    // 4. Duplicate filename collision + Auto-accept Favorite
    node_b.settings.toggle_favorite(node_a.device_id.clone()).unwrap();
    let src_dup = node_a.dir.join("dup_doc.txt");
    std::fs::write(&src_dup, b"hello duplicate").unwrap();

    let (r1, r2) = tokio::join!(
        send_file_request_core(
            &node_a.transfer_mgr,
            &node_a.discovery_mgr,
            &node_a.settings,
            Some(&node_a.db),
            node_b.device_id.clone(),
            src_dup.to_string_lossy().to_string(),
        ),
        send_file_request_core(
            &node_a.transfer_mgr,
            &node_a.discovery_mgr,
            &node_a.settings,
            Some(&node_a.db),
            node_b.device_id.clone(),
            src_dup.to_string_lossy().to_string(),
        )
    );
    let t1 = r1.unwrap();
    let t2 = r2.unwrap();
    for _ in 0..100 {
        let d1 = node_b.transfer_mgr.get_transfer(&t1.transfer_id).map(|t| t.status == TransferStatus::Completed).unwrap_or(false);
        let d2 = node_b.transfer_mgr.get_transfer(&t2.transfer_id).map(|t| t.status == TransferStatus::Completed).unwrap_or(false);
        if d1 && d2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let p1 = node_b.transfer_mgr.get_transfer(&t1.transfer_id).unwrap().file_path.unwrap();
    let p2 = node_b.transfer_mgr.get_transfer(&t2.transfer_id).unwrap().file_path.unwrap();
    assert_ne!(p1, p2, "Concurrent same-name files must reserve distinct atomic paths");
    assert_eq!(std::fs::read(&p1).unwrap(), b"hello duplicate");
    assert_eq!(std::fs::read(&p2).unwrap(), b"hello duplicate");

    // 5. Pause & Resume real chunked upload across TLS (6 MB)
    let big_file = node_a.dir.join("pause_resume_test.bin");
    let big_data: Vec<u8> = (0..(6 * 1024 * 1024)).map(|i| ((i * 17) % 253) as u8).collect();
    std::fs::write(&big_file, &big_data).unwrap();

    let big_t = send_file_request_core(
        &node_a.transfer_mgr,
        &node_a.discovery_mgr,
        &node_a.settings,
        Some(&node_a.db),
        node_b.device_id.clone(),
        big_file.to_string_lossy().to_string(),
    )
    .await
    .unwrap();
    // Node A and Node B are already favorites so transfer auto-starts; pause immediately
    pause_file_transfer_core(&node_a.transfer_mgr, &big_t.transfer_id)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(80)).await;
    let a_paused = node_a.transfer_mgr.get_transfer(&big_t.transfer_id).unwrap();
    let b_paused = node_b.transfer_mgr.get_transfer(&big_t.transfer_id).unwrap();
    if a_paused.status != TransferStatus::Completed {
        assert_eq!(a_paused.status, TransferStatus::Paused);
        assert_eq!(b_paused.status, TransferStatus::Paused);
    }
    resume_file_transfer_core(&node_a.transfer_mgr, &big_t.transfer_id)
        .await
        .unwrap();
    let mut big_completed = false;
    for _ in 0..200 {
        let a_done = node_a
            .transfer_mgr
            .get_transfer(&big_t.transfer_id)
            .map(|t| t.status == TransferStatus::Completed)
            .unwrap_or(false);
        let b_done = node_b
            .transfer_mgr
            .get_transfer(&big_t.transfer_id)
            .map(|t| t.status == TransferStatus::Completed)
            .unwrap_or(false);
        if a_done && b_done {
            big_completed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(big_completed, "Paused & resumed 6 MB transfer must finish intact");
    let big_recv_path = node_b
        .transfer_mgr
        .get_transfer(&big_t.transfer_id)
        .unwrap()
        .file_path
        .unwrap();
    assert_eq!(std::fs::read(&big_recv_path).unwrap(), big_data);

    // 6. Cancel pending transfer propagates across HTTPS
    node_b.settings.toggle_favorite(node_a.device_id.clone()).unwrap(); // disable auto-accept
    let cancel_t = send_file_request_core(
        &node_a.transfer_mgr,
        &node_a.discovery_mgr,
        &node_a.settings,
        Some(&node_a.db),
        node_b.device_id.clone(),
        src_dup.to_string_lossy().to_string(),
    )
    .await
    .unwrap();
    assert_eq!(cancel_t.status, TransferStatus::Pending);
    cancel_file_transfer_core(&node_a.transfer_mgr, &cancel_t.transfer_id)
        .await
        .unwrap();
    assert_eq!(
        node_a.transfer_mgr.get_transfer(&cancel_t.transfer_id).unwrap().status,
        TransferStatus::Cancelled
    );
    assert_eq!(
        node_b.transfer_mgr.get_transfer(&cancel_t.transfer_id).unwrap().status,
        TransferStatus::Cancelled
    );

    // 7. Blocked device silently declined
    node_b.settings.toggle_blocked(node_a.device_id.clone()).unwrap();
    let t_blocked = send_file_request_core(
        &node_a.transfer_mgr,
        &node_a.discovery_mgr,
        &node_a.settings,
        Some(&node_a.db),
        node_b.device_id.clone(),
        src_dup.to_string_lossy().to_string(),
    )
    .await
    .unwrap();
    assert_eq!(t_blocked.status, TransferStatus::Declined);
}
