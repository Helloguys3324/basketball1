use std::collections::{HashMap, VecDeque};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;
use serde::{Deserialize, Serialize};

// =============================================================================
// AI MODERATOR CONFIGURATION & CONSTANTS
// =============================================================================

const OPENAI_SAFE_THRESHOLD: f64 = 0.45;
const OPENAI_SEVERE_THRESHOLD: f64 = 0.82;
const OPENAI_CATEGORY_SEVERE_THRESHOLD: f64 = 0.70;

const MAX_CONTEXT_HISTORY: usize = 12;
const MAX_CHANNELS_TRACKED: usize = 300;

// Hostility markers for gamer chat analysis
const TARGET_PRONOUNS: &[&str] = &[
    "you", "u", "ur", "your", "yours", "yourself",
    "ты", "тебя", "тебе", "тобой", "твой", "твоя", "твои", "твою", "вы", "вас", "вам"
];

const TARGET_INSULTS: &[&str] = &[
    "clown", "trash", "dog", "kid", "noob", "debil", "idiot", "loser", "bitch",
    "клоун", "мусор", "нуб", "дебил", "идиот", "даун", "лох", "чмо", "бездарь", "крыса"
];

const GAMING_SAFE_SUBSTRINGS: &[&str] = &[
    "kill boss", "kill him", "he killed me", "i died", "dead game", "headshot",
    "killstreak", "damage", "убей его", "меня убили", "убили босса", "взорви", "задави"
];

pub fn get_env_var(name: &str) -> Option<String> {
    let env_paths = [
        PathBuf::from(".env"),
        if let Ok(exe) = env::current_exe() {
            exe.parent().map(|p| p.join(".env")).unwrap_or_default()
        } else {
            PathBuf::new()
        },
    ];

    let prefix = format!("{}=", name);
    for path in &env_paths {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                for line in content.lines() {
                    let line = line.trim();
                    if line.starts_with('#') || line.is_empty() {
                        continue;
                    }
                    if let Some(val) = line.strip_prefix(&prefix) {
                        let cleaned = val.trim().trim_matches('"').trim_matches('\'');
                        if !cleaned.is_empty() {
                            if name == "OPENAI_API_KEY" && cleaned.starts_with("gsk_") {
                                continue;
                            }
                            return Some(cleaned.to_string());
                        }
                    }
                }
            }
        }
    }

    if let Ok(val) = env::var(name) {
        let trimmed = val.trim().to_string();
        if !trimmed.is_empty() {
            if name == "OPENAI_API_KEY" && trimmed.starts_with("gsk_") {
                // Ignore mistyped Groq key in system OPENAI_API_KEY variable
            } else {
                return Some(trimmed);
            }
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct ChatEntry {
    pub message_id: u64,
    pub author_name: String,
    pub author_id: u64,
    pub content: String,
}

pub struct MessageContext<'a> {
    pub guild_id: Option<u64>,
    pub guild_name: Option<String>,
    pub channel_id: u64,
    pub channel_name: Option<String>,
    pub message_id: u64,
    pub timestamp_unix: i64,
    pub author_name: &'a str,
    pub author_id: u64,
    pub author_nick: Option<String>,
    pub account_age_days: Option<u64>,
    pub server_member_days: Option<u64>,
    pub roles_count: usize,
    pub content: &'a str,
    pub reply_to: Option<(&'a str, u64, u64, &'a str)>, // (author_name, author_id, message_id, content)
    pub mentions: &'a [(u64, String)], // (user_id, username)
    pub attachments_info: &'a [String],
}

#[derive(Debug, Clone)]
pub enum ModerationVerdict {
    Allow,
    DeleteConfirmed {
        reason: String,
        score: f64,
        category: String,
        model_used: String,
    },
    FlagSuspicious {
        reason: String,
        score: f64,
        category: String,
        model_used: String,
    },
}

#[derive(Debug, Clone)]
pub enum ImageModerationVerdict {
    Clean,
    NsfwDetected {
        category: String,
        score: f64,
        details: String,
    },
}

#[derive(Serialize)]
struct OpenAiModRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Deserialize)]
struct OpenAiModResponse {
    results: Vec<OpenAiModResult>,
}

#[derive(Deserialize)]
struct OpenAiModResult {
    category_scores: HashMap<String, f64>,
}

#[derive(Serialize)]
struct GroqMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct GroqChatRequest {
    model: String,
    messages: Vec<GroqMessage>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Deserialize)]
struct GroqChatResponse {
    choices: Vec<GroqChoice>,
}

#[derive(Deserialize)]
struct GroqChoice {
    message: GroqMessageContent,
}

#[derive(Deserialize)]
struct GroqMessageContent {
    content: String,
}

pub struct AiModerator {
    http_client: reqwest::Client,
    openai_key: Option<String>,
    groq_keys: Vec<String>,
    groq_fast_model: String,
    groq_deep_model: String,
    groq_counter: AtomicUsize,
    chat_history: RwLock<HashMap<u64, VecDeque<ChatEntry>>>,
}

impl AiModerator {
    pub fn new(http_client: reqwest::Client) -> Self {
        let openai_key = get_env_var("OPENAI_API_KEY");

        let groq_keys: Vec<String> = get_env_var("GROQ_API_KEYS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let groq_fast_model = get_env_var("GROQ_FAST_MODEL")
            .unwrap_or_else(|| "qwen/qwen3.8-27b".to_string());

        let groq_deep_model = get_env_var("GROQ_DEEP_MODEL")
            .unwrap_or_else(|| "qwen/qwen3.8-27b".to_string());

        Self {
            http_client,
            openai_key,
            groq_keys,
            groq_fast_model,
            groq_deep_model,
            groq_counter: AtomicUsize::new(0),
            chat_history: RwLock::new(HashMap::new()),
        }
    }

    pub fn record_message(&self, channel_id: u64, message_id: u64, author_id: u64, author_name: &str, content: &str) {
        if content.trim().is_empty() {
            return;
        }
        let mut history = self.chat_history.write().unwrap();
        if history.len() > MAX_CHANNELS_TRACKED {
            history.retain(|_, v| !v.is_empty());
            if history.len() > MAX_CHANNELS_TRACKED {
                history.clear();
            }
        }
        let queue = history.entry(channel_id).or_insert_with(|| VecDeque::with_capacity(MAX_CONTEXT_HISTORY + 2));
        if queue.len() >= MAX_CONTEXT_HISTORY + 2 {
            queue.pop_front();
        }
        queue.push_back(ChatEntry {
            message_id,
            author_id,
            author_name: author_name.to_string(),
            content: content.to_string(),
        });
    }

    pub fn get_history_count(&self, channel_id: u64) -> usize {
        let history = self.chat_history.read().unwrap();
        history.get(&channel_id).map(|q| q.len()).unwrap_or(0)
    }

    fn get_context_snapshot(&self, channel_id: u64) -> Vec<ChatEntry> {
        let history = self.chat_history.read().unwrap();
        if let Some(queue) = history.get(&channel_id) {
            queue.iter().rev().take(MAX_CONTEXT_HISTORY).cloned().collect::<Vec<_>>().into_iter().rev().collect()
        } else {
            Vec::new()
        }
    }

    fn is_fast_whitelisted(content: &str) -> bool {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return true;
        }
        let lower = trimmed.to_lowercase();
        let words: Vec<&str> = lower.split_whitespace().collect();
        if words.len() <= 2 && trimmed.len() <= 12 && !trimmed.contains("<@") {
            if matches!(lower.as_str(), "gg" | "ez" | "lol" | "lmao" | "bruh" | "wtf" | "no" | "yes" | "ok" | "nice" | "w" | "l" | "gl" | "hf" | "wp" | "afk" | "brb" | "o7" | "rip" | "f" | "omg" | "idk" | "kek" | "кринж" | "база" | "пон" | "лол" | "кек" | "гг" | "имба" | "хз" | "пж" | "спс" | "жесть" | "краш") {
                return true;
            }
        }
        for sub in GAMING_SAFE_SUBSTRINGS {
            if lower.contains(sub) {
                return true;
            }
        }
        false
    }

    fn is_directed_or_targeted(content: &str, has_reply: bool) -> bool {
        if has_reply || content.contains("<@") {
            return true;
        }
        let lower = content.to_lowercase();
        let words: Vec<&str> = lower.split_whitespace().collect();
        let has_pronoun = words.iter().any(|w| {
            let clean = w.trim_matches(|c: char| !c.is_alphanumeric());
            TARGET_PRONOUNS.contains(&clean)
        });
        let has_insult = words.iter().any(|w| {
            let clean = w.trim_matches(|c: char| !c.is_alphanumeric());
            TARGET_INSULTS.contains(&clean)
        });
        has_pronoun || has_insult
    }

    fn format_telemetry(
        &self,
        ctx: &MessageContext<'_>,
        history: &[ChatEntry],
        max_score: f64,
        top_cat: &str,
        cat_breakdown: &str,
    ) -> String {
        let mut p = String::with_capacity(3072);

        p.push_str("=== 1. SERVER & CHANNEL TELEMETRY ===\n");
        if let Some(gid) = ctx.guild_id {
            let gname = ctx.guild_name.as_deref().unwrap_or("Server");
            p.push_str(&format!("Server: {} (ID: {})\n", gname, gid));
        } else {
            p.push_str("Server: Direct Message\n");
        }
        let cname = ctx.channel_name.as_deref().unwrap_or("general");
        p.push_str(&format!("Channel: #{} (ID: {})\n", cname, ctx.channel_id));

        p.push_str("\n=== 2. AUTHOR IDENTITY & REPUTATION ===\n");
        let nick_str = match &ctx.author_nick {
            Some(n) => format!(" (Server Nickname: '{}')", n),
            None => String::new(),
        };
        p.push_str(&format!("User: @{}{}, ID: {}\n", ctx.author_name, nick_str, ctx.author_id));

        if let Some(age) = ctx.account_age_days {
            let risk = if age < 7 {
                " ⚠️ [HIGH RISK: Account created < 7 days ago!]"
            } else if age < 30 {
                " ⚠️ [MODERATE: New account < 30 days]"
            } else {
                " [Established account]"
            };
            p.push_str(&format!("Account Age: {} days{}\n", age, risk));
        }
        if let Some(joined) = ctx.server_member_days {
            p.push_str(&format!("Server Member For: {} days (Roles count: {})\n", joined, ctx.roles_count));
        } else if ctx.roles_count > 0 {
            p.push_str(&format!("Roles count: {}\n", ctx.roles_count));
        }

        p.push_str("\n=== 3. TARGET MESSAGE TELEMETRY ===\n");
        p.push_str(&format!("Message ID: {}\n", ctx.message_id));
        p.push_str(&format!("Timestamp (Unix): {}\n", ctx.timestamp_unix));
        p.push_str(&format!("Message Text: \"{}\"\n", ctx.content.trim()));

        if let Some((rep_author, rep_id, rep_msg_id, rep_text)) = ctx.reply_to {
            p.push_str(&format!(
                "Replying To Message ID: {} by @{} (ID: {}): \"{}\"\n",
                rep_msg_id, rep_author, rep_id, rep_text
            ));
        } else {
            p.push_str("Replying To: (None / Standalone message)\n");
        }

        if !ctx.mentions.is_empty() {
            let m_str: Vec<String> = ctx.mentions.iter().map(|(id, name)| format!("@{} (ID: {})", name, id)).collect();
            p.push_str(&format!("Direct Mentions: {}\n", m_str.join(", ")));
        } else {
            p.push_str("Direct Mentions: (None)\n");
        }

        if !ctx.attachments_info.is_empty() {
            p.push_str(&format!("Attached Files: {}\n", ctx.attachments_info.join(", ")));
        } else {
            p.push_str("Attached Files: (None)\n");
        }

        p.push_str("\n=== 4. AUTOMATED MODERATION SIGNALS (OpenAI omni-moderation) ===\n");
        p.push_str(&format!("Highest Score: {:.2} (Top Category: '{}')\n", max_score, top_cat));
        if !cat_breakdown.is_empty() {
            p.push_str(&format!("Full Category Breakdown: {}\n", cat_breakdown));
        }

        p.push_str("\n=== 5. CHRONOLOGICAL RECENT CHANNEL MESSAGES ===\n");
        p.push_str(&self.format_history(history));

        p
    }

    pub async fn check_message(&self, ctx: &MessageContext<'_>) -> ModerationVerdict {
        let trimmed = ctx.content.trim();

        // ── 0. FAST GATE: Local instant bypass (0ms, 0 API) ───────────────────
        if Self::is_fast_whitelisted(trimmed) {
            return ModerationVerdict::Allow;
        }

        // ── 1. TIER 1: OpenAI Moderation (omni-moderation-latest, $0) ────────
        let openai_key = match &self.openai_key {
            Some(k) => k,
            None => return ModerationVerdict::Allow,
        };

        let (max_score, severe_score, top_cat, cat_breakdown) = match self.call_openai_moderation(openai_key, trimmed).await {
            Ok(scores) => scores,
            Err(_) => (0.0, 0.0, String::new(), String::new()),
        };

        // 1A. Clear clean content -> Instant ALLOW
        if max_score < OPENAI_SAFE_THRESHOLD {
            return ModerationVerdict::Allow;
        }

        let history = self.get_context_snapshot(ctx.channel_id);

        // 1B. High Score (>0.82 or severe category >0.70) ──────────────────────
        // NEVER BLINDLY DELETE ON RAW SCORE: ALWAYS PASS TO LLM GUARD FIRST
        if severe_score > OPENAI_CATEGORY_SEVERE_THRESHOLD || max_score > OPENAI_SEVERE_THRESHOLD {
            if !self.groq_keys.is_empty() {
                let double_check_prompt = "You are a Senior Discord Community Safety Arbiter & False-Positive Guard for a gaming Discord server.\n\
                    An automated classifier flagged this message with high severity.\n\
                    Analyze all message and user telemetry (account age, reply context, channel history, roles):\n\
                    - ALLOW: Mutual gaming banter, friendly trash talk (e.g. 'im gonna obliterate you', 'die noob', 'trash team'), casual cursing among friends ('fuck you', 'stfu'), quoting lyrics/memes, game rage.\n\
                    - SUSPICIOUS: Borderline hostility, personal heated argument, or ambiguous intent -> send for human moderator review.\n\
                    - DELETE: ONLY genuine toxic attacks, real-world death/physical threats, doxxing, severe racial/ethnic slurs, encouraging suicide/self-harm ('kys').\n\
                    Format strictly:\n\
                    VERDICT: [ALLOW or DELETE or SUSPICIOUS]\n\
                    REASON: [under 12 words]";

                let user_prompt = format!(
                    "{}\nEvaluate all telemetry and determine: is this safe banter/quote (ALLOW), needs staff review (SUSPICIOUS), or true severe violation (DELETE)?",
                    self.format_telemetry(ctx, &history, max_score, &top_cat, &cat_breakdown)
                );

                match self.call_groq_failover(&self.groq_fast_model, double_check_prompt, &user_prompt).await {
                    Ok((verdict, reason)) => {
                        if verdict.contains("ALLOW") {
                            // Prevented false deletion of gaming banter!
                            return ModerationVerdict::Allow;
                        } else if verdict.contains("DELETE") {
                            // Confirmed real-world threat or severe attack
                            return ModerationVerdict::DeleteConfirmed {
                                reason: format!("{}: {}", top_cat, reason),
                                score: max_score,
                                category: top_cat,
                                model_used: format!("OpenAI + {} Guard", self.groq_fast_model),
                            };
                        } else {
                            // SUSPICIOUS -> Send interactive card to staff channel instead of blind deletion!
                            return ModerationVerdict::FlagSuspicious {
                                reason: format!("High score ({:.2}), pending mod review: {}", max_score, reason),
                                score: max_score,
                                category: top_cat,
                                model_used: format!("{} (Guard Checked)", self.groq_fast_model),
                            };
                        }
                    }
                    Err(e) => {
                        eprintln!("[GROQ GUARD FAILOVER] Error: {}. Falling back to FlagSuspicious.", e);
                        // If LLM unavailable, NEVER blindly auto-delete! Send to human moderators!
                        return ModerationVerdict::FlagSuspicious {
                            reason: format!("High score ({:.2}), pending mod review", max_score),
                            score: max_score,
                            category: top_cat,
                            model_used: "OpenAI Guard (Pending Review)".to_string(),
                        };
                    }
                }
            }

            // If no Groq keys, send to mod review instead of blind deletion
            return ModerationVerdict::FlagSuspicious {
                reason: format!("High score ({:.2}): pending staff review", max_score),
                score: max_score,
                category: top_cat,
                model_used: "OpenAI (Pending Staff Review)".to_string(),
            };
        }

        // ── 2. SMART GREY-ZONE PRE-FILTER (0.45 ..= 0.82) ─────────────────────
        let is_directed = Self::is_directed_or_targeted(trimmed, ctx.reply_to.is_some());
        if !is_directed && max_score < 0.60 {
            // General game frustration ("fuck this lag", "damn bug") -> ALLOW instantly
            return ModerationVerdict::Allow;
        }

        if self.groq_keys.is_empty() {
            return ModerationVerdict::Allow;
        }

        // ── 3. TIER 2: DEEP LLM FOR GREY ZONE (TUNED TO THE ABSOLUTE MAXIMUM) ──
        // Supplies LLM with full server telemetry, IDs, reply targets, mentions, and OpenAI category breakdowns!
        let system_prompt = "You are a Supreme Community Arbiter for a high-intensity gaming Discord server.\n\
            Your task is to review grey-zone flagged messages with complete conversation, identity, and channel telemetry.\n\
            Carefully distinguish between:\n\
            - FRIENDLY_BANTER / GAMING_FRUSTRATION (ALLOW): Mutual joking, friendly trash-talk ('im gonna obliterate you', 'noob', 'ez'), complaining about game mechanics, quotes, casual swearing ('fuck you').\n\
            - TARGETED_TOXICITY (DELETE): Malicious bullying, unprovoked toxic attacks, hate harassment, real threats, or doxxing.\n\
            - AMBIGUOUS (SUSPICIOUS): Borderline or unclear intent where human staff judgment is needed.\n\
            Return strictly in this format:\n\
            VERDICT: [ALLOW or DELETE or SUSPICIOUS]\n\
            REASON: [precise explanation under 12 words]";

        let user_prompt = format!(
            "{}\nAnalyze all telemetry, context, and intent. What is your final verdict?",
            self.format_telemetry(ctx, &history, max_score, &top_cat, &cat_breakdown)
        );

        match self.call_groq_failover(&self.groq_deep_model, system_prompt, &user_prompt).await {
            Ok((verdict, reason)) => {
                if verdict.contains("DELETE") || verdict.contains("SUSPICIOUS") {
                    // Send to mod alert with interactive buttons for 1-click execution
                    ModerationVerdict::FlagSuspicious {
                        reason,
                        score: max_score,
                        category: top_cat,
                        model_used: format!("{} (Deep Context)", self.groq_deep_model),
                    }
                } else {
                    ModerationVerdict::Allow
                }
            }
            Err(_) => ModerationVerdict::Allow,
        }
    }

    fn format_history(&self, history: &[ChatEntry]) -> String {
        let mut out = String::new();
        if history.is_empty() {
            out.push_str("(No recent channel messages)\n");
            return out;
        }
        for entry in history {
            out.push_str(&format!(
                "[MsgID: {}] {} (ID: {}): {}\n",
                entry.message_id, entry.author_name, entry.author_id, entry.content
            ));
        }
        out
    }

    async fn call_openai_moderation(&self, api_key: &str, text: &str) -> Result<(f64, f64, String, String), reqwest::Error> {
        let req_body = OpenAiModRequest {
            model: "omni-moderation-latest",
            input: text,
        };

        let resp = self
            .http_client
            .post("https://api.openai.com/v1/moderations")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&req_body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            eprintln!("[OPENAI API ERROR] Status {}: {}", status, err_text);
            return Ok((0.0, 0.0, String::new(), String::new()));
        }

        let body: OpenAiModResponse = resp.json().await?;
        if let Some(first) = body.results.first() {
            let mut max_score = 0.0_f64;
            let mut severe_score = 0.0_f64;
            let mut top_cat = String::new();
            let mut breakdown_parts = Vec::new();

            for (cat, &score) in &first.category_scores {
                if score > 0.10 {
                    breakdown_parts.push(format!("{}: {:.2}", cat, score));
                }
                if score > max_score {
                    max_score = score;
                    top_cat = cat.clone();
                }
                if matches!(cat.as_str(), "hate" | "hate/threatening" | "harassment/threatening" | "self-harm" | "violence/graphic") {
                    if score > severe_score {
                        severe_score = score;
                    }
                }
            }
            Ok((max_score, severe_score, top_cat, breakdown_parts.join(", ")))
        } else {
            Ok((0.0, 0.0, String::new(), String::new()))
        }
    }

    pub async fn check_image_bytes(&self, image_bytes: &[u8], mime_type: &str) -> ImageModerationVerdict {
        let openai_key = match &self.openai_key {
            Some(k) => k,
            None => return ImageModerationVerdict::Clean,
        };

        use base64::Engine;
        let b64 = base64::prelude::BASE64_STANDARD.encode(image_bytes);
        let data_uri = format!("data:{};base64,{}", mime_type, b64);

        let req_body = serde_json::json!({
            "model": "omni-moderation-latest",
            "input": [
                {
                    "type": "image_url",
                    "image_url": {
                        "url": data_uri
                    }
                }
            ]
        });

        let resp = match self
            .http_client
            .post("https://api.openai.com/v1/moderations")
            .header("Authorization", format!("Bearer {}", openai_key))
            .header("Content-Type", "application/json")
            .json(&req_body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[IMAGE MODERATION ERROR] Network: {}", e);
                return ImageModerationVerdict::Clean;
            }
        };

        let status = resp.status();
        if !status.is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            eprintln!("[IMAGE MODERATION ERROR] Status {}: {}", status, err_text);
            return ImageModerationVerdict::Clean;
        }

        let body: Result<OpenAiModResponse, _> = resp.json().await;
        if let Ok(body) = body {
            if let Some(first) = body.results.first() {
                let sexual_score = *first.category_scores.get("sexual").unwrap_or(&0.0);
                let minors_score = *first.category_scores.get("sexual/minors").unwrap_or(&0.0);
                let gore_score = *first.category_scores.get("violence/graphic").unwrap_or(&0.0);

                if minors_score > 0.35 {
                    return ImageModerationVerdict::NsfwDetected {
                        category: "sexual/minors".to_string(),
                        score: minors_score,
                        details: format!("Exploitation / minor safety violation ({:.2})", minors_score),
                    };
                }

                if sexual_score > 0.65 {
                    return ImageModerationVerdict::NsfwDetected {
                        category: "sexual (NSFW)".to_string(),
                        score: sexual_score,
                        details: format!("Pornographic / explicit nudity content ({:.2})", sexual_score),
                    };
                }

                if gore_score > 0.70 {
                    return ImageModerationVerdict::NsfwDetected {
                        category: "violence/graphic (Gore)".to_string(),
                        score: gore_score,
                        details: format!("Graphic gore / extreme violence ({:.2})", gore_score),
                    };
                }
            }
        }

        ImageModerationVerdict::Clean
    }

    async fn call_groq_failover(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<(String, String), String> {
        let total_keys = self.groq_keys.len();
        if total_keys == 0 {
            return Err("No Groq keys configured".to_string());
        }

        let mut models_to_try = vec![model];
        if !models_to_try.contains(&"qwen/qwen3.8-27b") {
            models_to_try.push("qwen/qwen3.8-27b");
        }
        if !models_to_try.contains(&"openai/gpt-oss-120b") {
            models_to_try.push("openai/gpt-oss-120b");
        }

        let start_idx = self.groq_counter.fetch_add(1, Ordering::Relaxed) % total_keys;
        for target_model in models_to_try {
            for i in 0..total_keys {
                let idx = (start_idx + i) % total_keys;
                let key = &self.groq_keys[idx];
                match self.execute_groq_call(key, target_model, system_prompt, user_prompt).await {
                    Ok(res) => return Ok(res),
                    Err(e) => {
                        eprintln!("[GROQ FAILOVER] Key {} model '{}' error: {}", idx, target_model, e);
                        continue;
                    }
                }
            }
        }
        Err("All Groq keys and models exhausted".to_string())
    }

    async fn execute_groq_call(
        &self,
        api_key: &str,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<(String, String), String> {
        let req_body = GroqChatRequest {
            model: model.to_string(),
            messages: vec![
                GroqMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                GroqMessage {
                    role: "user".to_string(),
                    content: user_prompt.to_string(),
                },
            ],
            max_tokens: 120,
            temperature: 0.0,
        };

        let resp = self
            .http_client
            .post("https://api.groq.com/openai/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&req_body)
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            return Err(format!("HTTP {}: {}", status, err_text));
        }

        let body: GroqChatResponse = resp.json().await.map_err(|e| format!("JSON decode error: {}", e))?;
        if let Some(choice) = body.choices.first() {
            let text = &choice.message.content;
            let upper = text.to_uppercase();
            let mut verdict = String::new();
            let mut reason = String::new();

            for line in text.lines() {
                let trimmed = line.trim();
                let upper_line = trimmed.to_uppercase();
                if upper_line.starts_with("VERDICT:") {
                    verdict = trimmed["VERDICT:".len()..].trim().to_uppercase();
                } else if upper_line.starts_with("REASON:") {
                    reason = trimmed["REASON:".len()..].trim().to_string();
                }
            }

            if verdict.is_empty() {
                if upper.contains("DELETE") {
                    verdict = "DELETE".to_string();
                } else if upper.contains("SUSPICIOUS") {
                    verdict = "SUSPICIOUS".to_string();
                } else {
                    verdict = "ALLOW".to_string();
                }
            }

            if reason.is_empty() {
                reason = "Context telemetry evaluation".to_string();
            }

            Ok((verdict, reason))
        } else {
            Err("Empty choices in response".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fast_whitelisted() {
        assert!(AiModerator::is_fast_whitelisted("gg"));
        assert!(AiModerator::is_fast_whitelisted("EZ"));
        assert!(AiModerator::is_fast_whitelisted("lol"));
        assert!(AiModerator::is_fast_whitelisted("kill boss"));
        assert!(!AiModerator::is_fast_whitelisted("you are trash"));
    }

    #[test]
    fn test_is_directed_or_targeted() {
        assert!(AiModerator::is_directed_or_targeted("ты клоун", false));
        assert!(AiModerator::is_directed_or_targeted("you are trash", false));
        assert!(AiModerator::is_directed_or_targeted("whatever man", true));
        assert!(!AiModerator::is_directed_or_targeted("fuck this lag", false));
    }

    #[test]
    fn test_chat_history_buffer() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        moderator.record_message(101, 1001, 2001, "Alice", "Hello everyone");
        moderator.record_message(101, 1002, 2002, "Bob", "Hey Alice");
        let history = moderator.get_context_snapshot(101);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].message_id, 1001);
        assert_eq!(history[0].author_id, 2001);
    }

    #[tokio::test]
    async fn test_check_message_offensive() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("general".to_string()),
            message_id: 2,
            timestamp_unix: 1727376000,
            author_name: "Machi",
            author_id: 12345,
            author_nick: Some("MachiPro".to_string()),
            account_age_days: Some(3),
            server_member_days: Some(1),
            roles_count: 1,
            content: "you should die noob",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'you should die noob': {:?}\n", verdict);
        assert!(matches!(verdict, ModerationVerdict::DeleteConfirmed { .. } | ModerationVerdict::FlagSuspicious { .. }));
    }
}
