# Powiadomienia z Librusa

Zautomatyzowany system powiadomień dla Librusa (polski system do zarządzania szkołą) z analizą AI i powiadomieniami e-mail.

## Co robi?

Ta usługa automatycznie monitoruje Twoje konto Librus pod kątem nowych ogłoszeń i wiadomości, analizuje je za pomocą OpenAI i wysyła inteligentne powiadomienia e-mail. Główne funkcje:

- **Automatyczne monitorowanie Librusa**: Loguje się do Librusa i pobiera nowe ogłoszenia i wiadomości
- **Inteligentne wykrywanie zmian**: Używa bazy danych SQLite do śledzenia już przetworzonych elementów
- **Analiza AI**: OpenAI (GPT-4o-mini) analizuje treść i:
  - Generuje zwięzłe podsumowania dostosowane do kontekstu klasy 1
  - Wyodrębnia kluczowe punkty z **pogrubionymi datami** i emoji (📅 📝 ⏰ 💰)
  - Klasyfikuje pilność (PILNE/NORMALNE/NIEPILNE) na podstawie wymagań działania rodzica
  - Oznacza jako pilne tylko te elementy, które wymagają działania rodzica lub mają bliskie terminy
- **Bogate powiadomienia e-mail w HTML**:
  - Podsumowanie AI i kluczowe punkty
  - Indywidualne zwijane sekcje dla każdego ogłoszenia/wiadomości
  - Bezpośrednie linki do wiadomości w interfejsie webowym Librusa
  - Wsparcie dla wielu odbiorców (oddzielonych przecinkami)
  - Niestandardowa nazwa nadawcy ("ETE librus <librus@flakm.com>")
- **Gotowe do produkcji**:
  - Logowanie kompatybilne z systemd
  - Moduł NixOS do deklaratywnego wdrożenia
  - Wzmocnienie bezpieczeństwa (PrivateTmp, NoNewPrivileges, ProtectSystem)

### Przykładowy e-mail

![Zrzut ekranu e-maila](email-screenshot.png)

E-mail pokazuje podsumowania wygenerowane przez AI z odznakami pilności, kluczowymi punktami z emoji i pogrubionymi datami oraz zwijalnymi sekcjami dla każdego ogłoszenia/wiadomości z bezpośrednimi linkami do Librusa.

## Konfiguracja

### Zmienne środowiskowe

Utwórz plik `.env` z następującą konfiguracją:

```env
# Dane logowania Librus (wymagane)
LIBRUS_USERNAME=111110000
LIBRUS_PASSWORD=TwojeHasło

# OpenAI API (wymagane)
OPENAI_API_KEY=sk-proj-xxxxxxxxxxxxx

# Konfiguracja e-mail
SEND_EMAIL=true                                 # Włącz/wyłącz wysyłanie e-maili
EMAIL_HOST=smtp.fastmail.com                    # Serwer SMTP
EMAIL_PORT=587                                  # Port SMTP (587 dla TLS)
EMAIL_USER=me@example.com                       # Nazwa użytkownika uwierzytelniania SMTP
EMAIL_PASSWORD=password                         # Hasło aplikacji (NIE główne hasło)
EMAIL_FROM=ETE librus <librus@example.com>      # Nazwa i adres nadawcy
EMAIL_TO=me@flakm.com,other@example.com         # Odbiorcy (oddzieleni przecinkami dla wielu)

# Ustawienia opcjonalne
LOG_LEVEL=INFO                                  # ERROR, WARN, INFO lub DEBUG
DB_PATH=./librus.db                             # Lokalizacja bazy danych SQLite
```

### Konfiguracja dostawcy e-mail

#### Fastmail (Zalecane)

1. Przejdź do Ustawienia → Hasło i bezpieczeństwo → Hasła aplikacji
2. Utwórz nowe hasło aplikacji dla "librus-notifications"
3. Użyj swojego głównego e-maila Fastmail jako `EMAIL_USER`
4. Użyj wygenerowanego hasła aplikacji jako `EMAIL_PASSWORD`
5. Użyj dowolnego aliasu jako `EMAIL_FROM` (np. `librus@twojadomena.com`)

#### Gmail

1. Włącz uwierzytelnianie dwuskładnikowe
2. Przejdź do Konto Google → Bezpieczeństwo → Hasła aplikacji
3. Wygeneruj hasło aplikacji dla "Poczta"
4. Użyj swojego adresu Gmail jako `EMAIL_USER`
5. Użyj wygenerowanego 16-znakowego hasła jako `EMAIL_PASSWORD`

## Rozwój lokalny

### Wymagania wstępne

- Node.js 20+
- Nix (opcjonalny, ale zalecany)

### Konfiguracja z Nix (Zalecane)

```bash
# Wejdź do shella - alternatywnie z direnv po prostu cd  
nix develop

# Zainstaluj zależności
npm install

# Skopiuj i skonfiguruj środowisko
cp .env.example .env
# Edytuj .env swoimi danymi logowania

# Uruchom usługę
npm start
```

### Konfiguracja bez Nix

```bash
# Zainstaluj Node.js 20+ i zależności
npm install

# Skonfiguruj środowisko
cp .env.example .env
# Edytuj .env swoimi danymi logowania

# Uruchom usługę
npm start
```

### Testowanie

```bash
# Test z wyłączonym e-mailem (tylko wyjście konsoli)
SEND_EMAIL=false npm start

# Test z włączonym e-mailem
npm start

# Usuń bazę danych, aby ponownie przetworzyć wszystkie elementy
rm librus.db && npm start
```

## Wdrożenie

### Opcja 1: Moduł NixOS (Zalecane dla NixOS)

#### 1. Dodaj do swoich input flake

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    librus-notifications.url = "github:yourusername/librus";
    # Lub użyj lokalnej ścieżki podczas rozwoju:
    # librus-notifications.url = "path:/path/to/librus";
  };

  outputs = { self, nixpkgs, librus-notifications }: {
    nixosConfigurations.yourhostname = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./configuration.nix
        librus-notifications.nixosModules.default
      ];
    };
  };
}
```

#### 2. Skonfiguruj usługę w `configuration.nix`

Sekrety najlepiej przekazywać przy użyciu sops/innego mechanizmu zarządzania sekretami lub jako plik z ograniczonymi uprawnieniami.

```nix
{
  services.librus-notifications = {
    enable = true;
    package = inputs.librus-notifications.packages.x86_64-linux.default;
    environmentFile = "/etc/librus-notifications/credentials.env";

    # Opcjonalnie: dostosuj harmonogram (domyślnie 7:00 i 15:00)
    schedule = [ "*-*-* 07:00:00" "*-*-* 15:00:00" ];

    # Opcjonalnie: dostosuj użytkownika/grupę i katalog danych
    user = "librus-notifications";
    group = "librus-notifications";
    dataDir = "/var/lib/librus-notifications";
  };
}
```

#### 3. Przebuduj i sprawdź status

```bash
sudo nixos-rebuild switch

# Sprawdź status usługi
sudo systemctl status librus-notifications.timer
sudo systemctl list-timers | grep librus

# Zobacz logi
journalctl -u librus-notifications -f

# Ręczny test
sudo systemctl start librus-notifications.service
```

Zobacz `NIXOS_MODULE.md` dla pełnej dokumentacji modułu i zaawansowanych konfiguracji.

### Opcja 2: Ręczny Systemd (Inne dystrybucje Linux)

#### 1. Zbuduj pakiet z Nix

```bash
nix build
# Binarka będzie w: ./result/bin/librus-notifications
```

#### 2. Zainstaluj w lokalizacji systemowej

```bash
# Skopiuj binarki
sudo cp -r result /opt/librus-notifications

# Utwórz dowiązanie symboliczne
sudo ln -s /opt/librus-notifications/bin/librus-notifications /usr/local/bin/librus-notifications
```

#### 3. Utwórz pliki usługi systemd

Edytuj `librus-notifications.service` i `librus-notifications.timer`, następnie:

```bash
sudo cp librus-notifications.service /etc/systemd/system/
sudo cp librus-notifications.timer /etc/systemd/system/
```

#### 4. Skonfiguruj dane logowania

```bash
sudo mkdir -p /etc/librus-notifications
sudo cp .env /etc/librus-notifications/credentials.env
sudo chmod 600 /etc/librus-notifications/credentials.env
```

#### 5. Włącz i uruchom

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now librus-notifications.timer

### Sprawdź status

```bash
sudo systemctl status librus-notifications.timer
journalctl -u librus-notifications -f
```

## Licencja

MIT
