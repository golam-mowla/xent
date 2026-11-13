use crate::settings::{AppSettings, SettingsState};
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tokio::net::UdpSocket;

pub const MULTICAST_IPV4: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 167);
pub const MULTICAST_PORT: u16 = 53317;

fn default_status() -> String {
    "available".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryPacket {
    pub msg_type: String, // "announce" | "reply" | "bye"
    pub device_id: String,
    pub alias: String,
    pub device_type: String,
    pub port: u16,
    pub fingerprint: String,
    pub protocol_version: u32,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub status_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub device_id: String,
    pub alias: String,
    pub device_type: String,
    pub ip: String,
    pub port: u16,
    pub fingerprint: String,
    pub last_seen: u64,
    pub status: String,
    pub status_message: String,
}

#[derive(Debug, Clone)]
pub struct LocalPresence {
    pub alias: String,
    pub status: String,
    pub status_message: String,
    pub status_expires_at: Option<u64>,
}

pub struct DiscoveryManager {
    peers: Arc<RwLock<HashMap<String, Peer>>>,
    settings: AppSettings,
    presence: Arc<RwLock<LocalPresence>>,
    fingerprint: String,
    app_handle: Option<AppHandle>,
    settings_state: Option<Arc<SettingsState>>,
}

impl DiscoveryManager {
    pub fn new(
        settings: AppSettings,
        fingerprint: String,
        app_handle: Option<AppHandle>,
        settings_state: Option<Arc<SettingsState>>,
    ) -> Self {
        let presence = Arc::new(RwLock::new(LocalPresence {
            alias: settings.alias.clone(),
            status: settings.status.clone(),
            status_message: settings.status_message.clone(),
            status_expires_at: settings.status_expires_at,
        }));

        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            settings,
            presence,
            fingerprint,
            app_handle,
            settings_state,
        }
    }

    pub fn get_peers(&self) -> Vec<Peer> {
        let lock = self.peers.read().unwrap();
        lock.values().cloned().collect()
    }

    pub fn update_alias(&self, new_alias: String) {
        let mut p = self.presence.write().unwrap();
        p.alias = new_alias;
    }

    pub async fn set_presence(&self, status: String, message: String, expires_at: Option<u64>) {
        {
            let mut p = self.presence.write().unwrap();
            p.status = status.clone();
            p.status_message = message;
            p.status_expires_at = expires_at;
        }

        if status == "offline" {
            self.send_bye().await;
        } else {
            self.send_multicast_announce("announce").await;
        }
    }

    pub fn start(self: Arc<Self>) {
        let mgr_listen = Arc::clone(&self);
        tokio::spawn(async move {
            if let Err(e) = mgr_listen.run_listener().await {
                eprintln!("[Discovery] Listener error: {}", e);
            }
        });

        let mgr_announce = Arc::clone(&self);
        tokio::spawn(async move {
            mgr_announce.run_announcer().await;
        });

        let mgr_prune = Arc::clone(&self);
        tokio::spawn(async move {
            mgr_prune.run_pruner().await;
        });
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn emit_peers_changed(&self) {
        if let Some(ref handle) = self.app_handle {
            let peers = self.get_peers();
            let _ = handle.emit("nearby_devices_changed", peers);
        }
    }

    async fn run_listener(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_reuse_address(true)?;
        #[cfg(unix)]
        socket.set_reuse_port(true)?;
        socket.set_multicast_loop_v4(true)?;

        let bind_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, MULTICAST_PORT);
        socket.bind(&bind_addr.into())?;
        socket.join_multicast_v4(&MULTICAST_IPV4, &Ipv4Addr::UNSPECIFIED)?;
        socket.set_nonblocking(true)?;

        let std_socket: std::net::UdpSocket = socket.into();
        let tokio_socket = UdpSocket::from_std(std_socket)?;
        let mut buf = [0u8; 4096];

        loop {
            match tokio_socket.recv_from(&mut buf).await {
                Ok((len, src_addr)) => {
                    if let Ok(packet) = serde_json::from_slice::<DiscoveryPacket>(&buf[..len]) {
                        if packet.device_id == self.settings.device_id {
                            // Ignore self announcements
                            continue;
                        }

                        let ip = match src_addr.ip() {
                            std::net::IpAddr::V4(ipv4) => {
                                if ipv4.is_loopback() || ipv4.is_unspecified() {
                                    "127.0.0.1".to_string()
                                } else {
                                    ipv4.to_string()
                                }
                            }
                            std::net::IpAddr::V6(ipv6) => {
                                if ipv6.is_loopback() {
                                    "127.0.0.1".to_string()
                                } else {
                                    ipv6.to_string()
                                }
                            }
                        };

                        if packet.msg_type == "bye" {
                            let mut lock = self.peers.write().unwrap();
                            if lock.remove(&packet.device_id).is_some() {
                                drop(lock);
                                self.emit_peers_changed();
                            }
                        } else if packet.msg_type == "announce" || packet.msg_type == "reply" {
                            let now = Self::now_secs();
                            let peer_status = if packet.status.trim().is_empty() {
                                "available".to_string()
                            } else {
                                packet.status.clone()
                            };

                            let peer = Peer {
                                device_id: packet.device_id.clone(),
                                alias: packet.alias.clone(),
                                device_type: packet.device_type.clone(),
                                ip,
                                port: packet.port,
                                fingerprint: packet.fingerprint.clone(),
                                last_seen: now,
                                status: peer_status,
                                status_message: packet.status_message.clone(),
                            };

                            let mut changed = false;
                            {
                                let mut lock = self.peers.write().unwrap();
                                if let Some(existing) = lock.get_mut(&packet.device_id) {
                                    existing.last_seen = now;
                                    if existing.alias != peer.alias
                                        || existing.port != peer.port
                                        || existing.fingerprint != peer.fingerprint
                                        || existing.status != peer.status
                                        || existing.status_message != peer.status_message
                                    {
                                        existing.alias = peer.alias;
                                        existing.port = peer.port;
                                        existing.fingerprint = peer.fingerprint;
                                        existing.status = peer.status;
                                        existing.status_message = peer.status_message;
                                        changed = true;
                                    }
                                } else {
                                    println!(
                                        "[Discovery] Discovered new peer: {} ({}) at {}:{}",
                                        peer.alias, peer.device_id, peer.ip, peer.port
                                    );
                                    lock.insert(packet.device_id.clone(), peer);
                                    changed = true;
                                }
                            }

                            if changed {
                                self.emit_peers_changed();
                            }

                            // If this was an announce packet, send a reply back so they know us immediately
                            if packet.msg_type == "announce" {
                                let current_presence = self.presence.read().unwrap().clone();
                                if current_presence.status != "offline" {
                                    let reply_packet = DiscoveryPacket {
                                        msg_type: "reply".to_string(),
                                        device_id: self.settings.device_id.clone(),
                                        alias: current_presence.alias,
                                        device_type: self.settings.device_type.clone(),
                                        port: self.settings.port,
                                        fingerprint: self.fingerprint.clone(),
                                        protocol_version: 1,
                                        status: current_presence.status,
                                        status_message: current_presence.status_message,
                                    };
                                    if let Ok(data) = serde_json::to_vec(&reply_packet) {
                                        let target_multicast =
                                            SocketAddr::V4(SocketAddrV4::new(MULTICAST_IPV4, MULTICAST_PORT));
                                        let _ = tokio_socket.send_to(&data, target_multicast).await;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[Discovery] recv_from error: {}", e);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    async fn run_announcer(&self) {
        // Ephemeral socket to send multicast packets
        let sender = match UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[Discovery] Failed to bind announcer sender socket: {}", e);
                return;
            }
        };
        let _ = sender.set_multicast_loop_v4(true);
        let target = SocketAddr::V4(SocketAddrV4::new(MULTICAST_IPV4, MULTICAST_PORT));

        // Initial rapid announces on startup
        for _ in 0..3 {
            let status = self.presence.read().unwrap().status.clone();
            if status != "offline" {
                self.send_announce(&sender, target, "announce").await;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }

        // Periodic announces
        let mut interval = tokio::time::interval(Duration::from_secs(3));
        loop {
            interval.tick().await;
            let status = self.presence.read().unwrap().status.clone();
            if status != "offline" {
                self.send_announce(&sender, target, "announce").await;
            }
        }
    }

    async fn send_announce(&self, sender: &UdpSocket, target: SocketAddr, msg_type: &str) {
        let pres = self.presence.read().unwrap().clone();
        let packet = DiscoveryPacket {
            msg_type: msg_type.to_string(),
            device_id: self.settings.device_id.clone(),
            alias: pres.alias,
            device_type: self.settings.device_type.clone(),
            port: self.settings.port,
            fingerprint: self.fingerprint.clone(),
            protocol_version: 1,
            status: pres.status,
            status_message: pres.status_message,
        };

        if let Ok(bytes) = serde_json::to_vec(&packet) {
            let _ = sender.send_to(&bytes, target).await;
        }
    }

    pub async fn send_multicast_announce(&self, msg_type: &str) {
        if let Ok(sender) = UdpSocket::bind("0.0.0.0:0").await {
            let _ = sender.set_multicast_loop_v4(true);
            let target = SocketAddr::V4(SocketAddrV4::new(MULTICAST_IPV4, MULTICAST_PORT));
            self.send_announce(&sender, target, msg_type).await;
        }
    }

    pub async fn send_bye(&self) {
        let pres = self.presence.read().unwrap().clone();
        if let Ok(sender) = UdpSocket::bind("0.0.0.0:0").await {
            let _ = sender.set_multicast_loop_v4(true);
            let target = SocketAddr::V4(SocketAddrV4::new(MULTICAST_IPV4, MULTICAST_PORT));
            let packet = DiscoveryPacket {
                msg_type: "bye".to_string(),
                device_id: self.settings.device_id.clone(),
                alias: pres.alias,
                device_type: self.settings.device_type.clone(),
                port: self.settings.port,
                fingerprint: self.fingerprint.clone(),
                protocol_version: 1,
                status: "offline".to_string(),
                status_message: "".to_string(),
            };
            if let Ok(bytes) = serde_json::to_vec(&packet) {
                let _ = sender.send_to(&bytes, target).await;
            }
        }
    }

    async fn run_pruner(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            let now = Self::now_secs();

            // Check presence expiration
            let mut expired = false;
            let mut updated_settings_opt = None;
            {
                let mut p = self.presence.write().unwrap();
                if let Some(exp) = p.status_expires_at {
                    if now >= exp {
                        p.status = "available".to_string();
                        p.status_message = "".to_string();
                        p.status_expires_at = None;
                        expired = true;
                    }
                }
            }

            if expired {
                if let Some(ref st) = self.settings_state {
                    if let Ok(updated) = st.set_presence("available".to_string(), "".to_string(), None) {
                        updated_settings_opt = Some(updated);
                    }
                }
                self.send_multicast_announce("announce").await;
                if let Some(ref handle) = self.app_handle {
                    if let Some(updated) = updated_settings_opt {
                        let _ = handle.emit("presence_changed", updated);
                    }
                }
            }

            let mut removed = false;
            {
                let mut lock = self.peers.write().unwrap();
                let initial_len = lock.len();
                // If not seen in 8 seconds, peer is considered offline
                lock.retain(|id, peer| {
                    let active = now.saturating_sub(peer.last_seen) < 8;
                    if !active {
                        println!("[Discovery] Peer offline (timed out): {} ({})", peer.alias, id);
                    }
                    active
                });
                if lock.len() != initial_len {
                    removed = true;
                }
            }
            if removed {
                self.emit_peers_changed();
            }
        }
    }
}
