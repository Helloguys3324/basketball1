#!/bin/sh
# Anti-Scam & AI Moderation Bot (Rust Edition) — Pre-compiled binary launcher

# Automatically download the latest static Linux binary from GitHub if missing
if [ ! -f "./antiscambot" ]; then
    echo "Downloading latest pre-compiled static Linux binary from GitHub..."
    curl -sL "https://github.com/Helloguys3324/basketball1/releases/download/latest/antiscambot" -o ./antiscambot
fi

chmod +x ./antiscambot
exec ./antiscambot
