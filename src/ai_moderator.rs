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
    "murder", "stab", "shoot", "hang", "slit", "rape", "torture", "strangle",
    "choke", "execute", "suicide", "kys",
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

const DOX_AND_EXTORTION_KEYWORDS: &[&str] = &[
    "spread all your information", "spread your information", "spread your info",
    "leak your info", "leak your information", "leak your data", "leak your face",
    "leak your address", "leak your photos", "leak your pics", "post your info",
    "post your address", "post your face", "post your photos", "expose you", "expose your",
    "doxx you", "dox you", "doxxed you", "doxed you", "i will doxx", "i will dox",
    "grabbed your ip", "got your ip", "have your ip", "know where you live", "find where you live",
    "send swat", "swat you", "swatting",
    "солью инфу", "солью твои фото", "солью твой адрес", "солью твои данные", "солью в сеть",
    "слив инфы", "слив фото", "слив данных", "деанон", "сдеаноню", "пробью тебя", "пробил тебя",
    "знаю где ты живешь", "знаю твой адрес", "распространю твои данные", "выложу твои данные",
    "выложу твой адрес", "выложу твой номер"
];

const SLUR_WORDS: &[&str] = &[
    // N-word and common evasion spellings
    "nga", "ngas", "ngga", "nggas", "niga", "nigas", "nigga", "niggas", "nigg", "niggers", "nigger", "n1gga", "n1gger", "niqqa", "niqqas",
    // Homophobic slurs
    "fag", "fags", "faggot", "faggots", "fagg", "f@g",
    // Antisemitic slurs
    "kike", "kikes",
    // Anti-Asian slurs
    "chink", "chinks",
    // Transphobic slurs
    "tranny", "trannies",
    // Russian ethnic/homophobic slurs
    "нигер", "нигеры", "ниггер", "ниггеры", "чурка", "чурки", "хач", "хачи", "пидор", "пидоры", "пидорас", "пидорасы", "хохол", "хохлы"
];

pub const SERVER_RULES_SYSTEM_PROMPT: &str = "\
Discord Arbiter for a gaming community. Mutes only (NO BAN/KICK).\n\
PUNISHMENT TIERS (SUSPICIOUS/DELETE):\n\
1. Minor/Mild -> SUSPICIOUS(1m): Malicious chat flooding, repetitive copy-paste raid spam, provocative gender bait ('i love sexism'). NEVER punish standard banter, complaints, or single messages under Minor/Mild!\n\
2. Mod -> SUSPICIOUS(15-30m): Explicit NSFW pornography links, deliberate toxic filter bypass. (NO mutes for gossip, rumors, or drama!)\n\
3. Major -> SUSPICIOUS(60m) or DELETE(120m): Direct real-world threats, stalking, publishing or threatening to leak private personal info (doxxing/extortion), malicious impersonation, server raid invites\n\
4. Crit -> DELETE(1440m): Racial/hate slurs ('nga','ngga','nigga','nigger','fag','faggot'), direct death wishes ('kys','you should die'), gore, malware\n\
QUOTES, OPINIONS, META-TALK & HYPOTHETICALS (ALLOW, RULE:None, MUTE:0):\n\
- Meta-talk and observations about doxxing or rules ('Its basically a doxx soo yeah', 'is that a doxx?', 'he got doxxed', 'thats a doxx', 'stop doxxing', 'you can't doxx people'): Discussing, observing, or reporting doxxing is META-TALK, NOT committing or threatening a doxx! Doxxing violations strictly require leaking actual private personal information (PII: address, phone, real full name, IP) or explicitly threatening/blackmailing someone to leak their info ('i will doxx you', 'im gonna leak your address/photos', 'солью твой адрес'). ALWAYS VERDICT: ALLOW (RULE: None, MUTE: 0).\n\
- Third-person remarks, opinions & hypotheticals ('They would think your dumb', 'people might think you're bad', 'I think that's stupid'): These are ordinary conversational remarks/opinions, NOT targeted harassment! ALWAYS ALLOW.\n\
- Casual words like 'dumb', 'stupid', 'silly', 'trash', 'noob', 'idiot' used colloquially in conversation ('thats dumb', 'they would think your dumb', 'im so dumb', 'dumb game'): This is standard casual chatter, NOT harassment! ALWAYS ALLOW.\n\
- Discussing server rules, testing bot triggers, quoting past messages, or explaining what caused a mute: 'Say I'll kill ... (Somone)', 'U said kill something ig', 'he got muted for saying die', 'the bot bans for kys'\n\
- Abstract placeholders & sentence templates ('...', '(someone)', '[user]', '<person>', 'кого-то', 'кто-то'): These are grammatical examples/templates, NOT real threats or harassment against members.\n\
- Only punish genuine direct malice/threats targeting real people. If demonstrating, quoting, explaining, or testing -> ALWAYS ALLOW.\n\
CRITIQUE & VENTING ABOUT 3RD-PARTY ENTITIES, GAMES, STUDIOS & DEVS (ALLOW, RULE:None, MUTE:0):\n\
- Complaining, venting, criticizing, or insulting game developers, game studios, companies, games, or public figures ('roblox devs becoming actual subhuman idiots', 'valve devs are brainless', 'ea is a trash company', 'riot balance team is clowns', 'this game sucks', 'devs are morons'): These are general gaming frustrations directed at external companies/studios, NOT interpersonal harassment or bullying of server members! ALWAYS VERDICT: ALLOW (RULE: None, MUTE: 0).\n\
CHAT GOSSIP, RUMORS & INTERPERSONAL QUESTIONS BETWEEN MEMBERS (ALLOW, RULE:None, MUTE:0):\n\
- Mentioning rumors, asking questions about what someone said or did, gossip, or playful accusations ('i heard from the grape vine that u paid someone to do something to me', 'did you talk behind my back?', 'why did you say that?'): This is natural social interaction and chat banter between Discord users. NEVER classify gossip, rumors, questions, or accusations as 'drama incitement' or harassment! ALWAYS VERDICT: ALLOW (RULE: None, MUTE: 0).\n\
SMART BANTER vs FAKE-GAME SHIELD BYPASS:\n\
- GENUINE BANTER & OPINIONS (ALLOW, RULE:None, MUTE:0):\n\
  Boasting, expressing opinions, casual critique ('They would think your dumb', 'you played bad', '1v1 me on mid', 'im gonna destroy/smash/wreck you in 1v1', 'fuck you bro haha', 'ez game', 'you played like trash').\n\
  Standalone casual profanity and common gaming insults ('fuck you', 'fuck u', 'stfu', 'fuck off', 'screw you', 'bitch', 'asshole', 'idiot', 'dumb') WITHOUT death wishes (kys/die) and WITHOUT slurs -> ALWAYS ALLOW (RULE: None, MUTE: 0). NEVER classify standalone 'fuck you' or 'stfu' as Harassment or Minor/Mild!\n\
  POST-IRONY, THEATRICAL HYPERBOLE & DRAMATIC TRASHTALK ('i will eviscerate you', 'i will obliterate you', 'im gonna demolish/annihilate/vaporize you', 'i will tear you to pieces', 'я тебя расщеплю на атомы/разорву/сотру в порошок'): These are 100% POST-IRONIC JOKES and comic gaming exaggeration between members, NOT credible real-world violence! Real threats involve real-world stalking, doxxing, addresses, weapons, or dates. NEVER punish cartoonish/fantasy threats like 'i will eviscerate you'! ALWAYS VERDICT: ALLOW (RULE: None, MUTE: 0).\n\
  IN-GAME 'KILL' & PVP TRASHTALK ('i will kill you', 'im gonna kill you', 'let me kill you', 'i will kill you if you throw', 'i will kill you in 1v1'): When the conversation context involves games, matches, ranks, characters, PvP, gameplay, anime, or gaming banter ('stop stealing my kills', 'one match till rankup', '1v1 me', 'playing together'): These refer to IN-GAME CHARACTER DEATH or gaming frustration between players, NOT real-world murder! ALWAYS VERDICT: ALLOW (RULE: None, MUTE: 0). Only punish 'kill' if it threatens REAL-WORLD physical harm (e.g. 'i know where you live and will kill you', real-life stalking, weapons, dates).\n\
  THIRD-PERSON GAMING CALLOUTS & PVP DIRECTIVES ('kill him', 'kill her', 'kill them', 'kill it', 'shoot him', 'get him', 'focus him', 'finish him', 'убей его', 'добивай', 'завали его'): These are standard tactical PvP/gaming callouts, spectator banter, or stream clip reactions referring to opponents, in-game characters, bosses, or enemies! They are NEVER real-world death threats! ALWAYS VERDICT: ALLOW (RULE: None, MUTE: 0). NEVER classify 'kill him/her/them' as Direct death threat, Harassment, or Violence!\n\
  Banter NEVER includes racial slurs or evasive abbreviations like 'nga'. Calling someone 'dumb nga' is a SLUR, NOT banter!\n\
- FAKE-GAME SHIELD EVASION (PUNISH STRICTLY - SUSPICIOUS/DELETE):\n\
  Trolls append game names ('in minecraft', 'in roblox', 'in game', '1v1') to disguise real toxicity, suicide incitement, or death threats.\n\
  * Suicide / Death wishes disguised with games ('kys in minecraft', 'die in roblox', 'kill yourself in game', 'hang yourself in cs') -> SUSPICIOUS(60m) or DELETE(120m)\n\
  * Real-world violence / doxxing disguised as game ('burn your house in rust', 'i will find where you live in game', 'Mizuri will be burned in minecraft') -> DELETE(120m)\n\
  * Cancer / severe malice wishes ('get cancer in game', 'hope you die in cs') -> SUSPICIOUS(30m)\n\
REAL VIOLATIONS (SUSPICIOUS/DELETE):\n\
- Racial slurs & slur abbreviations ('nga', 'ngga', 'niga', 'nigga', 'nigger', 'fag', 'kike', etc.): Calling someone 'dumb nga', 'nga', etc. -> DELETE(1440m) under Crit (Slurs/Hate). NEVER ALLOW as casual slang or banter under any pretext.\n\
- Hate speech, racism, calls to kill or exterminate protected groups ('kill all blacks') -> DELETE(1440m). NEVER ALLOW as banter.\n\
- Edgy baiting or trolling on gender/protected topics ('i love sexism', 'i love misogyny', 'i live misogyny') -> SUSPICIOUS(1m) under Minor/Mild.\n\
- Threats naming users (or short nicknames like 'miz' for 'Mizuri') or with dates/methods -> DELETE(120m)\n\
- Direct death wishes/suicide: 'kys', 'you should die', 'die idiot' -> SUSPICIOUS(30m)\n\
- Credible real-world threats with doxxing/stalking: 'i know where you live' -> DELETE(120m)\n\
- Doxxing, blackmail, extortion & leaking personal info ('I will spread all your information', 'leak your info/photos', 'expose your address', 'i will doxx you', 'солью инфу', 'деанон') -> DELETE(120m) under Major (Threats/Doxx). Must be an actual threat to leak private information or publishing PII. Does NOT apply to meta-discussion ('its a doxx').\n\
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
    pub timestamp_unix: i64,
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
    WarnOnly {
        reason: String,
        warning_text: String,
    },
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
struct OpenAiBatchModRequest<'a> {
    model: &'a str,
    input: Vec<&'a str>,
}

#[derive(Debug, Clone, Default)]
pub struct OpenAiScores {
    pub max_score: f64,
    pub severe_score: f64,
    pub top_cat: String,
    pub breakdown: String,
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

    pub fn record_message(&self, channel_id: u64, message_id: u64, author_id: u64, author_name: &str, content: &str, timestamp_unix: i64) {
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
            timestamp_unix,
        });
    }

    pub fn get_author_recent_context(&self, channel_id: u64, author_id: u64, current_timestamp: i64) -> Vec<String> {
        let history = self.chat_history.read().unwrap();
        if let Some(queue) = history.get(&channel_id) {
            queue
                .iter()
                .rev()
                .filter(|e| e.author_id == author_id && (current_timestamp - e.timestamp_unix).abs() <= 60)
                .take(2)
                .map(|e| e.content.clone())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect()
        } else {
            Vec::new()
        }
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

    pub fn contains_word(text: &str, target: &str) -> bool {
        let text_lower = text.to_lowercase();
        let target_lower = target.to_lowercase();
        let t_len = target_lower.len();

        if t_len == 0 {
            return false;
        }

        // For multi-word phrases or non-alphanumeric patterns (e.g. "burn alive", "spread your info"), check substring
        if target_lower.chars().any(|c| !c.is_alphanumeric()) {
            return text_lower.contains(&target_lower);
        }

        // For single alphanumeric words, find matches bounded by non-alphanumeric chars or boundaries
        for (i, _) in text_lower.match_indices(&target_lower) {
            let prev_ok = if i == 0 {
                true
            } else {
                let prev_char = text_lower[..i].chars().last();
                prev_char.map(|c| !c.is_alphanumeric()).unwrap_or(true)
            };

            let end_idx = i + t_len;
            let next_ok = if end_idx >= text_lower.len() {
                true
            } else {
                let next_char = text_lower[end_idx..].chars().next();
                next_char.map(|c| !c.is_alphanumeric()).unwrap_or(true)
            };

            if prev_ok && next_ok {
                return true;
            }
        }

        false
    }

    pub fn is_dox_meta_talk(text: &str) -> bool {
        let lower = text.to_lowercase();
        const DOXX_META_PHRASES: &[&str] = &[
            "basically a doxx", "basically a dox", "is a doxx", "is a dox",
            "thats a doxx", "that's a doxx", "thats a dox", "that's a dox",
            "it is a doxx", "it is a dox", "its a doxx", "it's a doxx",
            "it's basically a doxx", "its basically a doxx", "it's basically a dox", "its basically a dox",
            "got doxxed", "was doxxed", "got doxed", "was doxed",
            "stop doxxing", "stop doxing", "you can't doxx", "dont doxx", "don't doxx",
            "why doxx", "reported for doxx", "banned for doxx",
            "это деанон", "это слив", "его задеанонили", "его слили", "перестань деанонить",
            "не деанонь", "зачем деанонить"
        ];
        let has_meta = DOXX_META_PHRASES.iter().any(|&p| lower.contains(p));
        if !has_meta {
            return false;
        }

        // Must NOT contain an actual threat to leak info
        const REAL_DOX_THREAT: &[&str] = &[
            "i will doxx", "im gonna doxx", "i'll doxx", "doxx you", "leak your",
            "post your address", "know where you live", "солью твой", "найду где живешь"
        ];
        !REAL_DOX_THREAT.iter().any(|&t| lower.contains(t))
    }

    pub fn is_3rd_party_dev_or_game_critique(text: &str) -> bool {
        let lower = text.to_lowercase();
        let mentions_dev = lower.contains("dev")
            || lower.contains("devs")
            || lower.contains("developer")
            || lower.contains("developers")
            || lower.contains("разраб")
            || lower.contains("разрабы")
            || lower.contains("разработчик")
            || lower.contains("разработчики")
            || lower.contains("studio")
            || lower.contains("studios")
            || lower.contains("company")
            || lower.contains("valve")
            || lower.contains("ea ")
            || lower.contains("ubisoft")
            || lower.contains("mojang")
            || lower.contains("riot")
            || lower.contains("blizzard")
            || lower.contains("roblox devs")
            || lower.contains("game devs");

        if !mentions_dev {
            return false;
        }

        // Must NOT contain direct real-world death threats or doxxing against members
        const MEMBER_SEVERE_ATTACK: &[&str] = &[
            "kys", "kill yourself", "burn your house", "know where you live", "find where you live",
            "doxx you", "leak your", "солью твой", "знаю где ты", "сдохни", "вскройся"
        ];

        !MEMBER_SEVERE_ATTACK.iter().any(|&m| lower.contains(m))
    }

    pub fn is_drama_or_gossip(text: &str) -> bool {
        let lower = text.to_lowercase();
        const GOSSIP_MARKERS: &[&str] = &[
            "grape vine", "grapevine", "heard that", "heard from", "i heard",
            "rumor", "rumors", "rumours", "talking behind", "said about me", "paid someone",
            "слышал что", "слухи", "говорят что", "за спиной"
        ];
        let has_marker = GOSSIP_MARKERS.iter().any(|&m| lower.contains(m));
        if !has_marker {
            return false;
        }

        const SEVERE_VIOLATION_MARKERS: &[&str] = &[
            "kys", "kill yourself", "burn alive", "burn your house", "know where you live",
            "doxx you", "leak your", "солью твой", "сдохни", "вскройся"
        ];
        !SEVERE_VIOLATION_MARKERS.iter().any(|&s| lower.contains(s))
    }

    fn is_game_shield_evasion(content: &str) -> bool {
        if Self::is_3rd_party_dev_or_game_critique(content) {
            return false;
        }

        let lower = content.to_lowercase();
        let has_shield = GAME_SHIELD_PATTERNS.iter().any(|p| lower.contains(p))
            || lower.contains("minecraft")
            || lower.contains("roblox")
            || lower.contains("майнкрафт")
            || lower.contains("роблокс");
        if !has_shield {
            return false;
        }

        const GAME_SHIELD_THREATS: &[&str] = &[
            "kys", "kill yourself", "die", "suicide", "burn alive", "burned alive",
            "burn your house", "find your house", "know where you live", "find where you live",
            "hang yourself", "slit your", "shoot you", "shoot yourself",
            "сдохни", "вскройся", "повесься", "сожгу", "сожгу твой дом", "найду где живешь", "убью тебя", "убью"
        ];

        GAME_SHIELD_THREATS.iter().any(|&t| Self::contains_word(&lower, t))
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

        if Self::is_dox_meta_talk(trimmed_lower) {
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

    pub fn is_standalone_profanity(text: &str) -> bool {
        let lower = text.to_lowercase();
        let clean: String = lower.chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect();
        let words: Vec<&str> = clean.split_whitespace().collect();
        if words.is_empty() || words.len() > 3 {
            return false;
        }
        let phrase = words.join(" ");
        if (phrase.starts_with("fuck you")
            || phrase.starts_with("fuck u")
            || phrase.starts_with("fuck tou")
            || phrase.starts_with("fuck off")
            || phrase.starts_with("stfu")
            || phrase.starts_with("screw you")
            || phrase.starts_with("shut up")
            || phrase.starts_with("пошел нахуй")
            || phrase.starts_with("иди нахуй"))
            && words.len() <= 3
        {
            return true;
        }
        matches!(
            phrase.as_str(),
            "f you"
                | "stfu"
                | "stfu bum"
                | "stfu noob"
                | "stfu idiot"
                | "stfu bro"
                | "screw u"
                | "пошел нахер"
                | "иди нахер"
                | "пошел на хер"
                | "иди в баню"
                | "отвали"
                | "завались"
                | "закройся"
        )
    }

    pub fn is_theatrical_hyperbole(text: &str) -> bool {
        let lower = text.to_lowercase();
        const HYPERBOLE_WORDS: &[&str] = &[
            "eviscerate", "obliterate", "annihilate", "demolish", "decimate",
            "disintegrate", "atomize", "vaporize", "turn into dust",
            "fold you like a lawn chair", "tear you to pieces", "rip you apart",
            "выпотрошу", "расщеплю", "сотру в порошок", "на атомы", "порву как грелку",
            "разорву на куски", "размажу по стенке"
        ];
        let has_hyperbole = HYPERBOLE_WORDS.iter().any(|&w| lower.contains(w));
        if !has_hyperbole {
            return false;
        }
        // Only safe if there is NO real-world stalking/doxxing/severe violence keywords
        const REAL_WORLD_MALICE: &[&str] = &[
            "doxx", "leak", "address", "where you live", "find your house", "ip",
            "burn alive", "burned alive", "cancer", "kys", "hang yourself", "suicide",
            "сожгу", "повесься", "вскройся", "деанон", "сват"
        ];
        !REAL_WORLD_MALICE.iter().any(|&m| lower.contains(m))
    }

    pub fn is_gaming_pvp_callout(text: &str) -> bool {
        let clean = text
            .trim()
            .trim_matches(|c: char| !c.is_alphanumeric() && !c.is_whitespace())
            .to_lowercase();

        let word_count = clean.split_whitespace().count();
        if word_count == 0 || word_count > 8 {
            return false;
        }

        const CALLOUTS: &[&str] = &[
            "kill him", "kill her", "kill them", "kill it", "kill this guy", "kill that guy",
            "go kill him", "just kill him", "please kill him", "kill him bro", "kill him now",
            "kill him first", "kill them all", "shoot him", "shoot her", "shoot them",
            "get him", "get her", "get them", "focus him", "focus her", "focus them",
            "drop him", "take him down", "finish him", "blow him up",
            "убей его", "убей ее", "убей их", "убейте его", "убейте их", "убей этого",
            "мочи его", "завали его", "добивай его", "добивай", "добивайте", "го убей его",
            "да убей его", "бей его"
        ];

        let is_match = CALLOUTS.iter().any(|&c| {
            clean == c
                || clean.starts_with(&format!("{} ", c))
                || clean.ends_with(&format!(" {}", c))
                || clean.contains(&format!(" {} ", c))
        });

        if is_match {
            const FORBIDDEN: &[&str] = &[
                "kys", "kill yourself", "die", "suicide", "irl", "in real life", "house", "address",
                "where you live", "ip", "doxx", "leak", "сдохни", "в реале", "вскройся", "повесься"
            ];
            return !FORBIDDEN.iter().any(|&f| clean.contains(f));
        }

        false
    }

    pub fn is_shut_up_or_silencing(text: &str) -> bool {
        let clean = text
            .trim()
            .trim_matches(|c: char| !c.is_alphanumeric() && !c.is_whitespace() && c != '?')
            .to_lowercase();

        let word_count = clean.split_whitespace().count();
        if word_count == 0 || word_count > 15 {
            return false;
        }

        // Self-referential statements (e.g. "ok i will shut up", "я заткнусь")
        const SELF_REFERENTIAL: &[&str] = &[
            "i will shut up", "ill shut up", "i'll shut up", "im gonna shut up",
            "i should shut up", "i'm shutting up", "im shutting up", "let me shut up",
            "я заткнусь", "я помолчу", "я замолчу", "я лучше заткнусь", "я лучше помолчу"
        ];
        if SELF_REFERENTIAL.iter().any(|&s| clean.contains(s)) {
            return false;
        }

        // Question asking why or what about shutting up (e.g. "why should i shut up?", "зачем мне затыкаться?")
        if clean.contains('?')
            && (clean.contains("why ")
                || clean.contains("did ")
                || clean.contains("how ")
                || clean.contains("почему ")
                || clean.contains("зачем "))
        {
            return false;
        }

        // Severe forbidden keywords (slurs/doxx/kys should NOT be excused as shut up)
        const FORBIDDEN: &[&str] = &[
            "kys", "kill", "die", "suicide", "doxx", "leak", "address", "ip",
            "сдохни", "убью", "сожгу", "вскройся", "деанон"
        ];
        if FORBIDDEN.iter().any(|&f| clean.contains(f)) {
            return false;
        }

        const SILENCING_PATTERNS: &[&str] = &[
            // English
            "shut up", "shutup", "shut ur mouth", "shut your mouth", "shut the fuck up",
            "shut the hell up", "shut it", "stfu", "stfu already", "shut up already",
            "shut ur face", "shut your face", "pipe down", "zip it", "be quiet", "quiet down",
            // Russian
            "заткнись", "заткнитесь", "закрой рот", "закрой свой рот", "заткни рот",
            "заткни пасть", "закрой пасть", "завали пасть", "завали рот",
            "завали ебало", "завали хлебало", "завались", "закройся", "рот закрой",
            "рот офф", "помолчи", "помолчите", "замолчи", "замолчите", "замолкни",
            "замолкните", "не вякай", "не отсвечивай", "хватит вякать", "хватит пиздеть",
            "схлопнись"
        ];

        SILENCING_PATTERNS.iter().any(|&p| {
            clean == p
                || clean.starts_with(&format!("{} ", p))
                || clean.ends_with(&format!(" {}", p))
                || clean.contains(&format!(" {} ", p))
                || (p.contains(' ') && clean.contains(p))
                || (!p.contains(' ')
                    && clean.split_whitespace().any(|w| {
                        let w_clean = w.trim_matches(|c: char| !c.is_alphanumeric());
                        w_clean == p || (p.len() >= 6 && w_clean.starts_with(p))
                    }))
        })
    }

    pub fn contains_slur(text: &str) -> bool {
        let lower = text.to_lowercase();
        // Check exact words split by non-alphanumeric characters
        for word in lower.split(|c: char| !c.is_alphanumeric()) {
            if word.is_empty() {
                continue;
            }
            if SLUR_WORDS.iter().any(|&s| word == s) {
                return true;
            }
        }
        // Also check with punctuation removed (e.g. "n.g.a" or "*nga*")
        let stripped = lower.replace(['.', '-', '_', '*', '`', '~', '/', '\\'], "");
        for word in stripped.split_whitespace() {
            if SLUR_WORDS.iter().any(|&s| word == s) {
                return true;
            }
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
        is_pvp_callout: bool,
        author_combined_thought: Option<&str>,
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
        if let Some(comb) = author_combined_thought {
            let short_comb = Self::safe_truncate(comb, 180);
            p.push_str(&format!("Author's Recent Combined Context: \"{}\"\n", short_comb.trim().replace('\n', " // ")));
        }
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
        if is_pvp_callout {
            p.push_str("ℹ️ PVP CALLOUT NOTE: Message is a short tactical PvP/gaming callout ('kill him/them', 'shoot him', 'убей его') referring to an in-game opponent or character. It is NOT a real-world death threat. You must return VERDICT: ALLOW (RULE: None, MUTE: 0).\n");
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

        // Fetch author's recent messages in this channel within 60 seconds
        let author_past_msgs = self.get_author_recent_context(ctx.channel_id, ctx.author_id, ctx.timestamp_unix);
        let has_author_context = !author_past_msgs.is_empty();
        let combined_text = if has_author_context {
            format!("{}\n{}", author_past_msgs.join("\n"), trimmed)
        } else {
            trimmed.to_string()
        };

        let inputs: Vec<&str> = if has_author_context {
            vec![trimmed, &combined_text]
        } else {
            vec![trimmed]
        };

        let scores_list = match self.call_openai_moderation(openai_key, &inputs).await {
            Ok(scores) => scores,
            Err(_) => vec![OpenAiScores::default(); inputs.len()],
        };

        let single_scores = scores_list.get(0).cloned().unwrap_or_default();
        let combined_scores = scores_list.get(1).cloned();

        let mut max_score = single_scores.max_score;
        let mut severe_score = single_scores.severe_score;
        let mut top_cat = single_scores.top_cat;
        let mut cat_breakdown = single_scores.breakdown;

        let history = self.get_context_snapshot(ctx.channel_id);
        let lower = trimmed.to_lowercase();
        let combined_lower = combined_text.to_lowercase();
        let has_severe_harm_keyword = SEVERE_HARM_KEYWORDS.iter().any(|k| Self::contains_word(&lower, k) || Self::contains_word(&combined_lower, k));
        let has_provocative_bait = PROVOCATIVE_BAIT_KEYWORDS.iter().any(|k| Self::contains_word(&lower, k));
        let has_dox_threat = DOX_AND_EXTORTION_KEYWORDS.iter().any(|k| Self::contains_word(&lower, k) || Self::contains_word(&combined_lower, k));
        let has_slur = Self::contains_slur(trimmed) || Self::contains_slur(&combined_text);
        let is_game_shield = Self::is_game_shield_evasion(trimmed) || Self::is_game_shield_evasion(&combined_text);
        let is_pvp_callout = Self::is_gaming_pvp_callout(trimmed);
        let is_shut_up = Self::is_shut_up_or_silencing(trimmed);

        if let Some(ref cs) = combined_scores {
            if cs.severe_score > severe_score + 0.10 || cs.max_score > max_score + 0.15 {
                println!(
                    "   ⚡ [SPLIT THREAT ESCALATION] Combined author thought scores higher! (Single: {:.2} -> Combined: {:.2} [{}] | Severe: {:.2} -> {:.2})",
                    max_score, cs.max_score, cs.top_cat, severe_score, cs.severe_score
                );
                max_score = cs.max_score;
                severe_score = cs.severe_score;
                top_cat = cs.top_cat.clone();
                cat_breakdown = cs.breakdown.clone();
            } else if max_score > 0.80 && is_pvp_callout && cs.severe_score < 0.15 {
                println!(
                    "   🎮 [AUTHOR CONTEXT CLARIFICATION] In-game context confirmed by combined author messages! Severe score: {:.2}",
                    cs.severe_score
                );
            }
        }

        let is_violent_category = matches!(
            top_cat.as_str(),
            "violence" | "violence/graphic" | "self-harm" | "self-harm/intent" | "self-harm/instructions" | "hate" | "hate/threatening" | "harassment/threatening"
        );

        if is_game_shield {
            println!("   🕵️ [GAME SHIELD DETECTED] Potential veiled toxicity/threat hiding behind game titles!");
        }
        if has_dox_threat {
            println!("   🚨 [DOX/EXTORTION THREAT DETECTED] Threat to leak personal information/doxx in message!");
        }
        if is_pvp_callout {
            println!("   🎮 [PVP CALLOUT DETECTED] Tactical in-game callout ('{}')", trimmed);
        }
        if is_shut_up {
            println!("   💬 [SHUT UP / SILENCING DETECTED] Telling others to shut up ('{}')", trimmed);
        }
        if has_slur {
            println!("   🚨 [SLUR DETECTED] Racial/hate slur or masked evasion detected in message!");
        }

        println!(
            "\n🔍 [AI SCANNER] Channel: #{} | Author: @{} ({}) | Text: \"{}\"",
            ctx.channel_name.as_deref().unwrap_or("unknown"),
            ctx.author_name,
            ctx.author_id,
            trimmed
        );
        if has_author_context {
            println!(
                "   👥 [AUTHOR CONTEXT BATCH] {} prior msgs | Combined: \"{}\"",
                author_past_msgs.len(),
                Self::safe_truncate(&combined_text.replace('\n', " // "), 80)
            );
        }
        println!(
            "   📊 [OPENAI] Max Score: {:.2} ({}) | Severe: {:.2} | Details: [{}]",
            max_score,
            if top_cat.is_empty() { "none" } else { &top_cat },
            severe_score,
            cat_breakdown
        );

        let is_directed = Self::is_directed_or_targeted(trimmed, ctx.reply_to.is_some(), ctx.mentions, &history);
        let is_meta = Self::is_meta_or_quote(trimmed);
        let is_abstract_placeholder = Self::is_abstract_placeholder(trimmed);

        if is_abstract_placeholder && !is_directed {
            println!("   ↳ [META-PLACEHOLDER] Abstract hypothetical example detected with (someone)/(user) -> ALLOW (0 tokens spent)");
            return ModerationVerdict::Allow;
        }

        if is_shut_up && !has_slur && !has_dox_threat && !has_severe_harm_keyword && !is_game_shield && !is_meta {
            let is_russian = trimmed.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));
            let warning_text = if is_russian {
                "пожалуйста, будьте спокойнее и общайтесь уважительно. Не стоит затыкать других участников!".to_string()
            } else {
                "please stay calm and keep the chat civil. Let everyone speak without telling them to shut up!".to_string()
            };
            println!("   💬 [SHUT UP ACTION] Issuing WarnOnly in chat (NO MUTE, no delete)");
            return ModerationVerdict::WarnOnly {
                reason: "Telling others to shut up / silencing users".to_string(),
                warning_text,
            };
        }

        // 1A. Clear clean content -> Instant ALLOW (only if no severe harm keywords, no provocative bait, no dox threats, no slurs, no game shield evasion, and not shut up)
        if max_score < OPENAI_SAFE_THRESHOLD && !has_severe_harm_keyword && !has_provocative_bait && !has_dox_threat && !has_slur && !is_game_shield && !is_shut_up {
            println!("   ↳ [SAFE] Score {:.2} < {:.2} safe threshold -> ALLOW (0 tokens spent)", max_score, OPENAI_SAFE_THRESHOLD);
            return ModerationVerdict::Allow;
        }

        // ── Smart Dynamic Model Routing: 120B Deep Reasoning for Drama/Hardcore/Threats/Slurs vs Fast Guard for Banter ──
        let is_hardcore_or_drama = severe_score > 0.65
            || max_score > 0.78
            || (ctx.reply_to.is_some() && max_score > 0.55)
            || has_severe_harm_keyword
            || has_dox_threat
            || has_slur
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
                let user_prompt = self.format_compact_prompt(
                    ctx,
                    &history,
                    max_score,
                    &top_cat,
                    is_game_shield,
                    is_meta,
                    is_pvp_callout,
                    if has_author_context { Some(combined_text.as_str()) } else { None },
                );

                match self.call_groq_failover(preferred_model, SERVER_RULES_SYSTEM_PROMPT, &user_prompt).await {
                    Ok((decision, model_used, elapsed_ms)) => {
                        println!(
                            "   ⚡ [AI RESPONSE] Model: {} (took {}ms) | Verdict: {} | Rule: {} | Mute: {}m | Reason: \"{}\"",
                            model_used, elapsed_ms, decision.verdict, decision.rule, decision.mute_minutes, decision.reason
                        );

                        if decision.verdict.contains("ALLOW") {
                            if has_slur && !is_meta {
                                println!("   🚨 [SLUR GUARD] Overriding LLM ALLOW for detected slur/evasion in message ('{}') -> DELETE(1440m)", trimmed);
                                return ModerationVerdict::DeleteConfirmed {
                                    reason: format!("Racial/hate slur or masked evasion detected in message: \"{}\"", trimmed),
                                    score: if max_score > 0.5 { max_score } else { 0.99 },
                                    category: "hate".to_string(),
                                    model_used: format!("Slur Guard ({})", model_used),
                                    rule_violated: "Crit (Slurs)".to_string(),
                                    mute_minutes: 1440,
                                };
                            }
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
                            if has_dox_threat && !is_meta {
                                println!("   🚨 [DOX GUARD] Overriding LLM ALLOW for direct doxxing/extortion threat ('{}') -> DELETE(120m)", trimmed);
                                return ModerationVerdict::DeleteConfirmed {
                                    reason: format!("Doxxing, blackmail, or personal info leak threat detected: \"{}\"", trimmed),
                                    score: if max_score > 0.5 { max_score } else { 0.95 },
                                    category: "harassment/threatening".to_string(),
                                    model_used: format!("Dox Guard ({})", model_used),
                                    rule_violated: "Major (Threats/Doxx)".to_string(),
                                    mute_minutes: 120,
                                };
                            }
                            println!("   ✅ [BANTER PASS] LLM verified message as safe gaming hyperbole -> ALLOW");
                            return ModerationVerdict::Allow;
                        } else if is_meta && !is_directed {
                            println!("   🛡️ [META GUARD] Overriding LLM {} on undirected meta-discussion / quote to ALLOW.", decision.verdict);
                            return ModerationVerdict::Allow;
                        } else if is_shut_up && !has_slur && !has_dox_threat && !has_severe_harm_keyword && !is_game_shield {
                            let is_russian = trimmed.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));
                            let warning_text = if is_russian {
                                "пожалуйста, будьте спокойнее и общайтесь уважительно. Не стоит затыкать других участников!".to_string()
                            } else {
                                "please stay calm and keep the chat civil. Let everyone speak without telling them to shut up!".to_string()
                            };
                            println!("   💬 [SHUT UP GUARD] Overriding LLM {} on silencing directive ('{}') -> WarnOnly in chat (NO MUTE)", decision.verdict, trimmed);
                            return ModerationVerdict::WarnOnly {
                                reason: "Telling others to shut up / silencing users".to_string(),
                                warning_text,
                            };
                        } else if is_pvp_callout && !has_slur && !is_game_shield && !has_dox_threat {
                            println!("   🎮 [PVP CALLOUT GUARD] Overriding LLM {} on tactical in-game callout ('{}') to ALLOW.", decision.verdict, trimmed);
                            return ModerationVerdict::Allow;
                        } else if Self::is_standalone_profanity(trimmed) && !has_severe_harm_keyword && !has_dox_threat && !has_slur && !is_game_shield {
                            println!("   🛡️ [BANTER GUARD] Overriding LLM {} on standalone profanity ('{}') to ALLOW.", decision.verdict, trimmed);
                            return ModerationVerdict::Allow;
                        } else if Self::is_theatrical_hyperbole(trimmed) && !has_dox_threat && !has_slur && !is_game_shield {
                            println!("   🎭 [POST-IRONY GUARD] Overriding LLM {} on theatrical hyperbole ('{}') to ALLOW.", decision.verdict, trimmed);
                            return ModerationVerdict::Allow;
                        } else if Self::is_dox_meta_talk(trimmed) && !has_slur && !has_dox_threat {
                            println!("   🛡️ [DOXX META GUARD] Overriding LLM {} on doxx meta-talk/observation ('{}') to ALLOW.", decision.verdict, trimmed);
                            return ModerationVerdict::Allow;
                        } else if Self::is_3rd_party_dev_or_game_critique(trimmed) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                            println!("   🛡️ [DEV CRITIQUE GUARD] Overriding LLM {} on developer/game critique ('{}') to ALLOW.", decision.verdict, trimmed);
                            return ModerationVerdict::Allow;
                        } else if (Self::is_drama_or_gossip(trimmed) || decision.rule.to_lowercase().contains("drama") || decision.reason.to_lowercase().contains("drama incitement")) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                            println!("   🛡️ [DRAMA / GOSSIP GUARD] Overriding LLM {} on gossip / drama rumor ('{}') to ALLOW.", decision.verdict, trimmed);
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
                        if is_shut_up && !has_slur && !has_dox_threat && !has_severe_harm_keyword && !is_game_shield {
                            let is_russian = trimmed.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));
                            let warning_text = if is_russian {
                                "пожалуйста, будьте спокойнее и общайтесь уважительно. Не стоит затыкать других участников!".to_string()
                            } else {
                                "please stay calm and keep the chat civil. Let everyone speak without telling them to shut up!".to_string()
                            };
                            println!("   💬 [SHUT UP GUARD] Failover fallback: silencing directive ('{}') -> WarnOnly in chat (NO MUTE)", trimmed);
                            return ModerationVerdict::WarnOnly {
                                reason: "Telling others to shut up / silencing users".to_string(),
                                warning_text,
                            };
                        }
                        if is_pvp_callout && !has_slur && !is_game_shield && !has_dox_threat {
                            println!("   🎮 [PVP CALLOUT GUARD] Failover fallback: tactical in-game callout ('{}') -> ALLOW.", trimmed);
                            return ModerationVerdict::Allow;
                        }
                        if Self::is_standalone_profanity(trimmed) && !has_severe_harm_keyword && !has_dox_threat && !has_slur && !is_game_shield {
                            println!("   🛡️ [BANTER GUARD] Failover fallback: standalone profanity ('{}') -> ALLOW.", trimmed);
                            return ModerationVerdict::Allow;
                        }
                        if Self::is_theatrical_hyperbole(trimmed) && !has_dox_threat && !has_slur && !is_game_shield {
                            println!("   🎭 [POST-IRONY GUARD] Failover fallback: theatrical hyperbole ('{}') -> ALLOW.", trimmed);
                            return ModerationVerdict::Allow;
                        }
                        if Self::is_dox_meta_talk(trimmed) && !has_slur && !has_dox_threat {
                            println!("   🛡️ [DOXX META GUARD] Failover fallback: doxx meta-talk -> ALLOW.");
                            return ModerationVerdict::Allow;
                        }
                        if Self::is_3rd_party_dev_or_game_critique(trimmed) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                            println!("   🛡️ [DEV CRITIQUE GUARD] Failover fallback: dev critique -> ALLOW.");
                            return ModerationVerdict::Allow;
                        }
                        if (Self::is_drama_or_gossip(trimmed) || top_cat.contains("drama")) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                            println!("   🛡️ [DRAMA / GOSSIP GUARD] Failover fallback: drama rumor -> ALLOW.");
                            return ModerationVerdict::Allow;
                        }
                        if is_meta && !is_directed {
                            println!("   🛡️ [META GUARD] Failover fallback: meta quote -> ALLOW.");
                            return ModerationVerdict::Allow;
                        }
                        if (top_cat == "hate" || top_cat == "hate/threatening" || top_cat == "harassment/threatening") && max_score > 0.80 && !is_meta {
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

            if is_shut_up && !has_slur && !has_dox_threat && !has_severe_harm_keyword && !is_game_shield {
                let is_russian = trimmed.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));
                let warning_text = if is_russian {
                    "пожалуйста, будьте спокойнее и общайтесь уважительно. Не стоит затыкать других участников!".to_string()
                } else {
                    "please stay calm and keep the chat civil. Let everyone speak without telling them to shut up!".to_string()
                };
                println!("   💬 [SHUT UP GUARD] High score fallback: silencing directive ('{}') -> WarnOnly in chat (NO MUTE)", trimmed);
                return ModerationVerdict::WarnOnly {
                    reason: "Telling others to shut up / silencing users".to_string(),
                    warning_text,
                };
            }

            if is_pvp_callout && !has_slur && !is_game_shield && !has_dox_threat {
                println!("   🎮 [PVP CALLOUT GUARD] High score fallback: tactical in-game callout ('{}') -> ALLOW.", trimmed);
                return ModerationVerdict::Allow;
            }
            if Self::is_standalone_profanity(trimmed) && !has_severe_harm_keyword && !has_dox_threat && !has_slur && !is_game_shield {
                println!("   🛡️ [BANTER GUARD] High score fallback: standalone profanity ('{}') -> ALLOW.", trimmed);
                return ModerationVerdict::Allow;
            }
            if Self::is_theatrical_hyperbole(trimmed) && !has_dox_threat && !has_slur && !is_game_shield {
                println!("   🎭 [POST-IRONY GUARD] High score fallback: theatrical hyperbole ('{}') -> ALLOW.", trimmed);
                return ModerationVerdict::Allow;
            }
            if Self::is_dox_meta_talk(trimmed) && !has_slur && !has_dox_threat {
                println!("   🛡️ [DOXX META GUARD] High score fallback: doxx meta-talk -> ALLOW.");
                return ModerationVerdict::Allow;
            }
            if Self::is_3rd_party_dev_or_game_critique(trimmed) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                println!("   🛡️ [DEV CRITIQUE GUARD] High score fallback: dev critique -> ALLOW.");
                return ModerationVerdict::Allow;
            }
            if (Self::is_drama_or_gossip(trimmed) || top_cat.contains("drama")) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                println!("   🛡️ [DRAMA / GOSSIP GUARD] High score fallback: drama rumor -> ALLOW.");
                return ModerationVerdict::Allow;
            }
            if is_meta && !is_directed {
                println!("   🛡️ [META GUARD] High score fallback: meta quote -> ALLOW.");
                return ModerationVerdict::Allow;
            }

            if (top_cat == "hate" || top_cat == "hate/threatening" || top_cat == "harassment/threatening") && max_score > 0.80 && !is_meta {
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
        if Self::is_3rd_party_dev_or_game_critique(trimmed) && !has_severe_harm_keyword && !has_dox_threat && !has_slur && max_score < 0.75 {
            println!("   ↳ [DEV CRITIQUE PRE-FILTER] 3rd-party dev / game critique (score {:.2}) -> ALLOW (0 tokens spent)", max_score);
            return ModerationVerdict::Allow;
        }
        if Self::is_dox_meta_talk(trimmed) && !has_dox_threat && !has_slur && max_score < 0.75 {
            println!("   ↳ [DOXX META PRE-FILTER] Meta-talk about doxxing (score {:.2}) -> ALLOW (0 tokens spent)", max_score);
            return ModerationVerdict::Allow;
        }
        if Self::is_drama_or_gossip(trimmed) && !has_severe_harm_keyword && !has_dox_threat && !has_slur && max_score < 0.75 {
            println!("   ↳ [DRAMA PRE-FILTER] Chat gossip / drama rumor (score {:.2}) -> ALLOW (0 tokens spent)", max_score);
            return ModerationVerdict::Allow;
        }

        // ONLY bypass if it's general non-violent gaming frustration (e.g. "fuck this lag")
        if !is_directed && !is_violent_category && !has_severe_harm_keyword && !has_provocative_bait && !has_dox_threat && !has_slur && !is_game_shield && max_score < 0.60 {
            println!("   ↳ [PRE-FILTER] General gaming frustration / non-directed (score {:.2}) -> ALLOW (0 tokens spent)", max_score);
            return ModerationVerdict::Allow;
        }

        if self.groq_keys.is_empty() {
            if has_dox_threat && !is_meta {
                return ModerationVerdict::DeleteConfirmed {
                    reason: format!("Doxxing, blackmail, or personal info leak threat detected: \"{}\"", trimmed),
                    score: 0.95,
                    category: "harassment/threatening".to_string(),
                    model_used: "Local Dox Guard".to_string(),
                    rule_violated: "Major (Threats/Doxx)".to_string(),
                    mute_minutes: 120,
                };
            }
            if has_severe_harm_keyword && !is_pvp_callout && !is_meta {
                return ModerationVerdict::FlagSuspicious {
                    reason: format!("Severe harm keyword detected in message: \"{}\"", trimmed),
                    score: if max_score > 0.5 { max_score } else { 0.85 },
                    category: if top_cat.is_empty() { "violence".to_string() } else { top_cat },
                    model_used: "Local Severe Keyword Guard".to_string(),
                    rule_violated: "Major (Threats/Harm)".to_string(),
                    mute_minutes: 60,
                };
            }
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
        let user_prompt = self.format_compact_prompt(
            ctx,
            &history,
            max_score,
            &top_cat,
            is_game_shield,
            is_meta,
            is_pvp_callout,
            if has_author_context { Some(combined_text.as_str()) } else { None },
        );

        match self.call_groq_failover(preferred_model, SERVER_RULES_SYSTEM_PROMPT, &user_prompt).await {
            Ok((decision, model_used, elapsed_ms)) => {
                println!(
                    "   ⚡ [AI RESPONSE] Model: {} (took {}ms) | Verdict: {} | Rule: {} | Mute: {}m | Reason: \"{}\"",
                    model_used, elapsed_ms, decision.verdict, decision.rule, decision.mute_minutes, decision.reason
                );
                if decision.verdict.contains("DELETE") || (has_slur && !is_meta) {
                    if is_meta && !is_directed {
                        println!("   🛡️ [META GUARD] Overriding LLM {} on undirected meta-discussion / quote to ALLOW.", decision.verdict);
                        return ModerationVerdict::Allow;
                    }
                    if is_shut_up && !has_slur && !has_dox_threat && !has_severe_harm_keyword && !is_game_shield {
                        let is_russian = trimmed.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));
                        let warning_text = if is_russian {
                            "пожалуйста, будьте спокойнее и общайтесь уважительно. Не стоит затыкать других участников!".to_string()
                        } else {
                            "please stay calm and keep the chat civil. Let everyone speak without telling them to shut up!".to_string()
                        };
                        println!("   💬 [SHUT UP GUARD] Overriding LLM DELETE on silencing directive ('{}') -> WarnOnly in chat (NO MUTE)", trimmed);
                        return ModerationVerdict::WarnOnly {
                            reason: "Telling others to shut up / silencing users".to_string(),
                            warning_text,
                        };
                    }
                    if is_pvp_callout && !has_slur && !is_game_shield && !has_dox_threat {
                        println!("   🎮 [PVP CALLOUT GUARD] Overriding LLM DELETE on tactical in-game callout ('{}') to ALLOW.", trimmed);
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_standalone_profanity(trimmed) && !has_severe_harm_keyword && !has_dox_threat && !has_slur && !is_game_shield {
                        println!("   🛡️ [BANTER GUARD] Overriding LLM DELETE on standalone profanity ('{}') to ALLOW.", trimmed);
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_theatrical_hyperbole(trimmed) && !has_dox_threat && !has_slur && !is_game_shield {
                        println!("   🎭 [POST-IRONY GUARD] Overriding LLM DELETE on theatrical hyperbole ('{}') to ALLOW.", trimmed);
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_dox_meta_talk(trimmed) && !has_slur && !has_dox_threat {
                        println!("   🛡️ [DOXX META GUARD] Overriding LLM DELETE on doxx meta-talk/observation to ALLOW.");
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_3rd_party_dev_or_game_critique(trimmed) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                        println!("   🛡️ [DEV CRITIQUE GUARD] Overriding LLM DELETE on developer/game critique to ALLOW.");
                        return ModerationVerdict::Allow;
                    }
                    if (Self::is_drama_or_gossip(trimmed) || decision.rule.to_lowercase().contains("drama") || decision.reason.to_lowercase().contains("drama incitement")) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                        println!("   🛡️ [DRAMA / GOSSIP GUARD] Overriding LLM DELETE on gossip / drama rumor to ALLOW.");
                        return ModerationVerdict::Allow;
                    }
                    let effective_mute = if has_slur { 1440 } else { decision.mute_minutes };
                    let effective_rule = if has_slur { "Crit (Slurs)".to_string() } else { decision.rule };
                    let effective_reason = if has_slur { format!("Racial/hate slur or masked evasion detected in message: \"{}\"", trimmed) } else { decision.reason };
                    println!("   🚨 [AI VERDICT: DELETE] Confirmed severe violation in grey zone! Mute: {}m (Rule: {})", effective_mute, effective_rule);
                    let model_label = if model_used.contains("120b") {
                        format!("OpenAI + {} (120B Deep Drama Arbiter)", model_used)
                    } else if model_used.contains("20b") {
                        format!("OpenAI + {} (20B Safety Arbiter)", model_used)
                    } else {
                        format!("OpenAI + {} Guard", model_used)
                    };
                    return ModerationVerdict::DeleteConfirmed {
                        reason: effective_reason,
                        score: max_score,
                        category: if has_slur { "hate".to_string() } else { top_cat },
                        model_used: model_label,
                        rule_violated: effective_rule,
                        mute_minutes: effective_mute,
                    };
                } else if decision.verdict.contains("SUSPICIOUS") {
                    if is_meta && !is_directed {
                        println!("   🛡️ [META GUARD] Overriding LLM {} on undirected meta-discussion / quote to ALLOW.", decision.verdict);
                        return ModerationVerdict::Allow;
                    }
                    if is_shut_up && !has_slur && !has_dox_threat && !has_severe_harm_keyword && !is_game_shield {
                        let is_russian = trimmed.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));
                        let warning_text = if is_russian {
                            "пожалуйста, будьте спокойнее и общайтесь уважительно. Не стоит затыкать других участников!".to_string()
                        } else {
                            "please stay calm and keep the chat civil. Let everyone speak without telling them to shut up!".to_string()
                        };
                        println!("   💬 [SHUT UP GUARD] Overriding LLM SUSPICIOUS on silencing directive ('{}') -> WarnOnly in chat (NO MUTE)", trimmed);
                        return ModerationVerdict::WarnOnly {
                            reason: "Telling others to shut up / silencing users".to_string(),
                            warning_text,
                        };
                    }
                    if is_pvp_callout && !has_slur && !is_game_shield && !has_dox_threat {
                        println!("   🎮 [PVP CALLOUT GUARD] Overriding LLM SUSPICIOUS on tactical in-game callout ('{}') to ALLOW.", trimmed);
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_standalone_profanity(trimmed) && !has_severe_harm_keyword && !has_dox_threat && !has_slur && !is_game_shield {
                        println!("   🛡️ [BANTER GUARD] Overriding LLM SUSPICIOUS on standalone profanity ('{}') to ALLOW.", trimmed);
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_theatrical_hyperbole(trimmed) && !has_dox_threat && !has_slur && !is_game_shield {
                        println!("   🎭 [POST-IRONY GUARD] Overriding LLM SUSPICIOUS on theatrical hyperbole ('{}') to ALLOW.", trimmed);
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_dox_meta_talk(trimmed) && !has_slur && !has_dox_threat {
                        println!("   🛡️ [DOXX META GUARD] Overriding LLM SUSPICIOUS on doxx meta-talk/observation to ALLOW.");
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_3rd_party_dev_or_game_critique(trimmed) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                        println!("   🛡️ [DEV CRITIQUE GUARD] Overriding LLM SUSPICIOUS on developer/game critique to ALLOW.");
                        return ModerationVerdict::Allow;
                    }
                    if (Self::is_drama_or_gossip(trimmed) || decision.rule.to_lowercase().contains("drama") || decision.reason.to_lowercase().contains("drama incitement")) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                        println!("   🛡️ [DRAMA / GOSSIP GUARD] Overriding LLM SUSPICIOUS on gossip / drama rumor to ALLOW.");
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
                    if has_slur && !is_meta {
                        println!("   🚨 [SLUR GUARD] Overriding LLM ALLOW for detected slur in grey zone ('{}') -> DELETE(1440m)", trimmed);
                        return ModerationVerdict::DeleteConfirmed {
                            reason: format!("Racial/hate slur or masked evasion detected in message: \"{}\"", trimmed),
                            score: if max_score > 0.5 { max_score } else { 0.99 },
                            category: "hate".to_string(),
                            model_used: format!("Slur Guard ({})", model_used),
                            rule_violated: "Crit (Slurs)".to_string(),
                            mute_minutes: 1440,
                        };
                    }
                    if has_dox_threat && !is_meta {
                        println!("   🚨 [DOX GUARD] Overriding LLM ALLOW for direct doxxing/extortion threat in grey zone ('{}') -> DELETE(120m)", trimmed);
                        return ModerationVerdict::DeleteConfirmed {
                            reason: format!("Doxxing, blackmail, or personal info leak threat detected: \"{}\"", trimmed),
                            score: if max_score > 0.5 { max_score } else { 0.95 },
                            category: "harassment/threatening".to_string(),
                            model_used: format!("Dox Guard ({})", model_used),
                            rule_violated: "Major (Threats/Doxx)".to_string(),
                            mute_minutes: 120,
                        };
                    }
                    println!("   ✅ [ALLOW] Grey-zone message allowed by LLM.");
                    ModerationVerdict::Allow
                }
            }
            Err(e) => {
                eprintln!("   ❌ [GROQ ERROR] Grey-zone call failed: {}. Checking Slur Guard.", e);
                if is_shut_up && !has_slur && !has_dox_threat && !has_severe_harm_keyword && !is_game_shield {
                    let is_russian = trimmed.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));
                    let warning_text = if is_russian {
                        "пожалуйста, будьте спокойнее и общайтесь уважительно. Не стоит затыкать других участников!".to_string()
                    } else {
                        "please stay calm and keep the chat civil. Let everyone speak without telling them to shut up!".to_string()
                    };
                    println!("   💬 [SHUT UP GUARD] Failover fallback on silencing directive ('{}') -> WarnOnly in chat (NO MUTE)", trimmed);
                    return ModerationVerdict::WarnOnly {
                        reason: "Telling others to shut up / silencing users".to_string(),
                        warning_text,
                    };
                }
                if is_pvp_callout && !has_slur && !is_game_shield && !has_dox_threat {
                    println!("   🎮 [PVP CALLOUT GUARD] Grey-zone failover fallback on tactical in-game callout ('{}') -> ALLOW.", trimmed);
                    return ModerationVerdict::Allow;
                }
                if Self::is_dox_meta_talk(trimmed) && !has_slur && !has_dox_threat {
                    println!("   🛡️ [DOXX META GUARD] Grey-zone failover fallback on doxx meta-talk -> ALLOW.");
                    return ModerationVerdict::Allow;
                }
                if Self::is_3rd_party_dev_or_game_critique(trimmed) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                    println!("   🛡️ [DEV CRITIQUE GUARD] Grey-zone failover fallback on dev critique -> ALLOW.");
                    return ModerationVerdict::Allow;
                }
                if (Self::is_drama_or_gossip(trimmed) || top_cat.contains("drama")) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                    println!("   🛡️ [DRAMA / GOSSIP GUARD] Grey-zone failover fallback on drama rumor -> ALLOW.");
                    return ModerationVerdict::Allow;
                }
                if has_slur && !is_meta {
                    return ModerationVerdict::DeleteConfirmed {
                        reason: format!("Racial/hate slur or masked evasion detected in message: \"{}\"", trimmed),
                        score: if max_score > 0.5 { max_score } else { 0.99 },
                        category: "hate".to_string(),
                        model_used: "Slur Guard (Failover)".to_string(),
                        rule_violated: "Crit (Slurs)".to_string(),
                        mute_minutes: 1440,
                    };
                }
                if has_dox_threat && !is_meta {
                    return ModerationVerdict::DeleteConfirmed {
                        reason: format!("Doxxing, blackmail, or personal info leak threat detected: \"{}\"", trimmed),
                        score: if max_score > 0.5 { max_score } else { 0.95 },
                        category: "harassment/threatening".to_string(),
                        model_used: "Dox Guard (Failover)".to_string(),
                        rule_violated: "Major (Threats/Doxx)".to_string(),
                        mute_minutes: 120,
                    };
                }
                if has_provocative_bait {
                    return ModerationVerdict::FlagSuspicious {
                        reason: "Provocative baiting on sensitive topic".to_string(),
                        score: max_score,
                        category: top_cat,
                        model_used: "Bait Guard (Failover)".to_string(),
                        rule_violated: "Minor/Mild (Baiting)".to_string(),
                        mute_minutes: 1,
                    };
                }
                if has_severe_harm_keyword && !is_pvp_callout && !is_meta {
                    return ModerationVerdict::FlagSuspicious {
                        reason: format!("Severe harm keyword detected in message: \"{}\"", trimmed),
                        score: if max_score > 0.5 { max_score } else { 0.85 },
                        category: if top_cat.is_empty() { "violence".to_string() } else { top_cat },
                        model_used: "Severe Keyword Guard (Failover)".to_string(),
                        rule_violated: "Major (Threats/Harm)".to_string(),
                        mute_minutes: 60,
                    };
                }
                ModerationVerdict::Allow
            }
        }
    }

    async fn call_openai_moderation(&self, api_key: &str, texts: &[&str]) -> Result<Vec<OpenAiScores>, reqwest::Error> {
        let req_body = OpenAiBatchModRequest {
            model: "omni-moderation-latest",
            input: texts.to_vec(),
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
            return Ok(vec![OpenAiScores::default(); texts.len()]);
        }

        let body: OpenAiModResponse = resp.json().await?;
        let mut results = Vec::with_capacity(body.results.len());

        for res in body.results {
            let mut max_score = 0.0_f64;
            let mut severe_score = 0.0_f64;
            let mut top_cat = String::new();
            let mut breakdown_parts = Vec::new();

            for (cat, &score) in &res.category_scores {
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
            results.push(OpenAiScores {
                max_score,
                severe_score,
                top_cat,
                breakdown: breakdown_parts.join(", "),
            });
        }

        while results.len() < texts.len() {
            results.push(OpenAiScores::default());
        }

        Ok(results)
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

            // Correction only if model explicitly assigned a punishment RULE other than None but output ALLOW
            if verdict == "ALLOW" {
                let lower_rule = rule.to_lowercase();
                if lower_rule.contains("crit") {
                    verdict = "DELETE".to_string();
                    mute_minutes = 1440;
                } else if (lower_rule.contains("minor") || lower_rule.contains("mild") || lower_rule.contains("mod") || lower_rule.contains("major")) && lower_rule != "none" {
                    verdict = "SUSPICIOUS".to_string();
                    mute_minutes = if lower_rule.contains("minor") || lower_rule.contains("mild") { 1 } else { 30 };
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
                timestamp_unix: 1727376000,
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
        moderator.record_message(101, 1001, 2001, "Alice", "Hello everyone", 1727376000);
        moderator.record_message(101, 1002, 2002, "Bob", "Hey Alice", 1727376005);
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

    #[test]
    fn test_contains_slur() {
        assert!(AiModerator::contains_slur("Like dumb nga"));
        assert!(AiModerator::contains_slur("nga"));
        assert!(AiModerator::contains_slur("DUMB NGA!"));
        assert!(AiModerator::contains_slur("n.g.a"));
        assert!(!AiModerator::contains_slur("manga"));
        assert!(!AiModerator::contains_slur("conga"));
        assert!(!AiModerator::contains_slur("gg ez"));
    }

    #[tokio::test]
    async fn test_check_message_slur_evasion() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("general".to_string()),
            message_id: 7,
            timestamp_unix: 1727376000,
            author_name: "gusherz38269547",
            author_id: 1233954009862377552,
            author_nick: None,
            account_age_days: Some(30),
            server_member_days: Some(10),
            roles_count: 1,
            content: "Like dumb nga",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for slur evasion ('Like dumb nga'): {:?}\n", verdict);
        match verdict {
            ModerationVerdict::DeleteConfirmed { mute_minutes, rule_violated, .. } => {
                assert_eq!(mute_minutes, 1440, "Expected 1440m timeout for slur, got {}m", mute_minutes);
                assert!(rule_violated.contains("Crit"), "Expected Crit rule, got {}", rule_violated);
            }
            other => panic!("Expected DeleteConfirmed, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_check_message_they_would_think_your_dumb() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 8,
            timestamp_unix: 1727376000,
            author_name: "gusherz38269547",
            author_id: 1233954009862377552,
            author_nick: Some("GusherZ".to_string()),
            account_age_days: Some(30),
            server_member_days: Some(10),
            roles_count: 1,
            content: "They would think your dumb",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'They would think your dumb': {:?}\n", verdict);
        assert!(matches!(verdict, ModerationVerdict::Allow), "Expected ALLOW for 'They would think your dumb', got {:?}", verdict);
    }

    #[tokio::test]
    async fn test_check_message_fuck_you() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 9,
            timestamp_unix: 1727376000,
            author_name: "polska8635",
            author_id: 795992869164679168,
            author_nick: Some("BN Meowing cat".to_string()),
            account_age_days: Some(30),
            server_member_days: Some(10),
            roles_count: 1,
            content: "Fuck you",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'Fuck you': {:?}\n", verdict);
        assert!(matches!(verdict, ModerationVerdict::Allow), "Expected 'Fuck you' to be ALLOW, got {:?}", verdict);
    }

    #[test]
    fn test_is_standalone_profanity() {
        assert!(AiModerator::is_standalone_profanity("fuck you"));
        assert!(AiModerator::is_standalone_profanity("Fuck you!"));
        assert!(AiModerator::is_standalone_profanity("fuck you for"));
        assert!(AiModerator::is_standalone_profanity("fuck you bro"));
        assert!(AiModerator::is_standalone_profanity("stfu bum"));
        assert!(AiModerator::is_standalone_profanity("stfu"));
        assert!(AiModerator::is_standalone_profanity("fuck tou"));
        assert!(AiModerator::is_standalone_profanity("пошел нахуй"));
        assert!(!AiModerator::is_standalone_profanity("hello world"));
    }

    #[test]
    fn test_is_theatrical_hyperbole() {
        assert!(AiModerator::is_theatrical_hyperbole("i will eviscerate you"));
        assert!(AiModerator::is_theatrical_hyperbole("<@12345> i will obliterate you"));
        assert!(AiModerator::is_theatrical_hyperbole("im gonna annihilate you in 1v1"));
        assert!(AiModerator::is_theatrical_hyperbole("я тебя сотру в порошок"));
        // Not safe if real-world doxxing or severe violence
        assert!(!AiModerator::is_theatrical_hyperbole("i will eviscerate you and i know where you live"));
        assert!(!AiModerator::is_theatrical_hyperbole("regular message"));
    }

    #[tokio::test]
    async fn test_check_message_eviscerate() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 10,
            timestamp_unix: 1727376000,
            author_name: "kroticzz",
            author_id: 615497792915505153,
            author_nick: Some("kroticzz".to_string()),
            account_age_days: Some(30),
            server_member_days: Some(10),
            roles_count: 1,
            content: "<@642949365861842994> i will eviscerate you",
            reply_to: None,
            mentions: &[(642949365861842994, "partlow".to_string())],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'i will eviscerate you': {:?}\n", verdict);
        assert!(matches!(verdict, ModerationVerdict::Allow), "Expected 'i will eviscerate you' to be ALLOW, got {:?}", verdict);
    }

    #[tokio::test]
    async fn test_check_message_in_game_kill_context() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        moderator.record_message(1, 101, 12345, "Machi", "one match till i rankup in cs", 1727375990);
        moderator.record_message(1, 102, 67890, "GusherZ", "im picking sniper on mid", 1727375995);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 103,
            timestamp_unix: 1727376000,
            author_name: "Machi",
            author_id: 12345,
            author_nick: Some("Machi".to_string()),
            account_age_days: Some(30),
            server_member_days: Some(10),
            roles_count: 1,
            content: "i will kill you if you throw",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for in-game 'i will kill you if you throw': {:?}\n", verdict);
        assert!(matches!(verdict, ModerationVerdict::Allow), "Expected in-game kill to be ALLOW, got {:?}", verdict);
    }

    #[tokio::test]
    async fn test_check_message_spread_info() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 104,
            timestamp_unix: 1727376000,
            author_name: "Troll",
            author_id: 99999,
            author_nick: Some("Troll".to_string()),
            account_age_days: Some(30),
            server_member_days: Some(10),
            roles_count: 1,
            content: "I will spread all your information in social media",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'spread info': {:?}\n", verdict);
        assert!(
            matches!(verdict, ModerationVerdict::DeleteConfirmed { .. } | ModerationVerdict::FlagSuspicious { .. }),
            "Expected doxxing threat to be punished with Delete or Suspicious, got {:?}",
            verdict
        );
    }

    #[tokio::test]
    async fn test_check_message_dox_russian() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 105,
            timestamp_unix: 1727376000,
            author_name: "TrollRu",
            author_id: 88888,
            author_nick: Some("TrollRu".to_string()),
            account_age_days: Some(30),
            server_member_days: Some(10),
            roles_count: 1,
            content: "жди я солью твои данные и адрес",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for Russian doxx threat: {:?}\n", verdict);
        assert!(
            matches!(verdict, ModerationVerdict::DeleteConfirmed { .. } | ModerationVerdict::FlagSuspicious { .. }),
            "Expected Russian doxxing threat to be punished with Delete or Suspicious, got {:?}",
            verdict
        );
    }

    #[tokio::test]
    async fn test_check_message_kill_him() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 106,
            timestamp_unix: 1727376000,
            author_name: "__mondej",
            author_id: 1403293782186922054,
            author_nick: Some("__mondej".to_string()),
            account_age_days: Some(30),
            server_member_days: Some(10),
            roles_count: 1,
            content: "kill him",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'kill him': {:?}\n", verdict);
        assert!(
            matches!(verdict, ModerationVerdict::Allow),
            "Expected 'kill him' (PvP callout) to be ALLOW, got {:?}",
            verdict
        );
    }

    #[test]
    fn test_is_shut_up_or_silencing_unit() {
        assert!(AiModerator::is_shut_up_or_silencing("SHUT UP"));
        assert!(AiModerator::is_shut_up_or_silencing("shut up!"));
        assert!(AiModerator::is_shut_up_or_silencing("stfu"));
        assert!(AiModerator::is_shut_up_or_silencing("stfu noob"));
        assert!(AiModerator::is_shut_up_or_silencing("shut your mouth"));
        assert!(AiModerator::is_shut_up_or_silencing("заткнись"));
        assert!(AiModerator::is_shut_up_or_silencing("закрой рот"));
        assert!(AiModerator::is_shut_up_or_silencing("завали ебало"));
        assert!(AiModerator::is_shut_up_or_silencing("да заткнись уже"));
        assert!(AiModerator::is_shut_up_or_silencing("замолчи"));

        // Negative cases (self-referential or questions)
        assert!(!AiModerator::is_shut_up_or_silencing("ok i will shut up"));
        assert!(!AiModerator::is_shut_up_or_silencing("я заткнусь"));
        assert!(!AiModerator::is_shut_up_or_silencing("why should i shut up?"));
        assert!(!AiModerator::is_shut_up_or_silencing("hello guys"));

        // Severe forbidden cases (must not be treated as mere silencing)
        assert!(!AiModerator::is_shut_up_or_silencing("shut up kys"));
        assert!(!AiModerator::is_shut_up_or_silencing("заткнись и сдохни"));
    }

    #[tokio::test]
    async fn test_check_message_shut_up() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 107,
            timestamp_unix: 1727376000,
            author_name: "Gamer1",
            author_id: 11111,
            author_nick: Some("Gamer1".to_string()),
            account_age_days: Some(30),
            server_member_days: Some(10),
            roles_count: 1,
            content: "SHUT UP",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'SHUT UP': {:?}\n", verdict);
        assert!(
            matches!(verdict, ModerationVerdict::WarnOnly { .. }),
            "Expected 'SHUT UP' to be WarnOnly, got {:?}",
            verdict
        );
    }

    #[tokio::test]
    async fn test_check_message_zatknis() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 108,
            timestamp_unix: 1727376000,
            author_name: "Gamer2",
            author_id: 22222,
            author_nick: Some("Gamer2".to_string()),
            account_age_days: Some(30),
            server_member_days: Some(10),
            roles_count: 1,
            content: "да заткнись уже",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'да заткнись уже': {:?}\n", verdict);
        assert!(
            matches!(verdict, ModerationVerdict::WarnOnly { .. }),
            "Expected 'да заткнись уже' to be WarnOnly, got {:?}",
            verdict
        );
    }

    #[tokio::test]
    async fn test_author_split_threat_detection() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        // Author sends message 1: "I will find you"
        moderator.record_message(1, 109, 33333, "TrollSplitter", "I will find you", 1727375990);
        // Author sends message 2: "and shoot you"
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 110,
            timestamp_unix: 1727376000,
            author_name: "TrollSplitter",
            author_id: 33333,
            author_nick: Some("TrollSplitter".to_string()),
            account_age_days: Some(30),
            server_member_days: Some(10),
            roles_count: 1,
            content: "and shoot you",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for split threat: {:?}\n", verdict);
        assert!(
            matches!(verdict, ModerationVerdict::DeleteConfirmed { .. } | ModerationVerdict::FlagSuspicious { .. }),
            "Expected split threat ('I will find you' + 'and shoot you') to be caught with Delete or Suspicious, got {:?}",
            verdict
        );
    }

    #[tokio::test]
    async fn test_author_gaming_context_batch() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        // Author sends message 1: "we are playing csgo match"
        moderator.record_message(1, 111, 44444, "PvPPlayer", "we are playing csgo match", 1727375990);
        // Author sends message 2: "kill him"
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 112,
            timestamp_unix: 1727376000,
            author_name: "PvPPlayer",
            author_id: 44444,
            author_nick: Some("PvPPlayer".to_string()),
            account_age_days: Some(30),
            server_member_days: Some(10),
            roles_count: 1,
            content: "kill him",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for gaming context 'kill him': {:?}\n", verdict);
        assert!(
            matches!(verdict, ModerationVerdict::Allow),
            "Expected 'kill him' in gaming context to be ALLOW, got {:?}",
            verdict
        );
    }

    #[tokio::test]
    async fn test_check_message_dox_meta_talk() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 113,
            timestamp_unix: 1727376000,
            author_name: "fr.",
            author_id: 460229578896572417,
            author_nick: Some("fr.".to_string()),
            account_age_days: Some(100),
            server_member_days: Some(50),
            roles_count: 2,
            content: "Its basically a doxx soo yeah",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'Its basically a doxx soo yeah': {:?}\n", verdict);
        assert!(
            matches!(verdict, ModerationVerdict::Allow),
            "Expected 'Its basically a doxx soo yeah' to be ALLOW, got {:?}",
            verdict
        );
    }

    #[tokio::test]
    async fn test_check_message_dev_critique_vent() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 114,
            timestamp_unix: 1727376000,
            author_name: "freshmanlovernothacked",
            author_id: 1534598207001329864,
            author_nick: Some("freshmanlovernothacked".to_string()),
            account_age_days: Some(50),
            server_member_days: Some(20),
            roles_count: 1,
            content: "roblox devs becoming actual subhuman idiots the moment their games boutta release",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'roblox devs becoming actual subhuman idiots...': {:?}\n", verdict);
        assert!(
            matches!(verdict, ModerationVerdict::Allow),
            "Expected developer critique vent to be ALLOW, got {:?}",
            verdict
        );
    }

    #[tokio::test]
    async fn test_check_message_grapevine_gossip() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 115,
            timestamp_unix: 1727376000,
            author_name: "Mizo",
            author_id: 357645179722661891,
            author_nick: Some("Mizo".to_string()),
            account_age_days: Some(200),
            server_member_days: Some(100),
            roles_count: 3,
            content: "<@357645179722661891> i heard from the grape vine that u paid someone to do something to me",
            reply_to: None,
            mentions: &[(357645179722661891, "TargetUser".to_string())],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for grapevine gossip: {:?}\n", verdict);
        assert!(
            matches!(verdict, ModerationVerdict::Allow),
            "Expected grapevine gossip to be ALLOW, got {:?}",
            verdict
        );
    }

    #[test]
    fn test_contains_word_boundaries() {
        assert!(!AiModerator::contains_word("i heard from the grape vine", "rape"));
        assert!(!AiModerator::contains_word("grapevine", "rape"));
        assert!(!AiModerator::contains_word("he scraped his knee", "rape"));
        assert!(AiModerator::contains_word("did he rape someone", "rape"));
        assert!(AiModerator::contains_word("rape!", "rape"));
        assert!(AiModerator::contains_word("rape", "rape"));

        assert!(!AiModerator::contains_word("он дурак", "рак"));
        assert!(!AiModerator::contains_word("трактор", "рак"));
        assert!(AiModerator::contains_word("ты рак", "рак"));
        assert!(AiModerator::contains_word("рак!", "рак"));

        assert!(!AiModerator::contains_word("system stability", "stab"));
        assert!(AiModerator::contains_word("i will stab you", "stab"));

        assert!(!AiModerator::contains_word("audience", "die"));
        assert!(!AiModerator::contains_word("diet", "die"));
        assert!(AiModerator::contains_word("you will die", "die"));

        // Multi-word checks
        assert!(AiModerator::contains_word("i will burn alive in hell", "burn alive"));
        assert!(!AiModerator::contains_word("i will burn hot", "burn alive"));
    }
}


