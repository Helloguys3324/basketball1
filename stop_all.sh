#!/bin/bash
# Stop all components (AntiScamBot, Vector Service, Qdrant)

echo "🛑 Stopping Anti-Scam Bot, Vector Service, and Qdrant..."
pkill -x antiscambot 2>/dev/null || true
pkill -f vector_moderation_service.py 2>/dev/null || true
pkill -x qdrant 2>/dev/null || true
echo "✅ All components stopped."
