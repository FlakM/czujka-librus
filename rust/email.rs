use std::env;

use anyhow::{anyhow, Result};
use chrono::{Datelike, Local, NaiveDate};
use lettre::message::MultiPart;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use once_cell::sync::Lazy;
use regex::Regex;

use crate::models::{
    ActionTask, Analysis, AnnouncementItem, GradeItem, GradeKind, HomeworkItem, MessageItem,
    RemarkItem,
};
use crate::vikunja::polish_weekday;

static BOLD_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\*\*([^*]+)\*\*").unwrap());
static TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]*>").unwrap());
static MARKDOWN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[*_`#>]+").unwrap());

const MAX_SUBJECT_LENGTH: usize = 140;

fn should_send_email() -> bool {
    env::var("SEND_EMAIL")
        .map(|val| val == "true")
        .unwrap_or(false)
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Escape HTML entities first, then turn the surviving `**` markers into
/// <strong> — user-provided content can never inject markup this way.
fn render_markdown(text: &str) -> String {
    BOLD_RE
        .replace_all(&escape_html(text), "<strong>$1</strong>")
        .to_string()
}

fn strip_markdown(text: &str) -> String {
    MARKDOWN_RE
        .replace_all(text, "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// "2026-10-20T14:30:00" → "20.10.2026 (pon)"
fn format_date_pl(date_str: &str) -> String {
    let date_part = date_str.split('T').next().unwrap_or(date_str);
    match NaiveDate::parse_from_str(date_part, "%Y-%m-%d") {
        Ok(date) => format!(
            "{} ({})",
            date.format("%d.%m.%Y"),
            polish_weekday(date.weekday())
        ),
        Err(_) => date_str.to_string(),
    }
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max - 1).collect::<String>())
    }
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

/// The sender already identifies Librus, so the subject carries only the
/// priority as a traffic-light emoji: red urgent, amber normal, green low.
fn urgency_emoji(urgency: &str) -> &'static str {
    match urgency {
        "PILNE" => "🔴",
        "NORMALNE" => "🟡",
        _ => "🟢",
    }
}

fn plural(n: usize, one: &str, few: &str, many: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        2..=4 => format!("{n} {few}"),
        _ => format!("{n} {many}"),
    }
}

/// The AI-written one-line gist for a section, or a locally composed fallback.
fn section_subject(analysis: Option<&Analysis>, fallback: String) -> String {
    analysis
        .and_then(|data| data.subject.as_deref())
        .map(|subject| subject.trim().to_string())
        .filter(|subject| !subject.is_empty())
        .unwrap_or(fallback)
}

fn fallback_subjects(
    announcements: &[AnnouncementItem],
    messages: &[MessageItem],
    grades: &[GradeItem],
    homework: &[HomeworkItem],
) -> (String, String, String, String) {
    let announcements_fallback = if announcements.is_empty() {
        String::new()
    } else if announcements.len() == 1 {
        truncate_chars(&announcements[0].title, 60)
    } else {
        format!(
            "📢 {} ogłoszeń (m.in. {})",
            announcements.len(),
            truncate_chars(&announcements[0].title, 40)
        )
    };

    let messages_fallback = if messages.is_empty() {
        String::new()
    } else if messages.len() == 1 {
        truncate_chars(&messages[0].title, 60)
    } else {
        format!(
            "✉️ {} wiadomości (m.in. {})",
            messages.len(),
            truncate_chars(&messages[0].title, 40)
        )
    };

    let grades_fallback = if grades.is_empty() {
        String::new()
    } else if grades.len() == 1 {
        format!("⭐ {}", grade_label(&grades[0]))
    } else {
        format!(
            "⭐ {} nowych ocen (pierwsza: {})",
            grades.len(),
            grade_label(&grades[0])
        )
    };

    let homework_fallback = if homework.is_empty() {
        String::new()
    } else if homework.len() == 1 {
        format!(
            "📝 {} {} — do {}",
            homework[0].subject,
            homework[0].kind.to_lowercase(),
            format_date_pl(&homework[0].to)
        )
    } else {
        format!(
            "📝 {} zadań, m.in. {} (do {})",
            homework.len(),
            homework[0].subject,
            format_date_pl(&homework[0].to)
        )
    };

    (
        announcements_fallback,
        messages_fallback,
        grades_fallback,
        homework_fallback,
    )
}

fn compose_subject(
    max_urgency: &str,
    announcements_data: Option<&Analysis>,
    messages_data: Option<&Analysis>,
    grades_data: Option<&Analysis>,
    homework_data: Option<&Analysis>,
    announcements: &[AnnouncementItem],
    messages: &[MessageItem],
    grades: &[GradeItem],
    homework: &[HomeworkItem],
) -> String {
    let (ann_fallback, msg_fallback, grade_fallback, hw_fallback) =
        fallback_subjects(announcements, messages, grades, homework);

    let parts = [
        section_subject(announcements_data, ann_fallback),
        section_subject(messages_data, msg_fallback),
        section_subject(grades_data, grade_fallback),
        section_subject(homework_data, hw_fallback),
    ]
    .iter()
    .filter(|part| !part.is_empty())
    .cloned()
    .collect::<Vec<_>>()
    .join(" | ");

    truncate_chars(
        &format!("{} Emilka: {}", urgency_emoji(max_urgency), parts),
        MAX_SUBJECT_LENGTH,
    )
}

// ---------------------------------------------------------------------------
// HTML rendering
// ---------------------------------------------------------------------------

fn sorted_by_date<'a, T, F>(items: &'a [T], date_of: F) -> Vec<&'a T>
where
    F: Fn(&'a T) -> &str,
{
    let mut refs: Vec<&'a T> = items.iter().collect();
    refs.sort_by(|a, b| date_of(a).cmp(date_of(b)));
    refs
}

/// Weak (1, 2, ndst) and shaky (3) grades stand out; solid ones stay green.
fn grade_class(value: &str) -> &'static str {
    match value.trim().chars().next().unwrap_or(' ') {
        '1' | '2' => "grade-bad",
        '3' => "grade-mid",
        _ => "grade-good",
    }
}

fn grade_css_class(item: &GradeItem) -> &'static str {
    match item.kind {
        GradeKind::Numeric => grade_class(&item.value),
        GradeKind::Text => "grade-text",
    }
}

/// One-line label used in subjects and fallbacks, e.g. `Matematyka: 5` or
/// `J. angielski: aktywność`.
fn grade_label(item: &GradeItem) -> String {
    match item.kind {
        GradeKind::Numeric => format!("{}: {}", item.subject, item.value),
        GradeKind::Text => format!("{}: {}", item.subject, item.category),
    }
}

fn grade_heading(item: &GradeItem) -> String {
    match item.kind {
        GradeKind::Numeric => format!("{} — {}", item.subject, item.value),
        GradeKind::Text => format!(
            "{} — {} · {}",
            item.subject,
            item.category,
            format_date_pl(&item.date)
        ),
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
    tasks_created: Option<usize>,
) -> String {
    // Hidden preheader: the inbox list shows the first section summary next to
    // the subject before anything is opened.
    let preheader = [
        announcements_data,
        messages_data,
        grades_data,
        homework_data,
    ]
    .iter()
    .flatten()
    .map(|data| strip_markdown(&data.summary))
    .next()
    .unwrap_or_default();

    let mut html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <style>
    * {{ box-sizing: border-box; }}
    body {{
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
      line-height: 1.6;
      color: #1a1a2e;
      max-width: 700px;
      margin: 0 auto;
      padding: 24px 16px;
      background: #f8fafc;
    }}
    .preheader {{ display: none; max-height: 0; overflow: hidden; }}
    .header {{
      background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
      color: white;
      padding: 24px;
      border-radius: 16px;
      margin-bottom: 24px;
      text-align: center;
    }}
    .header h1 {{
      margin: 0;
      font-size: 24px;
      font-weight: 600;
    }}
    .header .date {{
      opacity: 0.9;
      font-size: 14px;
      margin-top: 8px;
    }}
    .section {{
      background: white;
      border-radius: 12px;
      padding: 20px;
      margin-bottom: 20px;
      box-shadow: 0 1px 3px rgba(0,0,0,0.08);
    }}
    .section-header {{
      display: flex;
      align-items: center;
      gap: 12px;
      margin-bottom: 16px;
      padding-bottom: 12px;
      border-bottom: 2px solid #f1f5f9;
    }}
    .section-icon {{
      font-size: 24px;
    }}
    .section-title {{
      font-size: 18px;
      font-weight: 600;
      color: #1e293b;
      margin: 0;
    }}
    .section-header a {{ margin-left: auto; }}
    .badge {{
      display: inline-block;
      padding: 4px 10px;
      border-radius: 20px;
      font-size: 11px;
      font-weight: 600;
      text-transform: uppercase;
      letter-spacing: 0.5px;
    }}
    .badge-urgent {{ background: #fee2e2; color: #dc2626; }}
    .badge-normal {{ background: #fef3c7; color: #d97706; }}
    .badge-low {{ background: #d1fae5; color: #059669; }}
    .summary-box {{
      background: linear-gradient(135deg, #f0f9ff 0%, #e0f2fe 100%);
      padding: 16px;
      border-radius: 10px;
      margin-bottom: 16px;
      border-left: 4px solid #0ea5e9;
    }}
    .summary-title {{
      font-weight: 600;
      color: #0369a1;
      margin-bottom: 8px;
      font-size: 13px;
      text-transform: uppercase;
      letter-spacing: 0.5px;
    }}
    .summary-text {{
      color: #334155;
      font-size: 14px;
    }}
    .key-points {{
      background: #fafafa;
      padding: 16px;
      border-radius: 10px;
    }}
    .key-points-title {{
      font-weight: 600;
      color: #475569;
      margin-bottom: 12px;
      font-size: 13px;
      text-transform: uppercase;
      letter-spacing: 0.5px;
    }}
    .key-points ul {{
      margin: 0;
      padding-left: 0;
      list-style: none;
    }}
    .key-points li {{
      padding: 8px 0;
      border-bottom: 1px solid #e2e8f0;
      font-size: 14px;
      color: #334155;
    }}
    .key-points li:last-child {{ border-bottom: none; }}
    .details-header {{
      font-size: 13px;
      font-weight: 600;
      color: #64748b;
      text-transform: uppercase;
      letter-spacing: 0.5px;
      margin: 20px 0 12px 0;
    }}
    .item {{
      background: #f8fafc;
      border-radius: 8px;
      padding: 14px;
      margin-bottom: 10px;
      border: 1px solid #e2e8f0;
    }}
    .item-header {{
      font-weight: 600;
      color: #1e293b;
      font-size: 14px;
      margin-bottom: 6px;
    }}
    .item-header a {{
      color: #6366f1;
      text-decoration: none;
      font-weight: 500;
      font-size: 12px;
    }}
    .item-header a:hover {{ text-decoration: underline; }}
    .item-meta {{
      font-size: 12px;
      color: #64748b;
      margin-bottom: 8px;
    }}
    details {{ margin-top: 8px; }}
    summary {{
      cursor: pointer;
      color: #6366f1;
      font-weight: 500;
      font-size: 13px;
      padding: 4px 0;
    }}
    summary:hover {{ color: #4f46e5; }}
    .item-content {{
      margin-top: 10px;
      padding: 12px;
      background: white;
      border-radius: 6px;
      font-size: 13px;
      color: #475569;
      white-space: pre-wrap;
      border: 1px solid #e2e8f0;
    }}
    .grade-item {{
      border-radius: 8px;
      padding: 14px;
      margin-bottom: 10px;
      border: 1px solid #e2e8f0;
    }}
    .grade-good {{ background: linear-gradient(135deg, #ecfdf5 0%, #d1fae5 100%); border-color: #a7f3d0; }}
    .grade-mid {{ background: linear-gradient(135deg, #fffbeb 0%, #fef3c7 100%); border-color: #fde68a; }}
    .grade-bad {{ background: linear-gradient(135deg, #fef2f2 0%, #fee2e2 100%); border-color: #fecaca; }}
    .grade-text {{ background: linear-gradient(135deg, #eef2ff 0%, #e0e7ff 100%); border-color: #c7d2fe; }}
    .tasks-note {{
      background: #ecfdf5;
      border: 1px solid #a7f3d0;
      border-radius: 10px;
      padding: 12px 16px;
      margin-bottom: 20px;
      font-size: 13px;
      color: #065f46;
    }}
    .footer {{
      text-align: center;
      padding: 20px;
      color: #94a3b8;
      font-size: 12px;
    }}
    .footer a {{ color: #6366f1; text-decoration: none; }}
  </style>
</head>
<body>
  <div class="preheader">{}</div>
  <div class="header">
    <h1>📚 Powiadomienia z Librusa</h1>
    <div class="date">Emilka • Klasa 2A SP ETE</div>
  </div>
"#,
        escape_html(&preheader)
    );

    if let Some(data) = announcements_data {
        html.push_str(&format!(
            r#"
  <div class="section">
    <div class="section-header">
      <span class="section-icon">📢</span>
      <h2 class="section-title">Ogłoszenia</h2>
      {}
      <a href="https://synergia.librus.pl/ogloszenia">Otwórz ogłoszenia ↗</a>
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
            render_markdown(&data.summary),
            data.key_points
                .iter()
                .map(|point| format!("<li>{}</li>", render_markdown(point)))
                .collect::<Vec<_>>()
                .join("")
        ));

        for (idx, item) in sorted_by_date(announcements, |item| &item.date)
            .iter()
            .enumerate()
        {
            html.push_str(&format!(
                r#"
    <div class="item">
      <div class="item-header">{}. {} <a href="https://synergia.librus.pl/ogloszenia">[Otwórz]</a></div>
      <div class="item-meta">👤 {} • 📅 {}</div>
      <details>
        <summary>Pokaż treść</summary>
        <div class="item-content">{}</div>
      </details>
    </div>
"#,
                idx + 1,
                escape_html(&item.title),
                escape_html(&item.author),
                escape_html(&format_date_pl(&item.date)),
                escape_html(&item.content)
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
      <a href="https://synergia.librus.pl/wiadomosci">Otwórz wiadomości ↗</a>
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
            render_markdown(&data.summary),
            data.key_points
                .iter()
                .map(|point| format!("<li>{}</li>", render_markdown(point)))
                .collect::<Vec<_>>()
                .join(""),
            messages.len()
        ));

        for (idx, item) in sorted_by_date(messages, |item| &item.date)
            .iter()
            .enumerate()
        {
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
                escape_html(&item.title),
                escape_html(&message_url),
                escape_html(&item.user),
                escape_html(&format_date_pl(&item.date)),
                escape_html(&item.body)
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
      <a href="https://synergia.librus.pl/przegladaj_oceny/uczen">Otwórz oceny ↗</a>
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
            render_markdown(&data.summary),
            data.key_points
                .iter()
                .map(|point| format!("<li>{}</li>", render_markdown(point)))
                .collect::<Vec<_>>()
                .join("")
        ));

        for (idx, item) in grades.iter().enumerate() {
            // A text grade *is* its description, so keep it visible. Numeric
            // comments stay collapsed to keep the email short.
            let body = match item.kind {
                GradeKind::Text => format!(
                    r#"<div class="item-content">{}</div>"#,
                    escape_html(&item.description)
                ),
                GradeKind::Numeric => format!(
                    r#"<details><summary>Pokaż szczegóły</summary><div class="item-content">{}</div></details>"#,
                    escape_html(&item.description)
                ),
            };
            html.push_str(&format!(
                r#"
    <div class="item grade-item {}">
      <div class="item-header">{}. {} <a href="https://synergia.librus.pl/przegladaj_oceny/uczen">[Otwórz]</a></div>
      {}
    </div>
"#,
                grade_css_class(item),
                idx + 1,
                escape_html(&grade_heading(item)),
                body
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
      <h2 class="section-title">Zadania i terminarz</h2>
      {}
      <a href="https://synergia.librus.pl/terminarz">Otwórz terminarz ↗</a>
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
            render_markdown(&data.summary),
            data.key_points
                .iter()
                .map(|point| format!("<li>{}</li>", render_markdown(point)))
                .collect::<Vec<_>>()
                .join("")
        ));

        for (idx, item) in sorted_by_date(homework, |item| &item.from)
            .iter()
            .enumerate()
        {
            let deadline = if item.from == item.to {
                format_date_pl(&item.from)
            } else {
                format!(
                    "{} → {}",
                    format_date_pl(&item.from),
                    format_date_pl(&item.to)
                )
            };
            html.push_str(&format!(
                r#"
    <div class="item">
      <div class="item-header">{}. {} — {}: {} <a href="https://synergia.librus.pl/terminarz">[Otwórz]</a></div>
      <div class="item-meta">👤 {} • 📅 {}</div>
      <details>
        <summary>Pokaż treść</summary>
        <div class="item-content">{}</div>
      </details>
    </div>
"#,
                idx + 1,
                escape_html(&item.subject),
                escape_html(&item.kind),
                escape_html(&item.title),
                escape_html(&item.user),
                escape_html(&deadline),
                escape_html(&item.content)
            ));
        }
        html.push_str("  </div>\n");
    }

    if let Some(count) = tasks_created {
        if count > 0 {
            html.push_str(&format!(
                r#"
  <div class="tasks-note">📋 Dodano {} do listy <strong>Librus — Emilka</strong> w Vikunja (z terminami).</div>
"#,
                plural(count, "zadanie", "zadania", "zadań")
            ));
        }
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

// ---------------------------------------------------------------------------
// Plain-text rendering
// ---------------------------------------------------------------------------

fn key_points_text(data: &Analysis) -> String {
    data.key_points
        .iter()
        .map(|point| format!("  - {}", strip_markdown(point)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn generate_plain_email(
    announcements_data: Option<&Analysis>,
    messages_data: Option<&Analysis>,
    grades_data: Option<&Analysis>,
    homework_data: Option<&Analysis>,
    announcements: &[AnnouncementItem],
    messages: &[MessageItem],
    grades: &[GradeItem],
    homework: &[HomeworkItem],
    tasks_created: Option<usize>,
) -> String {
    let mut text = format!(
        "EMILKA — LIBRUS, {}\n",
        Local::now().format("%d.%m.%Y (%a) %H:%M")
    );

    if let Some(data) = announcements_data {
        text.push_str(&format!(
            "\n📢 OGŁOSZENIA [{}]\nPodsumowanie: {}\nDo zrobienia:\n{}\n",
            data.urgency,
            strip_markdown(&data.summary),
            key_points_text(data)
        ));
        for (idx, item) in sorted_by_date(announcements, |item| &item.date)
            .iter()
            .enumerate()
        {
            text.push_str(&format!(
                "  {}. {} — {}, {}\n     {}\n",
                idx + 1,
                item.title,
                item.author,
                format_date_pl(&item.date),
                item.content.replace('\n', "\n     ")
            ));
        }
    }

    if let Some(data) = messages_data {
        text.push_str(&format!(
            "\n✉️ WIADOMOŚCI [{}]\nPodsumowanie: {}\nDo zrobienia:\n{}\n",
            data.urgency,
            strip_markdown(&data.summary),
            key_points_text(data)
        ));
        for (idx, item) in sorted_by_date(messages, |item| &item.date)
            .iter()
            .enumerate()
        {
            text.push_str(&format!(
                "  {}. {} — {}, {}\n     {}\n",
                idx + 1,
                item.title,
                item.user,
                format_date_pl(&item.date),
                item.body.replace('\n', "\n     ")
            ));
        }
    }

    if let Some(data) = grades_data {
        text.push_str(&format!(
            "\n⭐ NOWE OCENY [{}]\nPodsumowanie: {}\n{}\n",
            data.urgency,
            strip_markdown(&data.summary),
            key_points_text(data)
        ));
        for (idx, item) in grades.iter().enumerate() {
            text.push_str(&format!(
                "  {}. {}{}\n",
                idx + 1,
                grade_label(item),
                if item.description.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", item.description)
                }
            ));
        }
    }

    if let Some(data) = homework_data {
        text.push_str(&format!(
            "\n📝 ZADANIA I TERMINARZ [{}]\nPodsumowanie: {}\nDo zrobienia:\n{}\n",
            data.urgency,
            strip_markdown(&data.summary),
            key_points_text(data)
        ));
        for (idx, item) in sorted_by_date(homework, |item| &item.from)
            .iter()
            .enumerate()
        {
            let deadline = if item.from == item.to {
                format_date_pl(&item.from)
            } else {
                format!(
                    "{} → {}",
                    format_date_pl(&item.from),
                    format_date_pl(&item.to)
                )
            };
            text.push_str(&format!(
                "  {}. {} — {}: {} ({}, {})\n     {}\n",
                idx + 1,
                item.subject,
                item.kind,
                item.title,
                item.user,
                deadline,
                item.content.replace('\n', "\n     ")
            ));
        }
    }

    if let Some(count) = tasks_created {
        if count > 0 {
            text.push_str(&format!(
                "\n📋 Dodano {} do listy \"Librus — Emilka\" w Vikunja (z terminami).\n",
                plural(count, "zadanie", "zadania", "zadań")
            ));
        }
    }

    text.push_str("\n-- \nWygenerowano automatycznie • https://synergia.librus.pl\n");
    text
}

// ---------------------------------------------------------------------------
// Sending
// ---------------------------------------------------------------------------

pub async fn send_notification(
    announcements_data: Option<&Analysis>,
    messages_data: Option<&Analysis>,
    grades_data: Option<&Analysis>,
    homework_data: Option<&Analysis>,
    announcements: &[AnnouncementItem],
    messages: &[MessageItem],
    grades: &[GradeItem],
    homework: &[HomeworkItem],
    tasks_created: Option<usize>,
) -> Result<()> {
    let has_announcements = announcements_data.is_some() && !announcements.is_empty();
    let has_messages = messages_data.is_some() && !messages.is_empty();
    let has_grades = grades_data.is_some() && !grades.is_empty();
    let has_homework = homework_data.is_some() && !homework.is_empty();

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

    let max_urgency = if urgencies.contains(&"PILNE") {
        "PILNE"
    } else if urgencies.contains(&"NORMALNE") {
        "NORMALNE"
    } else {
        "NIEPILNE"
    };

    let subject = compose_subject(
        max_urgency,
        announcements_data,
        messages_data,
        grades_data,
        homework_data,
        announcements,
        messages,
        grades,
        homework,
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
        tasks_created,
    );
    let plain = generate_plain_email(
        announcements_data,
        messages_data,
        grades_data,
        homework_data,
        announcements,
        messages,
        grades,
        homework,
        tasks_created,
    );

    send_email(&subject, plain, html).await
}

fn remark_html(remarks: &[RemarkItem]) -> Result<String> {
    if remarks.is_empty() {
        return Err(anyhow!("Refusing to send an empty remarks email"));
    }
    if remarks
        .iter()
        .any(|remark| remark.content.trim().is_empty())
    {
        return Err(anyhow!("Refusing to send an empty uwaga"));
    }

    let mut body = String::new();
    for (idx, remark) in remarks.iter().enumerate() {
        if idx > 0 {
            body.push_str("<hr style=\"border:none;border-top:1px solid #e2e8f0;margin:24px 0\">");
        }
        body.push_str(&format!(
            "<h3>{} — {}</h3><p>{} · {} · {}</p><div style=\"white-space:pre-wrap\">{}</div>",
            escape_html(&remark.kind),
            escape_html(&remark.date),
            escape_html(&remark.teacher),
            escape_html(&remark.category),
            escape_html(&format_date_pl(&remark.date)),
            escape_html(&remark.content)
        ));
    }

    Ok(format!(
        "<html><body><h2>{} z Librusa</h2>{}<p><a href=\"https://synergia.librus.pl/uwagi\">Otwórz uwagi w Librusie</a></p></body></html>",
        plural(remarks.len(), "Nowa uwaga", "Nowe uwagi", "Nowe uwagi"),
        body
    ))
}

fn remark_plain(remarks: &[RemarkItem]) -> String {
    let mut text = format!(
        "EMILKA — LIBRUS: {}\n",
        plural(remarks.len(), "nowa uwaga", "nowe uwagi", "nowych uwag")
    );
    for (idx, remark) in remarks.iter().enumerate() {
        text.push_str(&format!(
            "\n{}. {} — {} ({}, {}, {})\n{}\n",
            idx + 1,
            remark.kind,
            remark.category,
            remark.teacher,
            format_date_pl(&remark.date),
            remark.date,
            remark.content
        ));
    }
    text.push_str("\n-- \nhttps://synergia.librus.pl/uwagi\n");
    text
}

pub async fn send_remarks(remarks: &[RemarkItem]) -> Result<()> {
    let subject = if remarks.len() == 1 {
        let remark = &remarks[0];
        truncate_chars(
            &format!(
                "Uwaga {}: {} — {}, {}",
                remark.kind.to_lowercase(),
                remark.category,
                remark.teacher,
                remark.date
            ),
            MAX_SUBJECT_LENGTH,
        )
    } else {
        let kinds = remarks
            .iter()
            .map(|remark| remark.kind.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        truncate_chars(
            &format!("{} nowych uwag ({})", remarks.len(), kinds),
            MAX_SUBJECT_LENGTH,
        )
    };
    send_email(&subject, remark_plain(remarks), remark_html(remarks)?).await
}

async fn send_email(subject: &str, plain: String, html: String) -> Result<()> {
    if !should_send_email() {
        tracing::info!("EMAIL SENDING DISABLED - Notification content:");
        println!("\n{}", "=".repeat(80));
        println!("Subject: {}", subject);
        println!("{}", "=".repeat(80));
        println!("\nPlain text version:");
        println!("{}", plain);
        let preview = TAG_RE.replace_all(&html, " ");
        let preview = preview.split_whitespace().collect::<Vec<_>>().join(" ");
        let preview = if preview.len() > 500 {
            format!("{}...", preview.chars().take(500).collect::<String>())
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
        .multipart(MultiPart::alternative_plain_html(plain, html))?;

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
        recipients = recipients.join(","),
        recipient_count = recipients.len(),
        "Sending email notification"
    );

    mailer
        .send(email)
        .await
        .map_err(|err| anyhow!(err.to_string()))?;

    tracing::info!(
        recipients = recipients.join(","),
        recipient_count = recipients.len(),
        "Email notification sent successfully"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remark() -> RemarkItem {
        RemarkItem {
            id: "1".into(),
            content: "Pełna uwaga <ważne>\nDruga linia".into(),
            date: "2026-10-04".into(),
            teacher: "Nauczyciel".into(),
            kind: "Negatywna".into(),
            category: "Zachowanie".into(),
        }
    }

    #[test]
    fn remark_is_visible_verbatim_without_ai_or_collapsed_details() {
        let html = remark_html(&[remark()]).unwrap();
        assert!(html.contains("Pełna uwaga &lt;ważne&gt;\nDruga linia"));
        assert!(html.contains("https://synergia.librus.pl/uwagi"));
        assert!(!html.contains("<details>"));
        assert!(!html.contains("Podsumowanie"));
        assert!(remark_html(&[RemarkItem {
            content: " \n ".into(),
            ..remark()
        }])
        .is_err());
        assert!(remark_html(&[]).is_err());
    }

    #[test]
    fn remarks_are_batched_into_one_email() {
        let mut second = remark();
        second.kind = "Pozytywna".into();
        let html = remark_html(&[remark(), second]).unwrap();
        assert!(html.contains("Nowe uwagi z Librusa"));
        assert!(html.contains("<h3>Negatywna — 2026-10-04</h3>"));
        assert!(html.contains("<h3>Pozytywna — 2026-10-04</h3>"));
        assert!(html.contains("Zachowanie"));
    }

    #[test]
    fn subject_carries_the_gist_per_section() {
        let subject = compose_subject(
            "PILNE",
            None,
            Some(&Analysis {
                urgency: "PILNE".into(),
                summary: "Wpłata na wycieczkę.".into(),
                key_points: vec!["x".into()],
                subject: Some("Wpłata 30 zł na wycieczkę do piątku".into()),
                tasks: vec![],
            }),
            None,
            None,
            &[],
            &[],
            &[],
            &[],
        );
        assert_eq!(subject, "🔴 Emilka: Wpłata 30 zł na wycieczkę do piątku");
    }

    #[test]
    fn subject_falls_back_to_local_gists_without_ai() {
        let subject = compose_subject(
            "NORMALNE",
            None,
            None,
            None,
            None,
            &[],
            &[],
            &[GradeItem {
                id: "1".into(),
                subject: "Matematyka".into(),
                value: "5-".into(),
                description: String::new(),
                category: String::new(),
                date: "2026-10-20".into(),
                kind: GradeKind::Numeric,
            }],
            &[HomeworkItem {
                id: "2".into(),
                subject: "J. polski".into(),
                title: "Czytanie".into(),
                kind: "Zadanie domowe".into(),
                from: "2026-10-20".into(),
                to: "2026-10-21".into(),
                content: String::new(),
                user: "Nauczyciel".into(),
            }],
        );
        assert_eq!(
            subject,
            "🟡 Emilka: ⭐ Matematyka: 5- | 📝 J. polski zadanie domowe — do 21.10.2026 (środa)"
        );
    }

    #[test]
    fn user_content_is_escaped_but_bold_is_rendered() {
        let html = render_markdown("Zmiana <planu> **ważne** & \"skrót\"");
        assert_eq!(
            html,
            "Zmiana &lt;planu&gt; <strong>ważne</strong> &amp; &quot;skrót&quot;"
        );
    }

    #[test]
    fn dates_are_polish_and_arrows_only_differ_when_range_differs() {
        assert_eq!(format_date_pl("2026-10-20T14:30:00"), "20.10.2026 (wtorek)");
        assert_eq!(format_date_pl("nie-data"), "nie-data");
    }

    #[test]
    fn urgency_is_an_emoji_in_the_subject() {
        assert_eq!(urgency_emoji("PILNE"), "🔴");
        assert_eq!(urgency_emoji("NORMALNE"), "🟡");
        assert_eq!(urgency_emoji("NIEPILNE"), "🟢");
        assert_eq!(urgency_emoji("nieznane"), "🟢");
    }

    #[test]
    fn grade_colors_reflect_value() {
        assert_eq!(grade_class("1"), "grade-bad");
        assert_eq!(grade_class("2-"), "grade-bad");
        assert_eq!(grade_class("3+"), "grade-mid");
        assert_eq!(grade_class("4"), "grade-good");
        assert_eq!(grade_class("5-"), "grade-good");
        assert_eq!(grade_class("86%"), "grade-good");
    }

    #[test]
    fn text_grades_use_category_instead_of_grade_symbol() {
        let item = GradeItem {
            id: "text:97964".into(),
            subject: "J. angielski".into(),
            value: "T".into(),
            description: "Bardzo aktywna na lekcji.".into(),
            category: "aktywność".into(),
            date: "2026-10-02".into(),
            kind: GradeKind::Text,
        };

        assert_eq!(grade_label(&item), "J. angielski: aktywność");
        assert_eq!(grade_css_class(&item), "grade-text");
        assert!(grade_heading(&item).starts_with("J. angielski — aktywność · 02.10.2026"));
    }

    #[test]
    #[ignore = "writes /tmp/opencode/email-preview.html for manual inspection"]
    fn render_preview_to_file() {
        let announcements = vec![AnnouncementItem {
            id: "1".into(),
            title: "Zbiórka pieniędzy na wycieczkę do teatru".into(),
            content: "Szanowni Państwo, prosimy o wpłatę 30 zł na konto klasowe do piątku (24.10). Wycieczka odbędzie się 30.10, zbiórka pod szkołą o 08:15. Prosimy o wygodne buty.".into(),
            date: "2026-10-19T18:00:00".into(),
            author: "Monika Podżorska".into(),
        }];
        let messages = vec![MessageItem {
            id: "7".into(),
            title: "Zeszyty ćwiczeń matematyka".into(),
            body: "Dzień dobry, proszę o uzupełnienie stron 42-43 w zeszytach ćwiczeń i sprawdzenie, czy dziecko ma odpowiednie przybory: kredki, linijkę.".into(),
            date: "2026-10-20T08:12:00".into(),
            user: "Anna Nowak".into(),
        }];
        let grades = vec![
            GradeItem {
                id: "1".into(),
                subject: "Matematyka".into(),
                value: "1".into(),
                description: "sprawdzian: dodawanie w zakresie 100".into(),
                category: String::new(),
                date: "2026-10-20".into(),
                kind: GradeKind::Numeric,
            },
            GradeItem {
                id: "text:2".into(),
                subject: "J. angielski".into(),
                value: "T".into(),
                description: "Emilko, znakomicie rozwiązałaś samodzielne zadania. Gratuluję!"
                    .into(),
                category: "sprawdzian".into(),
                date: "2026-10-21".into(),
                kind: GradeKind::Text,
            },
        ];
        let homework = vec![
            HomeworkItem {
                id: "1".into(),
                subject: "Przyroda".into(),
                title: "Karty pracy str. 12".into(),
                kind: "Zadanie domowe".into(),
                from: "2026-10-20".into(),
                to: "2026-10-20".into(),
                content: "Karty pracy str. 12".into(),
                user: "Jan Kowalski".into(),
            },
            HomeworkItem {
                id: "2".into(),
                subject: "J. polski".into(),
                title: "Czytanie lektury".into(),
                kind: "Praca własna".into(),
                from: "2026-10-20".into(),
                to: "2026-10-24".into(),
                content: "Czytać rozdział 3".into(),
                user: "Monika Podżorska".into(),
            },
        ];
        let analysis = |urgency: &str, summary: &str, subject: &str, tasks: usize| Analysis {
            urgency: urgency.into(),
            summary: summary.into(),
            key_points: vec![
                "📖 **24.10.2026** — wpłata 30 zł".into(),
                "📚 **str. 42-43**".into(),
            ],
            subject: Some(subject.into()),
            tasks: (0..tasks)
                .map(|i| ActionTask {
                    title: format!("Zadanie przykładowe {i}"),
                    due: Some("2026-10-24".into()),
                })
                .collect(),
        };
        let html = generate_html_email(
            Some(&analysis(
                "PILNE",
                "Wpłata 30 zł do piątku, przygotować wygodne buty.",
                "Wpłata 30 zł na wycieczkę do 24.10",
                2,
            )),
            Some(&analysis(
                "NORMALNE",
                "Uzupełnić strony 42-43.",
                "Uzupełnić str. 42-43 (matematyka)",
                1,
            )),
            Some(&analysis(
                "NIEPILNE",
                "Nowe oceny: matematyka 1, angielski 5-.",
                "Oceny: matematyka 1, angielski 5-",
                0,
            )),
            Some(&analysis(
                "NORMALNE",
                "Karty pracy str. 12, czytanie rozdziału 3.",
                "Karty pracy str. 12 (przyroda), czytanie (polski)",
                1,
            )),
            &announcements,
            &messages,
            &grades,
            &homework,
            Some(3),
        );
        std::fs::create_dir_all("/tmp/opencode").unwrap();
        std::fs::write("/tmp/opencode/email-preview.html", html).unwrap();
    }
}
