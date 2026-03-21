use std::env;

use anyhow::{anyhow, Result};
use chrono::Local;
use lettre::message::SinglePart;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use once_cell::sync::Lazy;
use regex::Regex;

use crate::models::{Analysis, AnnouncementItem, GradeItem, HomeworkItem, MessageItem};

static BOLD_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\*\*([^*]+)\*\*").unwrap());
static TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]*>").unwrap());

fn should_send_email() -> bool {
    env::var("SEND_EMAIL")
        .map(|val| val == "true")
        .unwrap_or(false)
}

fn generate_urgency_badge(urgency: &str) -> String {
    let class = match urgency {
        "PILNE" => "badge-urgent",
        "NORMALNE" => "badge-normal",
        "NIEPILNE" => "badge-low",
        _ => "badge-normal",
    };
    format!(r#"<span class="badge {}">{}</span>"#, class, urgency)
}

fn format_markdown(text: &str) -> String {
    BOLD_RE.replace_all(text, "<strong>$1</strong>").to_string()
}

fn format_date(date_str: &str) -> String {
    if let Some(date_part) = date_str.split('T').next() {
        date_part.to_string()
    } else {
        date_str.to_string()
    }
}

fn get_subject_prefix(urgency: &str) -> &'static str {
    match urgency {
        "PILNE" => "[PILNE]",
        "NIEPILNE" => "[NIEPILNE]",
        _ => "[NORMALNE]",
    }
}

fn generate_html_email(
    announcements_data: Option<&Analysis>,
    messages_data: Option<&Analysis>,
    grades_data: Option<&Analysis>,
    homework_data: Option<&Analysis>,
    announcements: &[AnnouncementItem],
    messages: &[MessageItem],
    grades: &[GradeItem],
    homework: &[HomeworkItem],
) -> String {
    let mut html = String::from(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <style>
    * { box-sizing: border-box; }
    body {
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
      line-height: 1.6;
      color: #1a1a2e;
      max-width: 700px;
      margin: 0 auto;
      padding: 24px 16px;
      background: #f8fafc;
    }
    .header {
      background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
      color: white;
      padding: 24px;
      border-radius: 16px;
      margin-bottom: 24px;
      text-align: center;
    }
    .header h1 {
      margin: 0;
      font-size: 24px;
      font-weight: 600;
    }
    .header .date {
      opacity: 0.9;
      font-size: 14px;
      margin-top: 8px;
    }
    .section {
      background: white;
      border-radius: 12px;
      padding: 20px;
      margin-bottom: 20px;
      box-shadow: 0 1px 3px rgba(0,0,0,0.08);
    }
    .section-header {
      display: flex;
      align-items: center;
      gap: 12px;
      margin-bottom: 16px;
      padding-bottom: 12px;
      border-bottom: 2px solid #f1f5f9;
    }
    .section-icon {
      font-size: 24px;
    }
    .section-title {
      font-size: 18px;
      font-weight: 600;
      color: #1e293b;
      margin: 0;
    }
    .badge {
      display: inline-block;
      padding: 4px 10px;
      border-radius: 20px;
      font-size: 11px;
      font-weight: 600;
      text-transform: uppercase;
      letter-spacing: 0.5px;
    }
    .badge-urgent { background: #fee2e2; color: #dc2626; }
    .badge-normal { background: #fef3c7; color: #d97706; }
    .badge-low { background: #d1fae5; color: #059669; }
    .summary-box {
      background: linear-gradient(135deg, #f0f9ff 0%, #e0f2fe 100%);
      padding: 16px;
      border-radius: 10px;
      margin-bottom: 16px;
      border-left: 4px solid #0ea5e9;
    }
    .summary-title {
      font-weight: 600;
      color: #0369a1;
      margin-bottom: 8px;
      font-size: 13px;
      text-transform: uppercase;
      letter-spacing: 0.5px;
    }
    .summary-text {
      color: #334155;
      font-size: 14px;
    }
    .key-points {
      background: #fafafa;
      padding: 16px;
      border-radius: 10px;
    }
    .key-points-title {
      font-weight: 600;
      color: #475569;
      margin-bottom: 12px;
      font-size: 13px;
      text-transform: uppercase;
      letter-spacing: 0.5px;
    }
    .key-points ul {
      margin: 0;
      padding-left: 0;
      list-style: none;
    }
    .key-points li {
      padding: 8px 0;
      border-bottom: 1px solid #e2e8f0;
      font-size: 14px;
      color: #334155;
    }
    .key-points li:last-child { border-bottom: none; }
    .details-header {
      font-size: 13px;
      font-weight: 600;
      color: #64748b;
      text-transform: uppercase;
      letter-spacing: 0.5px;
      margin: 20px 0 12px 0;
    }
    .item {
      background: #f8fafc;
      border-radius: 8px;
      padding: 14px;
      margin-bottom: 10px;
      border: 1px solid #e2e8f0;
    }
    .item-header {
      font-weight: 600;
      color: #1e293b;
      font-size: 14px;
      margin-bottom: 6px;
    }
    .item-header a {
      color: #6366f1;
      text-decoration: none;
      font-weight: 500;
      font-size: 12px;
    }
    .item-header a:hover { text-decoration: underline; }
    .item-meta {
      font-size: 12px;
      color: #64748b;
      margin-bottom: 8px;
    }
    details { margin-top: 8px; }
    summary {
      cursor: pointer;
      color: #6366f1;
      font-weight: 500;
      font-size: 13px;
      padding: 4px 0;
    }
    summary:hover { color: #4f46e5; }
    .item-content {
      margin-top: 10px;
      padding: 12px;
      background: white;
      border-radius: 6px;
      font-size: 13px;
      color: #475569;
      white-space: pre-wrap;
      border: 1px solid #e2e8f0;
    }
    .grade-item {
      background: linear-gradient(135deg, #ecfdf5 0%, #d1fae5 100%);
      border: 1px solid #a7f3d0;
    }
    .footer {
      text-align: center;
      padding: 20px;
      color: #94a3b8;
      font-size: 12px;
    }
    .footer a { color: #6366f1; text-decoration: none; }
  </style>
</head>
<body>
  <div class="header">
    <h1>📚 Powiadomienia z Librusa</h1>
    <div class="date">Emilka • Klasa 1A SP ETE</div>
  </div>
"#,
    );

    if let Some(data) = announcements_data {
        html.push_str(&format!(
            r#"
  <div class="section">
    <div class="section-header">
      <span class="section-icon">📢</span>
      <h2 class="section-title">Ogłoszenia</h2>
      {}
    </div>
    <div class="summary-box">
      <div class="summary-title">Podsumowanie</div>
      <div class="summary-text">{}</div>
    </div>
    <div class="key-points">
      <div class="key-points-title">Do zrobienia</div>
      <ul>{}</ul>
    </div>
    <div class="details-header">Szczegóły ogłoszeń</div>
"#,
            generate_urgency_badge(&data.urgency),
            format_markdown(&data.summary),
            data.key_points
                .iter()
                .map(|point| format!("<li>{}</li>", format_markdown(point)))
                .collect::<Vec<_>>()
                .join("")
        ));

        for (idx, item) in announcements.iter().enumerate() {
            html.push_str(&format!(
                r#"
    <div class="item">
      <div class="item-header">{}. {}</div>
      <div class="item-meta">👤 {} • 📅 {}</div>
      <details>
        <summary>Pokaż treść</summary>
        <div class="item-content">{}</div>
      </details>
    </div>
"#,
                idx + 1,
                item.title,
                item.author,
                format_date(&item.date),
                item.content
            ));
        }
        html.push_str("  </div>\n");
    }

    if let Some(data) = messages_data {
        html.push_str(&format!(
            r#"
  <div class="section">
    <div class="section-header">
      <span class="section-icon">✉️</span>
      <h2 class="section-title">Wiadomości</h2>
      {}
    </div>
    <div class="summary-box">
      <div class="summary-title">Podsumowanie</div>
      <div class="summary-text">{}</div>
    </div>
    <div class="key-points">
      <div class="key-points-title">Do zrobienia</div>
      <ul>{}</ul>
    </div>
    <div class="details-header">Szczegóły wiadomości ({} szt.)</div>
"#,
            generate_urgency_badge(&data.urgency),
            format_markdown(&data.summary),
            data.key_points
                .iter()
                .map(|point| format!("<li>{}</li>", format_markdown(point)))
                .collect::<Vec<_>>()
                .join(""),
            messages.len()
        ));

        for (idx, item) in messages.iter().enumerate() {
            let message_url = format!("https://synergia.librus.pl/wiadomosci/1/5/{}/f0", item.id);
            html.push_str(&format!(
                r#"
    <div class="item">
      <div class="item-header">{}. {} <a href="{}">[Otwórz]</a></div>
      <div class="item-meta">👤 {} • 📅 {}</div>
      <details>
        <summary>Pokaż treść</summary>
        <div class="item-content">{}</div>
      </details>
    </div>
"#,
                idx + 1,
                item.title,
                message_url,
                item.user,
                format_date(&item.date),
                item.body
            ));
        }
        html.push_str("  </div>\n");
    }

    if let Some(data) = grades_data {
        html.push_str(&format!(
            r#"
  <div class="section">
    <div class="section-header">
      <span class="section-icon">⭐</span>
      <h2 class="section-title">Nowe oceny</h2>
      {}
    </div>
    <div class="summary-box">
      <div class="summary-title">Podsumowanie</div>
      <div class="summary-text">{}</div>
    </div>
    <div class="key-points">
      <div class="key-points-title">Szczegóły</div>
      <ul>{}</ul>
    </div>
    <div class="details-header">Oceny</div>
"#,
            generate_urgency_badge(&data.urgency),
            format_markdown(&data.summary),
            data.key_points
                .iter()
                .map(|point| format!("<li>{}</li>", format_markdown(point)))
                .collect::<Vec<_>>()
                .join("")
        ));

        for (idx, item) in grades.iter().enumerate() {
            html.push_str(&format!(
                r#"
    <div class="item grade-item">
      <div class="item-header">{}. {} — {}</div>
      <details>
        <summary>Pokaż szczegóły</summary>
        <div class="item-content">{}</div>
      </details>
    </div>
"#,
                idx + 1,
                item.subject,
                item.value,
                item.info
            ));
        }
        html.push_str("  </div>\n");
    }

    if let Some(data) = homework_data {
        html.push_str(&format!(
            r#"
  <div class="section">
    <div class="section-header">
      <span class="section-icon">📝</span>
      <h2 class="section-title">Zadania domowe</h2>
      {}
    </div>
    <div class="summary-box">
      <div class="summary-title">Podsumowanie</div>
      <div class="summary-text">{}</div>
    </div>
    <div class="key-points">
      <div class="key-points-title">Do zrobienia</div>
      <ul>{}</ul>
    </div>
    <div class="details-header">Szczegóły zadań</div>
"#,
            generate_urgency_badge(&data.urgency),
            format_markdown(&data.summary),
            data.key_points
                .iter()
                .map(|point| format!("<li>{}</li>", format_markdown(point)))
                .collect::<Vec<_>>()
                .join("")
        ));

        for (idx, item) in homework.iter().enumerate() {
            html.push_str(&format!(
                r#"
    <div class="item">
      <div class="item-header">{}. {} — {}</div>
      <div class="item-meta">👤 {} • 📅 {} → {}</div>
      <details>
        <summary>Pokaż treść</summary>
        <div class="item-content">{}</div>
      </details>
    </div>
"#,
                idx + 1,
                item.subject,
                item.title,
                item.user,
                format_date(&item.from),
                format_date(&item.to),
                item.content
            ));
        }
        html.push_str("  </div>\n");
    }

    let timestamp = Local::now().format("%Y-%m-%d %H:%M");
    html.push_str(&format!(
        r#"
  <div class="footer">
    Wygenerowano automatycznie {} • <a href="https://synergia.librus.pl">Otwórz Librus</a>
  </div>
</body>
</html>
"#,
        timestamp
    ));

    html
}

pub async fn send_notification(
    announcements_data: Option<&Analysis>,
    messages_data: Option<&Analysis>,
    grades_data: Option<&Analysis>,
    homework_data: Option<&Analysis>,
    announcements: &[AnnouncementItem],
    messages: &[MessageItem],
    grades: &[GradeItem],
    homework: &[HomeworkItem],
) -> Result<()> {
    let has_announcements = announcements_data.map_or(false, |d| !d.key_points.is_empty());
    let has_messages = messages_data.map_or(false, |d| !d.key_points.is_empty());
    let has_grades = grades_data.map_or(false, |d| !d.key_points.is_empty());
    let has_homework = homework_data.map_or(false, |d| !d.key_points.is_empty());

    if !has_announcements && !has_messages && !has_grades && !has_homework {
        tracing::info!("No new items to notify about");
        return Ok(());
    }

    let mut urgencies = Vec::new();
    if let Some(data) = announcements_data {
        urgencies.push(data.urgency.as_str());
    }
    if let Some(data) = messages_data {
        urgencies.push(data.urgency.as_str());
    }
    if let Some(data) = grades_data {
        urgencies.push(data.urgency.as_str());
    }
    if let Some(data) = homework_data {
        urgencies.push(data.urgency.as_str());
    }

    let max_urgency = if urgencies.iter().any(|u| *u == "PILNE") {
        "PILNE"
    } else if urgencies.iter().any(|u| *u == "NORMALNE") {
        "NORMALNE"
    } else {
        "NIEPILNE"
    };

    let subject = format!(
        "{} Nowe powiadomienia z Librusa",
        get_subject_prefix(max_urgency)
    );

    let html = generate_html_email(
        announcements_data,
        messages_data,
        grades_data,
        homework_data,
        announcements,
        messages,
        grades,
        homework,
    );

    if !should_send_email() {
        tracing::info!("EMAIL SENDING DISABLED - Notification content:");
        println!("\n{}", "=".repeat(80));
        println!("Subject: {}", subject);
        println!("{}", "=".repeat(80));
        let preview = TAG_RE.replace_all(&html, " ");
        let preview = preview.split_whitespace().collect::<Vec<_>>().join(" ");
        let preview = if preview.len() > 500 {
            format!("{}...", &preview[..500])
        } else {
            preview
        };
        println!("\nHTML Preview (simplified):");
        println!("{}", preview);
        println!("\n{}\n", "=".repeat(80));
        return Ok(());
    }

    let email_to =
        env::var("EMAIL_TO").map_err(|_| anyhow!("EMAIL_TO must be set when SEND_EMAIL=true"))?;
    let recipients: Vec<String> = email_to
        .split(',')
        .map(|email| email.trim().to_string())
        .filter(|email| !email.is_empty())
        .collect();

    if recipients.is_empty() {
        return Err(anyhow!("EMAIL_TO must contain at least one recipient"));
    }

    let from = env::var("EMAIL_FROM").or_else(|_| env::var("EMAIL_USER"))?;

    let mut builder = Message::builder().from(from.parse()?);
    for recipient in &recipients {
        builder = builder.to(recipient.parse()?);
    }

    let email = builder
        .subject(subject)
        .singlepart(SinglePart::html(html))?;

    let host = env::var("EMAIL_HOST").map_err(|_| anyhow!("EMAIL_HOST must be set"))?;
    let port: u16 = env::var("EMAIL_PORT")
        .ok()
        .and_then(|val| val.parse().ok())
        .unwrap_or(587);

    let user = env::var("EMAIL_USER").map_err(|_| anyhow!("EMAIL_USER must be set"))?;
    let password = env::var("EMAIL_PASSWORD").map_err(|_| anyhow!("EMAIL_PASSWORD must be set"))?;
    let creds = Credentials::new(user, password);

    let mailer = if port == 465 {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&host)
            .map_err(|err| anyhow!("Failed to create SMTP transport: {}", err))?
            .port(port)
            .credentials(creds)
            .build()
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)
            .map_err(|err| anyhow!("Failed to create SMTP transport: {}", err))?
            .port(port)
            .credentials(creds)
            .build()
    };

    tracing::info!(
        urgency = max_urgency,
        recipients = recipients.join(","),
        recipient_count = recipients.len(),
        "Sending email notification"
    );

    mailer
        .send(email)
        .await
        .map_err(|err| anyhow!(err.to_string()))?;

    tracing::info!(
        urgency = max_urgency,
        recipients = recipients.join(","),
        recipient_count = recipients.len(),
        "Email notification sent successfully"
    );

    Ok(())
}
