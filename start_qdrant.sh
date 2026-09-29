#!/bin/bash
# Script to launch Qdrant Vector Database on Linux (x86_64)

set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

# 1. Download official static Linux binary if missing
if [ ! -f "./qdrant" ]; then
    echo "⬇️  Downloading official Qdrant Linux binary..."
    curl -sL "https://github.com/qdrant/qdrant/releases/latest/download/qdrant-x86_64-unknown-linux-musl.tar.gz" | tar -xz
    chmod +x ./qdrant
    echo "✅ Qdrant binary ready."
fi

# 2. Extract pre-indexed scam dataset if qdrant_storage.zip is present
if [ -f "./qdrant_storage.zip" ] && [ ! -d "./storage/collections/scam_patterns" ]; then
    echo "📦 Extracting pre-indexed Qdrant storage (120,000+ scam patterns)..."
    unzip -q ./qdrant_storage.zip -d ./
    echo "✅ Storage extracted to ./storage"
fi

echo "🚀 Starting Qdrant on port 6333 (http://127.0.0.1:6333)..."
exec ./qdrant
