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
    } else if (type === 'homework') {
      return `${idx + 1}. ${item.title || 'Brak tytułu'}\n   Przedmiot: ${item.subject || 'Nieznany'}\n   Nauczyciel: ${item.user || 'Nieznany'}\n   Typ: ${item.type || 'Nieznany'}\n   Od: ${item.from || 'Brak'}\n   Do: ${item.to || 'Brak'}\n   Treść: ${item.content || 'Brak treści'}`;
    }
  }).join('\n\n');

  let prompt;
  if (type === 'grades') {
    prompt = `Przeanalizuj nowe oceny Emilki z klasy 1A SP.

OCENY:
${itemsText}

SYSTEM OCENIANIA:
- T = znakomicie (najlepsza)
- + = dobrze
- +/- = przeciętnie

KATEGORIE OCEN: aktywność, sprawdzian, odpowiedź ustna, praca na lekcji, podsumowanie miesiąca

ZADANIA:
1. Wyodrębnij kluczowe uwagi nauczycieli (np. "mniej rozmawiać z koleżanką", wyniki procentowe sprawdzianów)
2. Określ pilność: PILNE tylko gdy są uwagi o problemach, NORMALNE dla pozytywnych ocen
3. Napisz podsumowanie (2-3 zdania) w pozytywnym tonie podkreślając sukcesy
4. Wypisz oceny: 📚 przedmiot | ⭐ **ocena** | 💬 istotna uwaga nauczyciela

FORMAT: Podsumowanie to Twoja analiza - NIE cytuj dosłownie komentarzy nauczycieli.

JSON:
{"urgency": "PILNE|NORMALNE|NIEPILNE", "summary": "Twoja analiza postępów", "keyPoints": ["📚 Przedmiot | ⭐ **T** | 💬 uwaga"]}`;
  } else if (type === 'events') {
    prompt = `Przeanalizuj nowe wydarzenia z kalendarza szkolnego dla klasy 1A SP ETE.

WYDARZENIA:
${itemsText}

ZADANIA:
1. Wyodrębnij daty i godziny wydarzeń
2. Określ pilność: PILNE (< 7 dni), NORMALNE (> 7 dni), NIEPILNE (informacyjne)
3. Napisz podsumowanie (2-3 zdania)
4. Wypisz: 📅 **data** - wydarzenie | ⏰ godzina

FORMAT: Podsumowanie to Twoja analiza - NIE cytuj treści.

JSON:
{"urgency": "PILNE|NORMALNE|NIEPILNE", "summary": "Twoja analiza", "keyPoints": ["📅 **data** - wydarzenie"]}`;
  } else if (type === 'homework') {
    prompt = `Przeanalizuj nowe zadania domowe dla klasy 1A SP ETE.

ZADANIA DOMOWE:
${itemsText}

ZADANIA:
1. Wyodrębnij terminy wykonania zadań
2. Określ pilność: PILNE (termin < 3 dni), NORMALNE (termin 3-7 dni), NIEPILNE (termin > 7 dni)
3. Napisz podsumowanie (2-3 zdania) - co trzeba zrobić
4. Wypisz: 📝 **przedmiot** - zadanie | ⏰ termin do

FORMAT: Podsumowanie to Twoja analiza CO TRZEBA ZROBIĆ - NIE cytuj treści zadań dosłownie.

JSON:
{"urgency": "PILNE|NORMALNE|NIEPILNE", "summary": "Co trzeba zrobić", "keyPoints": ["📝 **przedmiot** - zadanie | ⏰ termin"]}`;
  } else {
    const typeLabel = type === 'announcements' ? 'OGŁOSZENIA' : 'WIADOMOŚCI';
    prompt = `Przeanalizuj ${typeLabel.toLowerCase()} ze szkoły dla klasy 1A SP ETE.

KONTEKST: Emilka, klasa 1A SP ETE w Gliwicach, wychowawca p. Monika Podżorska.

${typeLabel}:
${itemsText}

IGNORUJ CAŁKOWICIE:
- Wiadomości WYSŁANE PRZEZ RODZICA (np. "Usprawiedliwienie", prośby o zwolnienie) - to nasze wiadomości, nie od szkoły
- Ogłoszenia z "4-8 SP", "7/8 SP", "1-4 LO", "klasy 4-8" - nie dotyczą klasy 1
- Wiadomości od "SuperAdministrator"
- Odpowiedzi "Re:" które tylko potwierdzają (np. "Dziękuję za informację")

TYPY WIADOMOŚCI DO ROZPOZNANIA:
- CZYTANIE: "tekst do ładnego czytania", strony z podręcznika (np. str. 45, 52)
- WYCIECZKA: koszty (np. 50zł), godziny wyjazdu/powrotu, co zabrać
- PRZYBORY: materiały na plastykę (włóczka, papier), mundurki
- KONSULTACJE: terminy spotkań z nauczycielami, godziny
- AKCJE: "Prezent pod Choinkę", zbiórki, terminy
- URODZINY: zaproszenia, daty, miejsca, godziny

PILNOŚĆ:
- PILNE: termin < 3 dni, przybory na jutro, wpłaty z krótkim terminem
- NORMALNE: wydarzenia 3-14 dni, informacje od wychowawcy
- NIEPILNE: podziękowania, potwierdzenia, informacje dla innych klas

WYODRĘBNIJ KONKRETNIE:
💰 Kwoty i terminy wpłat (np. "50 zł do 9.10")
🎒 Przybory do przyniesienia (np. "2 kłębki włóczki")
📖 Strony do czytania (np. "str. 52-53 na poniedziałek")
📅 Daty i godziny wydarzeń
⏰ Godziny zbiórek i powrotów

FORMAT: Podsumowanie to Twoja analiza CO TRZEBA ZROBIĆ - NIE cytuj treści wiadomości dosłownie.

JSON:
{"urgency": "PILNE|NORMALNE|NIEPILNE", "summary": "Co rodzic musi zrobić", "keyPoints": ["emoji **termin** - działanie"]}`;
  }

  try {
    logger.debug(`Sending request to OpenAI for ${type}`);

    const systemMessage = type === 'grades'
      ? 'Jesteś asystentem rodzica uczennicy 1 klasy SP. Analizujesz oceny i wyciągasz konstruktywne uwagi nauczycieli. Odpowiadasz TYLKO JSON. Podsumowanie to Twoja analiza, nie cytat.'
      : type === 'events'
      ? 'Jesteś asystentem rodzica uczennicy klasy 1A SP. Analizujesz kalendarz szkolny. Odpowiadasz TYLKO JSON. Podsumowanie to Twoja analiza.'
      : type === 'homework'
      ? 'Jesteś asystentem rodzica uczennicy klasy 1A SP. Analizujesz zadania domowe i wyodrębniasz terminy oraz wymagania. Odpowiadasz TYLKO JSON. Podsumowanie to Twoja analiza CO TRZEBA ZROBIĆ.'
      : 'Jesteś asystentem rodzica uczennicy klasy 1A SP ETE. Wyodrębniasz KONKRETNE działania: co kupić, ile zapłacić, jakie strony przeczytać, kiedy przyjść. Ignorujesz wiadomości dla innych klas. Odpowiadasz TYLKO JSON. Podsumowanie to Twoja analiza CO ZROBIĆ, nie cytat wiadomości.';

    const response = await getOpenAI().chat.completions.create({
      model: 'gpt-5.2',
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
    const typeLabel = type === 'announcements' ? 'ogłoszeń' : type === 'messages' ? 'wiadomości' : type === 'grades' ? 'ocen' : type === 'homework' ? 'zadań domowych' : 'wydarzeń';
    return {
      urgency: 'NORMALNE',
      summary: `Nie udało się wygenerować podsumowania. Liczba ${typeLabel}: ${items.length}`,
      keyPoints: items.map(item => item.title || item.subject || 'Brak tytułu')
    };
  }
}
