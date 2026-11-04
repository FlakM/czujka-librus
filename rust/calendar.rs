use std::env;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::{Datelike, Days, Months, NaiveDate, NaiveTime};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::db::Database;
use crate::models::{looks_like_teacher_absence, CalendarEvent};

#[derive(Deserialize)]
struct Credentials {
    client_id: String,
    client_secret: String,
    refresh_token: String,
}

#[derive(Deserialize)]
struct AccessToken {
    access_token: String,
}

pub struct GoogleCalendar {
    http: reqwest::Client,
    events_url: Url,
    access_token: String,
}

impl GoogleCalendar {
    pub fn configured() -> bool {
        env::var("GOOGLE_CALENDAR_ID").is_ok()
            || env::var("GOOGLE_CALENDAR_CREDENTIALS_FILE").is_ok()
    }

    pub async fn from_env() -> Result<Self> {
        let id = env::var("GOOGLE_CALENDAR_ID").context("GOOGLE_CALENDAR_ID must be set")?;
        if id.trim().is_empty() {
            bail!("GOOGLE_CALENDAR_ID cannot be empty");
        }
        let path = env::var("GOOGLE_CALENDAR_CREDENTIALS_FILE")
            .context("GOOGLE_CALENDAR_CREDENTIALS_FILE must point to a SOPS-managed JSON file")?;
        let credentials: Credentials = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        let response = http
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", credentials.client_id.as_str()),
                ("client_secret", credentials.client_secret.as_str()),
                ("refresh_token", credentials.refresh_token.as_str()),
            ])
            .send()
            .await?;
        if !response.status().is_success() {
            bail!("Google OAuth refresh failed (HTTP {})", response.status());
        }
        let token: AccessToken = response.json().await?;
        let mut events_url = Url::parse("https://www.googleapis.com/calendar/v3/calendars/")?;
        events_url
            .path_segments_mut()
            .map_err(|_| anyhow::anyhow!("Invalid calendar URL"))?
            .pop_if_empty()
            .push(&id)
            .push("events");
        Ok(Self {
            http,
            events_url,
            access_token: token.access_token,
        })
    }

    pub async fn sync(&self, db: &Database, events: &[CalendarEvent]) -> Result<()> {
        // Teacher absences are noise, and earlier runs already wrote some of
        // them into the calendar (including the family calendar used before the
        // dedicated school calendar existed). Scrub those first, then never
        // create new ones.
        let purged = self.purge_teacher_absences(db).await?;
        let total = events.len();
        let absences = events
            .iter()
            .filter(|event| event.is_teacher_absence())
            .count();
        let events: Vec<&CalendarEvent> = events
            .iter()
            .filter(|event| !event.is_teacher_absence())
            .collect();
        let mut changed = 0;
        for event in &events {
            let payload = event_payload(event)?;
            let serialized = serde_json::to_string(&payload)?;
            let cache_id = format!("{}#{}", self.events_url, event.id);
            if db.calendar_payload(&cache_id)?.as_deref() == Some(&serialized) {
                continue;
            }
            let mut item_url = self.events_url.clone();
            item_url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("Invalid calendar URL"))?
                .push(&event.id);
            let update = self
                .http
                .put(item_url.clone())
                .bearer_auth(&self.access_token)
                .json(&payload)
                .send()
                .await?;
            let response = if update.status() == StatusCode::NOT_FOUND {
                let created = self
                    .http
                    .post(self.events_url.clone())
                    .bearer_auth(&self.access_token)
                    .json(&payload)
                    .send()
                    .await?;
                if created.status() == StatusCode::CONFLICT {
                    self.http
                        .put(item_url)
                        .bearer_auth(&self.access_token)
                        .json(&payload)
                        .send()
                        .await?
                } else {
                    created
                }
            } else {
                update
            };
            if !response.status().is_success() {
                bail!("Google Calendar sync failed (HTTP {}); verify API activation and calendar write access", response.status());
            }
            db.mark_calendar_synced(&cache_id, &serialized)?;
            changed += 1;
        }
        let today = chrono::Local::now().date_naive();
        let horizon = today
            .with_day(1)
            .context("Invalid calendar month")?
            .checked_add_months(Months::new(3))
            .context("Invalid sync horizon")?;
        let prefix = format!("{}#", self.events_url);
        let mut removed = 0;
        for (cache_id, serialized) in db.calendar_entries(&prefix)? {
            let payload: Value = serde_json::from_str(&serialized)?;
            let start = calendar_date(&payload["start"])?;
            let end = calendar_date(&payload["end"])?;
            let id = cache_id
                .strip_prefix(&prefix)
                .context("Invalid cached event ID")?;
            if end <= today || start >= horizon || events.iter().any(|event| event.id == id) {
                continue;
            }
            let mut item_url = self.events_url.clone();
            item_url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("Invalid calendar URL"))?
                .push(id);
            let response = self
                .http
                .delete(item_url)
                .bearer_auth(&self.access_token)
                .send()
                .await?;
            if !response.status().is_success()
                && !matches!(response.status(), StatusCode::NOT_FOUND | StatusCode::GONE)
            {
                bail!(
                    "Google Calendar removal failed (HTTP {})",
                    response.status()
                );
            }
            db.forget_calendar_entry(&cache_id)?;
            removed += 1;
        }
        tracing::info!(
            changed,
            removed,
            purged,
            absences,
            total,
            "Google Calendar sync completed"
        );
        Ok(())
    }

    /// Removes every previously synced teacher absence from every calendar this
    /// service has written to, regardless of date — the generic reconciliation
    /// below skips past events, which would leave old absences behind forever.
    async fn purge_teacher_absences(&self, db: &Database) -> Result<usize> {
        let mut removed = 0;
        for (cache_id, serialized) in db.all_calendar_entries()? {
            let payload: Value = serde_json::from_str(&serialized)?;
            if !payload_looks_like_teacher_absence(&payload) {
                continue;
            }
            let Some((calendar_url, event_id)) = cache_id.split_once('#') else {
                continue;
            };
            let mut item_url = Url::parse(calendar_url).context("Invalid cached calendar URL")?;
            item_url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("Invalid calendar URL"))?
                .push(event_id);
            let response = self
                .http
                .delete(item_url)
                .bearer_auth(&self.access_token)
                .send()
                .await?;
            if !response.status().is_success()
                && !matches!(response.status(), StatusCode::NOT_FOUND | StatusCode::GONE)
            {
                if response.status() == StatusCode::FORBIDDEN {
                    // We can no longer write to that calendar (e.g. access was
                    // revoked); drop the row instead of retrying forever.
                    tracing::warn!(
                        event = %event_id,
                        calendar = %calendar_url,
                        "No write access to remove a teacher absence; dropping cached entry"
                    );
                    db.forget_calendar_entry(&cache_id)?;
                } else {
                    // Keep the cache row so the next run retries; one
                    // problematic event must not block the rest of the sync.
                    tracing::warn!(
                        status = %response.status(),
                        event = %event_id,
                        calendar = %calendar_url,
                        "Could not remove a teacher absence from the calendar; will retry"
                    );
                }
                continue;
            }
            db.forget_calendar_entry(&cache_id)?;
            removed += 1;
        }
        if removed > 0 {
            tracing::info!(
                count = removed,
                "Removed teacher absences from the calendar"
            );
        }
        Ok(removed)
    }
}

fn payload_looks_like_teacher_absence(payload: &Value) -> bool {
    let source_url = payload["source"]["url"]
        .as_str()
        .or_else(|| payload["extendedProperties"]["private"]["librusSource"].as_str())
        .unwrap_or_default();
    looks_like_teacher_absence(
        payload["summary"].as_str().unwrap_or_default(),
        payload["description"].as_str().unwrap_or_default(),
        source_url,
    )
}

fn calendar_date(value: &Value) -> Result<NaiveDate> {
    let date = value["date"]
        .as_str()
        .or_else(|| value["dateTime"].as_str())
        .context("Cached calendar date missing")?;
    NaiveDate::parse_from_str(
        date.get(..10).context("Invalid cached calendar date")?,
        "%Y-%m-%d",
    )
    .context("Invalid cached calendar date")
}

fn event_payload(event: &CalendarEvent) -> Result<Value> {
    let start = NaiveDate::parse_from_str(&event.date, "%Y-%m-%d")?;
    let end = NaiveDate::parse_from_str(&event.end_date, "%Y-%m-%d")?;
    let (start, end) = match (&event.time_from, &event.time_to) {
        (Some(from), Some(to)) => {
            let from = NaiveTime::parse_from_str(from, "%H:%M")?;
            let to = NaiveTime::parse_from_str(to, "%H:%M")?;
            if end.and_time(to) <= start.and_time(from) {
                bail!("Calendar event has an invalid time range");
            }
            (
                json!({"dateTime": format!("{}T{}", start, from.format("%H:%M:%S")), "timeZone": "Europe/Warsaw"}),
                json!({"dateTime": format!("{}T{}", end, to.format("%H:%M:%S")), "timeZone": "Europe/Warsaw"}),
            )
        }
        (None, None) => (
            json!({"date": start.to_string()}),
            json!({"date": end.checked_add_days(Days::new(1)).context("Invalid end date")?.to_string()}),
        ),
        _ => bail!("Calendar event has an incomplete time range"),
    };
    Ok(json!({
        "id": event.id,
        "summary": event.title,
        "description": format!("{}\n\n{}", event.description, event.source_url),
        "start": start,
        "end": end,
        "source": {"title": "Librus — terminarz", "url": event.source_url},
        "extendedProperties": {"private": {"librusSource": event.source_url}},
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn mock_calendar(
        responses: Vec<(&'static str, u16)>,
    ) -> (GoogleCalendar, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!(
            "http://{}/calendars/test/events",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let task = tokio::spawn(async move {
            for (method, status) in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 4096];
                loop {
                    let n = socket.read(&mut buffer).await.unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&buffer[..n]);
                    if let Some(header_end) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..header_end]);
                        let content_length = headers
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if request.len() >= header_end + 4 + content_length {
                            assert!(
                                headers.starts_with(&format!("{method} /calendars/test/events"))
                            );
                            break;
                        }
                    }
                }
                let response = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        (
            GoogleCalendar {
                http: reqwest::Client::new(),
                events_url: url,
                access_token: "test-token".into(),
            },
            task,
        )
    }

    fn event() -> CalendarEvent {
        CalendarEvent {
            id: "abc123".into(),
            title: "Sprawdzian".into(),
            date: "2026-10-06".into(),
            end_date: "2026-10-07".into(),
            time_from: None,
            time_to: None,
            description: "Unit 1".into(),
            source_url: "https://synergia.librus.pl/terminarz/szczegoly/1".into(),
        }
    }

    #[test]
    fn all_day_end_is_exclusive_and_identity_is_preserved() {
        let payload = event_payload(&event()).unwrap();
        assert_eq!(payload["end"]["date"], "2026-10-08");
        assert_eq!(payload["id"], "abc123");
        assert_eq!(payload["start"]["date"], "2026-10-06");
    }

    #[test]
    fn cached_absence_payloads_are_recognised() {
        let absence = json!({
            "summary": "Librus: Nieobecność nauczyciela — Minkina Justyna",
            "description": "Nauczyciel: Minkina Justyna\n\nhttps://synergia.librus.pl/terminarz/szczegoly_wolne/3135416",
            "start": {"date": "2026-10-28"},
            "end": {"date": "2026-10-29"},
            "source": {"title": "Librus — terminarz", "url": "https://synergia.librus.pl/terminarz/szczegoly_wolne/3135416"},
            "extendedProperties": {"private": {"librusSource": "https://synergia.librus.pl/terminarz/szczegoly_wolne/3135416"}}
        });
        assert!(payload_looks_like_teacher_absence(&absence));
        assert!(!payload_looks_like_teacher_absence(
            &event_payload(&event()).unwrap()
        ));
    }

    #[tokio::test]
    async fn teacher_absences_are_purged_and_never_uploaded() {
        let (calendar, server) =
            mock_calendar(vec![("DELETE", 204), ("PUT", 404), ("POST", 200)]).await;
        let db = Database::new(":memory:").unwrap();
        let prefix = format!("{}#", calendar.events_url);

        // Synced by an earlier run, already in the past: the generic
        // reconciliation would never delete it.
        let cached_absence = json!({
            "summary": "Librus: Nieobecność nauczyciela — Minkina Justyna",
            "description": "Nauczyciel: Minkina Justyna\n\nhttps://synergia.librus.pl/terminarz/szczegoly_wolne/3135416",
            "start": {"date": "2020-01-01"},
            "end": {"date": "2020-01-02"},
            "source": {"url": "https://synergia.librus.pl/terminarz/szczegoly_wolne/3135416"}
        });
        db.mark_calendar_synced(&format!("{prefix}absence-1"), &cached_absence.to_string())
            .unwrap();

        let mut regular = event();
        regular.date = (chrono::Local::now().date_naive() + Days::new(1)).to_string();
        regular.end_date = regular.date.clone();
        let new_absence = CalendarEvent {
            title: "Librus: Nieobecność nauczyciela — Krzyminska Katarzyna".into(),
            description: "Nauczyciel: Krzyminska Katarzyna".into(),
            source_url: "https://synergia.librus.pl/terminarz/szczegoly_wolne/3113432".into(),
            ..event()
        };

        calendar.sync(&db, &[regular, new_absence]).await.unwrap();

        let entries = db.calendar_entries(&prefix).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].0.ends_with("#abc123"));
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }

    #[test]
    fn explicit_times_use_warsaw_timezone() {
        let mut event = event();
        event.end_date = event.date.clone();
        event.time_from = Some("09:00".into());
        event.time_to = Some("09:45".into());
        let payload = event_payload(&event).unwrap();
        assert_eq!(payload["start"]["dateTime"], "2026-10-06T09:00:00");
        assert_eq!(payload["start"]["timeZone"], "Europe/Warsaw");
        event.time_to = Some("08:00".into());
        assert!(event_payload(&event).is_err());
    }

    #[tokio::test]
    async fn sync_creates_once_updates_changes_and_removes_cancelled_events() {
        let (calendar, server) = mock_calendar(vec![
            ("PUT", 404),
            ("POST", 200),
            ("PUT", 200),
            ("DELETE", 204),
        ])
        .await;
        let db = Database::new(":memory:").unwrap();
        let mut event = event();
        event.date = (chrono::Local::now().date_naive() + Days::new(1)).to_string();
        event.end_date = event.date.clone();
        calendar.sync(&db, &[event.clone()]).await.unwrap();
        calendar.sync(&db, &[event.clone()]).await.unwrap();
        event.description = "Updated scope".into();
        calendar.sync(&db, &[event]).await.unwrap();
        calendar.sync(&db, &[]).await.unwrap();
        assert!(db
            .calendar_entries(&format!("{}#", calendar.events_url))
            .unwrap()
            .is_empty());
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn failed_calendar_delivery_is_not_marked_as_synced() {
        let (calendar, server) = mock_calendar(vec![("PUT", 403)]).await;
        let db = Database::new(":memory:").unwrap();
        assert!(calendar.sync(&db, &[event()]).await.is_err());
        assert!(db
            .calendar_entries(&format!("{}#", calendar.events_url))
            .unwrap()
            .is_empty());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn retries_recover_an_insert_that_already_reached_google() {
        let (calendar, server) =
            mock_calendar(vec![("PUT", 404), ("POST", 409), ("PUT", 200)]).await;
        let db = Database::new(":memory:").unwrap();
        calendar.sync(&db, &[event()]).await.unwrap();
        assert_eq!(
            db.calendar_entries(&format!("{}#", calendar.events_url))
                .unwrap()
                .len(),
            1
        );
        server.await.unwrap();
    }
}
