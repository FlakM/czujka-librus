mod db;
mod email;
mod logger;
mod models;
mod openai;

use std::collections::{HashMap, HashSet};
use std::env;

use anyhow::{Context, Result};
use librus_rs::Client;

use crate::db::Database;
use crate::email::send_notification;
use crate::models::{AnnouncementItem, GradeItem, HomeworkItem, MessageItem};
use crate::openai::OpenAiClient;

const DEFAULT_DB_PATH: &str = "./librus.db";

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    logger::init_logging();

    ensure_required_env()?;

    let db_path = env::var("DB_PATH").unwrap_or_else(|_| DEFAULT_DB_PATH.to_string());
    let db = Database::new(&db_path).context("Failed to initialize database")?;

    tracing::info!("Starting Librus notification service");

    if env::var("SEND_EMAIL").map(|v| v == "true").unwrap_or(false) {
        if let Ok(email_to) = env::var("EMAIL_TO") {
            let recipients: Vec<_> = email_to
                .split(',')
                .map(|email| email.trim())
                .filter(|email| !email.is_empty())
                .collect();
            tracing::info!(
                recipient_count = recipients.len(),
                recipients = recipients.join(","),
                "Email notifications enabled"
            );
        } else {
            tracing::info!("Email notifications enabled, but EMAIL_TO is not set");
        }
    } else {
        tracing::info!("Email notifications disabled");
    }

    tracing::info!("Authenticating with Librus");
    let mut client = Client::from_env().await.context("Librus authentication failed")?;
    tracing::info!("Authentication successful");

    let new_announcements = fetch_new_announcements(&client, &db).await?;
    let new_messages = fetch_new_messages(&mut client, &db).await?;
    let new_grades = fetch_new_grades(&client, &db).await?;
    let new_homeworks = fetch_new_homeworks(&client, &db).await?;

    if new_announcements.is_empty() && new_messages.is_empty() && new_grades.is_empty() && new_homeworks.is_empty() {
        tracing::info!("No new items to process");
        return Ok(());
    }

    let openai = OpenAiClient::from_env()?;

    let announcements_analysis = if new_announcements.is_empty() {
        None
    } else {
        tracing::info!("Analyzing announcements with OpenAI");
        match openai.summarize_announcements(&new_announcements).await {
            Ok(result) => Some(result),
            Err(err) => {
                tracing::error!(error = %err, "OpenAI analysis failed for announcements");
                Some(fallback_announcements(&new_announcements))
            }
        }
    };

    let messages_analysis = if new_messages.is_empty() {
        None
    } else {
        tracing::info!("Analyzing messages with OpenAI");
        match openai.summarize_messages(&new_messages).await {
            Ok(result) => Some(result),
            Err(err) => {
                tracing::error!(error = %err, "OpenAI analysis failed for messages");
                Some(fallback_messages(&new_messages))
            }
        }
    };

    let grades_analysis = if new_grades.is_empty() {
        None
    } else {
        tracing::info!("Analyzing grades with OpenAI");
        match openai.summarize_grades(&new_grades).await {
            Ok(result) => Some(result),
            Err(err) => {
                tracing::error!(error = %err, "OpenAI analysis failed for grades");
                Some(fallback_grades(&new_grades))
            }
        }
    };

    let homework_analysis = if new_homeworks.is_empty() {
        None
    } else {
        tracing::info!("Analyzing homework with OpenAI");
        match openai.summarize_homework(&new_homeworks).await {
            Ok(result) => Some(result),
            Err(err) => {
                tracing::error!(error = %err, "OpenAI analysis failed for homework");
                Some(fallback_homework(&new_homeworks))
            }
        }
    };

    send_notification(
        announcements_analysis.as_ref(),
        messages_analysis.as_ref(),
        grades_analysis.as_ref(),
        homework_analysis.as_ref(),
        &new_announcements,
        &new_messages,
        &new_grades,
        &new_homeworks,
    )
    .await?;

    tracing::info!("Service completed successfully");
    Ok(())
}

fn ensure_required_env() -> Result<()> {
    if env::var("LIBRUS_USERNAME").is_err() || env::var("LIBRUS_PASSWORD").is_err() {
        anyhow::bail!("LIBRUS_USERNAME and LIBRUS_PASSWORD must be set in .env file");
    }
    if env::var("OPENAI_API_KEY").is_err() {
        anyhow::bail!("OPENAI_API_KEY must be set in .env file");
    }
    Ok(())
}

fn fallback_announcements(items: &[AnnouncementItem]) -> crate::models::Analysis {
    crate::models::Analysis {
        urgency: "NORMALNE".to_string(),
        summary: format!(
            "Nie udało się wygenerować podsumowania. Liczba ogłoszeń: {}",
            items.len()
        ),
        key_points: items.iter().map(|item| item.title.clone()).collect(),
    }
}

fn fallback_messages(items: &[MessageItem]) -> crate::models::Analysis {
    crate::models::Analysis {
        urgency: "NORMALNE".to_string(),
        summary: format!(
            "Nie udało się wygenerować podsumowania. Liczba wiadomości: {}",
            items.len()
        ),
        key_points: items.iter().map(|item| item.title.clone()).collect(),
    }
}

fn fallback_grades(items: &[GradeItem]) -> crate::models::Analysis {
    crate::models::Analysis {
        urgency: "NORMALNE".to_string(),
        summary: format!(
            "Nie udało się wygenerować podsumowania. Liczba ocen: {}",
            items.len()
        ),
        key_points: items
            .iter()
            .map(|item| format!("📚 {} | ⭐ **{}**", item.subject, item.value))
            .collect(),
    }
}

fn fallback_homework(items: &[HomeworkItem]) -> crate::models::Analysis {
    crate::models::Analysis {
        urgency: "NORMALNE".to_string(),
        summary: format!(
            "Nie udało się wygenerować podsumowania. Liczba zadań domowych: {}",
            items.len()
        ),
        key_points: items
            .iter()
            .map(|item| format!("📝 **{}** - {}", item.subject, item.title))
            .collect(),
    }
}

async fn fetch_new_announcements(client: &Client, db: &Database) -> Result<Vec<AnnouncementItem>> {
    let existing_ids = db.existing_announcement_ids()?;
    let limit = env::var("LIBRUS_ANNOUNCEMENTS_LIMIT")
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or(50);

    let notices = client.school_notices_latest(limit).await?;
    let mut author_cache: HashMap<i32, String> = HashMap::new();
    let mut new_announcements = Vec::new();

    for notice in notices {
        if existing_ids.contains(&notice.id) {
            continue;
        }

        let author_name = if let Ok(author_id) = notice.added_by.id.parse::<i32>() {
            if let Some(name) = author_cache.get(&author_id) {
                name.clone()
            } else {
                let name = client
                    .user(author_id)
                    .await
                    .ok()
                    .and_then(|resp| resp.user)
                    .map(|user| format!("{} {}", user.first_name, user.last_name))
                    .unwrap_or_else(|| "Nieznany".to_string());
                author_cache.insert(author_id, name.clone());
                name
            }
        } else {
            "Nieznany".to_string()
        };

        let item = AnnouncementItem {
            id: notice.id,
            title: notice.subject,
            content: Client::notice_content_to_text(&notice.content),
            date: notice.creation_date,
            author: author_name,
        };

        db.save_announcement(&item)?;
        new_announcements.push(item);
    }

    if new_announcements.is_empty() {
        tracing::info!("No new announcements");
    } else {
        tracing::info!(count = new_announcements.len(), "Found new announcements");
    }

    Ok(new_announcements)
}

async fn fetch_new_messages(client: &mut Client, db: &Database) -> Result<Vec<MessageItem>> {
    let existing_ids = db.existing_message_ids()?;
    let max_pages = env::var("LIBRUS_MESSAGES_MAX_PAGES")
        .ok()
        .and_then(|val| val.parse::<u32>().ok())
        .unwrap_or(10);
    let limit = env::var("LIBRUS_MESSAGES_PAGE_SIZE")
        .ok()
        .and_then(|val| val.parse::<u32>().ok())
        .unwrap_or(50);

    let outbox_ids = fetch_outbox_ids(client, max_pages, limit).await?;
    let inbox_messages = fetch_inbox_messages(client, max_pages, limit).await?;

    let mut new_messages = Vec::new();

    for message in inbox_messages {
        if outbox_ids.contains(&message.message_id) {
            continue;
        }
        if existing_ids.contains(&message.message_id) {
            continue;
        }

        let body = Client::decode_message_content(&message.content).unwrap_or_default();
        let item = MessageItem {
            id: message.message_id.clone(),
            title: message.topic,
            body,
            date: message.send_date,
            user: message.sender_name,
        };
        db.save_message(&item)?;
        new_messages.push(item);
    }

    if new_messages.is_empty() {
        tracing::info!("No new messages");
    } else {
        tracing::info!(count = new_messages.len(), "Found new messages");
    }

    Ok(new_messages)
}

async fn fetch_outbox_ids(
    client: &mut Client,
    max_pages: u32,
    limit: u32,
) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();
    for page in 1..=max_pages {
        let messages = client.outbox_messages(page, limit).await?;
        if messages.is_empty() {
            break;
        }
        let batch_len = messages.len() as u32;
        for message in messages {
            ids.insert(message.message_id);
        }
        if batch_len < limit {
            break;
        }
    }
    Ok(ids)
}

async fn fetch_inbox_messages(
    client: &mut Client,
    max_pages: u32,
    limit: u32,
) -> Result<Vec<librus_rs::InboxMessage>> {
    let mut all = Vec::new();
    for page in 1..=max_pages {
        let messages = client.inbox_messages(page, limit).await?;
        if messages.is_empty() {
            break;
        }
        let batch_len = messages.len() as u32;
        all.extend(messages);
        if batch_len < limit {
            break;
        }
    }
    Ok(all)
}

async fn fetch_new_grades(client: &Client, db: &Database) -> Result<Vec<GradeItem>> {
    let existing_ids = db.existing_grade_ids()?;
    let response = client.grades().await?;

    let mut subject_cache: HashMap<i32, String> = HashMap::new();
    let mut new_grades = Vec::new();

    for grade in response.grades {
        let grade_id = grade.id.to_string();
        if existing_ids.contains(&grade_id) {
            continue;
        }

        let subject_id = grade.subject.id;
        let subject_name = if let Some(name) = subject_cache.get(&subject_id) {
            name.clone()
        } else {
            let name = client
                .subject(subject_id)
                .await
                .ok()
                .and_then(|resp| resp.subject)
                .map(|subject| subject.name)
                .unwrap_or_else(|| "Nieznany".to_string());
            subject_cache.insert(subject_id, name.clone());
            name
        };

        let mut info = String::new();
        if let Some(comments) = grade.comments.as_ref() {
            if let Some(comment_ref) = comments.first() {
                if let Ok(comment) = client.grade_comment(comment_ref.id).await {
                    if let Some(comment) = comment.comment {
                        info = comment.text;
                    }
                }
            }
        }

        let item = GradeItem {
            id: grade_id,
            subject: subject_name,
            value: grade.grade,
            info,
        };

        db.save_grade(&item)?;
        new_grades.push(item);
    }

    if new_grades.is_empty() {
        tracing::info!("No new grades");
    } else {
        tracing::info!(count = new_grades.len(), "Found new grades");
    }

    Ok(new_grades)
}

async fn fetch_new_homeworks(client: &Client, db: &Database) -> Result<Vec<HomeworkItem>> {
    let existing_ids = db.existing_homework_ids()?;
    let response = client.homeworks().await?;

    let mut subject_cache: HashMap<i32, String> = HashMap::new();
    let mut teacher_cache: HashMap<i32, String> = HashMap::new();
    let mut new_homeworks = Vec::new();

    for hw in response.homeworks {
        let homework_id = hw.id.to_string();
        if existing_ids.contains(&homework_id) {
            continue;
        }

        let subject_name = if let Some(subject_ref) = hw.subject.as_ref() {
            let subject_id = subject_ref.id as i32;
            if let Some(name) = subject_cache.get(&subject_id) {
                name.clone()
            } else {
                let name = client
                    .subject(subject_id)
                    .await
                    .ok()
                    .and_then(|resp| resp.subject)
                    .map(|subject| subject.name)
                    .unwrap_or_else(|| "Nieznany".to_string());
                subject_cache.insert(subject_id, name.clone());
                name
            }
        } else {
            "Nieznany".to_string()
        };

        let teacher_name = {
            let teacher_id = hw.created_by.id as i32;
            if let Some(name) = teacher_cache.get(&teacher_id) {
                name.clone()
            } else {
                let name = client
                    .user(teacher_id)
                    .await
                    .ok()
                    .and_then(|resp| resp.user)
                    .map(|user| format!("{} {}", user.first_name, user.last_name))
                    .unwrap_or_else(|| "Nieznany".to_string());
                teacher_cache.insert(teacher_id, name.clone());
                name
            }
        };

        let title = derive_title(&hw.content);
        let item = HomeworkItem {
            id: homework_id,
            subject: subject_name,
            title,
            kind: "Zadanie domowe".to_string(),
            from: hw.date.clone(),
            to: hw.date.clone(),
            content: hw.content,
            user: teacher_name,
        };

        db.save_homework(&item)?;
        new_homeworks.push(item);
    }

    if new_homeworks.is_empty() {
        tracing::info!("No new homework");
    } else {
        tracing::info!(count = new_homeworks.len(), "Found new homework assignments");
    }

    Ok(new_homeworks)
}

fn derive_title(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return "Zadanie domowe".to_string();
    }
    let first_line = trimmed.lines().next().unwrap_or(trimmed).trim();
    if first_line.len() <= 60 {
        first_line.to_string()
    } else {
        format!("{}...", &first_line[..60])
    }
}
