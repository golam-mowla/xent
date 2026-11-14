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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkDiagnostics {
    pub local_addrs: Vec<String>,
    pub https_port: u16,
    pub udp_listener_bound: bool,
    pub last_packet_time: Option<u64>,
    pub peer_count: usize,
    pub firewall_command: String,
}

pub struct DiscoveryManager {
    peers: Arc<RwLock<HashMap<String, Peer>>>,
    settings: AppSettings,
    presence: Arc<RwLock<LocalPresence>>,
    fingerprint: String,
    app_handle: Option<AppHandle>,
    settings_state: Option<Arc<SettingsState>>,
    udp_bound: Arc<RwLock<bool>>,
    last_packet_time: Arc<RwLock<Option<u64>>>,
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
            udp_bound: Arc::new(RwLock::new(false)),
            last_packet_time: Arc::new(RwLock::new(None)),
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

        let mgr_scan = Arc::clone(&self);
        tokio::spawn(async move {
            mgr_scan.run_subnet_scanner().await;
        });

        let mgr_manual = Arc::clone(&self);
        tokio::spawn(async move {
            mgr_manual.run_manual_peers_prober().await;
        });

        let mgr_net = Arc::clone(&self);
        tokio::spawn(async move {
            mgr_net.run_network_watcher().await;
        });
    }

    pub async fn probe_peer(client: &reqwest::Client, target_ip: &str, target_port: u16, my_id: &str) -> Option<Peer> {
        let url = format!("https://{}:{}/api/info", target_ip, target_port);
        let resp = client.get(&url).send().await.ok()?;
        if resp.status().is_success() {
            let info = resp.json::<crate::server::DeviceInfoResponse>().await.ok()?;
            if info.device_id != my_id {
                return Some(Peer {
                    device_id: info.device_id,
                    alias: info.alias,
                    device_type: info.device_type,
                    ip: target_ip.to_string(),
                    port: info.port,
                    fingerprint: info.fingerprint,
                    last_seen: Self::now_secs(),
                    status: if info.status.is_empty() { "available".to_string() } else { info.status },
                    status_message: info.status_message,
                });
            }
        }
        None
    }

    pub async fn add_peer_by_ip(&self, target: String) -> Result<Peer, String> {
        let parts: Vec<&str> = target.split(':').collect();
        let ip = parts[0].trim();
        let port = if parts.len() > 1 {
            parts[1].trim().parse::<u16>().map_err(|_| "Invalid port".to_string())?
        } else {
            53317
        };

        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_millis(1500))
            .build()
            .map_err(|e| e.to_string())?;

        if let Some(peer) = Self::probe_peer(&client, ip, port, &self.settings.device_id).await {
            {
                let mut lock = self.peers.write().unwrap();
                lock.insert(peer.device_id.clone(), peer.clone());
            }
            if let Some(ref st) = self.settings_state {
                let _ = st.add_manual_peer(format!("{}:{}", ip, port));
            }
            self.emit_peers_changed();
            Ok(peer)
        } else {
            Err(format!("Could not connect to Xent device at {}:{}", ip, port))
        }
    }

    pub fn get_network_diagnostics(&self) -> NetworkDiagnostics {
        let mut addrs_info = Vec::new();
        if let Ok(list) = local_ip_address::list_afinet_netifas() {
            for (name, ip) in list {
                if let std::net::IpAddr::V4(a) = ip {
                    if !a.is_loopback() && !a.is_unspecified() {
                        addrs_info.push(format!("{} ({})", a, name));
                    }
                }
            }
        }
        let exe_path = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "xent.exe".to_string());
        
        let firewall_cmd = format!(
            "netsh advfirewall firewall add rule name=\"Xent\" dir=in action=allow program=\"{}\" enable=yes profile=any && netsh advfirewall firewall add rule name=\"Xent UDP\" dir=in action=allow protocol=UDP localport=53317 enable=yes profile=any",
            exe_path
        );

        NetworkDiagnostics {
            local_addrs: addrs_info,
            https_port: self.settings.port,
            udp_listener_bound: *self.udp_bound.read().unwrap(),
            last_packet_time: *self.last_packet_time.read().unwrap(),
            peer_count: self.peers.read().unwrap().len(),
            firewall_command: firewall_cmd,
        }
    }

    async fn run_subnet_scanner(self: Arc<Self>) {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let client = match reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_millis(400))
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };

        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            interval.tick().await;

            if !self.peers.read().unwrap().is_empty() {
                continue;
            }

            let addrs = Self::local_v4_addrs();
            let my_id = self.settings.device_id.clone();
            let my_port = self.settings.port;

            for local_ip in addrs {
                let octets = local_ip.octets();
                let prefix = format!("{}.{}.{}", octets[0], octets[1], octets[2]);

                let sem = Arc::new(tokio::sync::Semaphore::new(64));
                let mut handles = Vec::new();

                for i in 1..255 {
                    let target_ip = format!("{}.{}", prefix, i);
                    if target_ip == local_ip.to_string() {
                        continue;
                    }

                    for port in [my_port, 53317] {
                        let permit = match sem.clone().acquire_owned().await {
                            Ok(p) => p,
                            Err(_) => break,
                        };
                        let c = client.clone();
                        let ip_clone = target_ip.clone();
                        let id_clone = my_id.clone();
                        let mgr = Arc::clone(&self);

                        handles.push(tokio::spawn(async move {
                            let _permit = permit;
                            if let Some(peer) = Self::probe_peer(&c, &ip_clone, port, &id_clone).await {
                                let mut lock = mgr.peers.write().unwrap();
                                let is_new = !lock.contains_key(&peer.device_id);
                                lock.insert(peer.device_id.clone(), peer);
                                drop(lock);
                                if is_new {
                                    mgr.emit_peers_changed();
                                }
                            }
                        }));
                    }
                }

                for h in handles {
                    let _ = h.await;
                }

                if !self.peers.read().unwrap().is_empty() {
                    break;
                }
            }
        }
    }

    async fn run_manual_peers_prober(self: Arc<Self>) {
        let client = match reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_millis(800))
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };

        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            if let Some(ref st) = self.settings_state {
                let manual_peers = st.get_settings().manual_peers;
                for target in manual_peers {
                    let parts: Vec<&str> = target.split(':').collect();
                    let ip = parts[0];
                    let port = if parts.len() > 1 {
                        parts[1].parse::<u16>().unwrap_or(53317)
                    } else {
                        53317
                    };
                    if let Some(peer) = Self::probe_peer(&client, ip, port, &self.settings.device_id).await {
                        let mut lock = self.peers.write().unwrap();
                        let is_new = !lock.contains_key(&peer.device_id);
                        lock.insert(peer.device_id.clone(), peer);
                        drop(lock);
                        if is_new {
                            self.emit_peers_changed();
                        }
                    }
                }
            }
        }
    }

    async fn run_network_watcher(self: Arc<Self>) {
        let mut last_addrs = Self::local_v4_addrs();
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            let current = Self::local_v4_addrs();
            if current != last_addrs {
                println!("[Discovery] Network adapters changed: {:?} -> {:?}", last_addrs, current);
                last_addrs = current;
                self.send_multicast_announce("announce").await;
            }
        }
    }


    /// All usable IPv4 addresses of this machine (one per network adapter).
    fn local_v4_addrs() -> Vec<Ipv4Addr> {
        let mut out: Vec<Ipv4Addr> = Vec::new();
        if let Ok(list) = local_ip_address::list_afinet_netifas() {
            for (_name, ip) in list {
                if let std::net::IpAddr::V4(a) = ip {
                    if !a.is_loopback() && !a.is_unspecified() && !a.is_link_local() && !out.contains(&a) {
                        out.push(a);
                    }
                }
            }
        }
        out
    }

    /// UDP socket bound to one adapter so packets leave through that adapter.
    fn build_sender(ip: Ipv4Addr) -> Option<UdpSocket> {
        let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).ok()?;
        s.bind(&SocketAddrV4::new(ip, 0).into()).ok()?;
        let _ = s.set_multicast_if_v4(&ip);
        let _ = s.set_multicast_loop_v4(true);
        let _ = s.set_multicast_ttl_v4(2);
        let _ = s.set_broadcast(true);
        s.set_nonblocking(true).ok()?;
        let std_s: std::net::UdpSocket = s.into();
        UdpSocket::from_std(std_s).ok()
    }

    /// Send one packet via multicast AND broadcast on every adapter.
    /// Many Wi-Fi routers drop multicast, broadcast is the fallback.
    async fn send_everywhere(data: &[u8]) {
        let multicast = SocketAddr::V4(SocketAddrV4::new(MULTICAST_IPV4, MULTICAST_PORT));
        let broadcast = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, MULTICAST_PORT));
        let mut sent = false;
        for ip in Self::local_v4_addrs() {
            if let Some(sock) = Self::build_sender(ip) {
                let _ = sock.send_to(data, multicast).await;
                let _ = sock.send_to(data, broadcast).await;
                sent = true;
            }
        }
        if !sent {
            if let Ok(sock) = UdpSocket::bind("0.0.0.0:0").await {
                let _ = sock.set_multicast_loop_v4(true);
                let _ = sock.set_broadcast(true);
                let _ = sock.send_to(data, multicast).await;
                let _ = sock.send_to(data, broadcast).await;
            }
        }
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
        let _ = socket.join_multicast_v4(&MULTICAST_IPV4, &Ipv4Addr::UNSPECIFIED);
        // Join the group on every real network adapter (Wi-Fi, Ethernet, ...), not only the default one.
        for addr in Self::local_v4_addrs() {
            let _ = socket.join_multicast_v4(&MULTICAST_IPV4, &addr);
        }
        socket.set_nonblocking(true)?;

        let std_socket: std::net::UdpSocket = socket.into();
        let tokio_socket = UdpSocket::from_std(std_socket)?;
        *self.udp_bound.write().unwrap() = true;
        let mut buf = [0u8; 4096];

        loop {
            match tokio_socket.recv_from(&mut buf).await {
                Ok((len, src_addr)) => {
                    *self.last_packet_time.write().unwrap() = Some(Self::now_secs());
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
                                        Self::send_everywhere(&data).await;
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
            let _ = (sender, target);
            Self::send_everywhere(&bytes).await;
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
        {
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
                Self::send_everywhere(&bytes).await;
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
                // If not seen in 15 seconds, peer is considered offline
                lock.retain(|id, peer| {
                    let active = now.saturating_sub(peer.last_seen) < 15;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_legacy_packet_parsing() {
        let legacy_json = r#"{
            "msg_type": "announce",
            "device_id": "test-id",
            "alias": "Swift Falcon",
            "device_type": "desktop",
            "port": 53317,
            "fingerprint": "abcd",
            "protocol_version": 1
        }"#;

        let packet: DiscoveryPacket = serde_json::from_str(legacy_json).expect("Failed to parse legacy packet");
        assert_eq!(packet.status, "available");
        assert_eq!(packet.status_message, "");
        assert_eq!(packet.alias, "Swift Falcon");
    }

    #[test]
    fn test_v2_packet_parsing() {
        let v2_json = r#"{
            "msg_type": "announce",
            "device_id": "test-id",
            "alias": "Silent Wolf",
            "device_type": "desktop",
            "port": 53325,
            "fingerprint": "1234",
            "protocol_version": 1,
            "status": "busy",
            "status_message": "In a meeting"
        }"#;

        let packet: DiscoveryPacket = serde_json::from_str(v2_json).expect("Failed to parse v2 packet");
        assert_eq!(packet.status, "busy");
        assert_eq!(packet.status_message, "In a meeting");
    }
}
