#!/bin/bash
# ==============================================================================
# 🚀 1-Click All-in-One Launcher for Linux (Qdrant + Vector Engine + AntiScamBot)
# ==============================================================================

set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

echo "================================================================="
echo "🛡️  STARTING COMPLETE ANTI-SCAM MODERATION STACK"
echo "================================================================="

# 1. Check .env file
if [ ! -f ".env" ]; then
    if [ -f ".env.example" ]; then
        echo "⚠️  .env not found! Creating from .env.example..."
        cp .env.example .env
        echo "👉 PLEASE EDIT .env WITH YOUR DISCORD_TOKEN AND GROQ/GEMINI KEYS!"
    else
        echo "❌ .env file missing! Please create .env before starting."
        exit 1
    fi
fi

# Ensure all binaries and scripts have execute permissions
chmod +x ./antiscambot ./qdrant *.sh 2>/dev/null || true

# 2. Check and start Qdrant ("Гидрант")
echo "📦 [1/3] Starting Qdrant Vector Database..."
if pgrep -x "qdrant" > /dev/null; then
    echo "✅ Qdrant is already running on port 6333."
else
    if [ -f "./qdrant" ]; then
        nohup ./qdrant > qdrant.log 2>&1 &
        sleep 2
        echo "✅ Qdrant started in background (PID: $!, Port: 6333, Logs: qdrant.log)"
    else
        echo "⚠️  qdrant binary not found. Running start_qdrant.sh to download..."
        ./start_qdrant.sh &
        sleep 3
    fi
fi

# 3. Check and start Python Vector Moderation Service
echo "🐍 [2/3] Starting Python Semantic Vector Service..."
if pgrep -f "vector_moderation_service.py" > /dev/null; then
    echo "✅ Vector Service is already running on port 6335."
else
    cd "$DIR/vector_engine"
    if [ ! -d ".venv" ]; then
        echo "📦 Installing Python virtualenv for Vector Service..."
        python3 -m venv .venv
        ./.venv/bin/pip install --upgrade pip -q
        ./.venv/bin/pip install -r requirements.txt -q
    fi
    nohup ./.venv/bin/python vector_moderation_service.py > ../vector_service.log 2>&1 &
    cd "$DIR"
    sleep 2
    echo "✅ Vector Service started in background (Port: 6335, Logs: vector_service.log)"
fi

# 4. Start Rust Anti-Scam Bot
echo "🤖 [3/3] Starting Anti-Scam Bot (Rust Native Engine)..."
echo "================================================================="
exec ./antiscambot
