use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct MessageItem {
    pub id: String,
    pub title: String,
    pub body: String,
    pub date: String,
    pub user: String,
}

#[derive(Debug, Clone)]
pub struct AnnouncementItem {
    pub id: String,
    pub title: String,
    pub content: String,
    pub date: String,
    pub author: String,
}

#[derive(Debug, Clone)]
pub struct GradeItem {
    pub id: String,
    pub subject: String,
    pub value: String,
    pub info: String,
}

#[derive(Debug, Clone)]
pub struct HomeworkItem {
    pub id: String,
    pub subject: String,
    pub title: String,
    pub kind: String,
    pub from: String,
    pub to: String,
    pub content: String,
    pub user: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub urgency: String,
    pub summary: String,
    #[serde(rename = "keyPoints")]
    pub key_points: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RemarkItem {
    pub id: String,
    pub content: String,
    pub date: String,
    pub teacher: String,
    pub kind: String,
    pub category: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub date: String,
    pub end_date: String,
    pub time_from: Option<String>,
    pub time_to: Option<String>,
    pub description: String,
    pub source_url: String,
}
