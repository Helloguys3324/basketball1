"""
Vector Moderation Service (L2 Semantic Engine + Normalizer)
Provides local HTTP REST API for antiscambot.exe Discord bot.
Connects to local Qdrant (http://127.0.0.1:6333) with 120,000+ scam patterns.
"""

import os
import re
import sys
import uuid
import unicodedata

if hasattr(sys.stdout, 'reconfigure'):
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')
if hasattr(sys.stderr, 'reconfigure'):
    sys.stderr.reconfigure(encoding='utf-8', errors='replace')

from aiohttp import web
from sentence_transformers import SentenceTransformer
from qdrant_client import QdrantClient

QDRANT_URL = os.getenv("QDRANT_URL", "http://127.0.0.1:6333")
COLLECTION_NAME = os.getenv("QDRANT_COLLECTION", "scam_patterns")
PORT = int(os.getenv("VECTOR_SERVICE_PORT", "6335"))
BASE_THRESHOLD = float(os.getenv("VECTOR_SCORE_THRESHOLD", "0.58"))

print(f"[VECTOR-ENGINE] Initializing Vector Moderation Service on port {PORT}...")
print(f"[VECTOR-ENGINE] Connecting to Qdrant at {QDRANT_URL} (Collection: {COLLECTION_NAME})...")

try:
    qdrant = QdrantClient(QDRANT_URL, timeout=5.0)
    col_info = qdrant.get_collection(COLLECTION_NAME)
    print(f"[VECTOR-ENGINE] Qdrant Connected! Collection '{COLLECTION_NAME}' active with {col_info.points_count} points.")
except Exception as e:
    print(f"[VECTOR-ENGINE] Warning: Could not verify Qdrant collection '{COLLECTION_NAME}': {e}")
    qdrant = QdrantClient(QDRANT_URL, timeout=5.0)

print("[VECTOR-ENGINE] Loading SentenceTransformer('all-MiniLM-L6-v2')...")
model = SentenceTransformer("all-MiniLM-L6-v2")
print("[VECTOR-ENGINE] Embedding Model Loaded (dim=384)!")

# Leet and symbol map for adversarial evasion normalization
LEET_MAP = {
    '4': 'a', '@': 'a', '3': 'e', '1': 'i', '!': 'i', '|': 'i',
    '0': 'o', '$': 's', '5': 's', '7': 't', '+': 't', '8': 'b', '2': 'z'
}

ZERO_WIDTH_CHARS = re.compile(r'[\u200B-\u200D\uFEFF\u00AD\u2060]')

# High-risk financial and urgency tokens (from js_parity.rs)
MONEY_TOKENS = {
    "usd", "eur", "btc", "eth", "ton", "trx", "usdt", "sol", "wallet",
    "card", "cvv", "otp", "pin", "crypto", "nitro", "steam", "airdrop"
}
URGENCY_TOKENS = {
    "urgent", "immediately", "now", "verify", "confirm", "claim",
    "winner", "bonus", "profit", "giveaway", "free"
}

RUST_DICT_PATH = os.getenv("RUST_DICT_PATH", os.path.join(os.path.dirname(os.path.abspath(__file__)), "rust_dict.txt"))
RUST_DICT = set()
if os.path.exists(RUST_DICT_PATH):
    try:
        with open(RUST_DICT_PATH, "r", encoding="utf-8", errors="ignore") as f:
            for line in f:
                word = line.strip().lower()
                if word and not word.startswith("#"):
                    RUST_DICT.add(word)
        print(f"[VECTOR-ENGINE] Loaded {len(RUST_DICT)} profane/slur patterns from {RUST_DICT_PATH}")
    except Exception as e:
        print(f"[VECTOR-ENGINE] Warning: Could not load rust_dict: {e}")

# Whitelist integration from profanity-destroyer project (234,435 clean dictionary words)
WHITELIST_WORDS_PATH = os.getenv("WHITELIST_WORDS_PATH", os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "whitelists", "profanity_destroyer_whitelist.txt"))
WHITELIST_WORDS = set()
if os.path.exists(WHITELIST_WORDS_PATH):
    try:
        with open(WHITELIST_WORDS_PATH, "r", encoding="utf-8", errors="ignore") as f:
            for line in f:
                w = line.strip().lower()
                if w:
                    WHITELIST_WORDS.add(w)
        print(f"[VECTOR-ENGINE] Loaded {len(WHITELIST_WORDS)} whitelist dictionary words from {WHITELIST_WORDS_PATH}")
    except Exception as e:
        print(f"[VECTOR-ENGINE] Warning: Could not load whitelist words: {e}")

# Dynamic persistent whitelist learned from LLM decisions
DYNAMIC_WHITELIST_PATH = os.getenv("DYNAMIC_WHITELIST_PATH", os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "whitelists", "dynamic_whitelist.txt"))
DYNAMIC_WHITELIST_PHRASES = set()
if os.path.exists(DYNAMIC_WHITELIST_PATH):
    try:
        with open(DYNAMIC_WHITELIST_PATH, "r", encoding="utf-8", errors="ignore") as f:
            for line in f:
                p = line.strip().lower()
                if p:
                    DYNAMIC_WHITELIST_PHRASES.add(p)
        print(f"[VECTOR-ENGINE] Loaded {len(DYNAMIC_WHITELIST_PHRASES)} dynamic learned phrases from {DYNAMIC_WHITELIST_PATH}")
    except Exception as e:
        print(f"[VECTOR-ENGINE] Warning: Could not load dynamic whitelist: {e}")

def normalize_text(text: str) -> str:
    if not text:
        return ""
    # 1. Unicode NFKC normalization
    text = unicodedata.normalize('NFKC', text)
    # 2. Strip zero-width and invisible characters
    text = ZERO_WIDTH_CHARS.sub('', text)
    # 3. Lowercase & map leet
    chars = [LEET_MAP.get(ch, ch) for ch in text.lower()]
    norm = ''.join(chars)
    # 4. Collapse repeated characters (e.g. 'frrrreeeee' -> 'free')
    norm = re.sub(r'(.)\1{2,}', r'\1\1', norm)
    # 5. Collapse spaces
    norm = re.sub(r'\s+', ' ', norm).strip()
    return norm


def extract_merged_candidates(text: str) -> list:
    """Multi-level SIMD chunk candidate extraction: joins spaced-out letters like 'n i g g a' or 's c a m'"""
    cleaned = re.sub(r'[^a-zA-Z0-9\s]', ' ', text)
    words = cleaned.split()
    chunks = [w for w in words if w]
    candidates = []
    seen = set()
    for start in range(len(chunks)):
        if len(chunks[start]) > 3:
            continue
        combined = chunks[start]
        single_count = 1 if len(chunks[start]) == 1 else 0
        for end in range(start + 1, min(start + 6, len(chunks))):
            if len(chunks[end]) > 3:
                break
            combined += chunks[end]
            if len(chunks[end]) == 1:
                single_count += 1
            if len(combined) >= 3 and single_count >= 2 and combined not in seen:
                seen.add(combined)
                candidates.append(combined)
                if len(candidates) >= 15:
                    return candidates
    return candidates


def has_trigger_combo(text: str) -> bool:
    tokens = set(re.findall(r'[a-z0-9]+', text.lower()))
    has_money = bool(tokens & MONEY_TOKENS)
    has_urgency = bool(tokens & URGENCY_TOKENS)
    return has_money and has_urgency


async def handle_check(request: web.Request) -> web.Response:
    try:
        data = await request.json()
        raw_text = data.get("text", "").strip()
        if not raw_text:
            return web.json_response({
                "is_scam": False,
                "score": 0.0,
                "category": "empty",
                "matched_text": ""
            })

        normalized = normalize_text(raw_text)
        candidates = extract_merged_candidates(normalized)

        # 0. Check dynamic learned whitelist
        clean_raw = raw_text.lower().strip()
        if clean_raw in DYNAMIC_WHITELIST_PHRASES or normalized in DYNAMIC_WHITELIST_PHRASES:
            return web.json_response({
                "is_scam": False,
                "score": 0.0,
                "threshold": BASE_THRESHOLD,
                "category": "whitelisted",
                "matched_text": "dynamic_whitelist"
            })

        # 1. Lexical fast-hit from profanity-destroyer rust_dict (slurs & critical obscenities)
        for cand in candidates + [normalized]:
            if cand in RUST_DICT:
                return web.json_response({
                    "is_scam": True,
                    "score": 1.0,
                    "threshold": 0.58,
                    "category": "lexical_profanity",
                    "matched_text": f"rust_dict_hit({cand})"
                })
        
        # Adaptive threshold: if message combines money/crypto tokens + urgency tokens,
        # lower threshold to 0.52 to catch subtle evasion
        threshold = BASE_THRESHOLD
        if has_trigger_combo(normalized) or has_trigger_combo(raw_text):
            threshold = 0.52

        # Allow client override if explicitly requested
        if "threshold" in data:
            threshold = float(data["threshold"])

        texts_to_embed = [normalized]
        if normalized != raw_text.lower():
            texts_to_embed.append(raw_text)
        for cand in candidates:
            if len(cand) >= 4 and cand not in texts_to_embed:
                texts_to_embed.append(cand)
                if len(texts_to_embed) >= 4:
                    break

        embeddings = model.encode(texts_to_embed).tolist()

        best_score = 0.0
        best_point = None

        for vec in embeddings:
            try:
                res = qdrant.query_points(
                    collection_name=COLLECTION_NAME,
                    query=vec,
                    limit=1
                )
                if res.points and res.points[0].score > best_score:
                    best_score = res.points[0].score
                    best_point = res.points[0]
            except Exception as e:
                print(f"[VECTOR-ENGINE] Error querying Qdrant: {e}", file=sys.stderr)

        matched_text = ""
        category = "clean"
        is_scam = False

        if best_point:
            payload = best_point.payload or {}
            matched_text = str(payload.get("text") or payload.get("pattern") or best_point.id)
            category = str(payload.get("category") or "scam_patterns")
            cat_lower = category.lower()

            is_true_scam = any(k in cat_lower for k in ("nitro", "scam", "phish", "fraud", "stealer", "drainer", "free nitro"))

            if is_true_scam:
                if best_score >= threshold:
                    is_scam = True
            elif cat_lower == "mined_toxic_context":
                if best_score >= 0.78:
                    is_scam = True
            else:
                if best_score >= 0.70:
                    is_scam = True

            # If all tokens are known standard whitelist words and none are in rust_dict, suppress false positive
            words = [w for w in re.findall(r'[a-z]+', normalized) if len(w) > 1]
            if words and all(w in WHITELIST_WORDS for w in words) and not any(w in RUST_DICT for w in words):
                if best_score < 0.78 or not is_true_scam:
                    is_scam = False

        return web.json_response({
            "is_scam": is_scam,
            "score": float(best_score),
            "threshold": float(threshold),
            "category": category,
            "matched_text": matched_text
        })

    except Exception as e:
        print(f"[VECTOR-ENGINE] Exception in /check: {e}", file=sys.stderr)
        return web.json_response({"error": str(e)}, status=500)


async def handle_train(request: web.Request) -> web.Response:
    try:
        data = await request.json()
        text = data.get("text", "").strip()
        category = data.get("category", "live_trained_scam")
        if not text:
            return web.json_response({"error": "Missing 'text'"}, status=400)

        normalized = normalize_text(text)
        vector = model.encode(normalized).tolist()

        point_id = str(uuid.uuid4())
        from qdrant_client.models import PointStruct
        point = PointStruct(
            id=point_id,
            vector=vector,
            payload={"text": text, "category": category, "normalized": normalized}
        )

        qdrant.upsert(collection_name=COLLECTION_NAME, points=[point])
        print(f"[VECTOR-ENGINE] Dynamic Training: Upserted new vector into '{COLLECTION_NAME}': '{text}' ({category})")

        return web.json_response({
            "status": "ok",
            "id": point_id,
            "message": f"Successfully indexed into {COLLECTION_NAME}"
        })
    except Exception as e:
        print(f"[VECTOR-ENGINE] Exception in /train: {e}", file=sys.stderr)
        return web.json_response({"error": str(e)}, status=500)


async def handle_whitelist(request: web.Request) -> web.Response:
    try:
        data = await request.json()
        text = data.get("text", "").strip().lower()
        if not text:
            return web.json_response({"error": "Missing 'text'"}, status=400)
        
        DYNAMIC_WHITELIST_PHRASES.add(text)
        try:
            with open(DYNAMIC_WHITELIST_PATH, "a", encoding="utf-8") as f:
                f.write(text + "\n")
        except Exception as e:
            print(f"[VECTOR-ENGINE] Error appending to {DYNAMIC_WHITELIST_PATH}: {e}")
        
        print(f"[VECTOR-ENGINE] Dynamic Whitelist Learned: '{text}' (total: {len(DYNAMIC_WHITELIST_PHRASES)})")
        return web.json_response({
            "status": "ok",
            "message": f"Added '{text}' to dynamic whitelist",
            "total_count": len(DYNAMIC_WHITELIST_PHRASES)
        })
    except Exception as e:
        print(f"[VECTOR-ENGINE] Exception in /whitelist: {e}", file=sys.stderr)
        return web.json_response({"error": str(e)}, status=500)


async def handle_health(request: web.Request) -> web.Response:
    try:
        col_info = qdrant.get_collection(COLLECTION_NAME)
        count = col_info.points_count
        status = "healthy"
    except Exception as e:
        count = -1
        status = f"unhealthy: {e}"

    return web.json_response({
        "status": status,
        "collection": COLLECTION_NAME,
        "points_count": count,
        "embedding_dim": 384,
        "default_threshold": BASE_THRESHOLD
    })


def create_app():
    app = web.Application()
    app.router.add_post("/check", handle_check)
    app.router.add_post("/train", handle_train)
    app.router.add_post("/whitelist", handle_whitelist)
    app.router.add_get("/health", handle_health)
    return app


if __name__ == "__main__":
    app = create_app()
    print(f"[VECTOR-ENGINE] Vector Moderation Service running on http://127.0.0.1:{PORT}")
    web.run_app(app, host="127.0.0.1", port=PORT)
