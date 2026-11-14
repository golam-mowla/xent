use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
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
        let dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Xent");
        let _ = fs::create_dir_all(&dir);

        let db_name = if instance == "default" {
            "chat.db".to_string()
        } else {
            format!("chat_{}.db", instance)
        };
        let db_path = dir.join(db_name);

        let conn = Connection::open(&db_path)?;

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
                local_path TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_transfers_created ON transfers(created_at DESC);"
        )?;

        // Safe migrations for existing databases
        let _ = conn.execute("ALTER TABLE messages ADD COLUMN quote TEXT", []);
        let _ = conn.execute("ALTER TABLE messages ADD COLUMN is_edited INTEGER DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE messages ADD COLUMN is_deleted INTEGER DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE messages ADD COLUMN reactions TEXT", []);
        let _ = conn.execute("ALTER TABLE transfers ADD COLUMN local_path TEXT", []);

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
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

    pub fn get_messages(&self, peer_id: &str, limit: usize) -> Result<Vec<ChatMessage>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, thread_peer_id, sender_id, sender_name, receiver_id, receiver_name, 
                    content, status, created_at, is_incoming, is_read, quote, is_edited, is_deleted, reactions
             FROM messages
             WHERE thread_peer_id = ?1
             ORDER BY created_at ASC
             LIMIT ?2"
        )?;

        let rows = stmt.query_map(params![peer_id, limit as i64], |row| {
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
        })?;

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
            "UPDATE messages SET content = ?1, is_edited = 1 WHERE id = ?2",
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
             WHERE thread_peer_id = ?1 AND status = 'queued' AND is_incoming = 0
             ORDER BY created_at ASC"
        )?;

        let rows = stmt.query_map(params![peer_id], |row| {
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
        })?;

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
            (id, peer_id, sender_name, receiver_name, file_name, file_size, status, created_at, local_path)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
            ],
        )?;
        Ok(())
    }

    pub fn get_transfers(&self, limit: usize) -> Result<Vec<TransferRecord>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, peer_id, sender_name, receiver_name, file_name, file_size, status, created_at, local_path
             FROM transfers
             ORDER BY created_at DESC
             LIMIT ?1"
        )?;

        let rows = stmt.query_map(params![limit as i64], |row| {
            let file_size_i64: i64 = row.get(5)?;
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
            })
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    pub fn get_timeline(&self, peer_id: &str, limit: usize) -> Result<Vec<TimelineItem>, rusqlite::Error> {
        let messages = self.get_messages(peer_id, limit)?;
        
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, peer_id, sender_name, receiver_name, file_name, file_size, status, created_at, local_path
             FROM transfers
             WHERE peer_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![peer_id, limit as i64], |row| {
            let file_size_i64: i64 = row.get(5)?;
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
            })
        })?;
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
