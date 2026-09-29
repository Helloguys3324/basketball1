#!/bin/bash
# Script to launch Python Vector Moderation Service on Linux

set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR/vector_engine"

if [ ! -d ".venv" ]; then
    echo "📦 Creating Python virtual environment..."
    python3 -m venv .venv
    echo "⬇️  Installing dependencies..."
    ./.venv/bin/pip install --upgrade pip
    ./.venv/bin/pip install -r requirements.txt
fi

echo "🚀 Starting Vector Moderation Service on port 6335..."
exec ./.venv/bin/python vector_moderation_service.py
