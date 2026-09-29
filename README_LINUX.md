# Anti-Scam & AI Moderation Bot — Linux Deployment Guide

Бот готов к развертыванию на любой Linux системе (Ubuntu, Debian, CentOS, Alpine, Arch, Discloud, VPS/VDS, Docker).

---

## 🚀 Вариант 1: Быстрый запуск через готовый бинарник (Рекомендуется)

Благодаря GitHub Actions бот компилируется в полностью статический бинарник (`x86_64-unknown-linux-musl`), который **работает без установки Rust, C++ компиляторов и зависимостей**.

1. Распакуйте архив на вашем сервере:
   ```bash
   unzip antiscambot-linux.zip -d antiscambot
   cd antiscambot
   ```

2. Настройте файл окружения:
   ```bash
   cp .env.example .env
   nano .env
   ```
   Укажите ваш `DISCORD_TOKEN`, а также API ключи Groq (`GROQ_API_KEYS`) или Gemini (`GEMINI_API_KEYS`).

3. Запустите скрипт запуска:
   ```bash
   chmod +x run.sh
   ./run.sh
   ```
   *Скрипт `run.sh` автоматически скачает свежий скомпилированный бинарник `antiscambot` с GitHub Releases (если его еще нет в папке), выдаст права на исполнение и запустит бота.*

---

## 🐳 Вариант 2: Запуск в Docker / Docker Compose

1. Создайте `.env` файл с вашими токенами и ключами:
   ```bash
   cp .env.example .env
   nano .env
   ```

2. Запустите контейнер в фоновом режиме:
   ```bash
   docker compose up -d --build
   ```

3. Просмотр логов бота:
   ```bash
   docker compose logs -f antiscambot
   ```

---

## ☁️ Вариант 3: Хостинг Discloud

Файл `discloud.config` уже настроен:
- `TYPE=bot`
- `MAIN=run.sh`
- `RAM=512`
- `VERSION=latest`

1. Заархивируйте содержимое папки (включая ваш настроенный `.env`).
2. Загрузите архив в панель управления [Discloud](https://discloudbot.com).
3. Бот запустится автоматически через `run.sh`.

---

## ⚙️ Вариант 4: Автозапуск через Systemd (для VPS / Выделенного сервера)

Чтобы бот работал в фоне 24/7 и автоматически перезапускался при сбоях или перезагрузке сервера:

1. Скачайте бинарник и дайте права:
   ```bash
   chmod +x ./run.sh
   ./run.sh  # Скачает бинарник antiscambot, затем нажмите Ctrl+C
   ```

2. Создайте юнит-файл:
   ```bash
   sudo nano /etc/systemd/system/antiscambot.service
   ```

3. Вставьте конфигурацию (замените `/path/to/antiscambot` на реальный путь):
   ```ini
   [Unit]
   Description=Anti-Scam Discord Moderation Bot
   After=network.target

   [Service]
   Type=simple
   User=root
   WorkingDirectory=/path/to/antiscambot
   ExecStart=/path/to/antiscambot/antiscambot
   Restart=always
   RestartSec=5
   EnvironmentFile=/path/to/antiscambot/.env

   [Install]
   WantedBy=multi-user.target
   ```

4. Включите и запустите сервис:
   ```bash
   sudo systemctl daemon-reload
   sudo systemctl enable antiscambot
   sudo systemctl start antiscambot
   sudo systemctl status antiscambot
   ```

---

## 🛠️ Самостоятельная компиляция из исходников (опционально)

Если вы хотите скомпилировать бот вручную прямо на вашем Linux-сервере:
```bash
sudo apt update && sudo apt install -y build-essential pkg-config libssl-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
cargo build --release
./target/release/antiscambot
```

---

## 📂 Структура файлов

- `antiscambot` — Статический Linux исполняемый файл (скачивается автоматически через `run.sh` или компилируется на GitHub).
- `rust_dict.txt` — 3,478 нативных правил анти-спама и мата с транслитерацией и опечатками.
- `whitelist.txt` — Словарь чистых слов (234,011 слов) для фильтрации ложных срабатываний.
- `dynamic_whitelist.txt` — Динамический белый список фраз сервера.
- `mod_config.json` — Конфигурация логики модерации, порогов и правил.
- `scam_vectors.json` & `scam_templates/` — База сигнатур скама и перцептивных хешей изображений.
- `run.sh` — Скрипт быстрого запуска с автоскачиванием бинарника.
- `Dockerfile` & `docker-compose.yml` — Скрипты развертывания в Docker.
- `src/` — Исходный код на Rust.
