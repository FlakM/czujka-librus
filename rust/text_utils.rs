use once_cell::sync::Lazy;
use regex::Regex;

// `(?s)` is essential: Librus replies carry their body in a CDATA block with
// real newlines, and without DOTALL the pattern never matches — the leftover
// "<![CDATA[...]]>" is then eaten by the tag stripper and the whole message
// becomes empty.
static CDATA_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)<!\[CDATA\[(.*?)\]\]>").unwrap());
static XML_WRAPPER_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"<Message><Content>|</Content><Actions>.*?</Actions></Message>").unwrap()
});
// Only strip real tags ("<br>", "</p>", "<!-- ... -->"), not a bare "<" or
// "<3" in the message text.
static HTML_TAG_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?s)<!--.*?-->|</?[a-zA-Z][^>]*>").unwrap());
// The quote separator ("-----") starts the quoted thread; everything below it
// is dropped (see `clean_message_body` for the empty-reply guard).
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

    // Remove the quoted thread — but only when the reply itself has text above
    // it. A message that is just a forwarded thread must not become empty.
    if let Some(quote) = THREAD_QUOTE_RE.find(&result) {
        if !result[..quote.start()].trim().is_empty() {
            result.truncate(quote.start());
        }
    }

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

pub fn is_remark_notification(body: &str) -> bool {
    let body = body.to_lowercase();
    body.contains("wiadomość została wygenerowana automatycznie")
        && body.contains("uwagi")
        && (body.contains("przejdź do menu") || body.contains("uwagi o uczniu"))
}

/// Lowercases and strips Polish diacritics so keyword matching does not depend
/// on how Librus spells a word (e.g. `Nieobecność` vs `nieobecnosc`).
pub fn normalize_for_match(input: &str) -> String {
    input
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'ą' => 'a',
            'ć' => 'c',
            'ę' => 'e',
            'ł' => 'l',
            'ń' => 'n',
            'ó' => 'o',
            'ś' => 's',
            'ź' => 'z',
            'ż' => 'z',
            other => other,
        })
        .collect()
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
    fn multiline_cdata_keeps_the_whole_body() {
        let input = "<Message><Content><![CDATA[Linia 1\nLinia 2\n\nLinia 3]]></Content><Actions></Actions></Message>";
        assert_eq!(clean_message_body(input), "Linia 1\nLinia 2\n\nLinia 3");
    }

    #[test]
    fn reply_wrapped_in_multiline_cdata_keeps_the_answer_and_drops_the_quote() {
        // Verbatim shape of Librus message 3923960 ("Re: Usprawiedliwienie").
        let input = "<Message><Content><![CDATA[Dzień dobry,\nDziękuję za informację.\nPozdrawiam\nMonika Podżorska\n\n-----\nUżytkownik: Flak Emilia (Osoba-Flak Patrycja) Rodzic 2a SP 2026-11-04 07:57:12 napisał:\nDzień dobry,\n\nEmilka dzisiaj przyjdzie na drugą lekcję lub ciut później. Bardzo proszę o usprawiedliwienie :)\n\nPozdrawiam,\nPatrycja Flak]]></Content><Actions></Actions></Message>";
        assert_eq!(
            clean_message_body(input),
            "Dzień dobry,\nDziękuję za informację.\nPozdrawiam\nMonika Podżorska"
        );
    }

    #[test]
    fn a_message_that_is_only_a_quoted_thread_is_not_emptied() {
        let input = "-----\nUżytkownik: Flak Emilia Rodzic 2026-11-04 07:57:12 napisał:\nDzień dobry,\nEmilka dzisiaj przyjdzie później.";
        let cleaned = clean_message_body(input);
        assert!(cleaned.contains("Emilka dzisiaj przyjdzie później"));
    }

    #[test]
    fn literal_angle_brackets_survive_tag_stripping() {
        assert_eq!(
            clean_message_body("Cena < 5 zł i > 3 zł, do tego <3"),
            "Cena < 5 zł i > 3 zł, do tego <3"
        );
    }

    #[test]
    fn test_clean_html() {
        let input = "Hello</br>world<br/>test";
        assert_eq!(clean_message_body(input), "Hello\nworld\ntest");
    }

    #[test]
    fn polish_diacritics_are_stripped_for_matching() {
        assert_eq!(
            normalize_for_match("Nieobecność Nauczyciela — Łódź"),
            "nieobecnosc nauczyciela — lodz"
        );
    }

    #[test]
    fn test_remove_thread_quotes() {
        let input = "New message\n-----\nOld quoted message";
        assert_eq!(clean_message_body(input), "New message");
    }

    #[test]
    fn generic_uwagi_notices_are_suppressed_but_teacher_messages_are_kept() {
        let notice = "Rodzicu, nauczyciel dodał uwagę. Przejdź do menu: Uczeń i wybierz: Uwagi. Wiadomość została wygenerowana automatycznie.";
        assert!(is_remark_notification(notice));
        assert!(!is_remark_notification(
            "Proszę zapoznać się z uwagą. Dziś uczennica przeszkadzała na lekcji."
        ));
        assert!(!is_remark_notification(
            "Nowa ocena. Wiadomość została wygenerowana automatycznie."
        ));
    }
}
