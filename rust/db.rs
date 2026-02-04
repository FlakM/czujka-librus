use std::collections::HashSet;

use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};

use crate::models::{AnnouncementItem, GradeItem, HomeworkItem, MessageItem};

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
            "#,
        )?;
        Ok(())
    }

    fn existing_ids(&self, table: &str) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare(&format!("SELECT id FROM {}", table))?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut ids = HashSet::new();
        for id in rows {
            if let Ok(id) = id {
                ids.insert(id);
            }
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
            "INSERT INTO grades (id, subject, value, info, fetched_at) VALUES (?, ?, ?, ?, ?)",
            params![grade.id, grade.subject, grade.value, grade.info, now],
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
}
