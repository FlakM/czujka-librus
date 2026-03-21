use std::env;

use anyhow::{anyhow, Context, Result};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

use chrono::Local;

use crate::models::{Analysis, AnnouncementItem, GradeItem, HomeworkItem, MessageItem};

const OPENAI_API_URL: &str = "https://api.openai.com/v1/chat/completions";

#[derive(Clone)]
pub struct OpenAiClient {
    http: reqwest::Client,
    api_key: String,
    model: String,
}

impl OpenAiClient {
    pub fn from_env() -> Result<Self> {
        let api_key =
            env::var("OPENAI_API_KEY").map_err(|_| anyhow!("OPENAI_API_KEY must be set"))?;
        let model = env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-5.2".to_string());
        Ok(Self {
            http: reqwest::Client::new(),
            api_key,
            model,
        })
    }

    async fn chat_json(&self, system: &str, user: &str) -> Result<Analysis> {
        let request = OpenAiRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user.to_string(),
                },
            ],
            response_format: ResponseFormat {
                r#type: "json_object".to_string(),
            },
            temperature: 0.3,
        };

        let resp = self
            .http
            .post(OPENAI_API_URL)
            .header(CONTENT_TYPE, "application/json")
            .header(AUTHORIZATION, format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .context("OpenAI request failed")?;

        let status = resp.status();
        let body = resp.text().await.context("OpenAI response read failed")?;

        if !status.is_success() {
            return Err(anyhow!("OpenAI API error: {} - {}", status, body));
        }

        let parsed: OpenAiResponse =
            serde_json::from_str(&body).context("OpenAI response parse failed")?;
        let content = parsed
            .choices
            .get(0)
            .ok_or_else(|| anyhow!("OpenAI response missing choices"))?
            .message
            .content
            .clone();

        let analysis: Analysis =
            serde_json::from_str(&content).context("OpenAI JSON content parse failed")?;
        Ok(analysis)
    }

    pub async fn summarize_messages(&self, items: &[MessageItem]) -> Result<Analysis> {
        let today = Local::now().format("%Y-%m-%d (%A)").to_string();
        let items_text = items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                format!(
                    "{}. {}\n   Od: {}\n   Data: {}\n   Treść: {}",
                    idx + 1,
                    item.title,
                    item.user,
                    item.date,
                    item.body
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let prompt = format!(
            "DZISIAJ JEST: {}\n\nPrzeanalizuj wiadomości ze szkoły dla klasy 1A SP ETE.\n\nKONTEKST: Emilka, klasa 1A SP ETE w Gliwicach, wychowawca p. Monika Podżorska.\n\nWIADOMOŚCI:\n{}\n\nIGNORUJ CAŁKOWICIE:\n- Wiadomości WYSŁANE PRZEZ RODZICA (np. \"Usprawiedliwienie\", prośby o zwolnienie) - to nasze wiadomości, nie od szkoły\n- Ogłoszenia z \"4-8 SP\", \"7/8 SP\", \"1-4 LO\", \"klasy 4-8\" - nie dotyczą klasy 1\n- Wiadomości od \"SuperAdministrator\"\n- Odpowiedzi \"Re:\" które tylko potwierdzają (np. \"Dziękuję za informację\")\n- WYDARZENIA KTÓRE JUŻ MINĘŁY (data w przeszłości względem dzisiaj)\n\nTYPY WIADOMOŚCI DO ROZPOZNANIA:\n- CZYTANIE: \"tekst do ładnego czytania\", strony z podręcznika\n- WYCIECZKA: koszty, godziny wyjazdu/powrotu, co zabrać\n- PRZYBORY: materiały na plastykę, mundurki\n- KONSULTACJE: terminy spotkań z nauczycielami\n- TESTY/SPRAWDZIANY: daty, zakres materiału\n- AKCJE: zbiórki, terminy, wpłaty\n\nPILNOŚĆ (względem dzisiejszej daty {}):\n- PILNE: termin w ciągu 3 dni\n- NORMALNE: termin 4-14 dni\n- NIEPILNE: termin > 14 dni lub informacyjne\n\nWYODRĘBNIJ KONKRETNIE:\n💰 Kwoty i terminy wpłat\n🎒 Przybory do przyniesienia\n📖 Strony/materiał do nauki\n📅 Daty i godziny wydarzeń\n⏰ Godziny zbiórek\n\nFORMAT: Podsumowanie (2-3 zdania) to Twoja analiza CO TRZEBA ZROBIĆ.\nkeyPoints: max 5-7 najważniejszych, posortowane od najpilniejszych.\n\nJSON:\n{{\"urgency\": \"PILNE|NORMALNE|NIEPILNE\", \"summary\": \"Co rodzic musi zrobić\", \"keyPoints\": [\"emoji **termin** - działanie\"]}}",
            today, items_text, today
        );

        let system = "Jesteś asystentem rodzica uczennicy klasy 1A SP ETE. Wyodrębniasz KONKRETNE działania: co kupić, ile zapłacić, jakie strony przeczytać, kiedy przyjść. Ignorujesz wiadomości dla innych klas i wydarzenia które już minęły. Odpowiadasz TYLKO JSON.";

        self.chat_json(system, &prompt).await
    }

    pub async fn summarize_announcements(&self, items: &[AnnouncementItem]) -> Result<Analysis> {
        let today = Local::now().format("%Y-%m-%d (%A)").to_string();
        let items_text = items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                format!(
                    "{}. {}\n   Autor: {}\n   Data: {}\n   Treść: {}",
                    idx + 1,
                    item.title,
                    item.author,
                    item.date,
                    item.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let prompt = format!(
            "DZISIAJ JEST: {}\n\nPrzeanalizuj ogłoszenia ze szkoły dla klasy 1A SP ETE.\n\nKONTEKST: Emilka, klasa 1A SP ETE w Gliwicach, wychowawca p. Monika Podżorska.\n\nOGŁOSZENIA:\n{}\n\nIGNORUJ CAŁKOWICIE:\n- Ogłoszenia z \"4-8 SP\", \"7/8 SP\", \"1-4 LO\", \"klasy 4-8\" - nie dotyczą klasy 1\n- Ogłoszenia od \"SuperAdministrator\" jeśli są ogólne\n- WYDARZENIA KTÓRE JUŻ MINĘŁY\n\nPILNOŚĆ (względem dzisiejszej daty):\n- PILNE: termin w ciągu 3 dni\n- NORMALNE: termin 4-14 dni\n- NIEPILNE: termin > 14 dni lub informacyjne\n\nWYODRĘBNIJ KONKRETNIE:\n💰 Kwoty i terminy wpłat\n🎒 Przybory do przyniesienia\n📅 Daty i godziny wydarzeń\n⏰ Godziny zbiórek\n\nFORMAT: Podsumowanie (2-3 zdania). keyPoints: max 5-7 najważniejszych.\n\nJSON:\n{{\"urgency\": \"PILNE|NORMALNE|NIEPILNE\", \"summary\": \"Co rodzic musi zrobić\", \"keyPoints\": [\"emoji **termin** - działanie\"]}}",
            today, items_text
        );

        let system = "Jesteś asystentem rodzica uczennicy klasy 1A SP ETE. Wyodrębniasz KONKRETNE działania. Ignorujesz ogłoszenia dla innych klas i minione wydarzenia. Odpowiadasz TYLKO JSON.";

        self.chat_json(system, &prompt).await
    }

    pub async fn summarize_grades(&self, items: &[GradeItem]) -> Result<Analysis> {
        let items_text = items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                format!(
                    "{}. Przedmiot: {}\n   Ocena: {}\n   Informacje: {}",
                    idx + 1,
                    item.subject,
                    item.value,
                    item.info
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let prompt = format!(
            "Przeanalizuj nowe oceny Emilki z klasy 1A SP.\n\nOCENY:\n{}\n\nSYSTEM OCENIANIA:\n- T = znakomicie (najlepsza)\n- + = dobrze\n- +/- = przeciętnie\n\nKATEGORIE OCEN: aktywność, sprawdzian, odpowiedź ustna, praca na lekcji, podsumowanie miesiąca\n\nZADANIA:\n1. Wyodrębnij kluczowe uwagi nauczycieli (np. \"mniej rozmawiać z koleżanką\", wyniki procentowe sprawdzianów)\n2. Określ pilność: PILNE tylko gdy są uwagi o problemach, NORMALNE dla pozytywnych ocen\n3. Napisz podsumowanie (2-3 zdania) w pozytywnym tonie podkreślając sukcesy\n4. Wypisz oceny: 📚 przedmiot | ⭐ **ocena** | 💬 istotna uwaga nauczyciela\n\nFORMAT: Podsumowanie to Twoja analiza - NIE cytuj dosłownie komentarzy nauczycieli.\n\nJSON:\n{{\"urgency\": \"PILNE|NORMALNE|NIEPILNE\", \"summary\": \"Twoja analiza postępów\", \"keyPoints\": [\"📚 Przedmiot | ⭐ **T** | 💬 uwaga\"]}}",
            items_text
        );

        let system = "Jesteś asystentem rodzica uczennicy 1 klasy SP. Analizujesz oceny i wyciągasz konstruktywne uwagi nauczycieli. Odpowiadasz TYLKO JSON. Podsumowanie to Twoja analiza, nie cytat.";

        self.chat_json(system, &prompt).await
    }

    pub async fn summarize_homework(&self, items: &[HomeworkItem]) -> Result<Analysis> {
        let today = Local::now().format("%Y-%m-%d (%A)").to_string();
        let items_text = items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                format!(
                    "{}. {}\n   Przedmiot: {}\n   Nauczyciel: {}\n   Typ: {}\n   Od: {}\n   Do: {}\n   Treść: {}",
                    idx + 1,
                    item.title,
                    item.subject,
                    item.user,
                    item.kind,
                    item.from,
                    item.to,
                    item.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let prompt = format!(
            "DZISIAJ JEST: {}\n\nPrzeanalizuj zadania domowe dla klasy 1A SP ETE.\n\nZADANIA DOMOWE:\n{}\n\nIGNORUJ zadania z terminem który już minął.\n\nPILNOŚĆ (względem dzisiejszej daty):\n- PILNE: termin w ciągu 3 dni\n- NORMALNE: termin 4-7 dni\n- NIEPILNE: termin > 7 dni\n\nFORMAT: Podsumowanie (2-3 zdania). keyPoints: posortowane od najpilniejszych.\n\nJSON:\n{{\"urgency\": \"PILNE|NORMALNE|NIEPILNE\", \"summary\": \"Co trzeba zrobić\", \"keyPoints\": [\"📝 **przedmiot** - zadanie | ⏰ termin\"]}}",
            today, items_text
        );

        let system = "Jesteś asystentem rodzica uczennicy klasy 1A SP. Analizujesz zadania domowe, ignorujesz minione terminy. Odpowiadasz TYLKO JSON.";

        self.chat_json(system, &prompt).await
    }
}

#[derive(Debug, Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<ChatMessage>,
    response_format: ResponseFormat,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    r#type: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: String,
}
