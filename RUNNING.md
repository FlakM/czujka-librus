# 🚀 Uruchamianie Czujki Librus

Ten dokument zawiera szczegółowe instrukcje uruchamiania aplikacji w różnych środowiskach.

---

## 📋 Spis treści

- [Docker](#-docker)
- [Docker Compose](#-docker-compose)
- [Nix](#-nix)
- [NixOS (systemd)](#-nixos-systemd)
- [Rust (natywnie)](#-rust-natywnie)
- [Systemd (inne dystrybucje)](#-systemd-inne-dystrybucje)

---

## 🐳 Docker

### Podstawowe uruchomienie

```bash
# Jednorazowe uruchomienie
docker run --rm \
  --env-file .env \
  -v $(pwd)/data:/data \
  ghcr.io/flakm/czujka-librus:latest
```

### Z konkretnymi zmiennymi środowiskowymi

```bash
docker run --rm \
  -e LIBRUS_USERNAME=twoj_login \
  -e LIBRUS_PASSWORD=twoje_haslo \
  -e OPENAI_API_KEY=sk-proj-xxx \
  -e SEND_EMAIL=true \
  -e EMAIL_HOST=smtp.fastmail.com \
  -e EMAIL_PORT=587 \
  -e EMAIL_USER=twoj@email.com \
  -e EMAIL_PASSWORD=haslo_aplikacji \
  -e EMAIL_FROM="Czujka <czujka@example.com>" \
  -e EMAIL_TO=odbiorca@example.com \
  -v $(pwd)/data:/data \
  ghcr.io/flakm/czujka-librus:latest
```

### Montowanie volumes

```bash
# Utwórz katalog dla danych
mkdir -p ~/czujka-data

# Uruchom z montowaniem
docker run --rm \
  --env-file .env \
  -v ~/czujka-data:/data \
  ghcr.io/flakm/czujka-librus:latest
```

**Ważne:** Volume `/data` zawiera:
- `librus.db` - baza danych SQLite z historią
- Inne pliki tymczasowe

### Uruchomienie z cron/systemd timer

```bash
# Utwórz skrypt
cat > /usr/local/bin/czujka-run.sh << 'EOF'
#!/bin/bash
docker run --rm \
  --env-file /etc/czujka-librus/.env \
  -v /var/lib/czujka-librus:/data \
  ghcr.io/flakm/czujka-librus:latest
EOF

chmod +x /usr/local/bin/czujka-run.sh

# Dodaj do crontab (2x dziennie: 7:00 i 15:00)
crontab -e
# 0 7,15 * * * /usr/local/bin/czujka-run.sh
```

---

## 🐳 Docker Compose

### Podstawowa konfiguracja

Stwórz `docker-compose.yml`:

```yaml
version: '3.8'

services:
  czujka-librus:
    image: ghcr.io/flakm/czujka-librus:latest
    container_name: czujka-librus
    restart: "no"  # Uruchom ręcznie lub przez cron

    env_file:
      - .env

    volumes:
      - ./data:/data

    environment:
      - LOG_LEVEL=INFO
      - DB_PATH=/data/librus.db
```

### Uruchomienie

```bash
# Jednorazowe uruchomienie
docker-compose run --rm czujka-librus

# Lub z automatycznym usunięciem kontenera
docker-compose up --remove-orphans
```

### Z cron (automatyczne uruchomienia)

Stwórz `docker-compose.cron.yml`:

```yaml
version: '3.8'

services:
  czujka-cron:
    image: alpine:latest
    container_name: czujka-cron
    restart: unless-stopped

    volumes:
      - /var/run/docker.sock:/var/run/docker.sock:ro
      - ./data:/data
      - ./.env:/app/.env:ro

    command: >
      sh -c "
      apk add --no-cache docker-cli &&
      echo '0 7,15 * * * cd /app && docker-compose run --rm czujka-librus' | crontab - &&
      crond -f -l 2
      "
```

---

## ❄️ Nix

### Jednorazowe uruchomienie

```bash
# Bezpośrednio z GitHub (najnowsza wersja)
nix run github:FlakM/czujka-librus

# Z lokalnego repo
nix run .
```

### Build i uruchomienie lokalnie

```bash
# Zbuduj pakiet
nix build

# Uruchom z .env
./result/bin/librus-notifications

# Lub z bezpośrednimi zmiennymi
LIBRUS_USERNAME=xxx \
LIBRUS_PASSWORD=xxx \
OPENAI_API_KEY=xxx \
SEND_EMAIL=true \
EMAIL_TO=twoj@email.com \
./result/bin/librus-notifications
```

### Development shell

```bash
# Wejdź do dev shell
nix develop

# Zbuduj i uruchom
cargo build --release
./target/release/librus-notifications
```

### Z direnv (automatyczny shell)

```bash
# Stwórz .envrc
echo "use flake" > .envrc

# Zezwól na direnv
direnv allow

# Od teraz przy cd do folderu automatycznie wchodzisz w dev shell
cd /path/to/czujka-librus  # automatycznie ładuje środowisko
```

---

## ❄️ NixOS (systemd)

### Konfiguracja podstawowa

W `flake.nix`:

```nix
{
  inputs.czujka-librus.url = "github:FlakM/czujka-librus";

  outputs = { self, nixpkgs, czujka-librus }: {
    nixosConfigurations.yourhost = nixpkgs.lib.nixosSystem {
      modules = [
        czujka-librus.nixosModules.default
        ./configuration.nix
      ];
    };
  };
}
```

W `configuration.nix`:

```nix
{
  services.librus-notifications = {
    enable = true;
    package = inputs.czujka-librus.packages.x86_64-linux.default;
    environmentFile = "/etc/czujka-librus/credentials.env";

    # Harmonogram (domyślnie 7:00 i 15:00)
    schedule = [ "*-*-* 07:00:00" "*-*-* 15:00:00" ];

    # Katalog danych
    dataDir = "/var/lib/czujka-librus";
  };
}
```

### Plik z credentials

```bash
# Stwórz katalog
sudo mkdir -p /etc/czujka-librus

# Stwórz plik credentials
sudo tee /etc/czujka-librus/credentials.env > /dev/null << 'EOF'
LIBRUS_USERNAME=twoj_login
LIBRUS_PASSWORD=twoje_haslo
OPENAI_API_KEY=sk-proj-xxx
SEND_EMAIL=true
EMAIL_HOST=smtp.fastmail.com
EMAIL_PORT=587
EMAIL_USER=twoj@email.com
EMAIL_PASSWORD=haslo_aplikacji
EMAIL_FROM=Czujka <czujka@example.com>
EMAIL_TO=odbiorca@example.com
LOG_LEVEL=INFO
EOF

# Ustaw uprawnienia
sudo chmod 600 /etc/czujka-librus/credentials.env
sudo chown root:root /etc/czujka-librus/credentials.env
```

### Wdrożenie i zarządzanie

```bash
# Przebuduj system
sudo nixos-rebuild switch

# Sprawdź status timera
sudo systemctl status librus-notifications.timer
sudo systemctl list-timers | grep librus

# Zobacz logi
journalctl -u librus-notifications -f

# Ręczne uruchomienie (test)
sudo systemctl start librus-notifications.service

# Zobacz ostatnie uruchomienie
journalctl -u librus-notifications -n 50

# Wyłącz timer
sudo systemctl stop librus-notifications.timer
sudo systemctl disable librus-notifications.timer
```

### Zaawansowana konfiguracja

Zobacz [NIXOS_MODULE.md](NIXOS_MODULE.md) dla:
- Użycia sops-nix do zarządzania sekretami
- Wielu instancji (różne konta)
- Custom schedulingu
- Override'owania ustawień

---

## 📦 Rust (natywnie)

### Instalacja

```bash
# Sklonuj repo
git clone https://github.com/FlakM/czujka-librus.git
cd czujka-librus

# Zainstaluj Rust (jeśli nie masz)
# https://www.rust-lang.org/tools/install

# Zainstaluj build tools (Linux)
# Ubuntu/Debian:
sudo apt-get install build-essential

# Zbuduj binarkę
cargo build --release
```

### Konfiguracja

```bash
# Skopiuj przykładową konfigurację
cp .env.example .env

# Edytuj plik .env
nano .env  # lub vim, code, etc.
```

### Uruchomienie

```bash
# Jednorazowe uruchomienie
./target/release/librus-notifications

# Z custom log level
LOG_LEVEL=DEBUG ./target/release/librus-notifications

# Test bez wysyłania emaili
SEND_EMAIL=false ./target/release/librus-notifications
```

### Czyszczenie bazy danych (re-process)

```bash
# Usuń bazę, aby ponownie przetworzyć wszystkie elementy
rm librus.db
./target/release/librus-notifications
```

---

## ⚙️ Systemd (inne dystrybucje)

### 1. Build z Nix

```bash
# Zainstaluj Nix (jeśli nie masz)
curl -L https://nixos.org/nix/install | sh

# Zbuduj pakiet
nix build github:FlakM/czujka-librus

# Zainstaluj w systemie
sudo cp -r result /opt/czujka-librus
sudo ln -s /opt/czujka-librus/bin/librus-notifications /usr/local/bin/czujka-librus
```

### 2. Konfiguracja

```bash
# Stwórz katalog dla credentials
sudo mkdir -p /etc/czujka-librus

# Skopiuj .env
sudo cp .env /etc/czujka-librus/credentials.env
sudo chmod 600 /etc/czujka-librus/credentials.env

# Stwórz katalog dla danych
sudo mkdir -p /var/lib/czujka-librus
```

### 3. Systemd service

Stwórz `/etc/systemd/system/czujka-librus.service`:

```ini
[Unit]
Description=Czujka Librus - Inteligentne powiadomienia
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
User=czujka
Group=czujka
WorkingDirectory=/var/lib/czujka-librus
EnvironmentFile=/etc/czujka-librus/credentials.env
Environment=NODE_ENV=production
Environment=DB_PATH=/var/lib/czujka-librus/librus.db
ExecStart=/usr/local/bin/czujka-librus

# Bezpieczeństwo
PrivateTmp=true
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/czujka-librus

# Logowanie
StandardOutput=journal
StandardError=journal
SyslogIdentifier=czujka-librus
```

### 4. Systemd timer

Stwórz `/etc/systemd/system/czujka-librus.timer`:

```ini
[Unit]
Description=Czujka Librus Timer
Requires=czujka-librus.service

[Timer]
# Uruchom o 7:00 i 15:00 każdego dnia
OnCalendar=*-*-* 07:00:00
OnCalendar=*-*-* 15:00:00

# Uruchom pominięte timery po restarcie
Persistent=true

[Install]
WantedBy=timers.target
```

### 5. Stwórz użytkownika

```bash
sudo useradd -r -s /bin/false -d /var/lib/czujka-librus czujka
sudo chown -R czujka:czujka /var/lib/czujka-librus
```

### 6. Uruchom i włącz

```bash
# Przeładuj systemd
sudo systemctl daemon-reload

# Włącz timer
sudo systemctl enable czujka-librus.timer
sudo systemctl start czujka-librus.timer

# Sprawdź status
sudo systemctl status czujka-librus.timer
sudo systemctl list-timers | grep czujka

# Test ręczny
sudo systemctl start czujka-librus.service

# Zobacz logi
journalctl -u czujka-librus -f
journalctl -u czujka-librus -n 50
```

### 7. Zarządzanie

```bash
# Stop timer
sudo systemctl stop czujka-librus.timer

# Wyłącz autostart
sudo systemctl disable czujka-librus.timer

# Restart timer (po zmianach w config)
sudo systemctl restart czujka-librus.timer

# Zobacz następne uruchomienie
systemctl list-timers czujka-librus.timer
```

---

## 🔧 Troubleshooting

### Docker

**Problem:** Brak połączenia z siecią
```bash
# Użyj host network
docker run --network host --env-file .env -v $(pwd)/data:/data ghcr.io/flakm/czujka-librus:latest
```

**Problem:** Permission denied na volume
```bash
# Sprawdź uprawnienia
ls -la data/
sudo chown -R $USER:$USER data/
```

### NixOS

**Problem:** Service nie startuje
```bash
# Sprawdź logi
journalctl -xeu librus-notifications.service

# Sprawdź env file
sudo cat /etc/czujka-librus/credentials.env
```

**Problem:** Brak uprawnień do pliku credentials
```bash
sudo chmod 600 /etc/czujka-librus/credentials.env
sudo chown root:root /etc/czujka-librus/credentials.env
```

### Native Rust

**Problem:** Brak zbudowanej binarki
```bash
# Zbuduj release
cargo build --release
```

**Problem:** Zmienne środowiskowe nie ładują się
```bash
# Upewnij się że .env istnieje
ls -la .env

# Testuj z bezpośrednimi zmiennymi
LIBRUS_USERNAME=xxx LIBRUS_PASSWORD=xxx ./target/release/librus-notifications
```

---

## 📊 Monitoring

### Logi w czasie rzeczywistym

```bash
# Docker
docker logs -f container_name

# Systemd/NixOS
journalctl -u czujka-librus -f

# Native
./target/release/librus-notifications  # logi na stdout
```

### Sprawdzanie ostatniego uruchomienia

```bash
# Systemd
systemctl status czujka-librus.service
journalctl -u czujka-librus -n 1 --no-pager

# Docker
docker ps -a | grep czujka
```

### Sprawdzanie bazy danych

```bash
# Lokalizacja bazy
# Docker: ./data/librus.db
# NixOS: /var/lib/czujka-librus/librus.db
# Native: ./librus.db

# Sprawdź zawartość
sqlite3 librus.db "SELECT COUNT(*) FROM announcements;"
sqlite3 librus.db "SELECT COUNT(*) FROM messages;"
sqlite3 librus.db "SELECT * FROM announcements ORDER BY fetched_at DESC LIMIT 5;"
```

---

## 🆘 Pomoc

Jeśli masz problemy:

1. Sprawdź [Issues na GitHub](https://github.com/FlakM/czujka-librus/issues)
2. Przeczytaj logi szczegółowo
3. Zgłoś issue z:
   - Metodą uruchomienia (Docker/Nix/Native)
   - Logami błędów
   - Wersją systemu operacyjnego
   - Krokami do reprodukcji

---

**[⬅️ Powrót do README](README.md)**
