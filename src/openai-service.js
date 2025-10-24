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
    } else {
      return `${idx + 1}. ${item.title || 'Brak tytułu'}\n   Od: ${item.user || 'Nieznany'}\n   Data: ${item.date || 'Brak daty'}\n   Treść: ${item.body || 'Brak treści'}`;
    }
  }).join('\n\n');

  const prompt = `Jesteś asystentem analizującym ${type === 'announcements' ? 'ogłoszenia' : 'wiadomości'} ze szkoły (Librus).

KONTEKST:
- Córka użytkownika uczęszcza do 1 klasy szkoły podstawowej
- Niektóre wydarzenia mogą być bardziej istotne dla innych klas
- Ważne jest, aby nie przegapić żadnych zadań lub terminów
- NIE oznaczaj ogólnych wydarzeń jako pilne, chyba że wymagają obecności rodzica lub aktywnego działania

${type === 'announcements' ? 'OGŁOSZENIA' : 'WIADOMOŚCI'}:
${itemsText}

Zadania:
1. Przeanalizuj wszystkie ${type === 'announcements' ? 'ogłoszenia' : 'wiadomości'}
2. Oceń poziom pilności (PILNE/NORMALNE/NIEPILNE) na podstawie:
   - PILNE: tylko jeśli wymaga działania rodzica, ma bliski termin (do 7 dni), lub dotyczy ucznia klasy 1
   - NORMALNE: ogólne informacje, wydarzenia bez wymaganego działania
   - NIEPILNE: wydarzenia dotyczące innych klas, ogólne przypomnienia bez terminu
3. Wygeneruj:
   - Krótkie podsumowanie (2-3 zdania) uwzględniające kontekst klasy 1
   - Listę najważniejszych punktów (bullet points) z formatowaniem:
     * Używaj **pogrubienia** dla dat i terminów
     * Dodawaj emoji aby wyróżnić typ informacji (📅 dla dat, ⏰ dla terminów, 💰 dla płatności, 📝 dla zadań, etc.)
     * Podkreśl terminy i wymagane działania
   - Klasyfikację pilności

Zwróć odpowiedź w formacie JSON:
{
  "urgency": "PILNE" | "NORMALNE" | "NIEPILNE",
  "summary": "krótkie podsumowanie",
  "keyPoints": ["punkt 1 z **datami** i emoji", "punkt 2", ...]
}`;

  try {
    logger.debug(`Sending request to OpenAI for ${type}`);

    const response = await getOpenAI().chat.completions.create({
      model: 'gpt-4o-mini',
      messages: [
        { role: 'system', content: 'Jesteś pomocnym asystentem analizującym komunikację szkolną.' },
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
    return {
      urgency: 'NORMALNE',
      summary: `Nie udało się wygenerować podsumowania. Liczba ${type === 'announcements' ? 'ogłoszeń' : 'wiadomości'}: ${items.length}`,
      keyPoints: items.map(item => item.title || 'Brak tytułu')
    };
  }
}
