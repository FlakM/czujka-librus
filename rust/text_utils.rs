use once_cell::sync::Lazy;
use regex::Regex;

static CDATA_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"<!\[CDATA\[(.*?)\]\]>").unwrap());
static XML_WRAPPER_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"<Message><Content>|</Content><Actions>.*?</Actions></Message>").unwrap()
});
static HTML_TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").unwrap());
static THREAD_QUOTE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^-{3,}[\s\S]*$").unwrap());
static USER_WROTE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^U[żz]ytkownik:.*?napisa[łl]:\s*").unwrap());
static MULTI_NEWLINE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\n{3,}").unwrap());
static MULTI_SPACE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r" {2,}").unwrap());

pub fn clean_message_body(body: &str) -> String {
    let mut result = body.to_string();

    // Extract CDATA content
    result = CDATA_RE.replace_all(&result, "$1").to_string();

    // Remove XML wrapper
    result = XML_WRAPPER_RE.replace_all(&result, "").to_string();

    // Convert HTML breaks to newlines
    result = result
        .replace("</br>", "\n")
        .replace("<br>", "\n")
        .replace("<br/>", "\n");

    // Remove remaining HTML tags
    result = HTML_TAG_RE.replace_all(&result, "").to_string();

    // Remove thread quotes (everything after "-----" line)
    result = THREAD_QUOTE_RE.replace_all(&result, "").to_string();

    // Remove "Użytkownik X napisał:" lines
    result = USER_WROTE_RE.replace_all(&result, "").to_string();

    // Normalize whitespace
    result = MULTI_NEWLINE_RE.replace_all(&result, "\n\n").to_string();
    result = MULTI_SPACE_RE.replace_all(&result, " ").to_string();

    result.trim().to_string()
}

pub fn is_simple_acknowledgment(body: &str) -> bool {
    let clean = body.to_lowercase();
    let short = clean.len() < 150;
    let ack_patterns = [
        "dziękuję za informację",
        "dziekuje za informacje",
        "dziękuję, przyjęłam",
        "dziękuję, przyjąłem",
        "ok, dziękuję",
        "rozumiem, dziękuję",
        "dobrze, dziękuję",
    ];
    short && ack_patterns.iter().any(|p| clean.contains(p))
}

pub fn is_within_days(date_str: &str, days: i64) -> bool {
    use chrono::{NaiveDateTime, Utc};
    let now = Utc::now().naive_utc();
    if let Ok(date) = NaiveDateTime::parse_from_str(date_str, "%Y-%m-%dT%H:%M:%S") {
        let diff = now.signed_duration_since(date);
        diff.num_days() <= days
    } else if let Ok(date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        let diff = now.date().signed_duration_since(date);
        diff.num_days() <= days
    } else {
        true // If we can't parse, include it
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_cdata() {
        let input =
            "<Message><Content><![CDATA[Hello world]]></Content><Actions></Actions></Message>";
        assert_eq!(clean_message_body(input), "Hello world");
    }

    #[test]
    fn test_clean_html() {
        let input = "Hello</br>world<br/>test";
        assert_eq!(clean_message_body(input), "Hello\nworld\ntest");
    }

    #[test]
    fn test_remove_thread_quotes() {
        let input = "New message\n-----\nOld quoted message";
        assert_eq!(clean_message_body(input), "New message");
    }
}
