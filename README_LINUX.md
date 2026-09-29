# Anti-Scam & AI Moderation Bot — Linux Deployment Guide

Полное руководство по запуску бота и векторной базы данных Qdrant («Гидрант») на любой Linux-системе (Ubuntu, Debian, CentOS, Alpine, Arch, VPS/VDS, Docker, Discloud).

---

## 🗄️ Как работает Qdrant («Гидрант») в системе?

Qdrant — это векторная база данных, хранящая **120,000+ сигнатур и векторов скама** для моментального семантического сопоставления за 2–5 мс.

### 🛡️ Обязателен ли Qdrant?
**Нет! Бот полностью автономен и отказоустойчив:**
- **Если Qdrant запущен**: бот использует его как L2-кэш векторов скама (`check_vector_engine`).
- **Если Qdrant выключен или недоступен**: бот **не падает и не зависает**, а мгновенно переходит на нативный Rust-движок `ProfanityEngine` (3,478 правил, транслитерация, Damerau-Levenshtein, словарь из 234,011 чистых слов) + сигнатуры `scam_vectors.json` + AI-эскалацию к умной LLM (Groq 120B / Gemini Flash-Lite).

---

## 🐳 Способ 1: Запуск ВСЕГО в 1 команду (Docker Compose) — Рекомендуется

Это самый удобный способ для Linux. Docker автоматически развернет:
1. Официальный контейнер **Qdrant** (порт `6333`);
2. Python микросервис **Vector Service** (порт `6335`);
3. Rust-бота **Anti-Scam Bot**.

```bash
# 1. Распакуйте архив проекта
unzip antiscambot-linux.zip -d antiscambot
cd antiscambot

# 2. Настройте токены бота в .env
cp .env.example .env
nano .env

# 3. (Опционально) Если хотите готовую базу из 120,000 векторов:
# распакуйте qdrant_storage.zip в папку qdrant/storage

# 4. Запустите все контейнеры
docker compose up -d

# 5. Просмотр логов:
docker compose logs -f
```

---

## 🚀 Способ 2: Запуск Qdrant через Docker (отдельно)

Если бот запущен напрямую на хосте, а Qdrant вы хотите держать в Docker:

```bash
# Запуск контейнера Qdrant с пробросом портов и хранилища
docker run -d \
  --name qdrant \
  --restart unless-stopped \
  -p 6333:6333 \
  -p 6334:6334 \
  -v $(pwd)/qdrant/storage:/qdrant/storage:z \
  qdrant/qdrant:latest
```

---

## ⚡ Способ 3: Запуск Qdrant БЕЗ Docker (чистый бинарник)

Qdrant написан на Rust и распространяется как единый статический бинарник под Linux.
В проект уже включен скрипт [`start_qdrant.sh`](file:///c:/Users/PC/Downloads/basketball/start_qdrant.sh):

```bash
chmod +x start_qdrant.sh
./start_qdrant.sh
```

Что делает скрипт:
1. Автоматически скачивает официальный Linux-бинарник Qdrant с GitHub Releases;
2. Если рядом лежит архив `qdrant_storage.zip`, автоматически распаковывает готовую базу скама;
3. Запускает Qdrant на порту `6333` (Web Dashboard доступен по адресу: `http://IP_СЕРВЕРА:6333/dashboard`).

---

## ☁️ Способ 4: Qdrant Cloud (Бесплатно навсегда в облаке)

Если на вашем сервере мало оперативной памяти (например, VPS с 1 GB RAM):
1. Зарегистрируйтесь на [cloud.qdrant.io](https://cloud.qdrant.io) и создайте **Free Forever Cluster** (1 ГБ памяти бесплатно).
2. В файле `.env` укажите полученный URL и API-ключ:
   ```env
   QDRANT_URL=https://xxxxxxxx.gcp.cloud.qdrant.io:6333
   QDRANT_API_KEY=ваш_ключ
   ```
3. Локальный Qdrant запускать вообще не потребуется!

---

## 🐍 Запуск Python Векторного Сервиса (для L2-векторов)

Микросервис векторизации переводит текст в эмбеддинги `all-MiniLM-L6-v2` и обращается к Qdrant.

В проект включен скрипт [`start_vector_engine.sh`](file:///c:/Users/PC/Downloads/basketball/start_vector_engine.sh):
```bash
chmod +x start_vector_engine.sh
./start_vector_engine.sh
```
Скрипт автоматически создаст виртуальное окружение `python3 -m venv`, установит зависимости из `requirements.txt` и запустит сервер на порту `6335`.

---

## 🤖 Запуск самого Discord-бота (Rust)

Скрипт [`run.sh`](file:///c:/Users/PC/Downloads/basketball/run.sh) автоматически скачивает свежий скомпилированный бинарник с GitHub Actions:
```bash
chmod +x run.sh
./run.sh
```

---

## ⚙️ Автозапуск через Systemd (фоновая служба 24/7)

Создайте файл службы `/etc/systemd/system/antiscambot.service`:
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

```bash
sudo systemctl daemon-reload
sudo systemctl enable antiscambot
sudo systemctl start antiscambot
```
