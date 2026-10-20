use std::env;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chrono::{Datelike, Local, NaiveDate, TimeZone};
use serde::Deserialize;
use serde_json::json;

use crate::db::Database;
use crate::models::ActionTask;

/// Client for the self-hosted Vikunja instance. Authenticates with username and
/// password (JWT from POST /api/v1/login, cached and refreshed on 401), and
/// creates tasks in a single dedicated project (VIKUNJA_PROJECT_ID).
pub struct VikunjaClient {
    http: reqwest::Client,
    base_url: String,
    username: String,
    password: String,
    project_id: i64,
    token: Option<String>,
}

#[derive(Deserialize)]
struct LoginResponse {
    token: String,
}

#[derive(Deserialize)]
struct CreatedTask {
    id: i64,
}

impl VikunjaClient {
    pub fn configured() -> bool {
        ["VIKUNJA_URL", "VIKUNJA_USERNAME", "VIKUNJA_PASSWORD"]
            .iter()
            .all(|var| env::var(var).map(|val| !val.trim().is_empty()).unwrap_or(false))
    }

    pub fn from_env() -> Result<Self> {
        let base_url = env::var("VIKUNJA_URL")
            .context("VIKUNJA_URL must be set")?
            .trim_end_matches('/')
            .to_string();
        let username = env::var("VIKUNJA_USERNAME").context("VIKUNJA_USERNAME must be set")?;
        let password = env::var("VIKUNJA_PASSWORD").context("VIKUNJA_PASSWORD must be set")?;
        let project_id: i64 = env::var("VIKUNJA_PROJECT_ID")
            .ok()
            .and_then(|val| val.trim().parse().ok())
            .unwrap_or(0);
        if project_id <= 0 {
            return Err(anyhow!("VIKUNJA_PROJECT_ID must be set to a project ID"));
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            base_url,
            username,
            password,
            project_id,
            token: None,
        })
    }

    async fn login(&self) -> Result<String> {
        let response = self
            .http
            .post(format!("{}/api/v1/login", self.base_url))
            .json(&json!({"username": self.username, "password": self.password}))
            .send()
            .await
            .context("Vikunja login request failed")?;
        if !response.status().is_success() {
            return Err(anyhow!(
                "Vikunja login failed (HTTP {})",
                response.status()
            ));
        }
        let parsed: LoginResponse = response.json().await.context("Vikunja login parse failed")?;
        Ok(parsed.token)
    }

    async fn token(&mut self) -> Result<String> {
        if self.token.is_none() {
            self.token = Some(self.login().await?);
        }
        Ok(self.token.clone().expect("token just set"))
    }

    pub async fn create_task(
        &mut self,
        title: &str,
        description: &str,
        due: Option<&str>,
    ) -> Result<i64> {
        let mut payload = json!({
            "title": title,
            "description": description,
            "project_id": self.project_id,
        });
        if let Some(due) = due {
            // Vikunja requires RFC3339 with an explicit offset; deadlines get a
            // fixed 18:00 local time so they surface on the right day.
            let naive = NaiveDate::parse_from_str(due, "%Y-%m-%d")
                .context("Invalid Vikunja due date")?
                .and_hms_opt(18, 0, 0)
                .context("Invalid Vikunja due time")?;
            let local = match chrono::Local.from_local_datetime(&naive) {
                chrono::LocalResult::Single(local) => local,
                chrono::LocalResult::Ambiguous(local, _) => local,
                chrono::LocalResult::None => {
                    return Err(anyhow!("Vikunja due date {} is not a valid local time", due))
                }
            };
            payload["due_date"] = json!(local.to_rfc3339());
        }
        for attempt in 0..2 {
            let token = self.token().await?;
            let response = self
                .http
                .put(format!(
                    "{}/api/v1/projects/{}/tasks",
                    self.base_url, self.project_id
                ))
                .bearer_auth(token)
                .json(&payload)
                .send()
                .await
                .context("Vikunja task creation request failed")?;
            if response.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                self.token = None;
                continue;
            }
            if !response.status().is_success() {
                return Err(anyhow!(
                    "Vikunja task creation failed (HTTP {})",
                    response.status()
                ));
            }
            let created: CreatedTask = response
                .json()
                .await
                .context("Vikunja task creation parse failed")?;
            return Ok(created.id);
        }
        unreachable!("the loop always returns or errors on the second attempt")
    }

    pub async fn delete_task(&mut self, task_id: i64) -> Result<()> {
        let token = self.token().await?;
        let response = self
            .http
            .delete(format!("{}/api/v1/tasks/{}", self.base_url, task_id))
            .bearer_auth(token)
            .send()
            .await
            .context("Vikunja task deletion request failed")?;
        if !response.status().is_success() {
            return Err(anyhow!(
                "Vikunja task deletion failed (HTTP {})",
                response.status()
            ));
        }
        Ok(())
    }
}

/// Create all pending tasks in Vikunja, skipping ones already recorded in the
/// database. Returns how many tasks were actually created.
pub async fn sync_tasks(
    db: &Database,
    pending: &[(String, ActionTask)],
) -> Result<usize> {
    let existing = db.existing_task_keys()?;
    let mut client = VikunjaClient::from_env()?;
    let mut created = 0;
    for (key, task) in pending {
        if existing.contains(key) {
            continue;
        }
        let task_id = client
            .create_task(&task.title, &task_description(task), task.due.as_deref())
            .await?;
        db.mark_task_created(key, task_id)?;
        created += 1;
    }
    Ok(created)
}

fn task_description(task: &ActionTask) -> String {
    let mut description = "Zadanie utworzone automatycznie z Librusa.".to_string();
    if let Some(due) = &task.due {
        if let Ok(date) = NaiveDate::parse_from_str(due, "%Y-%m-%d") {
            description = format!(
                "{}\nTermin: {} ({})",
                description,
                date.format("%d.%m.%Y"),
                polish_weekday(date.weekday())
            );
        }
    }
    description
}

/// Polish weekday names for human-readable descriptions.
pub(crate) fn polish_weekday(weekday: chrono::Weekday) -> &'static str {
    match weekday {
        chrono::Weekday::Mon => "poniedziałek",
        chrono::Weekday::Tue => "wtorek",
        chrono::Weekday::Wed => "środa",
        chrono::Weekday::Thu => "czwartek",
        chrono::Weekday::Fri => "piątek",
        chrono::Weekday::Sat => "sobota",
        chrono::Weekday::Sun => "niedziela",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_requires_all_three_vars() {
        // This test runs without the service env, so all vars are typically unset;
        // set them explicitly to observe both branches.
        let vars = ["VIKUNJA_URL", "VIKUNJA_USERNAME", "VIKUNJA_PASSWORD"];
        for var in vars {
            env::remove_var(var);
        }
        assert!(!VikunjaClient::configured());
        env::set_var("VIKUNJA_URL", "https://tasks.house.flakm.com");
        env::set_var("VIKUNJA_USERNAME", "bot");
        env::set_var("VIKUNJA_PASSWORD", "secret");
        assert!(VikunjaClient::configured());
        for var in vars {
            env::remove_var(var);
        }
    }

    #[tokio::test]
    #[ignore = "live test: logs into tasks.house.flakm.com and creates a probe task"]
    async fn live_login_create_and_delete_probe_task() {
        let vars = ["VIKUNJA_URL", "VIKUNJA_USERNAME", "VIKUNJA_PASSWORD"];
        for var in vars {
            env::remove_var(var);
        }
        env::set_var("VIKUNJA_URL", "https://tasks.house.flakm.com");
        env::set_var("VIKUNJA_USERNAME", "bot");
        env::set_var(
            "VIKUNJA_PASSWORD",
            std::fs::read_to_string("/tmp/opencode/vikunja_probe_password")
                .expect("probe password file")
                .trim(),
        );
        env::set_var("VIKUNJA_PROJECT_ID", "29");

        let mut client = VikunjaClient::from_env().unwrap();
        let task_id = client
            .create_task(
                "[probe] czujka-librus live test",
                "Zadanie testowe — usuwane automatycznie.",
                Some("2026-10-24"),
            )
            .await
            .unwrap();
        assert!(task_id > 0);
        client.delete_task(task_id).await.unwrap();
    }
}
