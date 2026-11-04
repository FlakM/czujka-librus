mod calendar;
mod db;
mod email;
mod logger;
mod models;
mod openai;
mod synergia;
mod text_utils;
mod vikunja;

use std::collections::{HashMap, HashSet};
use std::env;

use anyhow::{Context, Result};
use librus_rs::Client;
use sha2::{Digest, Sha256};

use crate::db::Database;
use crate::email::{send_notification, send_remarks};
use crate::models::{
    ActionTask, AnnouncementItem, GradeItem, GradeKind, HomeworkItem, MessageItem,
};
use crate::openai::OpenAiClient;
use crate::vikunja::{sync_tasks, VikunjaClient};

const DEFAULT_DB_PATH: &str = "./librus.db";

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    logger::init_logging();

    ensure_required_env()?;

    let web = synergia::SynergiaClient::from_env()
        .await
        .context("Librus web authentication failed")?;
    let remarks = web.remarks().await?;
    let events =
        if calendar::GoogleCalendar::configured() || env::args().any(|arg| arg == "--check") {
            web.events(chrono::Local::now().date_naive()).await?
        } else {
            Vec::new()
        };
    if env::args().any(|arg| arg == "--check") {
        tracing::info!(
            remarks = remarks.len(),
            events = events.len(),
            "Librus web access verified; no email, calendar writes or database changes"
        );
        return Ok(());
    }

    let db_path = env::var("DB_PATH").unwrap_or_else(|_| DEFAULT_DB_PATH.to_string());
    let db = Database::new(&db_path).context("Failed to initialize database")?;

    let sent = db.existing_remark_ids()?;
    let unsent_remarks: Vec<_> = remarks
        .iter()
        .filter(|remark| !sent.contains(&remark.id))
        .cloned()
        .collect();
    if !unsent_remarks.is_empty() {
        match send_remarks(&unsent_remarks).await {
            Ok(()) => {
                if env::var("SEND_EMAIL").as_deref() == Ok("true") {
                    for remark in &unsent_remarks {
                        db.mark_remark_sent(remark)?;
                    }
                }
            }
            Err(err) => {
                tracing::error!(
                    error = %err,
                    count = unsent_remarks.len(),
                    "Failed to send remarks email; will retry next run"
                );
            }
        }
    }
    if calendar::GoogleCalendar::configured() {
        match calendar::GoogleCalendar::from_env().await {
            Ok(calendar) => {
                if let Err(err) = calendar.sync(&db, &events).await {
                    tracing::error!(error = %err, "Calendar sync failed; will retry next run");
                }
            }
            Err(err) => tracing::error!(error = %err, "Calendar setup failed; will retry next run"),
        }
    }

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
    let mut client = Client::from_env()
        .await
        .context("Librus authentication failed")?;
    tracing::info!("Authentication successful");

    let new_announcements = fetch_new_announcements(&client, &db).await?;
    let new_messages = fetch_new_messages(&mut client, &db).await?;
    let new_grades = fetch_new_grades(&client, &db).await?;
    let categories = web.homework_categories().await?;
    let new_homeworks = fetch_new_homeworks(&client, &db, &categories).await?;

    if new_announcements.is_empty()
        && new_messages.is_empty()
        && new_grades.is_empty()
        && new_homeworks.is_empty()
    {
        tracing::info!("No new items to process");
        return Ok(());
    }

    let openai = OpenAiClient::from_env()?;

    // All four analyses are independent — run them concurrently.
    let announcements_future = async {
        if new_announcements.is_empty() {
            return (None, false);
        }
        tracing::info!("Analyzing announcements with OpenAI");
        match openai.summarize_announcements(&new_announcements).await {
            Ok(result) => (Some(result), true),
            Err(err) => {
                tracing::error!(error = %err, "OpenAI analysis failed for announcements");
                (Some(fallback_announcements(&new_announcements)), false)
            }
        }
    };
    let messages_future = async {
        if new_messages.is_empty() {
            return (None, false);
        }
        tracing::info!("Analyzing messages with OpenAI");
        match openai.summarize_messages(&new_messages).await {
            Ok(result) => (Some(result), true),
            Err(err) => {
                tracing::error!(error = %err, "OpenAI analysis failed for messages");
                (Some(fallback_messages(&new_messages)), false)
            }
        }
    };
    let grades_future = async {
        if new_grades.is_empty() {
            return (None, false);
        }
        tracing::info!("Analyzing grades with OpenAI");
        match openai.summarize_grades(&new_grades).await {
            Ok(result) => (Some(result), true),
            Err(err) => {
                tracing::error!(error = %err, "OpenAI analysis failed for grades");
                (Some(fallback_grades(&new_grades)), false)
            }
        }
    };
    let homework_future = async {
        if new_homeworks.is_empty() {
            return (None, false);
        }
        tracing::info!("Analyzing homework with OpenAI");
        match openai.summarize_homework(&new_homeworks).await {
            Ok(result) => (Some(result), true),
            Err(err) => {
                tracing::error!(error = %err, "OpenAI analysis failed for homework");
                (Some(fallback_homework(&new_homeworks)), false)
            }
        }
    };

    let (
        (announcements_analysis, announcements_ai_ok),
        (messages_analysis, messages_ai_ok),
        (grades_analysis, grades_ai_ok),
        (homework_analysis, homework_ai_ok),
    ) = tokio::join!(
        announcements_future,
        messages_future,
        grades_future,
        homework_future
    );

    // Collect action items for Vikunja: AI-extracted duties per section, with a
    // deterministic fallback for homework (always actionable) when AI failed.
    let mut pending_tasks: Vec<(String, ActionTask)> = Vec::new();
    if announcements_ai_ok {
        if let Some(analysis) = &announcements_analysis {
            pending_tasks.extend(collect_tasks(
                "ogloszenia",
                analysis,
                &new_announcements
                    .iter()
                    .map(|item| item.id.clone())
                    .collect::<Vec<_>>(),
            ));
        }
    }
    if messages_ai_ok {
        if let Some(analysis) = &messages_analysis {
            pending_tasks.extend(collect_tasks(
                "wiadomosci",
                analysis,
                &new_messages
                    .iter()
                    .map(|item| item.id.clone())
                    .collect::<Vec<_>>(),
            ));
        }
    }
    if grades_ai_ok {
        if let Some(analysis) = &grades_analysis {
            pending_tasks.extend(collect_tasks(
                "oceny",
                analysis,
                &new_grades
                    .iter()
                    .map(|item| item.id.clone())
                    .collect::<Vec<_>>(),
            ));
        }
    }
    match (&homework_analysis, homework_ai_ok) {
        (Some(analysis), true) => pending_tasks.extend(collect_tasks(
            "zadania",
            analysis,
            &new_homeworks
                .iter()
                .map(|item| item.id.clone())
                .collect::<Vec<_>>(),
        )),
        (Some(_), false) => pending_tasks.extend(fallback_homework_tasks(&new_homeworks)),
        (None, _) => {}
    }

    // Tasks are created before the email so the email can report them.
    let mut tasks_created = None;
    if !pending_tasks.is_empty() && VikunjaClient::configured() {
        match sync_tasks(&db, &pending_tasks).await {
            Ok(count) => {
                if count > 0 {
                    tracing::info!(count, "Created Vikunja tasks");
                }
                tasks_created = Some(count);
            }
            Err(err) => {
                tracing::error!(error = %err, "Vikunja task sync failed; email continues without task note");
            }
        }
    }

    send_notification(
        announcements_analysis.as_ref(),
        messages_analysis.as_ref(),
        grades_analysis.as_ref(),
        homework_analysis.as_ref(),
        &new_announcements,
        &new_messages,
        &new_grades,
        &new_homeworks,
        tasks_created,
    )
    .await?;

    tracing::info!("Service completed successfully");
    Ok(())
}

/// Stable dedup key per (section, source items, task title): the DB records
/// created keys so retries never duplicate tasks in Vikunja.
fn collect_tasks(
    section: &str,
    analysis: &models::Analysis,
    context_ids: &[String],
) -> Vec<(String, ActionTask)> {
    analysis
        .tasks
        .iter()
        .filter(|task| !task.title.trim().is_empty())
        .map(|task| {
            let mut hasher = Sha256::new();
            hasher.update(section.as_bytes());
            hasher.update([0xFFu8]);
            hasher.update(context_ids.join(",").as_bytes());
            hasher.update(task.title.trim().as_bytes());
            let digest = format!("{:x}", hasher.finalize());
            (
                format!("{}:{}", section, &digest[..16]),
                ActionTask {
                    title: truncate_title(task.title.trim(), 120),
                    due: task.due.clone(),
                },
            )
        })
        .collect()
}

fn fallback_homework_tasks(items: &[HomeworkItem]) -> Vec<(String, ActionTask)> {
    items
        .iter()
        .map(|item| {
            let mut hasher = Sha256::new();
            hasher.update(b"zadania-fallback");
            hasher.update(item.id.as_bytes());
            let digest = format!("{:x}", hasher.finalize());
            (
                format!("zadania:{}", &digest[..16]),
                ActionTask {
                    title: truncate_title(
                        &format!("{} — {} ({})", item.subject, item.kind, item.title),
                        120,
                    ),
                    due: Some(item.to.clone()),
                },
            )
        })
        .collect()
}

fn truncate_title(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max - 1).collect::<String>())
    }
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
        subject: None,
        tasks: Vec::new(),
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
        subject: None,
        tasks: Vec::new(),
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
            .map(|item| match item.kind {
                GradeKind::Numeric => format!("📚 {} | ⭐ **{}**", item.subject, item.value),
                GradeKind::Text => {
                    let description: String = item.description.chars().take(90).collect();
                    let ellipsis = if item.description.chars().count() > 90 {
                        "…"
                    } else {
                        ""
                    };
                    format!(
                        "📚 {} | 📝 **{}** — {}{}",
                        item.subject, item.category, description, ellipsis
                    )
                }
            })
            .collect(),
        subject: None,
        tasks: Vec::new(),
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
        subject: None,
        tasks: Vec::new(),
    }
}

async fn fetch_new_announcements(client: &Client, db: &Database) -> Result<Vec<AnnouncementItem>> {
    let existing_ids = db.existing_announcement_ids()?;
    let limit = env::var("LIBRUS_ANNOUNCEMENTS_LIMIT")
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or(50);

    let notices = match client.school_notices().await {
        Ok(response) => {
            let mut notices = response.school_notices;
            notices.sort_by(|a, b| b.creation_date.cmp(&a.creation_date));
            notices.truncate(limit);
            notices
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to fetch school notices, skipping announcements");
            return Ok(Vec::new());
        }
    };
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

        // Skip messages older than 30 days
        if !text_utils::is_within_days(&message.send_date, 30) {
            continue;
        }

        let raw_body = match client.message(&message.message_id).await {
            Ok(detail) => Client::decode_message_content(&detail.message).unwrap_or_default(),
            Err(e) => {
                tracing::warn!(
                    message_id = %message.message_id,
                    error = %e,
                    "Failed to fetch full message, using truncated content"
                );
                Client::decode_message_content(&message.content).unwrap_or_default()
            }
        };

        // Clean the message body
        let body = text_utils::clean_message_body(&raw_body);
        if body.is_empty() && !raw_body.trim().is_empty() {
            // Should not happen; if it does, surface it instead of mailing an
            // empty message again.
            tracing::warn!(
                message_id = %message.message_id,
                "Message body became empty after cleaning"
            );
        }

        // Skip simple acknowledgments, but always deliver replies: a teacher
        // answering our message is exactly the content we want in the email.
        let reply = text_utils::strip_quoted_thread(&body);
        let is_reply = message.topic.trim_start().to_lowercase().starts_with("re:");
        if text_utils::is_remark_notification(&body)
            || (!is_reply && text_utils::is_simple_acknowledgment(&reply))
        {
            tracing::debug!(
                message_id = %message.message_id,
                "Skipping acknowledgment or automatic uwagi notice"
            );
            db.save_message(&MessageItem {
                id: message.message_id.clone(),
                title: message.topic.clone(),
                body: body.clone(),
                date: message.send_date.clone(),
                user: message.sender_name.clone(),
            })?;
            continue;
        }

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
    let school_year = school_year();
    let mut subject_cache: HashMap<i32, String> = HashMap::new();
    let mut new_grades = Vec::new();

    // Numeric grades, e.g. "5", "4+", "86%". Their IDs do not reset between
    // school years, so the raw ID stays the dedupe key (and remains compatible
    // with rows written before text grades existed).
    for grade in client.grades().await?.grades {
        let grade_id = grade.id.to_string();
        if existing_ids.contains(&grade_id) {
            continue;
        }

        let subject_name = subject_name(client, &mut subject_cache, grade.subject.id).await;

        let mut description = String::new();
        if let Some(comments) = grade.comments.as_ref() {
            if let Some(comment_ref) = comments.first() {
                if let Ok(response) = client.grade_comment(comment_ref.id).await {
                    if let Some(comment) = response.comment {
                        description = comment.text;
                    }
                }
            }
        }

        let item = GradeItem {
            id: grade_id,
            subject: subject_name,
            value: grade.grade,
            description,
            category: String::new(),
            date: grade.date,
            kind: GradeKind::Numeric,
        };

        db.save_grade(&item)?;
        new_grades.push(item);
    }

    // Text grades: "oceny kształtujące" (skill-based) and "oceny bieżące"
    // (category-based). BaseTextGrades returns both in one response.
    let text_grades = client.base_text_grades().await?.grades;

    // Resolve only the lookups the response actually references. The skills
    // list is large (~300 KB), so skip it for accounts that have no
    // skill-based grades.
    let categories: HashMap<i32, String> =
        if text_grades.iter().any(|grade| grade.category.is_some()) {
            match client.text_grade_categories().await {
                Ok(response) => response
                    .categories
                    .into_iter()
                    .map(|category| (category.id as i32, category.name))
                    .collect(),
                Err(err) => {
                    tracing::warn!(error = %err, "Could not fetch text grade categories");
                    HashMap::new()
                }
            }
        } else {
            HashMap::new()
        };
    let skills: HashMap<i32, String> = if text_grades.iter().any(|grade| grade.skill.is_some()) {
        match client.descriptive_grade_skills().await {
            Ok(response) => response
                .skills
                .into_iter()
                .map(|skill| (skill.id as i32, skill.name))
                .collect(),
            Err(err) => {
                tracing::warn!(error = %err, "Could not fetch descriptive grade skills");
                HashMap::new()
            }
        }
    } else {
        HashMap::new()
    };

    // The first run for a school year records the grades that already exist
    // without notifying about them. Librus resets text grade IDs every school
    // year, so the baseline is scoped to the year.
    let baseline_key = format!("text_grades_baselined:{school_year}");
    let baselined = db.meta_get(&baseline_key)?.is_some();
    let mut seeded = 0usize;

    for grade in text_grades {
        // Prefix the dedupe key so text and numeric grade ID spaces can never
        // collide, even if Librus reuses an ID.
        let grade_id = format!("text:{school_year}:{}", grade.id);
        if existing_ids.contains(&grade_id) || !grade.show_in_grades_view {
            continue;
        }

        let subject_name = subject_name(client, &mut subject_cache, grade.subject.id).await;
        let category = grade
            .category
            .as_ref()
            .and_then(|category| categories.get(&category.id).cloned())
            .or_else(|| {
                grade
                    .skill
                    .as_ref()
                    .and_then(|skill| skills.get(&skill.id).cloned())
            })
            .unwrap_or_else(|| "ocena opisowa".to_string());

        let item = GradeItem {
            id: grade_id,
            subject: subject_name,
            value: "T".to_string(),
            description: grade.grade,
            category,
            date: grade.date,
            kind: GradeKind::Text,
        };

        db.save_grade(&item)?;
        if baselined {
            new_grades.push(item);
        } else {
            seeded += 1;
        }
    }

    if !baselined {
        db.meta_set(&baseline_key, "1")?;
        tracing::info!(
            school_year,
            seeded,
            "Recorded existing text grades as the notification baseline"
        );
    }

    new_grades.sort_by(|a, b| a.date.cmp(&b.date));

    if new_grades.is_empty() {
        tracing::info!("No new grades");
    } else {
        tracing::info!(count = new_grades.len(), "Found new grades");
    }

    Ok(new_grades)
}

/// The school year that a grade added today belongs to: September starts a new
/// year, so anything before September belongs to the previous calendar year.
fn school_year() -> i32 {
    school_year_for(chrono::Local::now().date_naive())
}

fn school_year_for(date: chrono::NaiveDate) -> i32 {
    use chrono::Datelike;
    if date.month() >= 9 {
        date.year()
    } else {
        date.year() - 1
    }
}

async fn subject_name(
    client: &Client,
    cache: &mut HashMap<i32, String>,
    subject_id: i32,
) -> String {
    if let Some(name) = cache.get(&subject_id) {
        return name.clone();
    }
    let name = client
        .subject(subject_id)
        .await
        .ok()
        .and_then(|resp| resp.subject)
        .map(|subject| subject.name)
        .unwrap_or_else(|| "Nieznany".to_string());
    cache.insert(subject_id, name.clone());
    name
}

async fn fetch_new_homeworks(
    client: &Client,
    db: &Database,
    categories: &HashMap<i64, String>,
) -> Result<Vec<HomeworkItem>> {
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
            kind: categories
                .get(&hw.category.id)
                .cloned()
                .unwrap_or_else(|| "Wpis terminarza".to_string()),
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
        tracing::info!(
            count = new_homeworks.len(),
            "Found new homework assignments"
        );
    }

    Ok(new_homeworks)
}

fn derive_title(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return "Zadanie domowe".to_string();
    }
    let first_line = trimmed.lines().next().unwrap_or(trimmed).trim();
    if first_line.chars().count() <= 60 {
        first_line.to_string()
    } else {
        format!("{}...", first_line.chars().take(60).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn school_year_starts_in_september() {
        let date = |value: &str| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap();
        assert_eq!(school_year_for(date("2026-08-31")), 2025);
        assert_eq!(school_year_for(date("2026-09-01")), 2026);
        assert_eq!(school_year_for(date("2027-08-31")), 2026);
        assert_eq!(school_year_for(date("2027-09-01")), 2027);
    }

    #[tokio::test]
    #[ignore = "live test: logs into Librus and writes a temporary database"]
    async fn live_grades_are_deduped_across_runs() {
        let path = "/tmp/opencode/librus-live-grades.db";
        let _ = std::fs::remove_file(path);
        let db = Database::new(path).unwrap();
        let client = Client::from_env().await.expect("live Librus login");

        let first = fetch_new_grades(&client, &db).await.unwrap();
        let baseline_key = format!("text_grades_baselined:{}", school_year());
        assert!(db.meta_get(&baseline_key).unwrap().is_some());

        let second = fetch_new_grades(&client, &db).await.unwrap();
        assert!(
            second.is_empty(),
            "second run reported duplicates: {:?}",
            second.iter().map(|grade| &grade.id).collect::<Vec<_>>()
        );
        assert!(
            first.iter().any(|grade| grade.kind == GradeKind::Numeric),
            "empty database should report existing numeric grades"
        );
    }
}
