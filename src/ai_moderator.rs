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

const MAX_CONTEXT_HISTORY: usize = 8;
const MAX_CHANNELS_TRACKED: usize = 300;

// Hostility markers for gamer chat analysis
const TARGET_PRONOUNS: &[&str] = &[
    // Second person
    "you", "u", "ur", "your", "yours", "yourself",
    "ты", "тебя", "тебе", "тобой", "твой", "твоя", "твои", "твою", "вы", "вас", "вам",
    // Third person targeting
    "he", "she", "they", "him", "her", "his", "hers", "them", "their", "theirs",
    "он", "она", "они", "его", "ее", "их", "ему", "ей", "им", "него", "нее", "них",
    // Addressing terms & gaming entities
    "bro", "dude", "guy", "man", "kid", "buddy",
    "чел", "чувак", "тип", "пацан", "малой", "брат", "бро"
];

const SEVERE_HARM_KEYWORDS: &[&str] = &[
    "burn alive", "burned alive", "burn you", "burn him", "burn her", "kill", "die",
    "murder", "stab", "shoot", "hang", "slit", "doxx", "rape", "torture", "strangle",
    "choke", "execute", "suicide", "kys", "burn",
    "сожгу", "сжечь", "сжечь заживо", "убью", "убить", "сдохни", "смерть", "зарезать",
    "пристрелить", "повесить", "расчленить", "вскройся", "самоубийство"
];

const TARGET_INSULTS: &[&str] = &[
    "clown", "trash", "dog", "kid", "noob", "debil", "idiot", "loser", "bitch",
    "клоун", "мусор", "нуб", "дебил", "идиот", "даун", "лох", "чмо", "бездарь", "крыса"
];

const GAMING_SAFE_SUBSTRINGS: &[&str] = &[
    "kill boss", "boss killed me", "he killed me in match", "i died", "dead game", "headshot",
    "killstreak", "damage", "меня убили", "убили босса", "взорви босса"
];

const GAME_SHIELD_PATTERNS: &[&str] = &[
    "in minecraft", "in roblox", "in game", "in-game", "ingame", "in cs", "in csgo", "in rust",
    "in gta", "in valorant", "in fortnite", "in dota", "in tf2",
    "в майнкрафте", "в роблоксе", "в игре", "в кс", "в расте", "в гта", "в доте",
    "в реале а не в игре", "по игре"
];

const COMMON_NON_NAMES: &[&str] = &[
    "ill", "i'll", "im", "i'm", "ive", "i've", "id", "i'd",
    "someone", "somone", "somebody", "something", "nobody", "anyone", "anything",
    "minecraft", "roblox", "rust", "discord", "game", "steam", "valve", "dota", "csgo",
    "january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december",
    "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday",
    "what", "where", "when", "why", "how", "who", "which",
    "yes", "yeah", "nope", "okay", "sure", "well", "look", "wait", "please", "thanks", "thank"
];

const PROVOCATIVE_BAIT_KEYWORDS: &[&str] = &[
    "sexism", "misogyny", "misogynist", "misogynistic", "сексизм", "мизогини", "женоненавист"
];

pub const SERVER_RULES_SYSTEM_PROMPT: &str = "\
Discord Arbiter for a gaming community. Mutes only (NO BAN/KICK).\n\
1.Minor/Mild(1m):Spam,off-topic,mild toxicity,edgy bait('i love sexism','i love misogyny'),ghost-ping,bot abuse\n\
2.Mod(15-30m):Bait,disruptive,NSFW ref,bypass,drama\n\
3.Major(60-120m):Impersonation,harassment,threats to members,ads,doxx\n\
4.Crit(720-1440m):Hate/slurs,death threats('kys','you should die'),gore,raid,cheats\n\
QUOTES, META-TALK & HYPOTHETICAL EXAMPLES (ALLOW, RULE:None, MUTE:0):\n\
- Discussing server rules, testing bot triggers, quoting past messages, or explaining what caused a mute: 'Say I'll kill ... (Somone)', 'U said kill something ig', 'he got muted for saying die', 'the bot bans for kys'\n\
- Abstract placeholders & sentence templates ('...', '(someone)', '[user]', '<person>', 'кого-то', 'кто-то'): These are grammatical examples/templates, NOT real threats or harassment against members.\n\
- Only punish genuine direct malice/threats targeting real people. If demonstrating, quoting, explaining, or testing -> ALWAYS ALLOW.\n\
SMART BANTER vs FAKE-GAME SHIELD BYPASS:\n\
- GENUINE BANTER (ALLOW, RULE:None, MUTE:0):\n\
  Boasting strictly about match/skill: '1v1 me on mid', 'im gonna destroy/smash/wreck you in 1v1', 'fuck you bro haha', 'ez game', 'you played like trash'\n\
- FAKE-GAME SHIELD EVASION (PUNISH STRICTLY - SUSPICIOUS/DELETE):\n\
  Trolls append game names ('in minecraft', 'in roblox', 'in game', '1v1') to disguise real toxicity, suicide incitement, or death threats.\n\
  * Suicide / Death wishes disguised with games ('kys in minecraft', 'die in roblox', 'kill yourself in game', 'hang yourself in cs') -> SUSPICIOUS(60m) or DELETE(120m)\n\
  * Real-world violence / doxxing disguised as game ('burn your house in rust', 'i will find where you live in game', 'Mizuri will be burned in minecraft') -> DELETE(120m)\n\
  * Cancer / severe malice wishes ('get cancer in game', 'hope you die in cs') -> SUSPICIOUS(30m)\n\
REAL VIOLATIONS (SUSPICIOUS/DELETE):\n\
- Hate speech, racism, slurs, calls to kill or exterminate protected racial/ethnic/religious groups ('kill all blacks', 'kill all X', slurs) -> DELETE(1440m). NEVER ALLOW as banter under any pretext.\n\
- Edgy baiting or trolling on gender/protected topics ('i love sexism', 'i love misogyny', 'i live misogyny') -> SUSPICIOUS(1m) under Minor/Mild.\n\
- Threats naming users (or short nicknames like 'miz' for 'Mizuri') or with dates/methods -> DELETE(120m)\n\
- Direct death wishes/suicide: 'kys', 'you should die', 'die idiot' -> SUSPICIOUS(30m)\n\
- Credible real-world threats with doxxing/stalking: 'i know where you live' -> DELETE(120m)\n\
Format strictly:\n\
VERDICT:[ALLOW|SUSPICIOUS|DELETE]\n\
RULE:[Rule name or None]\n\
MUTE_MINUTES:[0|1|15|30|60|120|1440]\n\
REASON:[<=8 words]";

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
pub struct GroqDecision {
    pub verdict: String,
    pub rule: String,
    pub mute_minutes: u64,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub enum ModerationVerdict {
    Allow,
    DeleteConfirmed {
        reason: String,
        score: f64,
        category: String,
        model_used: String,
        rule_violated: String,
        mute_minutes: u64,
    },
    FlagSuspicious {
        reason: String,
        score: f64,
        category: String,
        model_used: String,
        rule_violated: String,
        mute_minutes: u64,
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
    #[serde(default)]
    content: String,
    #[serde(default)]
    reasoning: Option<String>,
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
            .unwrap_or_else(|| "openai/gpt-oss-120b".to_string());

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

        // Never fast-whitelist if there are insult markers, game shield patterns, or severe malice
        if TARGET_INSULTS.iter().any(|i| lower.contains(i))
            || GAME_SHIELD_PATTERNS.iter().any(|p| lower.contains(p))
            || lower.contains("kys")
            || lower.contains("burn alive")
            || lower.contains("burned alive")
            || lower.contains("cancer")
            || lower.contains("doxx")
            || lower.contains("leak")
            || lower.contains("swat")
            || lower.contains("hang")
            || lower.contains("minecraft")
            || lower.contains("roblox")
            || lower.contains("майнкрафт")
            || lower.contains("роблокс")
            || lower.contains("сдохни")
            || lower.contains("вскройся")
        {
            return false;
        }

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

    fn is_game_shield_evasion(content: &str) -> bool {
        let lower = content.to_lowercase();
        let has_shield = GAME_SHIELD_PATTERNS.iter().any(|p| lower.contains(p))
            || lower.contains("minecraft")
            || lower.contains("roblox")
            || lower.contains("майнкрафт")
            || lower.contains("роблокс");
        if !has_shield {
            return false;
        }
        let has_hostility = SEVERE_HARM_KEYWORDS.iter().any(|k| lower.contains(k))
            || TARGET_INSULTS.iter().any(|i| lower.contains(i))
            || lower.contains("kys")
            || lower.contains("die")
            || lower.contains("cancer")
            || lower.contains("doxx")
            || lower.contains("leak")
            || lower.contains("ip")
            || lower.contains("swat")
            || lower.contains("hang")
            || lower.contains("сожгу")
            || lower.contains("сдохни")
            || lower.contains("рак")
            || lower.contains("вскройся");
        has_hostility
    }

    fn is_abstract_placeholder(text: &str) -> bool {
        let lower = text.to_lowercase();
        const PLACEHOLDERS: &[&str] = &[
            "(someone)", "(somone)", "(somebody)", "(person)", "(user)", "(target)", "(anyone)",
            "[someone]", "[somone]", "[somebody]", "[person]", "[user]", "[name]", "[target]",
            "<someone>", "<user>", "<person>", "<name>",
            "(кого-то)", "(кого то)", "(человека)", "(юзера)", "[кого-то]", "[человека]",
            "... someone", "... somone", "... somebody", "... person", "... user",
            "... кого-то", "... кого то", "... человека"
        ];
        PLACEHOLDERS.iter().any(|p| lower.contains(p))
    }

    fn is_meta_or_quote(text: &str) -> bool {
        let lower = text.to_lowercase();
        let trimmed_lower = lower.trim();

        if Self::is_abstract_placeholder(trimmed_lower) {
            return true;
        }

        const META_PREFIXES: &[&str] = &[
            "say ", "saying ", "say: ", "say '", "say \"",
            "if you say ", "if u say ", "if i say ", "if someone says ",
            "like saying ", "like when you say ",
            "u said ", "you said ", "he said ", "she said ", "they said ",
            "i said ", "we said ",
            "got muted for ", "muted for saying ", "timed out for ", "triggers on ",
            "bot triggers on ", "banned for saying ", "ban for saying ",
            "он сказал ", "ты сказал ", "я сказал ", "мутит за ",
            "типа если сказать ", "типа сказать ", "типа '", "типа \""
        ];

        if META_PREFIXES.iter().any(|prefix| trimmed_lower.starts_with(prefix)) {
            return true;
        }

        if (trimmed_lower.starts_with('"') && trimmed_lower.ends_with('"'))
            || (trimmed_lower.starts_with('\'') && trimmed_lower.ends_with('\''))
            || (trimmed_lower.starts_with('«') && trimmed_lower.ends_with('»'))
        {
            return true;
        }

        false
    }

    fn is_directed_or_targeted(content: &str, has_reply: bool, mentions: &[(u64, String)], history: &[ChatEntry]) -> bool {
        if has_reply || !mentions.is_empty() || content.contains("<@") || content.contains("@") {
            return true;
        }

        let lower = content.to_lowercase();
        let words: Vec<&str> = lower.split_whitespace().collect();

        // 1. Check if any participant name or abbreviation from recent channel history is mentioned
        for entry in history {
            let author_lower = entry.author_name.to_lowercase();
            let base = author_lower.trim_end_matches(|c: char| c.is_ascii_digit());
            if base.len() >= 3 {
                if lower.contains(base) {
                    return true;
                }
                // Check if any word is a 3+ letter prefix abbreviation (e.g. "miz" or "mizu" for "mizuri")
                for w in &words {
                    let clean = w.trim_matches(|c: char| !c.is_alphanumeric());
                    if clean.len() >= 3 && base.len() > clean.len() && base.starts_with(clean) {
                        return true;
                    }
                }
            }
        }
        let has_pronoun = words.iter().any(|w| {
            let clean = w.trim_matches(|c: char| !c.is_alphanumeric());
            TARGET_PRONOUNS.contains(&clean)
        });
        if has_pronoun {
            return true;
        }

        // 3. Check insult markers
        let has_insult = words.iter().any(|w| {
            let clean = w.trim_matches(|c: char| !c.is_alphanumeric());
            TARGET_INSULTS.contains(&clean)
        });
        if has_insult {
            return true;
        }

        // 4. Check proper noun / name patterns (e.g. "Mizuri will be...", "Alex is...")
        let orig_words: Vec<&str> = content.split_whitespace().collect();
        if orig_words.len() > 1 {
            let first_word = orig_words[0];
            let clean_first = first_word.trim_matches(|c: char| !c.is_alphanumeric());
            let clean_first_lower = clean_first.to_lowercase();
            if clean_first.len() >= 3
                && clean_first.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                && !COMMON_NON_NAMES.contains(&clean_first_lower.as_str())
            {
                let next_clean = orig_words[1].trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
                if matches!(next_clean.as_str(), "will" | "is" | "should" | "must" | "can" | "needs" | "будет" | "должен" | "надо" | "это" | "was" | "бы" | "has") {
                    return true;
                }
            }
        }

        // Check for direct action verb targeting a capitalized name (e.g. "kill Alex", "burn Mizuri")
        for i in 1..orig_words.len() {
            let word = orig_words[i];
            if word.starts_with('(') || word.starts_with('[') || word.ends_with(')') || word.ends_with(']') {
                continue;
            }
            let clean = word.trim_matches(|c: char| !c.is_alphanumeric());
            let clean_lower = clean.to_lowercase();
            if clean.len() >= 3
                && clean.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                && !COMMON_NON_NAMES.contains(&clean_lower.as_str())
            {
                let prev = orig_words[i - 1].trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
                if matches!(prev.as_str(), "kill" | "burn" | "destroy" | "smash" | "hurt" | "to" | "at" | "убить" | "сожечь" | "зарезать") {
                    return true;
                }
            }
        }

        false
    }

    fn safe_truncate(s: &str, max_chars: usize) -> &str {
        match s.char_indices().nth(max_chars) {
            Some((idx, _)) => &s[..idx],
            None => s,
        }
    }

    fn format_compact_prompt(
        &self,
        ctx: &MessageContext<'_>,
        history: &[ChatEntry],
        max_score: f64,
        top_cat: &str,
        is_game_shield: bool,
        is_meta: bool,
    ) -> String {
        let mut p = String::with_capacity(512);

        // Include recent conversation context (up to 7 prior messages)
        let recent: Vec<_> = history.iter().rev().take(7).collect();
        if !recent.is_empty() {
            p.push_str("Recent chat context:\n");
            for (idx, e) in recent.into_iter().rev().enumerate() {
                let short_c = Self::safe_truncate(&e.content, 90);
                p.push_str(&format!("{}. {}: \"{}\"\n", idx + 1, e.author_name, short_c.trim()));
            }
            p.push('\n');
        }

        p.push_str("FLAGGED MESSAGE TO EVALUATE:\n");
        p.push_str(&format!("Author: @{}\n", ctx.author_name));
        p.push_str(&format!("Content: \"{}\"\n", ctx.content.trim()));
        if let Some((rep_author, _, _, rep_text)) = ctx.reply_to {
            let short_rep = Self::safe_truncate(rep_text, 90);
            p.push_str(&format!("Replying to: @{}: \"{}\"\n", rep_author, short_rep.trim()));
        }
        p.push_str(&format!("OpenAI Flag: {} (score: {:.2})\n", top_cat, max_score));
        if is_game_shield {
            p.push_str("⚠️ EVASION ALERT: Message uses game shield ('in minecraft/roblox/game') to disguise toxicity/threats! Do NOT excuse death wishes, suicide or harassment as banter.\n");
        }
        if is_meta {
            p.push_str("ℹ️ META CONTEXT NOTE: Message appears to be a meta-discussion, quote, rule discussion, or hypothetical placeholder example (e.g. discussing what words trigger the bot or using '(someone)'). Do NOT punish users for quoting, discussing bot rules, or hypothetical templates. Only punish genuine threats directed at real people.\n");
        }
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

        let history = self.get_context_snapshot(ctx.channel_id);
        let lower = trimmed.to_lowercase();
        let has_severe_harm_keyword = SEVERE_HARM_KEYWORDS.iter().any(|k| lower.contains(k));
        let has_provocative_bait = PROVOCATIVE_BAIT_KEYWORDS.iter().any(|k| lower.contains(k));
        let is_game_shield = Self::is_game_shield_evasion(trimmed);
        let is_violent_category = matches!(
            top_cat.as_str(),
            "violence" | "violence/graphic" | "self-harm" | "self-harm/intent" | "self-harm/instructions" | "hate" | "hate/threatening" | "harassment/threatening"
        );

        if is_game_shield {
            println!("   🕵️ [GAME SHIELD DETECTED] Potential veiled toxicity/threat hiding behind game titles!");
        }

        println!(
            "\n🔍 [AI SCANNER] Channel: #{} | Author: @{} ({}) | Text: \"{}\"",
            ctx.channel_name.as_deref().unwrap_or("unknown"),
            ctx.author_name,
            ctx.author_id,
            trimmed
        );
        println!(
            "   📊 [OPENAI] Max Score: {:.2} ({}) | Severe: {:.2} | Details: [{}]",
            max_score,
            if top_cat.is_empty() { "none" } else { &top_cat },
            severe_score,
            cat_breakdown
        );

        // 1A. Clear clean content -> Instant ALLOW (only if no severe harm keywords, no provocative bait and no game shield evasion)
        if max_score < OPENAI_SAFE_THRESHOLD && !has_severe_harm_keyword && !has_provocative_bait && !is_game_shield {
            println!("   ↳ [SAFE] Score {:.2} < {:.2} safe threshold -> ALLOW (0 tokens spent)", max_score, OPENAI_SAFE_THRESHOLD);
            return ModerationVerdict::Allow;
        }

        let is_directed = Self::is_directed_or_targeted(trimmed, ctx.reply_to.is_some(), ctx.mentions, &history);
        let is_meta = Self::is_meta_or_quote(trimmed);
        let is_abstract_placeholder = Self::is_abstract_placeholder(trimmed);

        if is_abstract_placeholder && !is_directed {
            println!("   ↳ [META-PLACEHOLDER] Abstract hypothetical example detected with (someone)/(user) -> ALLOW (0 tokens spent)");
            return ModerationVerdict::Allow;
        }

        // ── Smart Dynamic Model Routing: 120B Deep Reasoning for Drama/Hardcore/Threats/Evasions vs Fast Guard for Banter ──
        let is_hardcore_or_drama = severe_score > 0.65
            || max_score > 0.78
            || (ctx.reply_to.is_some() && max_score > 0.55)
            || has_severe_harm_keyword
            || is_violent_category
            || is_game_shield;

        let preferred_model = if is_hardcore_or_drama {
            &self.groq_deep_model // openai/gpt-oss-120b (120B reasoning model for drama, threats, evasions & complex context)
        } else {
            &self.groq_fast_model // qwen/qwen3.8-27b (27B ultra-fast for quick banter & standard flags)
        };

        // 1B. High Score (>0.82 or severe category >0.70) ──────────────────────
        // NEVER BLINDLY DELETE ON RAW SCORE: ALWAYS PASS TO LLM GUARD FIRST
        if severe_score > OPENAI_CATEGORY_SEVERE_THRESHOLD || max_score > OPENAI_SEVERE_THRESHOLD {
            if !self.groq_keys.is_empty() {
                println!(
                    "   🤖 [AI ROUTER] High severity score ({:.2})! Routing to {} (Context: {} prior msgs | Mode: {})",
                    max_score,
                    preferred_model,
                    history.len(),
                    if is_hardcore_or_drama { "120B Deep Reasoning" } else { "27B Fast Guard" }
                );
                let user_prompt = self.format_compact_prompt(ctx, &history, max_score, &top_cat, is_game_shield, is_meta);

                match self.call_groq_failover(preferred_model, SERVER_RULES_SYSTEM_PROMPT, &user_prompt).await {
                    Ok((decision, model_used, elapsed_ms)) => {
                        println!(
                            "   ⚡ [AI RESPONSE] Model: {} (took {}ms) | Verdict: {} | Rule: {} | Mute: {}m | Reason: \"{}\"",
                            model_used, elapsed_ms, decision.verdict, decision.rule, decision.mute_minutes, decision.reason
                        );

                        if decision.verdict.contains("ALLOW") {
                            if (top_cat == "hate" || top_cat == "hate/threatening" || cat_breakdown.contains("hate: 0.8") || cat_breakdown.contains("hate: 0.9") || cat_breakdown.contains("hate: 1.0") || cat_breakdown.contains("hate/threatening: 0.8") || cat_breakdown.contains("hate/threatening: 0.9") || cat_breakdown.contains("hate/threatening: 1.0")) && max_score > 0.80 && !is_meta {
                                println!("   🚨 [HATE SPEECH GUARD] Overriding LLM ALLOW for severe hate speech/hate-threatening violation (score {:.2}) -> DELETE(1440m)", max_score);
                                return ModerationVerdict::DeleteConfirmed {
                                    reason: format!("{}: Hate speech inciting violence or racial hatred", top_cat),
                                    score: max_score,
                                    category: top_cat,
                                    model_used: format!("OpenAI Omni-Mod Guard ({})", model_used),
                                    rule_violated: "Crit (Hate Speech)".to_string(),
                                    mute_minutes: 1440,
                                };
                            }
                            println!("   ✅ [BANTER PASS] LLM verified message as safe gaming hyperbole -> ALLOW");
                            return ModerationVerdict::Allow;
                        } else if is_meta && !is_directed {
                            println!("   🛡️ [META GUARD] Overriding LLM {} on undirected meta-discussion / quote to ALLOW.", decision.verdict);
                            return ModerationVerdict::Allow;
                        } else if decision.verdict.contains("DELETE") {
                            println!("   🚨 [AI VERDICT: DELETE] Confirmed severe violation! Mute: {}m (Rule: {})", decision.mute_minutes, decision.rule);
                            let model_label = if model_used.contains("120b") {
                                format!("OpenAI + {} (120B Deep Drama Arbiter)", model_used)
                            } else if model_used.contains("20b") {
                                format!("OpenAI + {} (20B Safety Arbiter)", model_used)
                            } else {
                                format!("OpenAI + {} Guard", model_used)
                            };
                            return ModerationVerdict::DeleteConfirmed {
                                reason: format!("{}: {}", top_cat, decision.reason),
                                score: max_score,
                                category: top_cat,
                                model_used: model_label,
                                rule_violated: decision.rule,
                                mute_minutes: decision.mute_minutes,
                            };
                        } else {
                            println!("   ⚠️ [AI VERDICT: SUSPICIOUS] Flagged for mod review + auto-timeout: {}m (Rule: {})", decision.mute_minutes, decision.rule);
                            let model_label = if model_used.contains("120b") {
                                format!("{} (120B Deep Drama Arbiter)", model_used)
                            } else if model_used.contains("20b") {
                                format!("{} (20B Safety Arbiter)", model_used)
                            } else {
                                format!("{} (Guard Checked)", model_used)
                            };
                            return ModerationVerdict::FlagSuspicious {
                                reason: format!("High score ({:.2}): {}", max_score, decision.reason),
                                score: max_score,
                                category: top_cat,
                                model_used: model_label,
                                rule_violated: decision.rule,
                                mute_minutes: decision.mute_minutes,
                            };
                        }
                    }
                    Err(e) => {
                        eprintln!("   ❌ [GROQ GUARD FAILOVER] Error: {}. Falling back to FlagSuspicious.", e);
                        if (top_cat == "hate" || top_cat == "hate/threatening") && max_score > 0.80 && !is_meta {
                            return ModerationVerdict::DeleteConfirmed {
                                reason: format!("{}: Severe hate speech / threatening violation", top_cat),
                                score: max_score,
                                category: top_cat,
                                model_used: "OpenAI Omni-Mod (Crit Guard)".to_string(),
                                rule_violated: "Crit (Hate Speech)".to_string(),
                                mute_minutes: 1440,
                            };
                        }
                        return ModerationVerdict::FlagSuspicious {
                            reason: format!("High score ({:.2}), pending mod review", max_score),
                            score: max_score,
                            category: top_cat,
                            model_used: "OpenAI Guard (Pending Review)".to_string(),
                            rule_violated: "Unreviewed High Score".to_string(),
                            mute_minutes: 0,
                        };
                    }
                }
            }

            if (top_cat == "hate" || top_cat == "hate/threatening") && max_score > 0.80 && !is_meta {
                return ModerationVerdict::DeleteConfirmed {
                    reason: format!("{}: Severe hate speech / threatening violation", top_cat),
                    score: max_score,
                    category: top_cat,
                    model_used: "OpenAI Omni-Mod (Crit Guard)".to_string(),
                    rule_violated: "Crit (Hate Speech)".to_string(),
                    mute_minutes: 1440,
                };
            }

            return ModerationVerdict::FlagSuspicious {
                reason: format!("High score ({:.2}): pending staff review", max_score),
                score: max_score,
                category: top_cat,
                model_used: "OpenAI (Pending Staff Review)".to_string(),
                rule_violated: "Unreviewed High Score".to_string(),
                mute_minutes: 0,
            };
        }

        // ── 2. SMART GREY-ZONE PRE-FILTER (0.45 ..= 0.82) ─────────────────────
        // ONLY bypass if it's general non-violent gaming frustration (e.g. "fuck this lag")
        if !is_directed && !is_violent_category && !has_severe_harm_keyword && !has_provocative_bait && !is_game_shield && max_score < 0.60 {
            println!("   ↳ [PRE-FILTER] General gaming frustration / non-directed (score {:.2}) -> ALLOW (0 tokens spent)", max_score);
            return ModerationVerdict::Allow;
        }

        if self.groq_keys.is_empty() {
            println!("   ↳ [NO KEYS] Groq keys not configured -> ALLOW");
            return ModerationVerdict::Allow;
        }

        // ── 3. TIER 2: DEEP LLM FOR GREY ZONE (COMPACT PROMPT) ────────────────
        println!(
            "   🤖 [AI ROUTER] Grey-zone evaluation! Routing to {} (Context: {} prior msgs | is_directed: {})",
            preferred_model,
            history.len(),
            is_directed
        );
        let user_prompt = self.format_compact_prompt(ctx, &history, max_score, &top_cat, is_game_shield, is_meta);

        match self.call_groq_failover(preferred_model, SERVER_RULES_SYSTEM_PROMPT, &user_prompt).await {
            Ok((decision, model_used, elapsed_ms)) => {
                println!(
                    "   ⚡ [AI RESPONSE] Model: {} (took {}ms) | Verdict: {} | Rule: {} | Mute: {}m | Reason: \"{}\"",
                    model_used, elapsed_ms, decision.verdict, decision.rule, decision.mute_minutes, decision.reason
                );
                if decision.verdict.contains("DELETE") || decision.verdict.contains("SUSPICIOUS") {
                    if is_meta && !is_directed {
                        println!("   🛡️ [META GUARD] Overriding LLM {} on undirected meta-discussion / quote to ALLOW.", decision.verdict);
                        return ModerationVerdict::Allow;
                    }
                    println!("   ⚠️ [AI VERDICT: SUSPICIOUS] Flagged grey-zone violation! Mute: {}m (Rule: {})", decision.mute_minutes, decision.rule);
                    let model_label = if model_used.contains("120b") {
                        format!("{} (120B Deep Drama Arbiter)", model_used)
                    } else if model_used.contains("20b") {
                        format!("{} (20B Safety Arbiter)", model_used)
                    } else {
                        format!("{} (Fast Context)", model_used)
                    };
                    ModerationVerdict::FlagSuspicious {
                        reason: decision.reason,
                        score: max_score,
                        category: top_cat,
                        model_used: model_label,
                        rule_violated: decision.rule,
                        mute_minutes: decision.mute_minutes,
                    }
                } else {
                    println!("   ✅ [ALLOW] Grey-zone message allowed by LLM.");
                    ModerationVerdict::Allow
                }
            }
            Err(e) => {
                eprintln!("   ❌ [GROQ ERROR] Grey-zone call failed: {}. Allowing.", e);
                ModerationVerdict::Allow
            }
        }
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
    ) -> Result<(GroqDecision, String, u128), String> {
        let total_keys = self.groq_keys.len();
        if total_keys == 0 {
            return Err("No Groq keys configured".to_string());
        }

        let mut models_to_try = vec![model];
        if !models_to_try.contains(&"openai/gpt-oss-120b") {
            models_to_try.push("openai/gpt-oss-120b");
        }
        if !models_to_try.contains(&"qwen/qwen3.8-27b") {
            models_to_try.push("qwen/qwen3.8-27b");
        }
        if !models_to_try.contains(&"openai/gpt-oss-20b") {
            models_to_try.push("openai/gpt-oss-20b");
        }

        let start_idx = self.groq_counter.fetch_add(1, Ordering::Relaxed) % total_keys;
        for target_model in models_to_try {
            for i in 0..total_keys {
                let idx = (start_idx + i) % total_keys;
                let key = &self.groq_keys[idx];
                match self.execute_groq_call(key, target_model, system_prompt, user_prompt).await {
                    Ok((res, elapsed_ms)) => return Ok((res, target_model.to_string(), elapsed_ms)),
                    Err(e) => {
                        eprintln!("[GROQ FAILOVER] Key {} model '{}' error: {}. Trying fallback...", idx, target_model, e);
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
    ) -> Result<(GroqDecision, u128), String> {
        let start_time = std::time::Instant::now();
        let max_tokens = if model.contains("gpt-oss") { 1024 } else { 85 };
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
            max_tokens,
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

        let raw_json = resp.text().await.map_err(|e| format!("Network error: {}", e))?;
        let body: GroqChatResponse = serde_json::from_str(&raw_json).map_err(|e| format!("JSON decode error: {}", e))?;
        let elapsed_ms = start_time.elapsed().as_millis();
        if let Some(choice) = body.choices.first() {
            let text = &choice.message.content;
            let upper = text.to_uppercase();
            let mut verdict = String::new();
            let mut rule = "Server Guidelines".to_string();
            let mut mute_minutes: u64 = 0;
            let mut reason = String::new();

            for line in text.lines() {
                let normalized = line.replace('*', "").replace('`', "").replace('#', "").replace('>', "").trim().to_string();
                let upper_l = normalized.to_uppercase();
                if let Some(rest) = upper_l.strip_prefix("VERDICT:") {
                    verdict = rest.trim().trim_matches(|c: char| c == '[' || c == ']' || c == '"' || c == '\'').to_uppercase();
                } else if let Some(_rest) = upper_l.strip_prefix("RULE:") {
                    let val = normalized["RULE:".len()..].trim().trim_matches(|c: char| c == '[' || c == ']' || c == '"' || c == '\'');
                    rule = val.to_string();
                } else if let Some(rest) = upper_l.strip_prefix("MUTE_MINUTES:") {
                    let num_str = rest.trim().trim_matches(|c: char| c == '[' || c == ']' || c == '"' || c == '\'');
                    mute_minutes = num_str.parse::<u64>().unwrap_or(0);
                } else if let Some(_rest) = upper_l.strip_prefix("REASON:") {
                    let val = normalized["REASON:".len()..].trim().trim_matches(|c: char| c == '[' || c == ']' || c == '"' || c == '\'');
                    reason = val.to_string();
                }
            }

            // If content was empty or didn't contain VERDICT, also try reasoning if available
            if verdict.is_empty() {
                if let Some(ref r) = choice.message.reasoning {
                    for line in r.lines() {
                        let normalized = line.replace('*', "").replace('`', "").replace('#', "").replace('>', "").trim().to_string();
                        let upper_l = normalized.to_uppercase();
                        if let Some(rest) = upper_l.strip_prefix("VERDICT:") {
                            verdict = rest.trim().trim_matches(|c: char| c == '[' || c == ']' || c == '"' || c == '\'').to_uppercase();
                        } else if let Some(_rest) = upper_l.strip_prefix("RULE:") {
                            let val = normalized["RULE:".len()..].trim().trim_matches(|c: char| c == '[' || c == ']' || c == '"' || c == '\'');
                            rule = val.to_string();
                        } else if let Some(rest) = upper_l.strip_prefix("MUTE_MINUTES:") {
                            let num_str = rest.trim().trim_matches(|c: char| c == '[' || c == ']' || c == '"' || c == '\'');
                            mute_minutes = num_str.parse::<u64>().unwrap_or(0);
                        } else if let Some(_rest) = upper_l.strip_prefix("REASON:") {
                            let val = normalized["REASON:".len()..].trim().trim_matches(|c: char| c == '[' || c == ']' || c == '"' || c == '\'');
                            reason = val.to_string();
                        }
                    }
                    if verdict.is_empty() {
                        let r_upper = r.to_uppercase();
                        if r_upper.contains("DELETE") || r_upper.contains("CRIT") || r_upper.contains("HATE SPEECH") || r_upper.contains("GENOCIDE") {
                            verdict = "DELETE".to_string();
                            rule = "Crit".to_string();
                            mute_minutes = 1440;
                            reason = "Severe violation identified in evaluation".to_string();
                        }
                    }
                }
            }

            if verdict.is_empty() {
                if upper.contains("DELETE") {
                    verdict = "DELETE".to_string();
                } else if upper.contains("SUSPICIOUS") {
                    verdict = "SUSPICIOUS".to_string();
                } else if text.trim().is_empty() {
                    // LLM ran out of tokens or returned empty completion - do NOT guess ALLOW!
                    return Err("LLM returned empty completion (ran out of tokens or filtered)".to_string());
                } else {
                    verdict = "ALLOW".to_string();
                }
            }

            if reason.is_empty() {
                if rule != "Server Guidelines" && !rule.is_empty() {
                    reason = format!("Violated: {}", rule);
                } else {
                    reason = "Context telemetry evaluation".to_string();
                }
            }

            // Fallback timeout scaling if model omitted MUTE_MINUTES
            if mute_minutes == 0 {
                if verdict == "DELETE" {
                    mute_minutes = 120;
                } else if verdict == "SUSPICIOUS" && rule != "None" {
                    if rule.to_lowercase().contains("minor") || rule.to_lowercase().contains("mild") {
                        mute_minutes = 1;
                    } else {
                        mute_minutes = 30;
                    }
                }
            }

            // Enforce 1 minute for Minor / Mild violations instead of 5
            if mute_minutes == 5 || (mute_minutes > 1 && mute_minutes <= 10 && (rule.to_lowercase().contains("minor") || rule.to_lowercase().contains("mild"))) {
                mute_minutes = 1;
            }

            Ok((GroqDecision {
                verdict,
                rule,
                mute_minutes,
                reason,
            }, elapsed_ms))
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
        let history = vec![
            ChatEntry {
                message_id: 1,
                author_id: 100,
                author_name: "Mizuri".to_string(),
                content: "hi".to_string(),
            }
        ];
        assert!(AiModerator::is_directed_or_targeted("ты клоун", false, &[], &[]));
        assert!(AiModerator::is_directed_or_targeted("you are trash", false, &[], &[]));
        assert!(AiModerator::is_directed_or_targeted("whatever man", true, &[], &[]));
        assert!(AiModerator::is_directed_or_targeted("he is so annoying", false, &[], &[]));
        assert!(AiModerator::is_directed_or_targeted("Mizuri will be burned alive", false, &[], &history));
        assert!(AiModerator::is_directed_or_targeted("miz will be burned alive", false, &[], &history));
        assert!(AiModerator::is_directed_or_targeted("mizu should leave", false, &[], &history));
        assert!(AiModerator::is_directed_or_targeted("Mizuri will be burned alive", false, &[], &[]));
        assert!(AiModerator::is_directed_or_targeted("kill Alex", false, &[], &[]));
        assert!(!AiModerator::is_directed_or_targeted("Say I'll kill ... (Somone)", false, &[], &[]));
        assert!(!AiModerator::is_directed_or_targeted("kill in Minecraft", false, &[], &[]));
        assert!(!AiModerator::is_directed_or_targeted("I love Rust", false, &[], &[]));
        assert!(!AiModerator::is_directed_or_targeted("fuck this lag", false, &[], &[]));
    }

    #[test]
    fn test_is_abstract_placeholder() {
        assert!(AiModerator::is_abstract_placeholder("Say I'll kill ... (Somone)"));
        assert!(AiModerator::is_abstract_placeholder("what if I say die to (user)"));
        assert!(AiModerator::is_abstract_placeholder("saying kys to (someone)"));
        assert!(AiModerator::is_abstract_placeholder("kill ... somone"));
        assert!(!AiModerator::is_abstract_placeholder("kill Alex"));
        assert!(!AiModerator::is_abstract_placeholder("you should die"));
    }

    #[test]
    fn test_safe_truncate() {
        let s = "So crazy slots and bandit n few others  isn’t gonna b in game next week. What else isn’t gonna b in game?😔";
        let truncated = AiModerator::safe_truncate(s, 40);
        assert!(truncated.chars().count() <= 40);
        let truncated_full = AiModerator::safe_truncate(s, 500);
        assert_eq!(truncated_full, s);
    }

    #[test]
    fn test_is_game_shield_evasion() {
        // Evasions that hide behind game context:
        assert!(AiModerator::is_game_shield_evasion("kys in minecraft"));
        assert!(AiModerator::is_game_shield_evasion("you should die in roblox"));
        assert!(AiModerator::is_game_shield_evasion("hang yourself in cs"));
        assert!(AiModerator::is_game_shield_evasion("i will burn your house in rust"));
        assert!(AiModerator::is_game_shield_evasion("сдохни в майнкрафте"));
        assert!(AiModerator::is_game_shield_evasion("сожгу тебя в роблоксе"));
        assert!(AiModerator::is_game_shield_evasion("вскройся в игре"));

        // Genuine gaming chatter / banter should NOT be flagged as game-shield evasion:
        assert!(!AiModerator::is_game_shield_evasion("1v1 me on mid"));
        assert!(!AiModerator::is_game_shield_evasion("im gonna destroy you in 1v1"));
        assert!(!AiModerator::is_game_shield_evasion("let's play minecraft together"));
        assert!(!AiModerator::is_game_shield_evasion("gg ez game"));
        assert!(!AiModerator::is_game_shield_evasion("kill boss in raid"));
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

    #[tokio::test]
    async fn test_check_message_burn_alive_threat() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("general".to_string()),
            message_id: 3,
            timestamp_unix: 1727376000,
            author_name: "polska8635",
            author_id: 795992869164679168,
            author_nick: None,
            account_age_days: Some(30),
            server_member_days: Some(10),
            roles_count: 1,
            content: "Mizuri will be burned alive on novemeber 24 2028",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'Mizuri will be burned alive...': {:?}\n", verdict);
        assert!(matches!(verdict, ModerationVerdict::DeleteConfirmed { .. } | ModerationVerdict::FlagSuspicious { .. }));
    }

    #[tokio::test]
    async fn test_check_message_say_ill_kill_placeholder() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 4,
            timestamp_unix: 1727376000,
            author_name: "yphn",
            author_id: 1018290809835094016,
            author_nick: Some("Mizuri | tester".to_string()),
            account_age_days: Some(300),
            server_member_days: Some(200),
            roles_count: 3,
            content: "Say I'll kill ... (Somone)",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'Say I\\'ll kill ... (Somone)': {:?}\n", verdict);
        assert!(matches!(verdict, ModerationVerdict::Allow));
    }

    #[tokio::test]
    async fn test_check_message_hate_speech() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("general".to_string()),
            message_id: 5,
            timestamp_unix: 1727376000,
            author_name: "ilikeclassics",
            author_id: 1542942068736000154,
            author_nick: None,
            account_age_days: Some(30),
            server_member_days: Some(10),
            roles_count: 1,
            content: "My opinion is, we should kill all blacks",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for hate speech: {:?}\n", verdict);
        assert!(matches!(verdict, ModerationVerdict::DeleteConfirmed { .. }));
    }

    #[tokio::test]
    async fn test_check_message_mild_bait() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("general".to_string()),
            message_id: 6,
            timestamp_unix: 1727376000,
            author_name: "tahyr2",
            author_id: 1226577641247735931,
            author_nick: None,
            account_age_days: Some(30),
            server_member_days: Some(10),
            roles_count: 1,
            content: "I love sexism",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for mild bait ('I love sexism'): {:?}\n", verdict);
        match verdict {
            ModerationVerdict::FlagSuspicious { mute_minutes, .. } => {
                assert_eq!(mute_minutes, 1, "Expected mild violation to mute for exactly 1 minute, got {}m", mute_minutes);
            }
            other => panic!("Expected FlagSuspicious with 1m mute, got {:?}", other),
        }
    }
}

