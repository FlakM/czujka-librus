import Database from 'better-sqlite3';
import { logger } from './logger.js';

let db;

function getDBPath() {
  return process.env.DB_PATH || './librus.db';
}

export function initDatabase() {
  db = new Database(getDBPath());

  db.exec(`
    CREATE TABLE IF NOT EXISTS announcements (
      id TEXT PRIMARY KEY,
      title TEXT,
      content TEXT,
      date TEXT,
      author TEXT,
      fetched_at TEXT
    )
  `);

  db.exec(`
    CREATE TABLE IF NOT EXISTS messages (
      id TEXT PRIMARY KEY,
      title TEXT,
      content TEXT,
      date TEXT,
      sender TEXT,
      fetched_at TEXT
    )
  `);

  db.exec(`
    CREATE TABLE IF NOT EXISTS grades (
      id TEXT PRIMARY KEY,
      subject TEXT,
      value TEXT,
      info TEXT,
      fetched_at TEXT
    )
  `);

  db.exec(`
    CREATE TABLE IF NOT EXISTS events (
      id TEXT PRIMARY KEY,
      title TEXT,
      day TEXT,
      description TEXT,
      fetched_at TEXT
    )
  `);

  db.exec(`
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
    )
  `);

  logger.info('Database initialized');
}

export function getExistingAnnouncementIds() {
  const stmt = db.prepare('SELECT id FROM announcements');
  return new Set(stmt.all().map(row => row.id.toString()));
}

export function getExistingMessageIds() {
  const stmt = db.prepare('SELECT id FROM messages');
  return new Set(stmt.all().map(row => row.id.toString()));
}

export function getExistingGradeIds() {
  const stmt = db.prepare('SELECT id FROM grades');
  return new Set(stmt.all().map(row => row.id.toString()));
}

export function getExistingEventIds() {
  const stmt = db.prepare('SELECT id FROM events');
  return new Set(stmt.all().map(row => row.id.toString()));
}

export function getExistingHomeworkIds() {
  const stmt = db.prepare('SELECT id FROM homework');
  return new Set(stmt.all().map(row => row.id.toString()));
}

export function saveAnnouncement(announcement) {
  const stmt = db.prepare(`
    INSERT INTO announcements (id, title, content, date, author, fetched_at)
    VALUES (?, ?, ?, ?, ?, ?)
  `);

  stmt.run(
    announcement.id.toString(),
    announcement.title || '',
    announcement.content || '',
    announcement.date || '',
    announcement.user || announcement.author || '',
    new Date().toISOString()
  );
}

export function saveMessage(message) {
  const stmt = db.prepare(`
    INSERT INTO messages (id, title, content, date, sender, fetched_at)
    VALUES (?, ?, ?, ?, ?, ?)
  `);

  stmt.run(
    message.id.toString(),
    message.title || '',
    message.body || '',
    message.date || '',
    message.user || '',
    new Date().toISOString()
  );
}

export function saveGrade(grade) {
  const stmt = db.prepare(`
    INSERT INTO grades (id, subject, value, info, fetched_at)
    VALUES (?, ?, ?, ?, ?)
  `);

  stmt.run(
    grade.id.toString(),
    grade.subject || '',
    grade.value || '',
    grade.info || '',
    new Date().toISOString()
  );
}

export function saveEvent(event) {
  const stmt = db.prepare(`
    INSERT INTO events (id, title, day, description, fetched_at)
    VALUES (?, ?, ?, ?, ?)
  `);

  stmt.run(
    event.id.toString(),
    event.title || '',
    event.day || '',
    event.description || '',
    new Date().toISOString()
  );
}

export function saveHomework(homework) {
  const stmt = db.prepare(`
    INSERT INTO homework (id, subject, title, type, date_from, date_to, content, teacher, fetched_at)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
  `);

  stmt.run(
    homework.id.toString(),
    homework.subject || '',
    homework.title || '',
    homework.type || '',
    homework.from || '',
    homework.to || '',
    homework.content || '',
    homework.user || '',
    new Date().toISOString()
  );
}

export function closeDatabase() {
  if (db) {
    db.close();
    logger.info('Database closed');
  }
}
