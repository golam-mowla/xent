use chrono::Local;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub thread_peer_id: String,
    pub sender_id: String,
    pub sender_name: String,
    pub receiver_id: String,
    pub receiver_name: String,
    pub content: String,
    pub status: String, // "sending", "delivered", "read", "failed", "queued"
    pub created_at: i64, // timestamp in ms
    pub is_incoming: bool,
    pub is_read: bool,
    #[serde(default)]
    pub quote: Option<String>,
    #[serde(default)]
    pub is_edited: bool,
    #[serde(default)]
    pub is_deleted: bool,
    #[serde(default)]
    pub reactions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRecord {
    pub id: String,
    pub peer_id: String,
    pub sender_name: String,
    pub receiver_name: String,
    pub file_name: String,
    pub file_size: u64,
    pub status: String, // "completed", "declined", "cancelled", "failed"
    pub created_at: i64,
    #[serde(default)]
    pub local_path: Option<String>,
    #[serde(default)]
    pub is_incoming: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownPeer {
    pub device_id: String,
    pub alias: String,
    pub device_type: String,
    #[serde(default)]
    pub os: String,
    pub ip: String,
    pub port: u16,
    #[serde(default)]
    pub fingerprint: String,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum TimelineItem {
    #[serde(rename = "message")]
    Message(ChatMessage),
    #[serde(rename = "transfer")]
    Transfer(TransferRecord),
}

pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn init(instance: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let dir = crate::paths::data_dir();
        let _ = fs::create_dir_all(&dir);

        let db_name = if instance == "default" {
            "chat.db".to_string()
        } else {
            format!("chat_{}.db", instance)
        };
        let db_path = dir.join(db_name);
        Self::open_path(&db_path)
    }

    /// Open a database at an explicit path (useful for integration tests and custom instances).
    pub fn open_path(db_path: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(parent) = db_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let conn = Connection::open(db_path)?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                thread_peer_id TEXT NOT NULL,
                sender_id TEXT NOT NULL,
                sender_name TEXT NOT NULL,
                receiver_id TEXT NOT NULL,
                receiver_name TEXT NOT NULL,
                content TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                is_incoming INTEGER NOT NULL,
                is_read INTEGER NOT NULL,
                quote TEXT,
                is_edited INTEGER DEFAULT 0,
                is_deleted INTEGER DEFAULT 0,
                reactions TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_thread ON messages(thread_peer_id, created_at);
            CREATE INDEX IF NOT EXISTS idx_unread ON messages(is_read, is_incoming);
            CREATE TABLE IF NOT EXISTS transfers (
                id TEXT PRIMARY KEY,
                peer_id TEXT NOT NULL,
                sender_name TEXT NOT NULL,
                receiver_name TEXT NOT NULL,
                file_name TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                local_path TEXT,
                is_incoming INTEGER DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_transfers_created ON transfers(created_at DESC);
            CREATE TABLE IF NOT EXISTS peers (
                device_id TEXT PRIMARY KEY,
                alias TEXT NOT NULL,
                device_type TEXT NOT NULL,
                os TEXT DEFAULT '',
                ip TEXT NOT NULL,
                port INTEGER NOT NULL,
                fingerprint TEXT DEFAULT '',
                last_seen INTEGER NOT NULL
            );"
        )?;

        // Safe migrations for existing databases
        let _ = conn.execute("ALTER TABLE messages ADD COLUMN quote TEXT", []);
        let _ = conn.execute("ALTER TABLE messages ADD COLUMN is_edited INTEGER DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE messages ADD COLUMN is_deleted INTEGER DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE messages ADD COLUMN reactions TEXT", []);
        let _ = conn.execute("ALTER TABLE transfers ADD COLUMN local_path TEXT", []);
        let _ = conn.execute("ALTER TABLE transfers ADD COLUMN is_incoming INTEGER DEFAULT 0", []);

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn upsert_peer(&self, peer: &KnownPeer) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO peers (device_id, alias, device_type, os, ip, port, fingerprint, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                peer.device_id,
                peer.alias,
                peer.device_type,
                peer.os,
                peer.ip,
                peer.port as i64,
                peer.fingerprint,
                peer.last_seen as i64,
            ],
        )?;
        Ok(())
    }

    pub fn get_known_peers(&self) -> Result<Vec<KnownPeer>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT device_id, alias, device_type, os, ip, port, fingerprint, last_seen
             FROM peers
             ORDER BY last_seen DESC"
        )?;
        let rows = stmt.query_map([], |row| {
            let port_i64: i64 = row.get(5)?;
            let last_seen_i64: i64 = row.get(7)?;
            Ok(KnownPeer {
                device_id: row.get(0)?,
                alias: row.get(1)?,
                device_type: row.get(2)?,
                os: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                ip: row.get(4)?,
                port: port_i64.clamp(0, u16::MAX as i64) as u16,
                fingerprint: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                last_seen: last_seen_i64.max(0) as u64,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn save_message(&self, msg: &ChatMessage) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO messages 
            (id, thread_peer_id, sender_id, sender_name, receiver_id, receiver_name, content, status, created_at, is_incoming, is_read, quote, is_edited, is_deleted, reactions)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                msg.id,
                msg.thread_peer_id,
                msg.sender_id,
                msg.sender_name,
                msg.receiver_id,
                msg.receiver_name,
                msg.content,
                msg.status,
                msg.created_at,
                if msg.is_incoming { 1 } else { 0 },
                if msg.is_read { 1 } else { 0 },
                msg.quote,
                if msg.is_edited { 1 } else { 0 },
                if msg.is_deleted { 1 } else { 0 },
                msg.reactions,
            ],
        )?;
        Ok(())
    }

    /// Save an incoming message idempotently: if the same message ID already arrived
    /// (for example, a queue retry after a dropped ACK), return Ok(false) so we do not
    /// overwrite `is_read` or `reactions`, nor fire a duplicate notification.
    pub fn save_incoming_message(&self, msg: &ChatMessage) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM messages WHERE id = ?1",
                params![msg.id],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if exists {
            return Ok(false);
        }
        conn.execute(
            "INSERT INTO messages
            (id, thread_peer_id, sender_id, sender_name, receiver_id, receiver_name, content, status, created_at, is_incoming, is_read, quote, is_edited, is_deleted, reactions)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                msg.id,
                msg.thread_peer_id,
                msg.sender_id,
                msg.sender_name,
                msg.receiver_id,
                msg.receiver_name,
                msg.content,
                msg.status,
                msg.created_at,
                if msg.is_incoming { 1 } else { 0 },
                if msg.is_read { 1 } else { 0 },
                msg.quote,
                if msg.is_edited { 1 } else { 0 },
                if msg.is_deleted { 1 } else { 0 },
                msg.reactions,
            ],
        )?;
        Ok(true)
    }

    pub fn get_message_by_id(&self, id: &str) -> Result<Option<ChatMessage>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, thread_peer_id, sender_id, sender_name, receiver_id, receiver_name,
                    content, status, created_at, is_incoming, is_read, quote, is_edited, is_deleted, reactions
             FROM messages WHERE id = ?1"
        )?;
        let mut rows = stmt.query_map(params![id], Self::map_message_row)?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    fn map_message_row(row: &rusqlite::Row<'_>) -> Result<ChatMessage, rusqlite::Error> {
        Ok(ChatMessage {
            id: row.get(0)?,
            thread_peer_id: row.get(1)?,
            sender_id: row.get(2)?,
            sender_name: row.get(3)?,
            receiver_id: row.get(4)?,
            receiver_name: row.get(5)?,
            content: row.get(6)?,
            status: row.get(7)?,
            created_at: row.get(8)?,
            is_incoming: row.get::<_, i64>(9)? != 0,
            is_read: row.get::<_, i64>(10)? != 0,
            quote: row.get(11)?,
            is_edited: row.get::<_, i64>(12)? != 0,
            is_deleted: row.get::<_, i64>(13)? != 0,
            reactions: row.get(14)?,
        })
    }

    /// Returns the LATEST `limit` messages in chronological (oldest-to-newest) order.
    pub fn get_messages(&self, peer_id: &str, limit: usize) -> Result<Vec<ChatMessage>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, thread_peer_id, sender_id, sender_name, receiver_id, receiver_name,
                    content, status, created_at, is_incoming, is_read, quote, is_edited, is_deleted, reactions
             FROM (
                 SELECT id, thread_peer_id, sender_id, sender_name, receiver_id, receiver_name,
                        content, status, created_at, is_incoming, is_read, quote, is_edited, is_deleted, reactions
                 FROM messages
                 WHERE thread_peer_id = ?1
                 ORDER BY created_at DESC
                 LIMIT ?2
             )
             ORDER BY created_at ASC"
        )?;

        let rows = stmt.query_map(params![peer_id, limit as i64], Self::map_message_row)?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    pub fn update_status(&self, message_id: &str, status: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET status = ?1 WHERE id = ?2",
            params![status, message_id],
        )?;
        Ok(())
    }

    pub fn mark_as_read(&self, peer_id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET is_read = 1 WHERE thread_peer_id = ?1 AND is_incoming = 1",
            params![peer_id],
        )?;
        Ok(())
    }

    /// Mark our outgoing messages to `peer_id` as Read when the remote device sends a read receipt.
    pub fn mark_outgoing_read(&self, peer_id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let time_str = Local::now().format("%I:%M %p").to_string();
        let read_status = format!("Read, {}", time_str);
        conn.execute(
            "UPDATE messages
             SET status = ?1
             WHERE thread_peer_id = ?2
               AND is_incoming = 0
               AND is_deleted = 0
               AND status NOT LIKE 'Read%'
               AND LOWER(status) != 'queued'
               AND status NOT LIKE 'Failed%'",
            params![read_status, peer_id],
        )?;
        Ok(())
    }

    pub fn get_unread_counts(&self) -> Result<HashMap<String, u32>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT thread_peer_id, COUNT(*) 
             FROM messages 
             WHERE is_read = 0 AND is_incoming = 1 
             GROUP BY thread_peer_id"
        )?;

        let rows = stmt.query_map([], |row| {
            let peer_id: String = row.get(0)?;
            let count: u32 = row.get(1)?;
            Ok((peer_id, count))
        })?;

        let mut map = HashMap::new();
        for r in rows {
            let (k, v) = r?;
            map.insert(k, v);
        }
        Ok(map)
    }

    pub fn edit_message(&self, message_id: &str, new_content: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET content = ?1, is_edited = 1 WHERE id = ?2 AND is_deleted = 0",
            params![new_content, message_id],
        )?;
        Ok(())
    }

    pub fn delete_message(&self, message_id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET content = 'This message was deleted', is_deleted = 1 WHERE id = ?1",
            params![message_id],
        )?;
        Ok(())
    }

    pub fn add_reaction(&self, message_id: &str, user_name: &str, emoji: &str) -> Result<String, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let current_json: Option<String> = conn
            .query_row("SELECT reactions FROM messages WHERE id = ?1", params![message_id], |row| row.get(0))
            .unwrap_or(None);

        let mut map: HashMap<String, Vec<String>> = current_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
            .unwrap_or_default();

        let users = map.entry(emoji.to_string()).or_default();
        if users.contains(&user_name.to_string()) {
            users.retain(|u| u != user_name);
            if users.is_empty() {
                map.remove(emoji);
            }
        } else {
            users.push(user_name.to_string());
        }

        let new_json = serde_json::to_string(&map).unwrap_or_default();
        conn.execute(
            "UPDATE messages SET reactions = ?1 WHERE id = ?2",
            params![&new_json, message_id],
        )?;
        Ok(new_json)
    }

    pub fn get_queued_messages(&self, peer_id: &str) -> Result<Vec<ChatMessage>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, thread_peer_id, sender_id, sender_name, receiver_id, receiver_name, 
                    content, status, created_at, is_incoming, is_read, quote, is_edited, is_deleted, reactions
             FROM messages
             WHERE thread_peer_id = ?1 AND LOWER(status) = 'queued' AND is_incoming = 0
             ORDER BY created_at ASC"
        )?;

        let rows = stmt.query_map(params![peer_id], Self::map_message_row)?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    /// Outgoing messages that were never delivered: "Queued" ones, plus ones stuck
    /// on "Sending" since before `stuck_before_ms` (the app was closed mid-send).
    pub fn get_undelivered_messages(&self, stuck_before_ms: i64) -> Result<Vec<ChatMessage>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, thread_peer_id, sender_id, sender_name, receiver_id, receiver_name, \
                    content, status, created_at, is_incoming, is_read, quote, is_edited, is_deleted, reactions \
             FROM messages \
             WHERE is_incoming = 0 AND is_deleted = 0 \
               AND (LOWER(status) = 'queued' OR (status LIKE 'Sending%' AND created_at < ?1)) \
             ORDER BY created_at ASC"
        )?;
        let rows = stmt.query_map(params![stuck_before_ms], Self::map_message_row)?;
        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    pub fn save_transfer(&self, record: &TransferRecord) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO transfers 
            (id, peer_id, sender_name, receiver_name, file_name, file_size, status, created_at, local_path, is_incoming)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                record.id,
                record.peer_id,
                record.sender_name,
                record.receiver_name,
                record.file_name,
                record.file_size as i64,
                record.status,
                record.created_at,
                record.local_path,
                if record.is_incoming { 1 } else { 0 },
            ],
        )?;
        Ok(())
    }

    fn map_transfer_row(row: &rusqlite::Row<'_>) -> Result<TransferRecord, rusqlite::Error> {
        let file_size_i64: i64 = row.get(5)?;
        let is_inc_i64: i64 = row.get::<_, Option<i64>>(9)?.unwrap_or(0);
        Ok(TransferRecord {
            id: row.get(0)?,
            peer_id: row.get(1)?,
            sender_name: row.get(2)?,
            receiver_name: row.get(3)?,
            file_name: row.get(4)?,
            file_size: file_size_i64.max(0) as u64,
            status: row.get(6)?,
            created_at: row.get(7)?,
            local_path: row.get(8)?,
            is_incoming: is_inc_i64 != 0,
        })
    }

    pub fn get_transfers(&self, limit: usize) -> Result<Vec<TransferRecord>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, peer_id, sender_name, receiver_name, file_name, file_size, status, created_at, local_path, is_incoming
             FROM transfers
             ORDER BY created_at DESC
             LIMIT ?1"
        )?;

        let rows = stmt.query_map(params![limit as i64], Self::map_transfer_row)?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    /// Delete every message and file record of one conversation.
    pub fn clear_thread(&self, peer_id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM messages WHERE thread_peer_id = ?1", params![peer_id])?;
        conn.execute("DELETE FROM transfers WHERE peer_id = ?1", params![peer_id])?;
        Ok(())
    }

    /// Delete all transfer history records.
    pub fn clear_all_transfers(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM transfers", [])?;
        Ok(())
    }

    pub fn get_timeline(&self, peer_id: &str, limit: usize) -> Result<Vec<TimelineItem>, rusqlite::Error> {
        let messages = self.get_messages(peer_id, limit)?;
        
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, peer_id, sender_name, receiver_name, file_name, file_size, status, created_at, local_path, is_incoming
             FROM transfers
             WHERE peer_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![peer_id, limit as i64], Self::map_transfer_row)?;
        let mut transfers = Vec::new();
        for r in rows {
            transfers.push(r?);
        }

        let mut timeline: Vec<TimelineItem> = Vec::new();
        for m in messages {
            timeline.push(TimelineItem::Message(m));
        }
        for t in transfers {
            timeline.push(TimelineItem::Transfer(t));
        }

        timeline.sort_by_key(|item| match item {
            TimelineItem::Message(m) => m.created_at,
            TimelineItem::Transfer(t) => t.created_at,
        });

        if timeline.len() > limit {
            timeline = timeline.split_off(timeline.len() - limit);
        }

        Ok(timeline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_messages_returns_latest_200_chronologically() {
        let tmp = std::env::temp_dir().join(format!("xent_db_msg_{}.db", uuid::Uuid::new_v4()));
        let db = Database::open_path(&tmp).unwrap();
        for i in 0..250 {
            let m = ChatMessage {
                id: format!("m-{}", i),
                thread_peer_id: "peer1".into(),
                sender_id: "me".into(),
                sender_name: "Me".into(),
                receiver_id: "peer1".into(),
                receiver_name: "Peer".into(),
                content: format!("msg {}", i),
                status: "Delivered".into(),
                created_at: 1000 + i,
                is_incoming: false,
                is_read: true,
                quote: None,
                is_edited: false,
                is_deleted: false,
                reactions: None,
            };
            db.save_message(&m).unwrap();
        }
        let msgs = db.get_messages("peer1", 200).unwrap();
        assert_eq!(msgs.len(), 200);
        assert_eq!(msgs.first().unwrap().content, "msg 50");
        assert_eq!(msgs.last().unwrap().content, "msg 249");
        let _ = fs::remove_file(tmp);
    }

    #[test]
    fn incoming_dedup_and_outgoing_read_receipt() {
        let tmp = std::env::temp_dir().join(format!("xent_db_read_{}.db", uuid::Uuid::new_v4()));
        let db = Database::open_path(&tmp).unwrap();
        let out_msg = ChatMessage {
            id: "out1".into(),
            thread_peer_id: "peer1".into(),
            sender_id: "me".into(),
            sender_name: "Me".into(),
            receiver_id: "peer1".into(),
            receiver_name: "Peer".into(),
            content: "hello".into(),
            status: "Delivered, 10:00 AM".into(),
            created_at: 1000,
            is_incoming: false,
            is_read: true,
            quote: None,
            is_edited: false,
            is_deleted: false,
            reactions: None,
        };
        db.save_message(&out_msg).unwrap();
        db.mark_outgoing_read("peer1").unwrap();
        let loaded = db.get_message_by_id("out1").unwrap().unwrap();
        assert!(loaded.status.starts_with("Read"));

        let inc = ChatMessage {
            id: "in1".into(),
            thread_peer_id: "peer1".into(),
            sender_id: "peer1".into(),
            sender_name: "Peer".into(),
            receiver_id: "me".into(),
            receiver_name: "Me".into(),
            content: "hi".into(),
            status: "delivered".into(),
            created_at: 2000,
            is_incoming: true,
            is_read: false,
            quote: None,
            is_edited: false,
            is_deleted: false,
            reactions: None,
        };
        assert!(db.save_incoming_message(&inc).unwrap());
        assert!(!db.save_incoming_message(&inc).unwrap(), "duplicate id should be ignored");
        let _ = fs::remove_file(tmp);
    }
}
