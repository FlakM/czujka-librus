use serde::{Deserialize, Serialize};

/// A single action item extracted by the AI, destined for the Vikunja list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionTask {
    /// Imperative, self-contained title (Polish), e.g. "Wpłacić 30 zł na wycieczkę".
    pub title: String,
    /// Deadline as YYYY-MM-DD, when the source states one explicitly.
    #[serde(default)]
    pub due: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub urgency: String,
    pub summary: String,
    #[serde(rename = "keyPoints")]
    pub key_points: Vec<String>,
    /// One-line gist of the section, used in the email subject (max ~70 chars).
    #[serde(default)]
    pub subject: Option<String>,
    /// Actions requiring parent/child intervention, synced to Vikunja.
    #[serde(default)]
    pub tasks: Vec<ActionTask>,
}

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

/// Whether a grade is numeric or a teacher-written text grade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradeKind {
    /// A regular grade such as `5`, `4+` or a percentage.
    Numeric,
    /// A descriptive/formative grade ("ocena kształtująca" / "ocena opisowa").
    Text,
}

#[derive(Debug, Clone)]
pub struct GradeItem {
    pub id: String,
    pub subject: String,
    /// Grade symbol (`5`, `4+`, `T` for text grades).
    pub value: String,
    /// Teacher comment (numeric grades) or the full description (text grades).
    pub description: String,
    /// Category or skill name, e.g. `sprawdzian` or `Czytanie`.
    pub category: String,
    /// Date the grade was given (`YYYY-MM-DD`).
    pub date: String,
    pub kind: GradeKind,
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

impl CalendarEvent {
    /// Teacher absences ("Nieobecność nauczyciela") are scheduling noise: the
    /// school handles substitutions, and a cancelled lesson is not something a
    /// parent needs on the family calendar.
    pub fn is_teacher_absence(&self) -> bool {
        looks_like_teacher_absence(&self.title, &self.description, &self.source_url)
    }
}

/// Heuristic for Librus teacher-absence entries.
///
/// Librus itself labels them, so no AI call is needed: the detail page kind is
/// `Nieobecność nauczyciela`, and every absence lives under the `szczegoly_wolne`
/// ("free day") terminarz path. The URL check alone is not enough (holidays use
/// the same path), hence the extra title/description requirement.
pub fn looks_like_teacher_absence(title: &str, description: &str, source_url: &str) -> bool {
    let title = crate::text_utils::normalize_for_match(title);
    if title.contains("nieobecnosc") {
        return true;
    }
    let url = source_url.to_ascii_lowercase();
    url.contains("/terminarz/szczegoly_wolne/")
        && (title.contains("nauczyciel")
            || crate::text_utils::normalize_for_match(description).contains("nauczyciel"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(title: &str, description: &str, source_url: &str) -> CalendarEvent {
        CalendarEvent {
            id: "x".into(),
            title: title.into(),
            date: "2026-11-01".into(),
            end_date: "2026-11-01".into(),
            time_from: None,
            time_to: None,
            description: description.into(),
            source_url: source_url.into(),
        }
    }

    #[test]
    fn librus_absence_titles_are_recognised_with_and_without_diacritics() {
        for title in [
            "Librus: Nieobecność nauczyciela — Minkina Justyna",
            "Librus: Nieobecność nauczyciela — ",
            "Librus: nieobecnosc nauczyciela",
        ] {
            assert!(
                event(
                    title,
                    "",
                    "https://synergia.librus.pl/terminarz/szczegoly_wolne/1"
                )
                .is_teacher_absence(),
                "{title}"
            );
        }
    }

    #[test]
    fn free_day_entries_without_teacher_marker_are_kept() {
        let holiday = event(
            "Librus: Dzień wolny — ",
            "Opis: ferie zimowe",
            "https://synergia.librus.pl/terminarz/szczegoly_wolne/2",
        );
        assert!(!holiday.is_teacher_absence());
        assert!(!event(
            "Librus: sprawdzian — Edukacja matematyczna",
            "Nauczyciel: Podżorska Monika",
            "https://synergia.librus.pl/terminarz/szczegoly/123",
        )
        .is_teacher_absence());
    }
}
