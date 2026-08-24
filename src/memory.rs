use rusqlite::{Connection, Result, params};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct MemoryDb {
    conn: Arc<Mutex<Connection>>,
}

impl MemoryDb {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Create Full-Text Search 5 (FTS5) table for fast keyword matching
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS server_memory USING fts5(
                topic,
                fact,
                added_by
            )",
            [],
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn save_memory(&self, topic: &str, fact: &str, added_by: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO server_memory (topic, fact, added_by) VALUES (?1, ?2, ?3)",
            params![topic, fact, added_by],
        )?;
        Ok(())
    }

    pub fn recall_memories(&self, query: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();

        // Sanitize query for FTS MATCH syntax (keep simple alphanumeric keywords)
        let clean_query: String = query
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect();

        if clean_query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let fts_query = clean_query
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" OR ");

        let mut stmt = conn.prepare(
            "SELECT topic, fact, added_by FROM server_memory
             WHERE server_memory MATCH ?1 ORDER BY rank LIMIT 5",
        )?;

        let rows = stmt.query_map(params![fts_query], |row| {
            let topic: String = row.get(0)?;
            let fact: String = row.get(1)?;
            let added_by: String = row.get(2)?;
            Ok(format!("[{topic}] {fact} (learned from {added_by})"))
        });

        match rows {
            Ok(mapped) => Ok(mapped.filter_map(|r| r.ok()).collect()),
            Err(_) => Ok(Vec::new()),
        }
    }
}
