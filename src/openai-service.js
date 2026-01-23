import OpenAI from 'openai';
import { logger } from './logger.js';

let openai;

function getOpenAI() {
  if (!openai) {
    openai = new OpenAI({
      apiKey: process.env.OPENAI_API_KEY
    });
  }
  return openai;
}

export async function summarizeAndClassify(items, type = 'announcements') {
  if (!items || items.length === 0) {
    return null;
  }

  const itemsText = items.map((item, idx) => {
    if (type === 'announcements') {
      return `${idx + 1}. ${item.title || 'Brak tytułu'}\n   Autor: ${item.user || item.author || 'Nieznany'}\n   Data: ${item.date || 'Brak daty'}\n   Treść: ${item.content || 'Brak treści'}`;
    } else if (type === 'messages') {
      return `${idx + 1}. ${item.title || 'Brak tytułu'}\n   Od: ${item.user || 'Nieznany'}\n   Data: ${item.date || 'Brak daty'}\n   Treść: ${item.body || 'Brak treści'}`;
    } else if (type === 'grades') {
      return `${idx + 1}. Przedmiot: ${item.subject || 'Nieznany'}\n   Ocena: ${item.value || 'Brak oceny'}\n   Informacje: ${item.info || 'Brak informacji'}`;
    } else if (type === 'events') {
      return `${idx + 1}. ${item.title || 'Brak tytułu'}\n   Data: ${item.day || 'Brak daty'}\n   Opis: ${item.description || 'Brak opisu'}`;
    }
  }).join('\n\n');

  let prompt;
  if (type === 'grades') {
    prompt = `Przeanalizuj nowe oceny uczennicy klasy 1 SP.

OCENY:
${itemsText}

System oceniania: T (znakomicie), + (dobrze), +/- (przeciętnie).

ZADANIA:
1. Wyodrębnij kluczowe informacje z komentarzy nauczycieli
2. Określ pilność: PILNE (problemy wymagające uwagi), NORMALNE (standardowe), NIEPILNE (bez znaczenia)
3. Napisz podsumowanie (2-3 zdania) w pozytywnym tonie
4. Wypisz oceny z emoji: 📚 przedmiot, ⭐ ocena, 💬 komentarz

WAŻNE: W podsumowaniu napisz TYLKO własną analizę. Nie cytuj treści wiadomości ani nie powtarzaj tekstu z ocen dosłownie.

JSON:
{"urgency": "PILNE|NORMALNE|NIEPILNE", "summary": "własna analiza", "keyPoints": ["📚 Przedmiot: **ocena** - komentarz"]}`;
  } else if (type === 'events') {
    prompt = `Przeanalizuj nowe wydarzenia z kalendarza szkolnego.

KONTEKST: Emilia, klasa 1A SP ETE w Gliwicach, wychowawca p. Monika Podżorska.

WYDARZENIA:
${itemsText}

ZADANIA:
1. Wyodrębnij ważne daty i wydarzenia dotyczące klasy 1
2. Określ pilność: PILNE (< 7 dni, wymaga działania), NORMALNE (> 7 dni), NIEPILNE (informacyjne)
3. Napisz podsumowanie (2-3 zdania)
4. Wypisz działania z emoji: 📅 data, 🏫 wydarzenie, ⏰ godzina

WAŻNE: W podsumowaniu napisz TYLKO własną analizę. Nie cytuj treści wydarzeń dosłownie.

JSON:
{"urgency": "PILNE|NORMALNE|NIEPILNE", "summary": "własna analiza", "keyPoints": ["📅 **data** - wydarzenie"]}`;
  } else {
    prompt = `Przeanalizuj ${type === 'announcements' ? 'ogłoszenia' : 'wiadomości'} ze szkoły.

KONTEKST: Emilia, klasa 1A SP ETE w Gliwicach, wychowawca p. Monika Podżorska.
IGNORUJ: Oznaczenia "7/8 SP", "klasy 4-8", "1-4 LO" - nie dotyczą klasy 1A. Wiadomości od "SuperAdministrator".

${type === 'announcements' ? 'OGŁOSZENIA' : 'WIADOMOŚCI'}:
${itemsText}

KATEGORIE: ZAOPATRZENIE (przybory), WYCIECZKA (wyjazdy), ZADANIE (do domu), WYDARZENIE (uroczystości), ORGANIZACJA (zmiany), ZDROWIE.

PILNOŚĆ:
- PILNE: termin < 7 dni, przybory na zajęcia, konsultacje
- NORMALNE: wydarzenia > 7 dni, informacje od wychowawcy
- NIEPILNE: podziękowania, potwierdzenia, inne klasy

WYODRĘBNIJ:
💰 Kwoty i terminy płatności
🎒 Przybory do przyniesienia
📖 Strony do przeczytania
📅 Daty wydarzeń
⏰ Godziny zbiórek

WAŻNE: W podsumowaniu napisz TYLKO własną analizę - co rodzic musi wiedzieć i zrobić. NIE cytuj dosłownie treści wiadomości. NIE przepisuj tekstu z ogłoszeń.

JSON:
{"urgency": "PILNE|NORMALNE|NIEPILNE", "summary": "własna analiza co zrobić", "keyPoints": ["emoji **data/kwota** - działanie"]}`;
  }

  try {
    logger.debug(`Sending request to OpenAI for ${type}`);

    const systemMessage = type === 'grades'
      ? 'Jesteś asystentem rodzica uczennicy 1 klasy SP. Analizujesz oceny i wyciągasz istotne informacje. Odpowiadasz TYLKO w formacie JSON. W podsumowaniu piszesz własną analizę, nie cytujesz treści.'
      : type === 'events'
      ? 'Jesteś asystentem rodzica uczennicy klasy 1A SP. Analizujesz wydarzenia szkolne. Odpowiadasz TYLKO w formacie JSON. W podsumowaniu piszesz własną analizę, nie cytujesz treści.'
      : 'Jesteś asystentem rodzica uczennicy klasy 1A SP ETE w Gliwicach. Wyodrębniasz konkretne działania i terminy, ignorując informacje dla innych klas. Odpowiadasz TYLKO w formacie JSON. W podsumowaniu piszesz własną analizę, NIE cytujesz dosłownie treści wiadomości.';

    const response = await getOpenAI().chat.completions.create({
      model: 'gpt-5-mini',
      messages: [
        { role: 'system', content: systemMessage },
        { role: 'user', content: prompt }
      ],
      response_format: { type: 'json_object' },
      temperature: 0.3
    });

    const result = JSON.parse(response.choices[0].message.content);
    logger.info(`OpenAI analysis completed for ${type}`, { urgency: result.urgency });

    return result;
  } catch (error) {
    logger.error(`OpenAI API error for ${type}:`, { error: error.message });
    const typeLabel = type === 'announcements' ? 'ogłoszeń' : type === 'messages' ? 'wiadomości' : type === 'grades' ? 'ocen' : 'wydarzeń';
    return {
      urgency: 'NORMALNE',
      summary: `Nie udało się wygenerować podsumowania. Liczba ${typeLabel}: ${items.length}`,
      keyPoints: items.map(item => item.title || item.subject || 'Brak tytułu')
    };
  }
}
