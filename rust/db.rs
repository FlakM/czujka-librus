use std::collections::HashSet;

use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{AnnouncementItem, GradeItem, HomeworkItem, MessageItem, RemarkItem};

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS announcements (
              id TEXT PRIMARY KEY,
              title TEXT,
              content TEXT,
              date TEXT,
              author TEXT,
              fetched_at TEXT
            );

            CREATE TABLE IF NOT EXISTS messages (
              id TEXT PRIMARY KEY,
              title TEXT,
              content TEXT,
              date TEXT,
              sender TEXT,
              fetched_at TEXT
            );

            CREATE TABLE IF NOT EXISTS grades (
              id TEXT PRIMARY KEY,
              subject TEXT,
              value TEXT,
              info TEXT,
              fetched_at TEXT
            );

            CREATE TABLE IF NOT EXISTS events (
              id TEXT PRIMARY KEY,
              title TEXT,
              day TEXT,
              description TEXT,
              fetched_at TEXT
            );

            CREATE TABLE IF NOT EXISTS homework (
              id TEXT PRIMARY KEY,
              subject TEXT,
              title TEXT,
              type TEXT,
              date_from TEXT,
              date_to TEXT,
              content TEXT,
              teacher TEXT,
              fetched_at TEXT
            );
            CREATE TABLE IF NOT EXISTS remarks_sent (
              id TEXT PRIMARY KEY,
              sent_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS calendar_sync (
              id TEXT PRIMARY KEY,
              payload TEXT NOT NULL,
              synced_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS vikunja_tasks (
              item_key TEXT PRIMARY KEY,
              task_id INTEGER NOT NULL,
              created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS meta (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    fn existing_ids(&self, table: &str) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare(&format!("SELECT id FROM {}", table))?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut ids = HashSet::new();
        for id in rows {
            ids.insert(id?);
        }
        Ok(ids)
    }

    pub fn existing_message_ids(&self) -> Result<HashSet<String>> {
        self.existing_ids("messages")
    }

    pub fn existing_announcement_ids(&self) -> Result<HashSet<String>> {
        self.existing_ids("announcements")
    }

    pub fn existing_grade_ids(&self) -> Result<HashSet<String>> {
        self.existing_ids("grades")
    }

    pub fn meta_get(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?",
                params![key],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn meta_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn existing_homework_ids(&self) -> Result<HashSet<String>> {
        self.existing_ids("homework")
    }

    pub fn save_message(&self, message: &MessageItem) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO messages (id, title, content, date, sender, fetched_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![
                message.id,
                message.title,
                message.body,
                message.date,
                message.user,
                now
            ],
        )?;
        Ok(())
    }

    pub fn save_announcement(&self, announcement: &AnnouncementItem) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO announcements (id, title, content, date, author, fetched_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![
                announcement.id,
                announcement.title,
                announcement.content,
                announcement.date,
                announcement.author,
                now
            ],
        )?;
        Ok(())
    }

    pub fn save_grade(&self, grade: &GradeItem) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR IGNORE INTO grades (id, subject, value, info, fetched_at) VALUES (?, ?, ?, ?, ?)",
            params![grade.id, grade.subject, grade.value, grade.description, now],
        )?;
        Ok(())
    }

    pub fn save_homework(&self, homework: &HomeworkItem) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO homework (id, subject, title, type, date_from, date_to, content, teacher, fetched_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                homework.id,
                homework.subject,
                homework.title,
                homework.kind,
                homework.from,
                homework.to,
                homework.content,
                homework.user,
                now
            ],
        )?;
        Ok(())
    }

    pub fn existing_remark_ids(&self) -> Result<HashSet<String>> {
        self.existing_ids("remarks_sent")
    }

    pub fn mark_remark_sent(&self, remark: &RemarkItem) -> Result<()> {
        self.conn.execute(
            "INSERT INTO remarks_sent (id, sent_at) VALUES (?, ?)",
            params![remark.id, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn calendar_payload(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT payload FROM calendar_sync WHERE id = ?",
                [id],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn mark_calendar_synced(&self, id: &str, payload: &str) -> Result<()> {
        self.conn.execute("INSERT INTO calendar_sync (id, payload, synced_at) VALUES (?, ?, ?) ON CONFLICT(id) DO UPDATE SET payload = excluded.payload, synced_at = excluded.synced_at",
            params![id, payload, Utc::now().to_rfc3339()])?;
        Ok(())
    }

    pub fn calendar_entries(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, payload FROM calendar_sync WHERE substr(id, 1, length(?1)) = ?1",
        )?;
        let rows = stmt.query_map([prefix], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Every synced event across every calendar this service has ever written
    /// to, including calendars it no longer targets (e.g. the family calendar
    /// used before the dedicated school calendar existed).
    pub fn all_calendar_entries(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare("SELECT id, payload FROM calendar_sync")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn forget_calendar_entry(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM calendar_sync WHERE id = ?", [id])?;
        Ok(())
    }

    pub fn existing_task_keys(&self) -> Result<HashSet<String>> {
        self.existing_ids("vikunja_tasks")
    }

    pub fn mark_task_created(&self, key: &str, task_id: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO vikunja_tasks (item_key, task_id, created_at) VALUES (?, ?, ?)",
            params![key, task_id, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_values_persist_and_can_be_updated() {
        let db = Database::new(":memory:").unwrap();
        assert!(db.meta_get("text_grades_baselined:2026").unwrap().is_none());

        db.meta_set("text_grades_baselined:2026", "1").unwrap();
        assert_eq!(
            db.meta_get("text_grades_baselined:2026")
                .unwrap()
                .as_deref(),
            Some("1")
        );

        db.meta_set("text_grades_baselined:2026", "2").unwrap();
        assert_eq!(
            db.meta_get("text_grades_baselined:2026")
                .unwrap()
                .as_deref(),
            Some("2")
        );
    }
}
