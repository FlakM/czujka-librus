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
    env::var("SEND_EMAIL").map(|val| val == "true").unwrap_or(false)
}

fn generate_urgency_badge(urgency: &str) -> String {
    let color = match urgency {
        "PILNE" => "#dc3545",
        "NORMALNE" => "#ffc107",
        "NIEPILNE" => "#28a745",
        _ => "#ffc107",
    };
    format!(
        "<span style=\"background-color: {}; color: white; padding: 4px 8px; border-radius: 4px; font-size: 12px; font-weight: bold;\">{}</span>",
        color, urgency
    )
}

fn format_markdown(text: &str) -> String {
    BOLD_RE.replace_all(text, "<strong>$1</strong>").to_string()
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
  <style>
    body { font-family: Arial, sans-serif; line-height: 1.6; color: #333; max-width: 800px; margin: 0 auto; padding: 20px; }
    h1 { color: #2c3e50; border-bottom: 3px solid #3498db; padding-bottom: 10px; }
    h2 { color: #34495e; margin-top: 30px; }
    h3 { color: #495057; font-size: 16px; margin-top: 20px; }
    .summary { background-color: #f8f9fa; padding: 15px; border-left: 4px solid #3498db; margin: 20px 0; }
    .key-points { background-color: #fff; padding: 15px; border: 1px solid #dee2e6; border-radius: 5px; }
    ul { margin: 10px 0; }
    li { margin: 8px 0; }
    .footer { margin-top: 40px; padding-top: 20px; border-top: 1px solid #dee2e6; color: #6c757d; font-size: 12px; }
    .section { margin-bottom: 40px; }
    .item { margin: 15px 0; padding: 15px; background: #f8f9fa; border-radius: 5px; border-left: 3px solid #6c757d; }
    .item-header { font-weight: bold; color: #2c3e50; margin-bottom: 5px; }
    .item-meta { font-size: 13px; color: #6c757d; margin-bottom: 10px; }
    .grade-item { margin: 15px 0; padding: 15px; background: #e8f4f8; border-radius: 5px; border-left: 3px solid #17a2b8; }
    details { margin-top: 10px; }
    summary { cursor: pointer; color: #3498db; font-weight: 500; }
    summary:hover { color: #2980b9; }
    .item-content { margin-top: 10px; padding: 10px; background: white; border-radius: 3px; white-space: pre-wrap; }
  </style>
</head>
<body>
  <h1>📚 Nowe powiadomienia z Librusa</h1>
"#,
    );

    if let Some(data) = announcements_data {
        html.push_str(&format!(
            "\n  <div class=\"section\">\n    <h2>📢 Ogłoszenia {}</h2>\n    <div class=\"summary\">\n      <strong>Podsumowanie:</strong><br>\n      {}\n    </div>\n    <div class=\"key-points\">\n      <strong>Kluczowe punkty:</strong>\n      <ul>\n        {}\n      </ul>\n    </div>\n    <h3>Szczegóły ogłoszeń:</h3>\n",
            generate_urgency_badge(&data.urgency),
            format_markdown(&data.summary),
            data
                .key_points
                .iter()
                .map(|point| format!("<li>{}</li>", format_markdown(point)))
                .collect::<Vec<_>>()
                .join("")
        ));

        for (idx, item) in announcements.iter().enumerate() {
            html.push_str(&format!(
                "\n    <div class=\"item\">\n      <div class=\"item-header\">{} . {}</div>\n      <div class=\"item-meta\">Autor: {} | Data: {}</div>\n      <details>\n        <summary>Pokaż treść</summary>\n        <div class=\"item-content\">{}</div>\n      </details>\n    </div>\n",
                idx + 1,
                item.title,
                item.author,
                item.date,
                item.content
            ));
        }

        html.push_str("  </div>\n");
    }

    if let Some(data) = messages_data {
        html.push_str(&format!(
            "\n  <div class=\"section\">\n    <h2>✉️ Wiadomości {}</h2>\n    <div class=\"summary\">\n      <strong>Podsumowanie:</strong><br>\n      {}\n    </div>\n    <div class=\"key-points\">\n      <strong>Kluczowe punkty:</strong>\n      <ul>\n        {}\n      </ul>\n    </div>\n    <h3>Szczegóły wiadomości:</h3>\n",
            generate_urgency_badge(&data.urgency),
            format_markdown(&data.summary),
            data
                .key_points
                .iter()
                .map(|point| format!("<li>{}</li>", format_markdown(point)))
                .collect::<Vec<_>>()
                .join("")
        ));

        for (idx, item) in messages.iter().enumerate() {
            let message_url = format!(
                "https://synergia.librus.pl/wiadomosci/1/5/{}/f0",
                item.id
            );
            html.push_str(&format!(
                "\n    <div class=\"item\">\n      <div class=\"item-header\">{} . {} <a href=\"{}\" style=\"color: #3498db; font-size: 13px;\">[Otwórz w Librus]</a></div>\n      <div class=\"item-meta\">Od: {} | Data: {}</div>\n      <details>\n        <summary>Pokaż treść</summary>\n        <div class=\"item-content\">{}</div>\n      </details>\n    </div>\n",
                idx + 1,
                item.title,
                message_url,
                item.user,
                item.date,
                item.body
            ));
        }

        html.push_str("  </div>\n");
    }

    if let Some(data) = grades_data {
        html.push_str(&format!(
            "\n  <div class=\"section\">\n    <h2>⭐ Nowe oceny {}</h2>\n    <div class=\"summary\">\n      <strong>Podsumowanie:</strong><br>\n      {}\n    </div>\n    <div class=\"key-points\">\n      <strong>Kluczowe punkty:</strong>\n      <ul>\n        {}\n      </ul>\n    </div>\n    <h3>Szczegóły ocen:</h3>\n",
            generate_urgency_badge(&data.urgency),
            format_markdown(&data.summary),
            data
                .key_points
                .iter()
                .map(|point| format!("<li>{}</li>", format_markdown(point)))
                .collect::<Vec<_>>()
                .join("")
        ));

        for (idx, item) in grades.iter().enumerate() {
            html.push_str(&format!(
                "\n    <div class=\"grade-item\">\n      <div class=\"item-header\">{} . {} - {}</div>\n      <details>\n        <summary>Pokaż szczegóły</summary>\n        <div class=\"item-content\">{}</div>\n      </details>\n    </div>\n",
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
            "\n  <div class=\"section\">\n    <h2>📝 Zadania domowe {}</h2>\n    <div class=\"summary\">\n      <strong>Podsumowanie:</strong><br>\n      {}\n    </div>\n    <div class=\"key-points\">\n      <strong>Kluczowe punkty:</strong>\n      <ul>\n        {}\n      </ul>\n    </div>\n    <h3>Szczegóły zadań:</h3>\n",
            generate_urgency_badge(&data.urgency),
            format_markdown(&data.summary),
            data
                .key_points
                .iter()
                .map(|point| format!("<li>{}</li>", format_markdown(point)))
                .collect::<Vec<_>>()
                .join("")
        ));

        for (idx, item) in homework.iter().enumerate() {
            html.push_str(&format!(
                "\n    <div class=\"item\">\n      <div class=\"item-header\">{} . {} - {}</div>\n      <div class=\"item-meta\">Nauczyciel: {} | Typ: {} | Termin: {} - {}</div>\n      <details>\n        <summary>Pokaż treść</summary>\n        <div class=\"item-content\">{}</div>\n      </details>\n    </div>\n",
                idx + 1,
                item.subject,
                item.title,
                item.user,
                item.kind,
                item.from,
                item.to,
                item.content
            ));
        }

        html.push_str("  </div>\n");
    }

    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    html.push_str(&format!(
        "  <div class=\"footer\">\n    <p>To powiadomienie zostało wygenerowane automatycznie przez system monitorowania Librusa.</p>\n    <p>Data wygenerowania: {}</p>\n  </div>\n</body>\n</html>\n",
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

    let email_to = env::var("EMAIL_TO").map_err(|_| anyhow!("EMAIL_TO must be set when SEND_EMAIL=true"))?;
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

    let email = builder.subject(subject).singlepart(SinglePart::html(html))?;

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

    mailer.send(email).await.map_err(|err| anyhow!(err.to_string()))?;

    tracing::info!(
        urgency = max_urgency,
        recipients = recipients.join(","),
        recipient_count = recipients.len(),
        "Email notification sent successfully"
    );

    Ok(())
}
