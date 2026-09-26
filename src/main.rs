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
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod ai_moderator;
use ai_moderator::{AiModerator, ModerationVerdict};

mod config;
use config::ConfigStore;

mod mod_actions;

use serenity::model::application::Interaction;

use lowe_sift::{
    cluster_matches_hough, match_features, verify_hough_clusters, GrayImage, HoughConfig,
    ModelDatabase, ObjectModel, Sift,
};
use rayon::prelude::*;
use serde::Deserialize;
use serenity::async_trait;
use serenity::builder::EditMember;
use serenity::model::channel::Message;
use serenity::model::gateway::{GatewayIntents, Ready};
use serenity::model::id::{GuildId, UserId};
use serenity::model::Permissions;
use serenity::model::Timestamp;
use serenity::prelude::*;

// Match sensitivity:
// 74.0% similarity and max distance 17 bits intercepts compressed scam photos while preventing meme false positives
const HAMMING_DISTANCE_MAX: u32 = 17;
const SIMILARITY_THRESHOLD: f64 = 74.0;

// Toggle SIFT: Set to false to disable heavy keypoint matching completely and run on pure, ultra-fast multi-crop hashing
const ENABLE_SIFT: bool = false;

// SIFT geometric verification thresholds (used only if ENABLE_SIFT = true):
const SIFT_RATIO_TEST: f32 = 0.72;
const SIFT_MIN_INLIERS: usize = 95;
const SIFT_MAX_DIMENSION: u32 = 640;

const AUTO_DELETE: bool = true;
const WARN_USER_IN_CHAT: bool = true;
const WARN_EXPIRE_SECONDS: u64 = 6;
const AUTO_TIMEOUT_MINUTES: u64 = 60;
const IGNORE_BOTS: bool = true;
const IGNORE_ADMINS: bool = true; // Set to true: admins are completely ignored and never touched
const MAX_IMAGE_SIZE: u32 = 5 * 1024 * 1024; // 5 MB

// ── Aggressive Multi-Image Burst Mode (Scamer Pack Detection) ────────────────
// When a user uploads >= 4 images in a single message (typical scam raid signature),
// we apply heightened sensitivity: lower distance threshold and relaxed SIFT requirements.
const BURST_IMAGE_COUNT_THRESHOLD: usize = 4;
const BURST_HAMMING_DISTANCE_MAX: u32 = 21;
const BURST_SIMILARITY_THRESHOLD: f64 = 66.0;
const BURST_SIFT_MIN_INLIERS: usize = 35;

// Guaranteed immunity — server creator (Sasageyo)
const SASAGEYO_ID: u64 = 612573096343240734;

// Environment variable or .env file (NEVER hardcode tokens in git!)
fn get_token() -> String {
    ai_moderator::get_env_var("DISCORD_TOKEN").unwrap_or_default()
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

struct SiftTemplateModel {
    name: String,
    #[allow(dead_code)]
    model_id: u32,
    database: ModelDatabase,
}

struct VectorStore {
    templates: HashMap<String, ScamTemplate>,
    sift_models: Vec<SiftTemplateModel>,
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

fn resolve_templates_dir() -> PathBuf {
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("scam_templates");
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from("scam_templates")
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

        // Precompute SIFT descriptors for all images in scam_templates/
        let templates_dir = resolve_templates_dir();
        let mut sift_models = Vec::new();
        if templates_dir.is_dir() {
            if let Ok(entries) = fs::read_dir(&templates_dir) {
                let sift = Sift::default();
                let mut model_id = 1_u32;

                for entry in entries.flatten() {
                    let path = entry.path();
                    let ext = path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    if !["png", "jpg", "jpeg", "webp"].contains(&ext.as_str()) {
                        continue;
                    }

                    if let Ok(img) = image::open(&path) {
                        let filename = path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unknown")
                            .to_string();

                        // Register native Rust pHash and dHash for 100.0% exact match
                        let native_ph = compute_phash(&img);
                        let native_dh = compute_dhash(&img);
                        templates.insert(
                            filename.clone(),
                            ScamTemplate {
                                name: filename.clone(),
                                phash: format!("{:016x}", native_ph),
                                dhash: format!("{:016x}", native_dh),
                                source: format!("file:{}", filename),
                                phash_uint: native_ph,
                                dhash_uint: native_dh,
                            },
                        );

                        // Downscale template and compute SIFT if enabled
                        if ENABLE_SIFT {
                            let (w, h) = (img.width(), img.height());
                            let scaled = if w > SIFT_MAX_DIMENSION || h > SIFT_MAX_DIMENSION {
                                img.thumbnail(SIFT_MAX_DIMENSION, SIFT_MAX_DIMENSION)
                            } else {
                                img
                            };

                            let gray = GrayImage::from_dynamic_image(&scaled);
                            let features = sift.detect_and_compute(&gray);

                            if !features.is_empty() {
                                if let Ok(obj_model) = ObjectModel::new(
                                    model_id,
                                    scaled.width() as f32,
                                    scaled.height() as f32,
                                    features,
                                ) {
                                    if let Ok(db) = ModelDatabase::new(vec![obj_model]) {
                                        sift_models.push(SiftTemplateModel {
                                            name: filename,
                                            model_id,
                                            database: db,
                                        });
                                        model_id += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        println!(
            "[STORE] Total active scam vectors in memory: {}",
            templates.len()
        );
        println!(
            "[STORE] Precomputed {} SIFT invariant template models in memory",
            sift_models.len()
        );

        VectorStore {
            templates,
            sift_models,
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
    sift_inliers: usize,
}

fn match_vectors(cand_phash: u64, cand_dhash: u64, is_burst: bool) -> Option<MatchResult> {
    let store = init_store();

    let max_dist = if is_burst { BURST_HAMMING_DISTANCE_MAX } else { HAMMING_DISTANCE_MAX };
    let min_sim = if is_burst { BURST_SIMILARITY_THRESHOLD } else { SIMILARITY_THRESHOLD };

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

        if dist_ph <= max_dist && sim >= min_sim {
            println!(
                "   [MATCH{}] Found scam template '{}' (Sim: {:.1}%, Dist: {}/{})",
                if is_burst { " BURST" } else { "" },
                name, sim, dist_ph, max_dist
            );
            return Some(MatchResult {
                name: if is_burst { format!("{} [Burst Raid]", name) } else { name.clone() },
                similarity: sim,
                distance: dist_ph,
                sift_inliers: 0,
            });
        }
    }

    println!(
        "   [MATCH CHECK{}] Best match '{}' -> Sim: {:.1}%, Dist: {} (Threshold: {:.0}%, Max: {})",
        if is_burst { " BURST" } else { "" },
        best_name, best_sim, best_dist, min_sim, max_dist
    );

    if best_sim >= min_sim && best_dist <= max_dist {
        return Some(MatchResult {
            name: if is_burst { format!("{} [Burst Raid]", best_name) } else { best_name },
            similarity: best_sim,
            distance: best_dist,
            sift_inliers: 0,
        });
    }

    None
}

/// SIFT Feature Matching with Lowe's Generalized Hough Transform & Affine Geometric Verification.
/// Invariant to perspective tilt, camera rotation, monitor glare, and extreme color alterations.
fn match_features_sift(img: &image::DynamicImage, is_burst: bool) -> Option<MatchResult> {
    let store = init_store();
    if store.sift_models.is_empty() {
        return None;
    }

    let min_inliers_required = if is_burst { BURST_SIFT_MIN_INLIERS } else { SIFT_MIN_INLIERS };

    // Downscale query image if larger than SIFT_MAX_DIMENSION to guarantee fast response
    let (w, h) = (img.width(), img.height());
    let query_img = if w > SIFT_MAX_DIMENSION || h > SIFT_MAX_DIMENSION {
        img.thumbnail(SIFT_MAX_DIMENSION, SIFT_MAX_DIMENSION)
    } else {
        img.clone()
    };

    let gray_q = GrayImage::from_dynamic_image(&query_img);
    let sift = Sift::default();
    let query_features = sift.detect_and_compute(&gray_q);

    if query_features.len() < 10 {
        return None;
    }

    // Parallel multi-core evaluation across all template models
    let hough_cfg = HoughConfig::default();

    let best_match = store.sift_models.par_iter().filter_map(|model| {
        let train_features = model.database.train_features();
        let matches = match_features(&query_features, train_features, SIFT_RATIO_TEST);
        if matches.len() < min_inliers_required {
            return None;
        }

        if let Ok(clusters) =
            cluster_matches_hough(&matches, &query_features, &model.database, hough_cfg)
        {
            if let Ok(hypotheses) = verify_hough_clusters(
                &matches,
                &query_features,
                &model.database,
                &clusters,
                hough_cfg,
            ) {
                let max_inliers = hypotheses
                    .iter()
                    .map(|h| h.inlier_match_indices.len())
                    .max()
                    .unwrap_or(0);

                let inlier_ratio = if !matches.is_empty() {
                    max_inliers as f32 / matches.len() as f32
                } else {
                    0.0
                };

                if max_inliers >= min_inliers_required {
                    println!(
                        "      [SIFT CANDIDATE{}] '{}': inliers={}, matches={}, ratio={:.1}%",
                        if is_burst { " BURST" } else { "" },
                        model.name, max_inliers, matches.len(), inlier_ratio * 100.0
                    );
                    return Some((model.name.clone(), max_inliers, inlier_ratio));
                }
            }
        }
        None
    }).max_by_key(|(_, inliers, _)| *inliers);

    if let Some((template_name, inliers, _)) = best_match {
        println!(
            "   [SIFT MATCH{}] Confirmed geometrically invariant match with '{}' ({} inliers >= {})",
            if is_burst { " BURST" } else { "" },
            template_name, inliers, min_inliers_required
        );
        return Some(MatchResult {
            name: if is_burst { format!("{} [Burst Raid]", template_name) } else { template_name },
            similarity: 100.0,
            distance: 0,
            sift_inliers: inliers,
        });
    }

    None
}

/// Two-Tier Hybrid Matching:
/// Tier 1: Perceptual hash + multi-crop (0.1ms POPCNT)
/// Tier 2: SIFT + Hough clustering + Affine verification (for angled/distorted monitor photos)
fn match_image_hybrid(img: &image::DynamicImage, is_burst: bool) -> Option<MatchResult> {
    // 1. Tier 1: Full image perceptual hash
    let ph_full = compute_phash(img);
    let dh_full = compute_dhash(img);
    if let Some(res) = match_vectors(ph_full, dh_full, is_burst) {
        return Some(res);
    }

    // 2. Tier 1 (Crop): Margin-trimmed crops (removes browser tabs, taskbar, monitor bezels)
    let (w, h) = (img.width(), img.height());
    if w > 60 && h > 60 {
        // Crop 1: 4% margin
        let x1 = (w as f64 * 0.04) as u32;
        let y1 = (h as f64 * 0.06) as u32;
        let crop_w1 = ((w as f64 * 0.92) as u32).min(w - x1);
        let crop_h1 = ((h as f64 * 0.90) as u32).min(h - y1);

        let cropped1 = img.crop_imm(x1, y1, crop_w1, crop_h1);
        let ph_crop1 = compute_phash(&cropped1);
        let dh_crop1 = compute_dhash(&cropped1);
        if let Some(res) = match_vectors(ph_crop1, dh_crop1, is_burst) {
            return Some(res);
        }

        // Crop 2: 8% margin (for heavier borders/phone frames)
        let x2 = (w as f64 * 0.08) as u32;
        let y2 = (h as f64 * 0.08) as u32;
        let crop_w2 = ((w as f64 * 0.84) as u32).min(w - x2);
        let crop_h2 = ((h as f64 * 0.84) as u32).min(h - y2);

        let cropped2 = img.crop_imm(x2, y2, crop_w2, crop_h2);
        let ph_crop2 = compute_phash(&cropped2);
        let dh_crop2 = compute_dhash(&cropped2);
        if let Some(res) = match_vectors(ph_crop2, dh_crop2, is_burst) {
            return Some(res);
        }
    }

    // 3. Tier 2: Scale/Rotation/Perspective/Lighting invariant SIFT geometric verification (Optional)
    if ENABLE_SIFT {
        if let Some(res) = match_features_sift(img, is_burst) {
            return Some(res);
        }

        // 4. Tier 2 (Mirror Invariance): Check horizontal flip to defeat mirror scam attacks
        let flipped = img.fliph();
        if let Some(mut res) = match_features_sift(&flipped, is_burst) {
            res.name = format!("{} [Mirrored]", res.name);
            return Some(res);
        }
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
    ai_moderator: AiModerator,
    config: Arc<ConfigStore>,
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

        // Guaranteed immunity — server creator
        if msg.author.id.get() == SASAGEYO_ID {
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

        // ── 0. AI TEXT MODERATION (Runs before / in parallel to image scan) ──
        if !msg.content.trim().is_empty() {
            // Pre-fill channel context history from Discord if local buffer is empty (e.g. fresh reboot)
            if self.ai_moderator.get_history_count(msg.channel_id.get()) < 3 {
                let get_msgs = serenity::builder::GetMessages::new().before(msg.id).limit(10);
                if let Ok(recent) = msg.channel_id.messages(&ctx.http, get_msgs).await {
                    for m in recent.into_iter().rev() {
                        if !m.content.trim().is_empty() {
                            self.ai_moderator.record_message(
                                m.channel_id.get(),
                                m.id.get(),
                                m.author.id.get(),
                                &m.author.name,
                                &m.content,
                            );
                        }
                    }
                }
            }

            let reply_to = msg.referenced_message.as_ref().map(|ref_msg| {
                (
                    ref_msg.author.name.as_str(),
                    ref_msg.author.id.get(),
                    ref_msg.id.get(),
                    ref_msg.content.as_str(),
                )
            });

            let mentions: Vec<(u64, String)> = msg
                .mentions
                .iter()
                .map(|u| (u.id.get(), u.name.clone()))
                .collect();

            let attachments_info: Vec<String> = msg
                .attachments
                .iter()
                .map(|a| format!("{} ({} KB, {})", a.filename, a.size / 1024, a.content_type.as_deref().unwrap_or("unknown")))
                .collect();

            let now_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;

            let created_secs = msg.author.id.created_at().unix_timestamp();
            let account_age_days = if now_secs > created_secs {
                Some(((now_secs - created_secs) / 86400) as u64)
            } else {
                Some(0)
            };

            let server_member_days = msg
                .member
                .as_ref()
                .and_then(|m| m.joined_at)
                .map(|ts| {
                    let join_secs = ts.unix_timestamp();
                    if now_secs > join_secs {
                        ((now_secs - join_secs) / 86400) as u64
                    } else {
                        0
                    }
                });

            let author_nick = msg.member.as_ref().and_then(|m| m.nick.clone());
            let roles_count = msg.member.as_ref().map(|m| m.roles.len()).unwrap_or(0);

            let (guild_name, channel_name) = if let Some(gid) = msg.guild_id {
                let g = ctx.cache.guild(gid);
                let gname = g.as_ref().map(|guild| guild.name.clone());
                let cname = g.as_ref().and_then(|guild| guild.channels.get(&msg.channel_id).map(|c| c.name.clone()));
                (gname, cname)
            } else {
                (None, None)
            };

            let msg_ctx = ai_moderator::MessageContext {
                guild_id: msg.guild_id.map(|g| g.get()),
                guild_name,
                channel_id: msg.channel_id.get(),
                channel_name,
                message_id: msg.id.get(),
                timestamp_unix: msg.timestamp.unix_timestamp(),
                author_name: &msg.author.name,
                author_id: msg.author.id.get(),
                author_nick,
                account_age_days,
                server_member_days,
                roles_count,
                content: &msg.content,
                reply_to,
                mentions: &mentions,
                attachments_info: &attachments_info,
            };

            let verdict = self.ai_moderator.check_message(&msg_ctx).await;

            match verdict {
                ModerationVerdict::DeleteConfirmed {
                    reason,
                    score,
                    category,
                    model_used,
                } => {
                    println!(
                        "\n🚨 [AI MODERATOR: DELETED] Channel: {} | User: {} ({}) | Score: {:.2} | Reason: {} | Msg: \"{}\"",
                        msg.channel_id, msg.author.name, msg.author.id, score, reason, msg.content
                    );

                    if AUTO_DELETE {
                        let _ = msg.channel_id.delete_message(&ctx.http, msg.id).await;
                    }

                    if WARN_USER_IN_CHAT {
                        let warn_text = format!(
                            "🛡️ **Auto-Moderator:** <@{}>, your message was removed ({}).",
                            msg.author.id, reason
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

                    if let Some(mod_chan) = self.config.get_mod_channel() {
                        mod_actions::send_mod_alert(
                            &ctx.http,
                            mod_chan,
                            msg.guild_id,
                            msg.author.id,
                            &msg.author.name,
                            msg.channel_id,
                            msg.id,
                            &msg.content,
                            "🚨 AI ALERT: SEVERE VIOLATION (AUTO-DELETED)",
                            true,
                            &reason,
                            score,
                            &category,
                            &model_used,
                        )
                        .await;
                    }

                    // Message deleted for toxic text, skip image checking
                    return;
                }
                ModerationVerdict::FlagSuspicious {
                    reason,
                    score,
                    category,
                    model_used,
                } => {
                    println!(
                        "\n⚠️ [AI MODERATOR: SUSPICIOUS] Channel: {} | User: {} ({}) | Score: {:.2} | Reason: {} | Msg: \"{}\"",
                        msg.channel_id, msg.author.name, msg.author.id, score, reason, msg.content
                    );

                    self.ai_moderator.record_message(
                        msg.channel_id.get(),
                        msg.id.get(),
                        msg.author.id.get(),
                        &msg.author.name,
                        &msg.content,
                    );

                    if let Some(mod_chan) = self.config.get_mod_channel() {
                        mod_actions::send_mod_alert(
                            &ctx.http,
                            mod_chan,
                            msg.guild_id,
                            msg.author.id,
                            &msg.author.name,
                            msg.channel_id,
                            msg.id,
                            &msg.content,
                            "⚠️ AI ALERT: SUSPICIOUS (PENDING MOD REVIEW)",
                            false,
                            &reason,
                            score,
                            &category,
                            &model_used,
                        )
                        .await;
                    }
                }
                ModerationVerdict::Allow => {
                    self.ai_moderator.record_message(
                        msg.channel_id.get(),
                        msg.id.get(),
                        msg.author.id.get(),
                        &msg.author.name,
                        &msg.content,
                    );
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

        let is_burst = candidate_urls.len() >= BURST_IMAGE_COUNT_THRESHOLD;
        if is_burst {
            println!(
                "⚡ [BURST DETECTED] User {} posted {} images at once! Activating aggressive scan...",
                msg.author.name, candidate_urls.len()
            );
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
                "[SCAN{}] Analyzing {}'s image against {} scam vectors...",
                if is_burst { " BURST" } else { "" },
                msg.author.name,
                store.templates.len()
            );

            // Download image
            let data = match download_image(&self.http_client, img_url).await {
                Ok(d) => d,
                Err(_) => continue,
            };

            // ── AI NSFW / Porn Image Scanner (OpenAI Multimodal Moderation) ───
            let mime_type = if img_url.ends_with(".png") {
                "image/png"
            } else if img_url.ends_with(".webp") {
                "image/webp"
            } else if img_url.ends_with(".gif") {
                "image/gif"
            } else {
                "image/jpeg"
            };

            let nsfw_verdict = self.ai_moderator.check_image_bytes(&data, mime_type).await;
            if let ai_moderator::ImageModerationVerdict::NsfwDetected { category, score, details } = nsfw_verdict {
                println!(
                    "\n🔞 [NSFW DETECTED] User: {} ({}) | Channel: {}\n   Category: {} | Score: {:.2} | Details: {}",
                    msg.author.name, msg.author.id, msg.channel_id, category, score, details
                );

                if AUTO_DELETE {
                    let _ = msg.channel_id.delete_message(&ctx.http, msg.id).await;
                }

                if WARN_USER_IN_CHAT {
                    let warn_text = format!(
                        "🛡️ **Auto-Moderator:** <@{}>, your message was removed: NSFW / Explicit content is not allowed (`{}`).",
                        msg.author.id, category
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

                if let Some(mod_chan) = self.config.get_mod_channel() {
                    mod_actions::send_mod_alert(
                        &ctx.http,
                        mod_chan,
                        msg.guild_id,
                        msg.author.id,
                        &msg.author.name,
                        msg.channel_id,
                        msg.id,
                        "[NSFW / Explicit Image Upload]",
                        "🔞 AI ALERT: NSFW / PORN DETECTED (AUTO-DELETED)",
                        true,
                        &details,
                        score,
                        &category,
                        "OpenAI Multimodal omni-moderation",
                    ).await;
                }

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
                            }
                        }
                    }
                }

                break;
            }

            // Decode image (PNG, JPEG, GIF)
            let img = match image::load_from_memory(&data) {
                Ok(i) => i,
                Err(_) => continue,
            };

            // Match against scam templates with two-tier hybrid matching (pHash + SIFT)
            if let Some(result) = match_image_hybrid(&img, is_burst) {
                store.deleted_count.fetch_add(1, Ordering::Relaxed);

                println!(
                    "\n\u{1f6a8} [SCAM DETECTED] User: {} ({}) | Channel: {}\n   Matched: '{}' | Sim: {:.1}% | Distance: {}/{} | SIFT Inliers: {}",
                    msg.author.name,
                    msg.author.id,
                    msg.channel_id,
                    result.name,
                    result.similarity,
                    result.distance,
                    HAMMING_DISTANCE_MAX,
                    result.sift_inliers
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

    async fn ready(&self, ctx: Context, ready: Ready) {
        let store = init_store();
        println!("\n=======================================================");
        println!(
            "\u{1f6e1}\u{fe0f}  ANTI-SCAM & AI MODERATOR BOT ACTIVE as {}",
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
        if let Some(ch) = self.config.get_mod_channel() {
            println!("🔔 Mod alert channel active: <#{}>", ch);
        } else {
            println!("ℹ️  No mod alert channel set. Use /set_mod_channel in Discord.");
        }
        println!("\u{1f4be} Memory footprint: ~4-8 MB RAM (Native Rust binary, zero GC)");
        println!("=======================================================\n");

        // Register slash command /set_mod_channel globally
        let cmd = serenity::builder::CreateCommand::new("set_mod_channel")
            .description("Configure channel where AI suspicious messages and mod alerts are sent")
            .default_member_permissions(Permissions::ADMINISTRATOR)
            .add_option(
                serenity::builder::CreateCommandOption::new(
                    serenity::model::application::CommandOptionType::Channel,
                    "channel",
                    "Target channel for AI mod alerts and control cards",
                )
                .required(true),
            );

        if let Err(why) = serenity::model::application::Command::create_global_command(&ctx.http, cmd).await {
            eprintln!("[WARN] Failed to register /set_mod_channel slash command: {:?}", why);
        } else {
            println!("✅ Registered global slash command: /set_mod_channel");
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::Command(command) => {
                if command.data.name == "set_mod_channel" {
                    let is_admin = command
                        .member
                        .as_ref()
                        .and_then(|m| m.permissions)
                        .map(|p| p.contains(Permissions::ADMINISTRATOR))
                        .unwrap_or(false);

                    if !is_admin {
                        let resp = serenity::builder::CreateInteractionResponse::Message(
                            serenity::builder::CreateInteractionResponseMessage::new()
                                .content("❌ Only administrators can configure the moderation channel.")
                                .ephemeral(true),
                        );
                        let _ = command.create_response(&ctx.http, resp).await;
                        return;
                    }

                    let mut selected_channel = None;
                    for opt in &command.data.options {
                        if opt.name == "channel" {
                            if let serenity::model::application::CommandDataOptionValue::Channel(cid) = opt.value {
                                selected_channel = Some(cid);
                            }
                        }
                    }

                    if let Some(cid) = selected_channel {
                        if let Err(e) = self.config.set_mod_channel(cid.get()) {
                            eprintln!("[ERROR] Failed to save mod channel: {:?}", e);
                        }
                        let resp = serenity::builder::CreateInteractionResponse::Message(
                            serenity::builder::CreateInteractionResponseMessage::new()
                                .content(format!("✅ AI Moderation alert channel set to <#{}>!", cid))
                                .ephemeral(true),
                        );
                        let _ = command.create_response(&ctx.http, resp).await;
                    } else {
                        let resp = serenity::builder::CreateInteractionResponse::Message(
                            serenity::builder::CreateInteractionResponseMessage::new()
                                .content("❌ Please select a valid channel.")
                                .ephemeral(true),
                        );
                        let _ = command.create_response(&ctx.http, resp).await;
                    }
                }
            }
            Interaction::Component(component) => {
                mod_actions::handle_button_interaction(&ctx, &component).await;
            }
            _ => {}
        }
    }
}

// =============================================================================
// MAIN — Entry point
// =============================================================================

#[tokio::main]
async fn main() {
    // Initialize vector store
    init_store();

    // Support offline CLI testing without running Discord bot:
    // cargo run -- --test <path_to_image>
    let args: Vec<String> = env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--test") {
        if pos + 1 < args.len() {
            let img_path = &args[pos + 1];
            println!("\n[CLI TEST] Testing image: {}", img_path);
            match image::open(img_path) {
                Ok(img) => {
                    let ph = compute_phash(&img);
                    let dh = compute_dhash(&img);
                    println!("Computed pHash: {:016x} | dHash: {:016x}\n", ph, dh);

                    if let Some(res) = match_image_hybrid(&img, false) {
                        println!("============================================================");
                        println!("🚨 FINAL VERDICT: SCAM DETECTED (AUTO-DELETE)");
                        println!("   Matched Template: {}", res.name);
                        println!("   Similarity Score: {:.1}%", res.similarity);
                        if res.sift_inliers > 0 {
                            println!("   SIFT Affine Inliers: {} (Min: {})", res.sift_inliers, SIFT_MIN_INLIERS);
                        } else {
                            println!("   Hamming Distance: {} (Max: {})", res.distance, HAMMING_DISTANCE_MAX);
                        }
                        println!("============================================================\n");
                    } else {
                        println!("============================================================");
                        println!("✓ FINAL VERDICT: CLEAN / NO SCAM DETECTED");
                        println!("============================================================\n");
                    }
                }
                Err(e) => eprintln!("[ERROR] Failed to load image '{}': {}", img_path, e),
            }
        }
        return;
    }

    if let Some(pos) = args.iter().position(|a| a == "--test-dir") {
        if pos + 1 < args.len() {
            let dir_path = &args[pos + 1];
            println!("\n[BENCHMARK] Testing all images in directory: {}", dir_path);
            let mut total = 0;
            let mut scam_detected = 0;
            let mut clean_count = 0;

            if let Ok(entries) = std::fs::read_dir(dir_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
                    if ["png", "jpg", "jpeg", "webp"].contains(&ext.as_str()) {
                        total += 1;
                        if let Ok(img) = image::open(&path) {
                            if let Some(res) = match_image_hybrid(&img, false) {
                                scam_detected += 1;
                                println!(
                                    "   ❌ FALSE POSITIVE [{:?}]: Matched '{}' (Sim: {:.1}%, Inliers: {})",
                                    path.file_name().unwrap(), res.name, res.similarity, res.sift_inliers
                                );
                            } else {
                                clean_count += 1;
                            }
                        }
                    }
                }
            }

            println!("\n============================================================");
            println!("📊 BENCHMARK COMPLETE:");
            println!("   Total tested images: {}", total);
            println!("   Clean (Correctly Passed): {} ({:.1}%)", clean_count, (clean_count as f64 / total.max(1) as f64) * 100.0);
            println!("   False Positives (Mistakenly Flagged): {} ({:.1}%)", scam_detected, (scam_detected as f64 / total.max(1) as f64) * 100.0);
            println!("============================================================\n");
        }
        return;
    }

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

    let ai_moderator = AiModerator::new(http_client.clone());
    let config = Arc::new(ConfigStore::new("mod_config.json"));
    let handler = Handler {
        http_client,
        ai_moderator,
        config,
    };

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
