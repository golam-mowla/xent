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
    pub status: String, // "sending", "delivered", "read", "failed"
    pub created_at: i64, // timestamp in ms
    pub is_incoming: bool,
    pub is_read: bool,
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
                is_read INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_thread ON messages(thread_peer_id, created_at);
            CREATE INDEX IF NOT EXISTS idx_unread ON messages(is_read, is_incoming);"
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn save_message(&self, msg: &ChatMessage) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO messages 
            (id, thread_peer_id, sender_id, sender_name, receiver_id, receiver_name, content, status, created_at, is_incoming, is_read)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
            ],
        )?;
        Ok(())
    }

    pub fn get_messages(&self, peer_id: &str, limit: usize) -> Result<Vec<ChatMessage>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, thread_peer_id, sender_id, sender_name, receiver_id, receiver_name, 
                    content, status, created_at, is_incoming, is_read
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
}
