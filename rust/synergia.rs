use std::collections::BTreeSet;
use std::env;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::{Datelike, Months, NaiveDate};
use scraper::{ElementRef, Html, Selector};
use sha2::{Digest, Sha256};

use crate::models::{CalendarEvent, RemarkItem};

const BASE: &str = "https://synergia.librus.pl";

pub struct SynergiaClient {
    http: reqwest::Client,
}

impl SynergiaClient {
    pub async fn from_env() -> Result<Self> {
        let http = reqwest::Client::builder()
            .cookie_store(true)
            .timeout(Duration::from_secs(30))
            .build()?;
        http.get(format!("{BASE}/loguj/portalRodzina"))
            .query(&[("v", chrono::Utc::now().timestamp())])
            .send()
            .await?
            .error_for_status()?;
        let login: serde_json::Value = http
            .post("https://api.librus.pl/OAuth/Authorization?client_id=46")
            .form(&[
                ("action", "login".to_string()),
                ("login", env::var("LIBRUS_USERNAME")?),
                ("pass", env::var("LIBRUS_PASSWORD")?),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let redirect = login["goTo"].as_str().context("Librus login failed")?;
        if !redirect.starts_with('/') || redirect.starts_with("//") {
            bail!("Unexpected Librus login redirect");
        }
        http.get(format!("https://api.librus.pl{redirect}"))
            .send()
            .await?
            .error_for_status()?;
        http.get(format!("{BASE}/gateway/api/2.0/Auth/TokenInfo/"))
            .send()
            .await?
            .error_for_status()?;
        Ok(Self { http })
    }

    async fn page(&self, path: &str) -> Result<String> {
        let response = self.http.get(format!("{BASE}{path}")).send().await?;
        if response.url().path() != path {
            bail!("Librus redirected away from {path}; refusing to treat it as an empty page");
        }
        Ok(response.error_for_status()?.text().await?)
    }

    pub async fn remarks(&self) -> Result<Vec<RemarkItem>> {
        parse_remarks(&self.page("/uwagi").await?)
    }

    pub async fn homework_categories(&self) -> Result<std::collections::HashMap<i64, String>> {
        let response: serde_json::Value = self
            .http
            .get(format!("{BASE}/gateway/api/2.0/HomeWorks/Categories"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        response["Categories"]
            .as_array()
            .context("Homework categories missing")?
            .iter()
            .map(|category| {
                Ok((
                    category["Id"].as_i64().context("Category ID missing")?,
                    category["Name"]
                        .as_str()
                        .context("Category name missing")?
                        .trim()
                        .to_string(),
                ))
            })
            .collect()
    }

    pub async fn events(&self, today: NaiveDate) -> Result<Vec<CalendarEvent>> {
        let mut paths = BTreeSet::new();
        let month = today.with_day(1).context("Invalid calendar month")?;
        for offset in 0..3 {
            let date = month
                .checked_add_months(Months::new(offset))
                .context("Invalid calendar range")?;
            let response = self
                .http
                .post(format!("{BASE}/terminarz"))
                .form(&[("miesiac", date.month()), ("rok", date.year() as u32)])
                .send()
                .await?;
            if response.url().path() != "/terminarz" {
                bail!("Librus calendar session expired");
            }
            paths.extend(event_paths(&response.error_for_status()?.text().await?)?);
        }
        let mut events = Vec::new();
        for path in paths {
            let event = parse_event(&path, &self.page(&path).await?)?;
            if NaiveDate::parse_from_str(&event.end_date, "%Y-%m-%d")? >= today {
                events.push(event);
            }
        }
        Ok(events)
    }
}

fn selector(value: &str) -> Selector {
    Selector::parse(value).expect("Static CSS selector")
}

fn text(element: ElementRef<'_>) -> String {
    fn visit(element: ElementRef<'_>, output: &mut String) {
        if element.value().name() == "br" {
            output.push('\n');
        }
        for child in element.children() {
            if let scraper::Node::Text(value) = child.value() {
                output.push_str(value);
            } else if let Some(child) = ElementRef::wrap(child) {
                visit(child, output);
            }
        }
        if matches!(element.value().name(), "p" | "div" | "li") {
            output.push('\n');
        }
    }
    let mut output = String::new();
    visit(element, &mut output);
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_remarks(html: &str) -> Result<Vec<RemarkItem>> {
    let doc = Html::parse_document(html);
    let table = doc
        .select(&selector("table.decorated.big.center"))
        .next()
        .context("Uwagi table missing; page layout or authentication changed")?;
    let mut remarks = Vec::new();
    for row in table.select(&selector("tbody > tr")) {
        let cells: Vec<_> = row.select(&selector("td")).map(text).collect();
        if cells.len() == 1 && row.select(&selector("td[colspan='5']")).next().is_some() {
            continue;
        }
        if cells.len() != 5 || cells[0].trim().is_empty() {
            bail!("Unexpected or empty uwagi row");
        }
        NaiveDate::parse_from_str(&cells[1], "%Y-%m-%d").context("Invalid uwaga date")?;
        let id = format!("{:x}", Sha256::digest(serde_json::to_vec(&cells)?));
        remarks.push(RemarkItem {
            id,
            content: cells[0].clone(),
            date: cells[1].clone(),
            teacher: cells[2].clone(),
            kind: cells[3].clone(),
            category: cells[4].clone(),
        });
    }
    Ok(remarks)
}

fn event_paths(html: &str) -> Result<BTreeSet<String>> {
    let doc = Html::parse_document(html);
    if doc.select(&selector("table.kalendarz")).next().is_none() {
        bail!("Terminarz table missing; page layout or authentication changed");
    }
    let pattern = regex::Regex::new(r"/terminarz/szczegoly(?:_wolne)?/\d+")?;
    Ok(pattern
        .find_iter(html)
        .map(|m| m.as_str().to_string())
        .collect())
}

pub fn parse_event(path: &str, html: &str) -> Result<CalendarEvent> {
    let doc = Html::parse_document(html);
    let table = doc
        .select(&selector("table.decorated.center"))
        .next()
        .context("Terminarz detail table missing")?;
    let mut fields = std::collections::HashMap::new();
    for row in table.select(&selector("tr")) {
        let cells: Vec<_> = row.select(&selector("td, th")).map(text).collect();
        if cells.len() == 2 {
            fields.insert(cells[0].trim_end_matches(':').to_string(), cells[1].clone());
        }
    }
    let dates = regex::Regex::new(r"\d{4}-\d{2}-\d{2}")?;
    let range = fields
        .get("Data")
        .or_else(|| fields.get("Zakres"))
        .or_else(|| fields.get("Przedział czasu"))
        .context("Calendar event date missing")?;
    let days: Vec<_> = dates.find_iter(range).map(|m| m.as_str()).collect();
    let date = *days.first().context("Calendar event date missing")?;
    let end_date = *days.last().context("Calendar event end date missing")?;
    let start = NaiveDate::parse_from_str(date, "%Y-%m-%d")?;
    let end = NaiveDate::parse_from_str(end_date, "%Y-%m-%d")?;
    if end < start {
        bail!("Calendar event ends before it starts");
    }
    let times = regex::Regex::new(r"\b\d{2}:\d{2}\b")?;
    let time_range = fields
        .get("Przedział czasu")
        .map(String::as_str)
        .unwrap_or("");
    let hours: Vec<_> = times
        .find_iter(time_range)
        .map(|m| m.as_str().to_string())
        .collect();
    let kind = fields
        .get("Rodzaj")
        .map(String::as_str)
        .unwrap_or("Nieobecność nauczyciela");
    let subject = fields
        .get("Przedmiot")
        .or_else(|| fields.get("Nauczyciel"))
        .map(String::as_str)
        .unwrap_or("");
    let description = ["Opis", "Nauczyciel", "Nr lekcji", "Sala", "Przedział czasu"]
        .iter()
        .filter_map(|key| fields.get(*key).map(|value| format!("{key}: {value}")))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(CalendarEvent {
        id: format!("{:x}", Sha256::digest(path.as_bytes())),
        title: format!("Librus: {kind} — {subject}"),
        date: date.to_string(),
        end_date: end_date.to_string(),
        time_from: hours.first().cloned().filter(|_| hours.len() == 2),
        time_to: hours.get(1).cloned(),
        description,
        source_url: format!("{BASE}{path}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_remark_text_and_stable_identity() {
        let html = "<table class='decorated big center'><tbody><tr><td>Uwaga &amp; opis<br>Druga linia.</td><td>2026-10-04</td><td>Nauczyciel</td><td>Negatywna</td><td>Zachowanie</td></tr></tbody></table>";
        let remarks = parse_remarks(html).unwrap();
        assert_eq!(remarks[0].content, "Uwaga & opis\nDruga linia.");
        assert_eq!(remarks[0].id, parse_remarks(html).unwrap()[0].id);
        assert_ne!(
            remarks[0].id,
            parse_remarks(&html.replace("Zachowanie", "Pochwała")).unwrap()[0].id
        );
        assert!(parse_remarks("<html>Login</html>").is_err());
        assert!(parse_remarks(&html.replace("Uwaga &amp; opis<br>Druga linia.", " ")).is_err());
    }

    #[test]
    fn calendar_paths_deduplicate_and_include_absences() {
        let html = "<table class='kalendarz'></table><td onclick=\"location.href='/terminarz/szczegoly/123'\"></td><a href='/terminarz/szczegoly/123'></a><a href='/terminarz/szczegoly_wolne/456'></a>";
        assert_eq!(event_paths(html).unwrap().len(), 2);
        assert!(event_paths("<html>Login</html>").is_err());
    }

    #[test]
    fn event_retains_dates_description_and_source() {
        let html = "<table class='decorated medium center'><tr><td>Data</td><td>2026-10-06</td></tr><tr><td>Rodzaj</td><td>Sprawdzian</td></tr><tr><td>Przedmiot</td><td>Angielski</td></tr><tr><td>Opis</td><td>Unit 1<br>Sporty &amp; dni tygodnia</td></tr></table>";
        let event = parse_event("/terminarz/szczegoly/123", html).unwrap();
        assert_eq!(event.date, "2026-10-06");
        assert_eq!(event.time_from, None);
        assert!(event.description.contains("Unit 1\nSporty & dni tygodnia"));
        assert!(event.source_url.ends_with("/123"));
    }

    #[test]
    fn absence_retains_multi_day_range() {
        let html = "<table class='decorated small center'><tr><td>Nauczyciel</td><td>Nauczyciel</td></tr><tr><td>Zakres</td><td>2026-10-05 - 2026-10-07</td></tr></table>";
        let event = parse_event("/terminarz/szczegoly_wolne/1", html).unwrap();
        assert_eq!(event.date, "2026-10-05");
        assert_eq!(event.end_date, "2026-10-07");
        assert!(event.title.contains("Nieobecność nauczyciela"));
    }
}
