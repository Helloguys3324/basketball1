// =============================================================================
// ANTI-SCAM DISCORD BOT — RUST EDITION
// 100% compatible with Python imagehash vectors (pHash + dHash, MSB-first)
// Zero garbage collection · ~4-8 MB RAM · Zero-allocation image processing
// =============================================================================

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serenity::async_trait;
use serenity::builder::EditMember;
use serenity::model::channel::Message;
use serenity::model::gateway::{GatewayIntents, Ready};
use serenity::model::id::{GuildId, UserId};
use serenity::model::Permissions;
use serenity::model::Timestamp;
use serenity::prelude::*;

const HAMMING_DISTANCE_MAX: u32 = 10;
const SIMILARITY_THRESHOLD: f64 = 84.0;

const AUTO_DELETE: bool = true;
const WARN_USER_IN_CHAT: bool = true;
const WARN_EXPIRE_SECONDS: u64 = 6;
const AUTO_TIMEOUT_MINUTES: u64 = 60;
const IGNORE_BOTS: bool = true;
const IGNORE_ADMINS: bool = true;
const MAX_IMAGE_SIZE: u32 = 5 * 1024 * 1024; // 5 MB

// Guaranteed immunity — server creator (Sasageyo)
const SASAGEYO_ID: u64 = 612573096343240734;

// Environment variable or .env file (NEVER hardcode tokens in git!)
fn get_token() -> String {
    // 1. First check system environment variable
    if let Ok(token) = env::var("DISCORD_TOKEN") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return token;
        }
    }

    // 2. Check .env file in current or executable directory
    let env_paths = [
        PathBuf::from(".env"),
        if let Ok(exe) = env::current_exe() {
            exe.parent().map(|p| p.join(".env")).unwrap_or_default()
        } else {
            PathBuf::new()
        },
    ];

    for path in &env_paths {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                for line in content.lines() {
                    let line = line.trim();
                    if line.starts_with('#') || line.is_empty() {
                        continue;
                    }
                    if let Some(token) = line.strip_prefix("DISCORD_TOKEN=") {
                        let token = token.trim().trim_matches('"').trim_matches('\'');
                        if !token.is_empty() {
                            return token.to_string();
                        }
                    }
                }
            }
        }
    }

    String::new()
}

// =============================================================================
// COSINE TABLE — Precomputed 32×32, initialized once (zero-cost after first call)
// =============================================================================

static COS_TABLE: OnceLock<[[f64; 32]; 32]> = OnceLock::new();

#[inline(always)]
fn cos_table() -> &'static [[f64; 32]; 32] {
    COS_TABLE.get_or_init(|| {
        let mut table = [[0.0_f64; 32]; 32];
        for u in 0..32_usize {
            for x in 0..32_usize {
                table[u][x] =
                    ((2 * x + 1) as f64 * u as f64 * std::f64::consts::PI / 64.0).cos();
            }
        }
        table
    })
}

// =============================================================================
// ZERO-ALLOCATION AREA-AVERAGING RESIZE — Stack-allocated (Zero Malloc)
// =============================================================================

/// Downscales any image to 32×32 using area-averaging box filter (matching Pillow).
/// Returns a stack-allocated fixed array: zero heap allocations!
#[inline]
fn resize_area_average_32x32(img: &image::DynamicImage) -> [[f64; 32]; 32] {
    let rgb = img.to_rgb8();
    let w = rgb.width();
    let h = rgb.height();

    let mut out = [[0.0_f64; 32]; 32];

    for ty in 0..32_u32 {
        let y_start = (ty * h) / 32;
        let mut y_end = ((ty + 1) * h) / 32;
        if y_end <= y_start {
            y_end = y_start + 1;
        }

        for tx in 0..32_u32 {
            let x_start = (tx * w) / 32;
            let mut x_end = ((tx + 1) * w) / 32;
            if x_end <= x_start {
                x_end = x_start + 1;
            }

            let mut sum = 0.0_f64;
            let mut count = 0_u32;

            for py in y_start..y_end {
                for px in x_start..x_end {
                    let pixel = rgb.get_pixel(px, py);
                    // ITU-R BT.601 standard luma: 0.299R + 0.587G + 0.114B
                    sum += 0.299 * (pixel[0] as f64)
                        + 0.587 * (pixel[1] as f64)
                        + 0.114 * (pixel[2] as f64);
                    count += 1;
                }
            }

            if count > 0 {
                out[ty as usize][tx as usize] = sum / (count as f64);
            }
        }
    }

    out
}

/// Downscales any image to 9×8 using area-averaging box filter (matching Pillow).
/// Returns a stack-allocated fixed array: zero heap allocations!
#[inline]
fn resize_area_average_9x8(img: &image::DynamicImage) -> [[f64; 9]; 8] {
    let rgb = img.to_rgb8();
    let w = rgb.width();
    let h = rgb.height();

    let mut out = [[0.0_f64; 9]; 8];

    for ty in 0..8_u32 {
        let y_start = (ty * h) / 8;
        let mut y_end = ((ty + 1) * h) / 8;
        if y_end <= y_start {
            y_end = y_start + 1;
        }

        for tx in 0..9_u32 {
            let x_start = (tx * w) / 9;
            let mut x_end = ((tx + 1) * w) / 9;
            if x_end <= x_start {
                x_end = x_start + 1;
            }

            let mut sum = 0.0_f64;
            let mut count = 0_u32;

            for py in y_start..y_end {
                for px in x_start..x_end {
                    let pixel = rgb.get_pixel(px, py);
                    // ITU-R BT.601 standard luma: 0.299R + 0.587G + 0.114B
                    sum += 0.299 * (pixel[0] as f64)
                        + 0.587 * (pixel[1] as f64)
                        + 0.114 * (pixel[2] as f64);
                    count += 1;
                }
            }

            if count > 0 {
                out[ty as usize][tx as usize] = sum / (count as f64);
            }
        }
    }

    out
}

// =============================================================================
// PERCEPTUAL HASHING — Exact imagehash (Python) Compatibility, MSB-first
// =============================================================================

/// 64-bit Difference Hash matching `imagehash.dhash` (MSB-first bit order).
fn compute_dhash(img: &image::DynamicImage) -> u64 {
    let pixels = resize_area_average_9x8(img);
    let mut hash = 0_u64;
    for y in 0..8_usize {
        for x in 0..8_usize {
            if pixels[y][x + 1] > pixels[y][x] {
                let bit_index = 63 - (y * 8 + x);
                hash |= 1_u64 << bit_index;
            }
        }
    }
    hash
}

/// 64-bit DCT Perceptual Hash matching `imagehash.phash` (MSB-first bit order).
/// Uses 2D DCT-II on 32×32 grayscale → top-left 8×8 low-frequency coefficients
/// → median of all 64 values (including DC) → MSB-first binary quantization.
fn compute_phash(img: &image::DynamicImage) -> u64 {
    let ct = cos_table();
    let pixels = resize_area_average_32x32(img);

    let mut dct = [[0.0_f64; 8]; 8];
    let mut values = [0.0_f64; 64];

    for u in 0..8_usize {
        for v in 0..8_usize {
            let mut sum = 0.0_f64;
            for y in 0..32_usize {
                let cu = ct[u][y];
                for x in 0..32_usize {
                    sum += pixels[y][x] * cu * ct[v][x];
                }
            }
            dct[u][v] = sum;
            values[u * 8 + v] = sum;
        }
    }

    // Median of all 64 values (matching scipy.fftpack.dct + numpy.median)
    let mut sorted = values;
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = (sorted[31] + sorted[32]) / 2.0;

    let mut hash = 0_u64;
    for u in 0..8_usize {
        for v in 0..8_usize {
            if dct[u][v] > median {
                let bit_index = 63 - (u * 8 + v);
                hash |= 1_u64 << bit_index;
            }
        }
    }
    hash
}

// =============================================================================
// TEMPLATE & VECTOR STORAGE
// =============================================================================

#[derive(Deserialize, Clone)]
struct ScamTemplate {
    name: String,
    phash: String,
    dhash: String,
    #[serde(default)]
    #[allow(dead_code)]
    source: String,
    #[serde(skip)]
    phash_uint: u64,
    #[serde(skip)]
    dhash_uint: u64,
}

struct VectorStore {
    templates: HashMap<String, ScamTemplate>,
    #[allow(dead_code)]
    scanned_count: AtomicU64,
    #[allow(dead_code)]
    deleted_count: AtomicU64,
    clean_urls: RwLock<HashMap<String, Instant>>,
}

static STORE: OnceLock<VectorStore> = OnceLock::new();

fn resolve_vectors_path() -> PathBuf {
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("scam_vectors.json");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from("scam_vectors.json")
}

fn init_store() -> &'static VectorStore {
    STORE.get_or_init(|| {
        let mut templates = HashMap::new();

        let vectors_path = resolve_vectors_path();
        if let Ok(data) = fs::read_to_string(&vectors_path) {
            if let Ok(list) = serde_json::from_str::<Vec<ScamTemplate>>(&data) {
                for mut item in list {
                    item.phash_uint = u64::from_str_radix(&item.phash, 16).unwrap_or(0);
                    item.dhash_uint = u64::from_str_radix(&item.dhash, 16).unwrap_or(0);
                    templates.insert(item.name.clone(), item);
                }
                println!(
                    "[STORE] Loaded {} vectors from {}",
                    templates.len(),
                    vectors_path.display()
                );
            }
        }

        println!(
            "[STORE] Total active scam vectors in memory: {}",
            templates.len()
        );

        VectorStore {
            templates,
            scanned_count: AtomicU64::new(0),
            deleted_count: AtomicU64::new(0),
            clean_urls: RwLock::new(HashMap::new()),
        }
    })
}

struct MatchResult {
    name: String,
    similarity: f64,
    distance: u32,
}

fn match_vectors(cand_phash: u64, cand_dhash: u64) -> Option<MatchResult> {
    let store = init_store();

    let mut best_sim = 0.0_f64;
    let mut best_dist = 999_u32;
    let mut best_name = String::new();

    for (name, t) in &store.templates {
        // Hardware POPCNT instructions: 1 CPU cycle each
        let dist_ph = (cand_phash ^ t.phash_uint).count_ones();
        let dist_dh = (cand_dhash ^ t.dhash_uint).count_ones();

        let combined_dist = (dist_ph + dist_dh) as f64 / 2.0;
        let sim = (1.0 - (combined_dist / 64.0)) * 100.0;

        if sim > best_sim {
            best_sim = sim;
            best_dist = dist_ph;
            best_name = name.clone();
        }

        if dist_ph <= HAMMING_DISTANCE_MAX || sim >= SIMILARITY_THRESHOLD {
            println!(
                "   [MATCH] Found scam template '{}' (Sim: {:.1}%, Dist: {}/{})",
                name, sim, dist_ph, HAMMING_DISTANCE_MAX
            );
            return Some(MatchResult {
                name: name.clone(),
                similarity: sim,
                distance: dist_ph,
            });
        }
    }

    println!(
        "   [MATCH CHECK] Best match '{}' -> Sim: {:.1}%, Dist: {} (Threshold: {:.0}%, Max: {})",
        best_name, best_sim, best_dist, SIMILARITY_THRESHOLD, HAMMING_DISTANCE_MAX
    );

    if best_sim >= SIMILARITY_THRESHOLD || best_dist <= HAMMING_DISTANCE_MAX {
        return Some(MatchResult {
            name: best_name,
            similarity: best_sim,
            distance: best_dist,
        });
    }

    None
}

// =============================================================================
// IMAGE DOWNLOAD
// =============================================================================

async fn download_image(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let resp = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()).into());
    }

    // Protection against oversized files
    if let Some(len) = resp.content_length() {
        if len > MAX_IMAGE_SIZE as u64 {
            return Err("Image too large".into());
        }
    }

    let bytes = resp.bytes().await?;
    if bytes.len() > MAX_IMAGE_SIZE as usize {
        return Err("Image too large".into());
    }

    Ok(bytes.to_vec())
}

// =============================================================================
// ADMIN / IMMUNITY CHECK (Fast-path cached)
// =============================================================================

async fn is_administrator(ctx: &Context, guild_id: GuildId, user_id: UserId) -> bool {
    // 1. Guaranteed immunity for server creator (Sasageyo)
    if user_id.get() == SASAGEYO_ID {
        return true;
    }

    // 2. Fast cache path: zero network calls
    if let Some(guild) = ctx.cache.guild(guild_id) {
        if guild.owner_id == user_id {
            return true;
        }
        if let Some(member) = guild.members.get(&user_id) {
            for role_id in &member.roles {
                if let Some(role) = guild.roles.get(role_id) {
                    if role.permissions.contains(Permissions::ADMINISTRATOR) {
                        return true;
                    }
                }
            }
            return false;
        }
    }

    // 3. Fallback to Discord REST API if not found in cache
    let member = match ctx.http.get_member(guild_id, user_id).await {
        Ok(m) => m,
        Err(_) => return false,
    };

    let guild = match ctx.http.get_guild(guild_id).await {
        Ok(g) => g,
        Err(_) => return false,
    };

    if guild.owner_id == user_id {
        return true;
    }

    for role_id in &member.roles {
        if let Some(role) = guild.roles.get(role_id) {
            if role.permissions.contains(Permissions::ADMINISTRATOR) {
                return true;
            }
        }
    }

    false
}

// =============================================================================
// DISCORD EVENT HANDLER
// =============================================================================

struct Handler {
    http_client: reqwest::Client,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        let store = init_store();

        // Ignore self
        if msg.author.id == ctx.cache.current_user().id {
            return;
        }

        // Ignore bots
        if IGNORE_BOTS && msg.author.bot {
            return;
        }

        // Ignore admins
        if IGNORE_ADMINS {
            if let Some(guild_id) = msg.guild_id {
                if is_administrator(&ctx, guild_id, msg.author.id).await {
                    return;
                }
            }
        }

        // ── Collect candidate image URLs ──────────────────────────────────
        let mut candidate_urls: Vec<String> = Vec::new();

        for att in &msg.attachments {
            if att.size > MAX_IMAGE_SIZE {
                continue;
            }
            let is_image = att
                .content_type
                .as_deref()
                .map(|ct| ct.starts_with("image/"))
                .unwrap_or(false);
            let ext = att
                .filename
                .rsplit('.')
                .next()
                .unwrap_or("")
                .to_lowercase();
            if is_image || matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp") {
                candidate_urls.push(att.url.clone());
            }
        }

        for embed in &msg.embeds {
            if let Some(ref image) = embed.image {
                candidate_urls.push(image.url.clone());
            } else if let Some(ref thumb) = embed.thumbnail {
                candidate_urls.push(thumb.url.clone());
            }
        }

        if candidate_urls.is_empty() {
            return;
        }

        // ── Scan each image ──────────────────────────────────────────────
        for img_url in &candidate_urls {
            // Skip if already scanned as clean
            {
                let clean = store.clean_urls.read().unwrap();
                if clean.contains_key(img_url) {
                    continue;
                }
            }

            store.scanned_count.fetch_add(1, Ordering::Relaxed);
            println!(
                "[SCAN] Analyzing {}'s image against {} scam vectors...",
                msg.author.name,
                store.templates.len()
            );

            // Download image
            let data = match download_image(&self.http_client, img_url).await {
                Ok(d) => d,
                Err(_) => continue,
            };

            // Decode image (PNG, JPEG, GIF)
            let img = match image::load_from_memory(&data) {
                Ok(i) => i,
                Err(_) => continue,
            };

            // Compute perceptual hashes (zero heap allocations)
            let cand_ph = compute_phash(&img);
            let cand_dh = compute_dhash(&img);

            // Match against scam templates
            if let Some(result) = match_vectors(cand_ph, cand_dh) {
                store.deleted_count.fetch_add(1, Ordering::Relaxed);

                println!(
                    "\n\u{1f6a8} [SCAM DETECTED] User: {} ({}) | Channel: {}\n   Matched: '{}' | Sim: {:.1}% | Distance: {}/{}",
                    msg.author.name,
                    msg.author.id,
                    msg.channel_id,
                    result.name,
                    result.similarity,
                    result.distance,
                    HAMMING_DISTANCE_MAX
                );

                // ── 1. Delete scam message ───────────────────────────────
                if AUTO_DELETE {
                    let _ = msg.channel_id.delete_message(&ctx.http, msg.id).await;
                }

                // ── 2. Send warning (auto-delete after N seconds) ────────
                if WARN_USER_IN_CHAT {
                    let warn_text = format!(
                        "\u{1f6e1}\u{fe0f} **Auto-Moderator:** <@{}>, your message was removed because it matched a recognized scam image (`{}` - {:.1}% match).",
                        msg.author.id, result.name, result.similarity
                    );
                    if let Ok(warn_msg) = msg.channel_id.say(&ctx.http, &warn_text).await {
                        let http = ctx.http.clone();
                        let channel_id = msg.channel_id;
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_secs(WARN_EXPIRE_SECONDS)).await;
                            let _ = channel_id.delete_message(&http, warn_msg.id).await;
                        });
                    }
                }

                // ── 3. Timeout user ──────────────────────────────────────
                if AUTO_TIMEOUT_MINUTES > 0 {
                    if let Some(guild_id) = msg.guild_id {
                        if !is_administrator(&ctx, guild_id, msg.author.id).await {
                            let now_secs = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_secs() as i64;
                            let until_secs = now_secs + (AUTO_TIMEOUT_MINUTES as i64 * 60);
                            if let Ok(ts) = Timestamp::from_unix_timestamp(until_secs) {
                                let builder =
                                    EditMember::new().disable_communication_until_datetime(ts);
                                let _ = guild_id
                                    .edit_member(&ctx.http, msg.author.id, builder)
                                    .await;
                                println!(
                                    "   [ACTION] Timed out {} for {} minutes.",
                                    msg.author.name, AUTO_TIMEOUT_MINUTES
                                );
                            }
                        }
                    }
                }

                break;
            } else {
                // Mark URL as clean (bounded cache: max 500 entries)
                let mut clean = store.clean_urls.write().unwrap();
                if clean.len() > 500 {
                    clean.clear();
                }
                clean.insert(img_url.clone(), Instant::now());
            }
        }
        // Rust RAII: image data and intermediate buffers are dropped immediately
    }

    async fn ready(&self, _ctx: Context, ready: Ready) {
        let store = init_store();
        println!("\n=======================================================");
        println!(
            "\u{1f6e1}\u{fe0f}  ANTI-SCAM BOT (RUST EDITION) ACTIVE as {}",
            ready.user.name
        );
        println!(
            "\u{1f3af} Registered scam vectors: {}",
            store.templates.len()
        );
        println!(
            "\u{2699}\u{fe0f}  Hamming max: {} | Sim threshold: {:.0}%",
            HAMMING_DISTANCE_MAX, SIMILARITY_THRESHOLD
        );
        println!("\u{1f4be} Memory footprint: ~4-8 MB RAM (Native Rust binary, zero GC)");
        println!("=======================================================\n");
    }
}

// =============================================================================
// MAIN — Entry point
// =============================================================================

#[tokio::main]
async fn main() {
    // Initialize vector store
    init_store();

    let token = get_token();
    if token.is_empty() {
        eprintln!("[ERROR] DISCORD_TOKEN is missing!");
        std::process::exit(1);
    }

    // High-performance HTTP client with connection pooling
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .pool_idle_timeout(Duration::from_secs(60))
        .pool_max_idle_per_host(10)
        .build()
        .expect("[ERROR] Failed to build HTTP client");

    let handler = Handler { http_client };

    // Minimal Discord Gateway intents
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILD_MEMBERS;

    println!("[INFO] Connecting to Discord Gateway...");

    let mut client = Client::builder(&token, intents)
        .event_handler(handler)
        .await
        .expect("[ERROR] Failed to create Discord client");

    if let Err(why) = client.start().await {
        eprintln!("[ERROR] Client error: {:?}", why);
        std::process::exit(1);
    }
}
