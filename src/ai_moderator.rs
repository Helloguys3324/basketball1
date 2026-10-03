use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use crate::profanity_engine::ProfanityEngine;
use regex::Regex;

// =============================================================================
// AI MODERATOR CONFIGURATION & CONSTANTS
// =============================================================================

const OPENAI_SAFE_THRESHOLD: f64 = 0.45;
const OPENAI_SEVERE_THRESHOLD: f64 = 0.82;
const OPENAI_CATEGORY_SEVERE_THRESHOLD: f64 = 0.70;

const MAX_CONTEXT_HISTORY: usize = 30;
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

static OPENAI_COOLDOWN_UNTIL: AtomicU64 = AtomicU64::new(0);

const EXTREME_REAL_HARM_KEYWORDS: &[&str] = &[
    "burn alive", "burned alive", "сжечь заживо", "расчленить", "расчленю",
    "slit your throat", "перережу горло", "вскрою горло", "сожгу твой дом", "сожгу тебя заживо"
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

#[allow(dead_code)]
pub const SERVER_RULES_SYSTEM_PROMPT: &str = "\
Discord Arbiter for a gaming community. Mutes only (NO BAN/KICK).\n\
CRITICAL EVALUATION PROTOCOL (3 MANDATORY QUESTIONS BEFORE DECIDING):\n\
Whenever any pattern, trigger, or flagged message is evaluated, the Arbiter MUST ask and resolve these 3 questions:\n\
1. ЭТО ОСКОРБЛЯЕТ ЧЕЛОВЕКА? (Does this actually insult, degrade, harass, or inflict real harm on an actual person? Or is it victimless banter, gaming slang, self-deprecation, or a quote?)\n\
2. МОЖЕТ ЛИ ЭТО БЫТЬ ШУТКОЙ НА СЕРВЕРЕ ГДЕ ШУТЯТ ПОЧТИ ВСЕГДА? (Could this reasonably be a joke, gaming irony, trash-talk, post-irony, meme, or friendly teasing on a Discord server where members joke almost 100% of the time? If it can reasonably be a joke or friendly banter without real malice -> ALWAYS VERDICT: ALLOW, RULE: None, MUTE: 0).\n\
3. ЯВЛЯЕТСЯ ЛИ ЭТО ГРУБЫМ НАРУШЕНИЕМ? (Is this a genuine severe violation: actual scam/phishing/token-stealer link, real crypto drainer, publishing real personal data (doxxing/PII), explicit death threat with real-world malice, or hate speech / racial slurs?)\n\
Autonomously determine the exact danger level based on context and intent without false positives on humor.\n\
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
  PLAYFUL WARNINGS & HYPERBOLIC BANTER ('say yo one more time and ur done for', 'say that again and you\'re cooked', 'ur done for', 'you\'re done for', 'ur cooked', 'you\'re cooked', 'you\'re toast', 'it\'s over for you', 'тебе конец', 'тебе хана'): Standard conversational memes, comedic teasing, and harmless exaggeration between Discord members. They are NEVER credible threats of real-world violence! Unless accompanied by real-world personal information, physical addresses, weapons, or extortion, ALWAYS VERDICT: ALLOW (RULE: None, MUTE: 0).\n\
  CASUAL SLANG & POPULAR IDIOMS ('hoes', 'thot', 'simp', 'bitch', 'no hoes', 'u get loads of hoes', 'hoes mad', 'bros before hoes'): These are standard internet/hip-hop slang and comedic memes, NOT hate speech or racial slurs! NEVER classify 'hoes', 'loads of hoes', or 'bitch' as Crit or Hate Speech! Phrases like 'u get loads of hoes', 'bros before hoes', 'no hoes' -> ALWAYS VERDICT: ALLOW (RULE: None, MUTE: 0).\n\
  FICTIONAL PLOTS, ANIME/MANGA NARRATIVE & SPOILERS ('||...||'): Recounting anime/manga/movie storylines, fiction, or spoilers in '||...||' (e.g. cannibalism, killings/dies in Chainsaw Man, Tokyo Ghoul, Hunter x Hunter quests like 'kill 5 chrollo') is fictional storytelling and RPG discussion, NOT real-world violence! ALWAYS VERDICT: ALLOW (RULE: None, MUTE: 0).\n\
  SENDING IN-GAME CHARACTERS / ROBLOX STANDS ('i\'m sending current aba wb after u', 'sending mahoraga after you', 'sending goku after u'): In Roblox anime games (ABA = Anime Battle Arena, WB = Whitebeard), players constantly joke about sending in-game characters/avatars after each other in-game. This is 100% in-game gaming trashtalk, NOT real-world physical violence! ALWAYS VERDICT: ALLOW (RULE: None, MUTE: 0).\n\
  IN-GAME 'KILL' & PVP TRASHTALK ('i will kill you', 'im gonna kill you', 'let me kill you', 'i will kill you if you throw', 'i will kill you in 1v1'): When the conversation context involves games, matches, ranks, characters, PvP, gameplay, anime, or gaming banter ('stop stealing my kills', 'one match till rankup', '1v1 me', 'playing together'): These refer to IN-GAME CHARACTER DEATH or gaming frustration between players, NOT real-world murder! ALWAYS VERDICT: ALLOW (RULE: None, MUTE: 0). Only punish 'kill' if it threatens REAL-WORLD physical harm (e.g. 'i know where you live and will kill you', real-life stalking, weapons, dates).\n\
  THIRD-PERSON GAMING CALLOUTS & PVP DIRECTIVES ('kill him', 'kill her', 'kill them', 'kill it', 'shoot him', 'get him', 'focus him', 'finish him', 'убей его', 'добивай', 'завали его'): These are standard tactical PvP/gaming callouts, spectator banter, or stream clip reactions referring to opponents, in-game characters, bosses, or enemies! They are NEVER real-world death threats! ALWAYS VERDICT: ALLOW (RULE: None, MUTE: 0). NEVER classify 'kill him/her/them' as Direct death threat, Harassment, or Violence!\n\
  ROBLOX & GAMING PVP / HUNTING BANTER ('I will find you and kill you', 'im gonna find u and kill u in roblox', 'найду тебя и убью'): In this Discord community, members constantly play Roblox and competitive multiplayer games. Phrases like 'I will find you and kill you', 'im gonna find you and destroy you in roblox', 'найду тебя на сервере и убью' are completely standard in-game hunting trashtalk referring to finding the player's avatar in Roblox or hunting them in a match! ALWAYS analyze context deeply. Unless someone specifies REAL-WORLD personal details (real physical address, city, street, school, phone, real full name, swatting, IRL weapons, leaked IP) or demands real-world extortion/money, statements like 'I will find you and kill you' must be treated as in-game Roblox hunting banter! ALWAYS VERDICT: ALLOW (RULE: None, MUTE: 0).\n\
  CALIBRATION FOR 0% OR LOW TOXICITY MESSAGES: If a message has 0% toxicity or near-zero toxicity (score <= 0.20), take it MUCH less seriously! NEVER issue auto-deletions or timeouts for low-score messages unless there is a blatant, explicit, unambiguous real-world violation: (1) An explicit, real-world doxx threat with real PII or blackmail ('i will leak your home address/phone/school', 'солью твой домашний адрес') NOT in the context of Roblox/games; (2) Direct, undeniable hate/racial slurs (N-word, etc.). If there is any doubt or ambiguity in a low-toxicity message, default to VERDICT: ALLOW.\n\
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

pub const SERVER_RULES_BATCH_SYSTEM_PROMPT: &str = r#"Discord Arbiter for a gaming community. You are auditing multiple flagged Discord messages from users.
Mutes only (NO BAN/KICK).

CRITICAL EVALUATION PROTOCOL (3 MANDATORY QUESTIONS BEFORE DECIDING):
Whenever any pattern, trigger, or flagged message is evaluated, the Arbiter MUST ask and resolve these 3 questions:
1. ЭТО ОСКОРБЛЯЕТ ЧЕЛОВЕКА? (Does this actually insult, degrade, harass, or inflict real harm on an actual person? Or is it victimless banter, gaming slang, self-deprecation, or a quote?)
2. МОЖЕТ ЛИ ЭТО БЫТЬ ШУТКОЙ НА СЕРВЕРЕ ГДЕ ШУТЯТ ПОЧТИ ВСЕГДА? (Could this reasonably be a joke, gaming irony, trash-talk, post-irony, meme, or friendly teasing on a Discord server where members joke almost 100% of the time?)
   -> If it can be understood as a joke, gaming banter, sarcasm, or non-malicious teasing -> VERDICT: ALLOW (RULE: None, MUTE: 0).
3. ЯВЛЯЕТСЯ ЛИ ЭТО ГРУБЫМ НАРУШЕНИЕМ? (Is this a genuine severe violation: actual scam/phishing/token-stealer link, real crypto drainer, publishing real personal data (doxxing/PII), explicit death threat with real-world malice, or hate speech / racial slurs?)
   -> If and ONLY if there is genuine malice or dangerous harm, autonomously determine the appropriate danger level without false positives on humor:

PUNISHMENT TIERS (SUSPICIOUS/DELETE):
1. Minor/Mild -> SUSPICIOUS(1m): Malicious chat flooding, repetitive copy-paste raid spam, provocative gender bait ('i love sexism'). NEVER punish standard banter, complaints, or single messages under Minor/Mild!
2. Mod -> SUSPICIOUS(15-30m): Explicit NSFW pornography links, deliberate toxic filter bypass. (NO mutes for gossip, rumors, or drama!)
3. Major -> SUSPICIOUS(60m) or DELETE(120m): Direct real-world threats, stalking, publishing or threatening to leak private personal info (doxxing/extortion), malicious impersonation, server raid invites, phishing/scams, fake free nitro links, steam gift/trade scams, crypto drainers
4. Crit -> DELETE(1440m): Racial/hate slurs ('nga','ngga','nigga','nigger','fag','faggot'), direct death wishes ('kys','you should die'), gore, malware, token stealers, credential theft
QUOTES, OPINIONS, META-TALK & HYPOTHETICALS (ALLOW, RULE:None, MUTE:0):
- Meta-talk and observations about doxxing or rules -> ALWAYS ALLOW (RULE: None, MUTE: 0).
- Casual words ('dumb', 'stupid', 'silly', 'trash', 'noob', 'idiot') in casual conversation -> ALWAYS ALLOW.
- Discussing server rules, testing bot triggers, quoting past messages, abstract placeholders -> ALWAYS ALLOW.
- General gaming frustrations directed at external companies, game studios or developers -> ALWAYS ALLOW.
- Chat gossip, rumors, questions, or accusations between members -> ALWAYS ALLOW.
- Third-person gaming callouts ('kill him', 'shoot him', 'убей его') -> ALWAYS ALLOW (RULE: None, MUTE: 0).
- Roblox / Gaming hunting trashtalk ('I will find you and kill you in roblox', 'найду тебя на сервере и убью') -> ALWAYS ALLOW (RULE: None, MUTE: 0).
- Standalone casual profanity ('fuck you', 'stfu') WITHOUT death wishes and WITHOUT slurs -> ALWAYS ALLOW.
- Post-irony, theatrical hyperbole & dramatic exaggeration ('i will eviscerate you', 'я тебя расщеплю на атомы') -> ALWAYS ALLOW.
- Playful warnings & hyperbolic banter ('say yo one more time and ur done for', 'ur cooked', 'you\'re done for', 'тебе конец') -> ALWAYS ALLOW (RULE: None, MUTE: 0).
- Fake-game shield evasion ('kys in minecraft', 'die in roblox', 'burn your house in rust') -> SUSPICIOUS(60m) or DELETE(120m).
CRITICAL OUTPUT FORMAT:
You MUST evaluate EACH item independently and output a dedicated block for EVERY [ITEM <number>] in the batch in order:
[ITEM <number>]
VERDICT:[ALLOW|SUSPICIOUS|DELETE]
RULE:[Rule name or None]
MUTE_MINUTES:[0|1|15|30|60|120|1440]
REASON:[<=8 words]"#;

static ENV_CACHE: OnceLock<HashMap<String, String>> = OnceLock::new();

fn init_env_cache() -> HashMap<String, String> {
    let mut map = HashMap::new();
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
                    if let Some((k, v)) = line.split_once('=') {
                        let key = k.trim().to_string();
                        let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
                        if !key.is_empty() && !val.is_empty() {
                            map.entry(key).or_insert(val);
                        }
                    }
                }
            }
        }
    }
    map
}

pub fn get_env_var(name: &str) -> Option<String> {
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

    let cache = ENV_CACHE.get_or_init(init_env_cache);
    if let Some(val) = cache.get(name) {
        if name == "OPENAI_API_KEY" && val.starts_with("gsk_") {
            return None;
        }
        return Some(val.clone());
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

#[allow(dead_code)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupremeVerdict {
    Allow,
    Confirm,
}

#[derive(Debug, Clone)]
pub struct SupremeDecision {
    pub verdict: SupremeVerdict,
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

#[derive(Deserialize, Debug, Clone)]
pub struct VectorCheckResponse {
    pub is_scam: bool,
    pub score: f64,
    #[serde(default)]
    pub threshold: f64,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub matched_text: String,
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
    #[serde(default)]
    reasoning_content: Option<String>,
}

#[derive(Serialize)]
struct GeminiRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent<'a>>,
    contents: Vec<GeminiContent<'a>>,
    #[serde(rename = "generationConfig")]
    generation_config: GeminiGenConfig,
}

#[derive(Serialize)]
struct GeminiContent<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<&'a str>,
    parts: Vec<GeminiPart<'a>>,
}

#[derive(Serialize)]
struct GeminiPart<'a> {
    text: &'a str,
}

#[derive(Serialize)]
struct GeminiGenConfig {
    temperature: f64,
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: u32,
}

#[derive(Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    #[serde(default)]
    error: Option<GeminiError>,
}

#[derive(Deserialize)]
struct GeminiError {
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContentResp>,
}

#[derive(Deserialize)]
struct GeminiContentResp {
    #[serde(default)]
    parts: Vec<GeminiPartResp>,
}

#[derive(Deserialize)]
struct GeminiPartResp {
    #[serde(default)]
    text: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BatchLogEntry {
    pub message_id: u64,
    pub channel_id: u64,
    pub channel_name: String,
    pub author_id: u64,
    pub author_name: String,
    pub content: String,
    pub timestamp_unix: i64,
    pub reply_to: Option<String>,
}

#[derive(Debug)]
pub struct OpenAiBatchRequest {
    pub inputs: Vec<String>,
    pub meta: BatchLogEntry,
    pub sender: oneshot::Sender<(Vec<OpenAiScores>, Arc<Vec<BatchLogEntry>>)>,
}

#[derive(Debug)]
pub struct GroqBatchItemRequest {
    pub message_id: u64,
    pub channel_id: u64,
    pub channel_name: String,
    pub author_name: String,
    pub author_id: u64,
    pub trimmed_content: String,
    pub telemetry_chunk: String,
    pub is_hardcore: bool,
    pub batch_transcript: Arc<Vec<BatchLogEntry>>,
    pub channel_history: Vec<ChatEntry>,
    pub sender: oneshot::Sender<Result<(GroqDecision, String, u128), String>>,
}

pub struct AiModerator {
    http_client: reqwest::Client,
    vector_service_url: String,
    openai_key: Option<String>,
    groq_keys: Vec<String>,
    groq_fast_model: String,
    groq_deep_model: String,
    groq_counter: Arc<AtomicUsize>,
    gemini_keys: Vec<String>,
    gemini_counter: Arc<AtomicUsize>,
    gemini_fast_model: String,
    gemini_deep_model: String,
    nvidia_keys: Vec<String>,
    nvidia_counter: Arc<AtomicUsize>,
    nvidia_model: String,
    nvidia_api_endpoint: String,
    chat_history: RwLock<HashMap<u64, VecDeque<ChatEntry>>>,
    batch_tx: Option<mpsc::UnboundedSender<OpenAiBatchRequest>>,
    groq_batch_tx: Option<mpsc::UnboundedSender<GroqBatchItemRequest>>,
    pub dynamic_whitelist: Arc<RwLock<HashSet<String>>>,
    pub profanity_engine: Arc<ProfanityEngine>,
}

impl AiModerator {
    pub fn new(http_client: reqwest::Client) -> Self {
        let default_batch_size = if cfg!(test) { 10 } else { 100 };
        let batch_size = get_env_var("OPENAI_MODERATION_BATCH_SIZE")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(default_batch_size);

        let default_timeout_ms = if cfg!(test) { 100 } else { 0 };
        let batch_timeout_ms = get_env_var("OPENAI_MODERATION_BATCH_TIMEOUT_MS")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(default_timeout_ms);

        let default_groq_batch_size = if cfg!(test) { 5 } else { 8 };
        let groq_batch_size = get_env_var("GROQ_MODERATION_BATCH_SIZE")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(default_groq_batch_size);

        let default_groq_timeout_ms = if cfg!(test) { 50 } else { 0 };
        let groq_batch_timeout_ms = get_env_var("GROQ_MODERATION_BATCH_TIMEOUT_MS")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(default_groq_timeout_ms);

        Self::new_full(
            http_client,
            batch_size,
            Duration::from_millis(batch_timeout_ms),
            groq_batch_size,
            Duration::from_millis(groq_batch_timeout_ms),
        )
    }

    #[allow(dead_code)]
    pub fn new_with_batch_config(
        http_client: reqwest::Client,
        batch_size: usize,
        batch_wait: Duration,
    ) -> Self {
        let default_groq_batch_size = if cfg!(test) { 5 } else { 8 };
        let groq_batch_size = get_env_var("GROQ_MODERATION_BATCH_SIZE")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(default_groq_batch_size);

        let default_groq_timeout_ms = if cfg!(test) { 50 } else { 500 };
        let groq_batch_timeout_ms = get_env_var("GROQ_MODERATION_BATCH_TIMEOUT_MS")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(default_groq_timeout_ms);

        Self::new_full(
            http_client,
            batch_size,
            batch_wait,
            groq_batch_size,
            Duration::from_millis(groq_batch_timeout_ms),
        )
    }

    pub fn new_full(
        http_client: reqwest::Client,
        openai_batch_size: usize,
        openai_batch_wait: Duration,
        groq_batch_size: usize,
        groq_batch_wait: Duration,
    ) -> Self {
        let vector_service_url = get_env_var("VECTOR_SERVICE_URL")
            .unwrap_or_else(|| "http://127.0.0.1:6335".to_string());
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

        let groq_counter = Arc::new(AtomicUsize::new(0));

        let gemini_keys: Vec<String> = get_env_var("GEMINI_API_KEYS")
            .or_else(|| get_env_var("GEMINI_API_KEY"))
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let gemini_counter = Arc::new(AtomicUsize::new(0));

        let gemini_fast_model = get_env_var("GEMINI_FAST_MODEL")
            .map(|m| if m.contains("3.8") { "gemini-3.1-flash-lite".to_string() } else { m })
            .unwrap_or_else(|| "gemini-3.1-flash-lite".to_string());
        let gemini_deep_model = get_env_var("GEMINI_DEEP_MODEL")
            .map(|m| if m.contains("3.8") { "gemini-3.5-flash-lite".to_string() } else { m })
            .unwrap_or_else(|| "gemini-3.5-flash-lite".to_string());

        let nvidia_keys: Vec<String> = get_env_var("NVIDIA_API_KEYS")
            .or_else(|| get_env_var("NVIDIA_API_KEY"))
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let nvidia_counter = Arc::new(AtomicUsize::new(0));

        let nvidia_model = get_env_var("NVIDIA_MODEL")
            .unwrap_or_else(|| "deepseek-ai/deepseek-v4.1-flash".to_string());

        let nvidia_api_endpoint = get_env_var("NVIDIA_API_ENDPOINT")
            .unwrap_or_else(|| "https://integrate.api.nvidia.com/v1/chat/completions".to_string());

        if !nvidia_keys.is_empty() {
            println!(
                "   ⚖️ [SUPREME ARBITER] Initialized with {} NVIDIA key(s) (Model: {}, Endpoint: {})",
                nvidia_keys.len(),
                nvidia_model,
                nvidia_api_endpoint
            );
        } else if !gemini_keys.is_empty() {
            println!(
                "   ⚖️ [SUPREME ARBITER] NVIDIA keys not set. Supreme failover active via Gemini Deep (Model: {})",
                gemini_deep_model
            );
        }

        let batch_tx = if let Ok(handle) = tokio::runtime::Handle::try_current() {
            if openai_batch_wait.is_zero() {
                println!(
                    "   📦 [OPENAI BATCHING] Worker initialized: max_batch = {}, interval = INSTANT (0ms)",
                    openai_batch_size
                );
            } else {
                println!(
                    "   📦 [OPENAI BATCHING] Worker initialized: max_batch = {}, interval = {:.1}s ({}ms)",
                    openai_batch_size,
                    openai_batch_wait.as_secs_f64(),
                    openai_batch_wait.as_millis()
                );
            }
            let (tx, rx) = mpsc::unbounded_channel::<OpenAiBatchRequest>();
            let client_clone = http_client.clone();
            let key_clone = openai_key.clone();
            handle.spawn(Self::run_batch_worker(
                rx,
                client_clone,
                key_clone,
                openai_batch_size,
                openai_batch_wait,
            ));
            Some(tx)
        } else {
            None
        };

        let groq_batch_tx = if let Ok(handle) = tokio::runtime::Handle::try_current() {
            if !groq_keys.is_empty() || !gemini_keys.is_empty() {
                if groq_batch_wait.is_zero() {
                    println!(
                        "   🤖 [LLM ARBITER] Worker initialized: max_batch = {}, interval = INSTANT (0ms) (Gemini Keys: {}, Groq Keys: {})",
                        groq_batch_size,
                        gemini_keys.len(),
                        groq_keys.len()
                    );
                } else {
                    println!(
                        "   🤖 [LLM ARBITER] Worker initialized: max_batch = {}, interval = {:.1}s ({}ms) (Gemini Keys: {}, Groq Keys: {})",
                        groq_batch_size,
                        groq_batch_wait.as_secs_f64(),
                        groq_batch_wait.as_millis(),
                        gemini_keys.len(),
                        groq_keys.len()
                    );
                }
                let (tx, rx) = mpsc::unbounded_channel::<GroqBatchItemRequest>();
                let client_clone = http_client.clone();
                let keys_clone = groq_keys.clone();
                let fast_clone = groq_fast_model.clone();
                let deep_clone = groq_deep_model.clone();
                let counter_clone = groq_counter.clone();
                let gemini_keys_clone = gemini_keys.clone();
                let gemini_counter_clone = gemini_counter.clone();
                let gemini_fast_clone = gemini_fast_model.clone();
                let gemini_deep_clone = gemini_deep_model.clone();
                handle.spawn(Self::run_groq_batch_worker(
                    rx,
                    client_clone,
                    keys_clone,
                    fast_clone,
                    deep_clone,
                    counter_clone,
                    gemini_keys_clone,
                    gemini_counter_clone,
                    gemini_fast_clone,
                    gemini_deep_clone,
                    groq_batch_size,
                    groq_batch_wait,
                ));
                Some(tx)
            } else {
                None
            }
        } else {
            None
        };

        let mut dynamic_whitelist = HashSet::new();
        // Load persistent dynamic whitelist using portable resolution
        let wl_candidates = [
            PathBuf::from("dynamic_whitelist.txt"),
            PathBuf::from("vector_engine/dynamic_whitelist.txt"),
        ];
        let mut wl_paths = wl_candidates.to_vec();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                wl_paths.push(parent.join("dynamic_whitelist.txt"));
                wl_paths.push(parent.join("vector_engine/dynamic_whitelist.txt"));
            }
        }
        for path in &wl_paths {
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(path) {
                    for line in content.lines() {
                        let w = line.trim().to_lowercase();
                        if !w.is_empty() {
                            dynamic_whitelist.insert(w);
                        }
                    }
                    if !dynamic_whitelist.is_empty() {
                        break;
                    }
                }
            }
        }
        if !dynamic_whitelist.is_empty() {
            println!("   📋 [DYNAMIC WHITELIST] Initialized with {} learned safe phrases.", dynamic_whitelist.len());
        }

        Self {
            http_client,
            vector_service_url,
            openai_key,
            groq_keys,
            groq_fast_model,
            groq_deep_model,
            groq_counter,
            gemini_keys,
            gemini_counter,
            gemini_fast_model,
            gemini_deep_model,
            nvidia_keys,
            nvidia_counter,
            nvidia_model,
            nvidia_api_endpoint,
            chat_history: RwLock::new(HashMap::new()),
            batch_tx,
            groq_batch_tx,
            dynamic_whitelist: Arc::new(RwLock::new(dynamic_whitelist)),
            profanity_engine: Arc::new(ProfanityEngine::new()),
        }
    }

    /// Dynamically learn a phrase allowed by LLM into the persistent whitelist
    /// DISABLED by user request to prevent rogue/ambiguous phrases from polluting the whitelist.
    pub async fn add_to_whitelist(&self, _text: &str) {
        // AI self-learning disabled: each message is evaluated cleanly on its own context
    }

    /// Query the local L2 Vector Engine (Qdrant + MiniLM with 120,000+ scam patterns)
    pub async fn check_vector_engine(&self, text: &str) -> Option<VectorCheckResponse> {
        #[cfg(test)]
        if text.contains("n!tr") || text.contains("nitro") || text.contains("fr33") {
            return Some(VectorCheckResponse {
                score: 0.95,
                threshold: 0.70,
                is_scam: true,
                category: "local_custom_scam".to_string(),
                matched_text: "free discord nitro scam pattern".to_string(),
            });
        }

        let payload = serde_json::json!({ "text": text });
        let resp = self.http_client
            .post(&format!("{}/check", self.vector_service_url))
            .json(&payload)
            .timeout(Duration::from_millis(2500))
            .send()
            .await
            .ok()?;

        if resp.status().is_success() {
            resp.json::<VectorCheckResponse>().await.ok()
        } else {
            None
        }
    }

    /// Dynamically train a new scam pattern into local Qdrant collection on the fly
    #[allow(dead_code)]
    pub async fn train_vector_engine(&self, text: &str, category: &str) -> Result<String, String> {
        let payload = serde_json::json!({
            "text": text,
            "category": category
        });
        let resp = self.http_client
            .post(&format!("{}/train", self.vector_service_url))
            .json(&payload)
            .timeout(Duration::from_millis(3000))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if resp.status().is_success() {
            Ok("Successfully indexed pattern into local Qdrant collection".to_string())
        } else {
            Err(format!("Vector service error (HTTP {})", resp.status()))
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
                // Evict only the least recently active channel (oldest timestamp)
                if let Some((&oldest_cid, _)) = history
                    .iter()
                    .min_by_key(|(_, q)| q.back().map(|e| e.timestamp_unix).unwrap_or(0))
                {
                    history.remove(&oldest_cid);
                }
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

    pub fn get_context_snapshot(&self, channel_id: u64) -> Vec<ChatEntry> {
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
            } else if text_lower.is_char_boundary(i) {
                let prev_char = text_lower[..i].chars().next_back();
                prev_char.map(|c| !c.is_alphanumeric()).unwrap_or(true)
            } else {
                false
            };

            let end_idx = i + t_len;
            let next_ok = if end_idx >= text_lower.len() {
                true
            } else if text_lower.is_char_boundary(end_idx) {
                let next_char = text_lower[end_idx..].chars().next();
                next_char.map(|c| !c.is_alphanumeric()).unwrap_or(true)
            } else {
                false
            };

            if prev_ok && next_ok {
                return true;
            }
        }

        false
    }

    #[inline]
    pub fn contains_suspicious_link(text: &str) -> bool {
        let lower = text.to_lowercase();
        lower.contains("http://")
            || lower.contains("https://")
            || lower.contains("discord.gg/")
            || lower.contains("discord.gift/")
            || lower.contains(".gift/")
            || lower.contains(".gift")
            || lower.contains(".xyz")
            || lower.contains(".top")
            || lower.contains(".ru/")
            || lower.contains(".com/")
            || lower.contains("t.me/")
            || lower.contains("steamcommunity.com")
            || lower.contains("steampowered.com")
            || (lower.contains("steam") && lower.contains("trade"))
    }

    #[inline]
    pub fn contains_suspicious_keywords(text: &str) -> bool {
        let lower = text.to_lowercase();
        let tokens = [
            "free", "nitro", "claim", "airdrop", "giveaway", "wallet", "crypto",
            "steam", "gift", "bonus", "winner", "hack", "cheat",
            "чит", "читы", "скам", "раздача", "халява", "нитро", "дроп", "кошелек"
        ];
        tokens.iter().any(|&token| Self::contains_word(&lower, token))
    }

    #[inline]
    pub fn contains_target_insult(text: &str) -> bool {
        let lower = text.to_lowercase();
        TARGET_INSULTS.iter().any(|&insult| Self::contains_word(&lower, insult))
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

    pub fn is_explicit_real_world_dox_threat(text: &str) -> bool {
        let lower = text.to_lowercase();
        // If it's meta-talk about doxxing, it's NOT a threat
        if Self::is_dox_meta_talk(text) {
            return false;
        }

        // Check combinations of action verb + target PII
        let is_leak_action = lower.contains("leak") || lower.contains("post") || lower.contains("expose")
            || lower.contains("spread") || lower.contains("солью") || lower.contains("выложу")
            || lower.contains("распространю");

        let has_real_pii = lower.contains("address") || lower.contains("home") || lower.contains("phone")
            || lower.contains("photos") || lower.contains("pics") || lower.contains("face")
            || lower.contains("school") || lower.contains("family") || lower.contains("parents")
            || lower.contains("адрес") || lower.contains("номер") || lower.contains("фото")
            || lower.contains("данные") || lower.contains("школу") || lower.contains("родител")
            || lower.contains("info") || lower.contains("information") || lower.contains("инфу") || lower.contains("информаци");

        if is_leak_action && has_real_pii {
            if lower.contains("roblox address") || lower.contains("in-game address") {
                return false;
            }
            return true;
        }

        // Stalking phrases: "know where you live", "find where you live", "знаю где ты живешь", "знаю твой адрес"
        const STALKING_PATTERNS: &[&str] = &[
            "know where you live", "find where you live", "know your address",
            "find your address", "got your address",
            "знаю где ты живешь", "знаю твой адрес", "найду твой адрес", "пробью тебя"
        ];
        if STALKING_PATTERNS.iter().any(|&p| lower.contains(p)) {
            if lower.contains("in roblox") || lower.contains("in-game") || lower.contains("в роблоксе") {
                return false;
            }
            return true;
        }

        // Generic doxx threat ("i will doxx you", "солью инфу", "сдеаноню")
        // ONLY real if NOT in a game context!
        const GENERIC_DOX_THREATS: &[&str] = &[
            "i will doxx you", "im gonna doxx you", "i'll doxx you", "doxx you",
            "i will dox you", "im gonna dox you", "dox you",
            "spread all your info", "spread your info", "spread all your information", "spread your information",
            "солью инфу", "сдеаноню тебя", "сдеаноню", "пробью тебя по базе"
        ];

        let has_generic_dox = GENERIC_DOX_THREATS.iter().any(|&g| lower.contains(g));
        if has_generic_dox {
            if lower.contains("in roblox") || lower.contains("in game") || lower.contains("по игре") || lower.contains("роблокс") {
                return false;
            }
            return true;
        }

        false
    }

    pub fn is_game_hunting_or_pvp_threat(text: &str) -> bool {
        let lower = text.to_lowercase();

        // 1. Check for hunting / avatar killing phrasing (common in Roblox/PvP)
        let has_hunting_phrase = lower.contains("find you and kill you")
            || lower.contains("find u and kill u")
            || lower.contains("find you and kill u")
            || lower.contains("find u and kill you")
            || lower.contains("find you and destroy you")
            || lower.contains("hunt you down and kill you")
            || lower.contains("hunt you down")
            || lower.contains("track you down and kill you")
            || lower.contains("track you down")
            || lower.contains("catch you and kill you")
            || lower.contains("найду тебя и убью")
            || lower.contains("найду и убью")
            || lower.contains("поймаю тебя и убью")
            || lower.contains("выслежу тебя и убью")
            || (lower.contains("найду тебя") && (lower.contains("убью") || lower.contains("уничтожу")))
            || (lower.contains("find you") && (lower.contains("kill") || lower.contains("destroy")))
            || (lower.contains("kill you") && (lower.contains("if you throw") || lower.contains("in 1v1") || lower.contains("if you feed") || lower.contains("if we lose") || lower.contains("if you miss")))
            || (lower.contains("убью") && (lower.contains("если сольешь") || lower.contains("если проиграем") || lower.contains("в 1v1") || lower.contains("1 на 1")))
            || (lower.contains("sending") && (lower.contains("after u") || lower.contains("after you")))
            || lower.contains("aba wb")
            || (lower.contains("we gonna kill you") || lower.contains("we're gonna kill you") || lower.contains("im gonna kill you") || lower.contains("i will kill you"));

        if !has_hunting_phrase {
            return false;
        }

        // 2. Disqualify if message contains explicit real-world physical identifying details or IRL weapons
        const REAL_WORLD_EVIDENCE: &[&str] = &[
            "irl", "in real life", "your house", "your address", "your city", "your street",
            "your school", "your parents", "your phone", "swat", "doxx", "leak", "gun", "knife",
            "в реале", "в жизни", "твой дом", "твой адрес", "твой город", "твою улицу",
            "твою школу", "родител", "твой номер", "деанон", "солью", "пистолет", "нож", "зарежу"
        ];

        !REAL_WORLD_EVIDENCE.iter().any(|&e| lower.contains(e))
    }

    fn is_game_shield_evasion(content: &str) -> bool {
        if Self::is_3rd_party_dev_or_game_critique(content) || Self::is_game_hunting_or_pvp_threat(content) {
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

        if trimmed_lower.starts_with("||") && trimmed_lower.ends_with("||") {
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
            "разорву на куски", "размажу по стенке",
            "done for", "ur done for", "you're done for", "youre done for",
            "ur cooked", "you're cooked", "youre cooked",
            "you're finished", "ur finished", "youre finished",
            "you're toast", "ur toast", "youre toast",
            "dead to me", "it's over for you", "its over for you",
            "тебе хана", "тебе крышка", "тебе конец"
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

    pub fn is_direct_death_wish(text: &str) -> bool {
        let lower = text.to_lowercase();
        const DEATH_WISH_PATTERNS: &[&str] = &[
            "you should die", "hope you die", "go die", "please die", "die idiot", "die noob",
            "kys", "kill yourself", "сдохни", "умри", "убейся", "пошел сдохни"
        ];
        DEATH_WISH_PATTERNS.iter().any(|&p| lower.contains(p))
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

    pub fn is_early_access_query(text: &str) -> bool {
        let clean = text.trim();
        if clean.is_empty() || clean.len() > 150 {
            return false;
        }

        let lower = clean.to_lowercase();

        // Severe forbidden keywords (slurs/doxx/kys should NOT be answered with EA info)
        if Self::contains_slur(&lower) || SEVERE_HARM_KEYWORDS.iter().any(|k| Self::contains_word(&lower, k)) {
            return false;
        }

        let words: Vec<&str> = lower.split_whitespace().collect();
        if words.is_empty() || words.len() > 20 {
            return false;
        }

        static TARGET_REGEX: OnceLock<Regex> = OnceLock::new();
        let target_re = TARGET_REGEX.get_or_init(|| {
            Regex::new(concat!(
                r"(?i)(?:",
                // Standalone EA tokens with repetitions and spacing/punctuation separators (e.g. ea, e a, e.a, e-a, eaaaa)
                r"\b(e+[\s\.\-_/]*a+|е+[\s\.\-_/]*а+)\b|",
                // Early Access variants with letter repetitions (e.g. early access, early accesss, erly acces, earlyacc)
                r"\b(e+a*r+l+y*|е+р+л+и+|э+р+л+и+)[\s\.\-_/]*(a+c+[es]*|а+к+[се]*)\b|",
                // Tester variants with suffixes (e.g. tester, testers, testing, тестер, тестерку)
                r"\b(t+e+s+t+e*r+[s]?|t+e+s+t+i+n+g+|т+е+с+т+е+р+[ыаомек]*)\b",
                r")"
            )).unwrap()
        });

        if !target_re.is_match(&lower) {
            return false;
        }

        static INTENT_REGEX: OnceLock<Regex> = OnceLock::new();
        let intent_re = INTENT_REGEX.get_or_init(|| {
            Regex::new(concat!(
                r"(?i)\b(?:",
                // English question & modal words
                r"h+o+w+|w+h+e+r+e+|w+h+e+n+|c+a+n+|c+o+u+l+d+|w+a+y+s*|",
                // English desires & requests & pleas
                r"w+a+n+t+[a-z]*|w+a+n+n+a+|n+e+e+d+[a-z]*|g+i+v+e+[a-z]*|g+i+m+m+e+|p+l+e+a*s+e*|p+l+[sz]+|",
                // English acquisition verbs
                r"g+e+t+[a-z]*|o+b+t+a+i+n+[a-z]*|a+c+q+u+i+r+e+[a-z]*|j+o+i+n+[a-z]*|e+n+t+e+r+[a-z]*|u+n+l+o+c+k+[a-z]*|b+e+c+o+m+e+|b+e+c+o+m+i+n+g+|",
                // English credentials/keys
                r"k+e+y+s*|p+a+s+s+[a-z]*|c+o+d+e+s*|d+r+o+p+s*|i+n+v+i+t+e+[a-z]*|r+o+l+e+s*|",
                // Russian question words
                r"к+а+к+|г+д+е+|к+о+г+д+а+|",
                // Russian modals & capability
                r"м+о+ж+н+о+|в+о+з+м+о+ж+н+о+|",
                // Russian desires, requests & pleas
                r"х+о+ч+[уа-я]*|н+у+ж+[а-я]*|н+а+д+о+|",
                r"д+а+й+|д+а+й+т+[еe]+|с+к+и+н+ь+[а-я]*|п+о+д+е+л+и+с+ь+|п+о+ж+а+л+у+й+с+т+а+|п+ж+[а-я]*|п+ж+л+с+т+|",
                // Russian acquisition verbs
                r"п+о+л+у+ч+[а-я]*|д+о+с+т+а+[а-я]*|в+з+я+т+ь+|п+о+п+а+с+т+ь+|з+а+й+т+и+|с+т+а+т+ь+|о+т+к+р+ы+т+ь+|",
                // Russian credentials/keys
                r"к+л+ю+ч+[а-я]*|к+о+д+[а-я]*|р+о+л+[а-я]*|п+р+о+п+у+с+к+[а-я]*|и+н+в+а+й+т+[а-я]*|т+е+с+т+е+р+к+[а-я]*",
                r")\b"
            )).unwrap()
        });

        if !intent_re.is_match(&lower) {
            return false;
        }

        true
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

    pub fn format_telemetry_chunk(
        max_score: f64,
        severe_score: f64,
        top_cat: &str,
        is_game_shield: bool,
        is_meta: bool,
        is_pvp_callout: bool,
        author_combined_thought: Option<&str>,
        reply_to: Option<(&str, u64, u64, &str)>,
        pattern_trigger_note: Option<&str>,
    ) -> String {
        let mut t = format!(
            "OpenAI Flag: {} (score: {:.2}, severe: {:.2})",
            if top_cat.is_empty() { "none" } else { top_cat },
            max_score,
            severe_score
        );
        if let Some((rep_author, _, _, rep_text)) = reply_to {
            let short_rep = Self::safe_truncate(rep_text, 50);
            t.push_str(&format!(" | Replying to @{}: \"{}\"", rep_author, short_rep.trim()));
        }
        if let Some(comb) = author_combined_thought {
            let short_comb = Self::safe_truncate(comb, 120);
            t.push_str(&format!(" | Author Recent Thoughts: \"{}\"", short_comb.trim().replace('\n', " // ")));
        }
        if let Some(pat) = pattern_trigger_note {
            t.push_str(&format!(" | 🎯 Pattern Alert: {}", pat));
        }
        if max_score <= 0.20 {
            t.push_str(" | ⚠️ LOW TOXICITY BASELINE (<=0.20)");
        }
        if is_game_shield {
            t.push_str(" | ⚠️ EVASION ALERT (fake-game shield)");
        }
        if is_meta {
            t.push_str(" | ℹ️ META TALK NOTE (rule discussion/quote)");
        }
        if is_pvp_callout {
            t.push_str(" | ℹ️ PVP CALLOUT (in-game tactical directive)");
        }
        t
    }

    pub fn build_batch_transcript_prompt(batch: &[GroqBatchItemRequest]) -> String {
        let count = batch.len();
        let mut all_batch_msgs: Vec<BatchLogEntry> = Vec::new();
        let mut seen_msg_ids: HashSet<u64> = HashSet::new();

        // 1. Gather all transcript entries from items in this batch
        for item in batch {
            for entry in item.batch_transcript.iter() {
                if entry.message_id > 0 && seen_msg_ids.insert(entry.message_id) {
                    all_batch_msgs.push(entry.clone());
                }
            }
        }

        // 2. If batch transcript was empty, fallback to channel_history
        if all_batch_msgs.is_empty() {
            for item in batch {
                for ch in &item.channel_history {
                    if ch.message_id > 0 && seen_msg_ids.insert(ch.message_id) {
                        all_batch_msgs.push(BatchLogEntry {
                            message_id: ch.message_id,
                            channel_id: item.channel_id,
                            channel_name: item.channel_name.clone(),
                            author_id: ch.author_id,
                            author_name: ch.author_name.clone(),
                            content: ch.content.clone(),
                            timestamp_unix: ch.timestamp_unix,
                            reply_to: None,
                        });
                    }
                }
            }
        }

        // 3. Ensure all flagged items in the batch are in the message list
        for item in batch {
            if seen_msg_ids.insert(item.message_id) {
                all_batch_msgs.push(BatchLogEntry {
                    message_id: item.message_id,
                    channel_id: item.channel_id,
                    channel_name: item.channel_name.clone(),
                    author_id: item.author_id,
                    author_name: item.author_name.clone(),
                    content: item.trimmed_content.clone(),
                    timestamp_unix: 0,
                    reply_to: None,
                });
            }
        }

        // 4. Sort messages chronologically by timestamp (stable)
        all_batch_msgs.sort_by_key(|m| m.timestamp_unix);

        // Map message_id to flagged item index
        let mut flagged_map: HashMap<u64, (usize, &GroqBatchItemRequest)> = HashMap::new();
        for (idx, item) in batch.iter().enumerate() {
            flagged_map.insert(item.message_id, (idx + 1, item));
        }

        let mut prompt = String::with_capacity(count * 512 + all_batch_msgs.len() * 128);

        prompt.push_str("══════════════════════════════════════════════════════════════════════════════\n");
        prompt.push_str("CHRONOLOGICAL BATCH LOG (Sequential messages from chat context):\n");
        prompt.push_str("══════════════════════════════════════════════════════════════════════════════\n\n");

        let mut displayed_flagged: HashSet<usize> = HashSet::new();

        for (log_idx, entry) in all_batch_msgs.iter().enumerate() {
            let line_no = log_idx + 1;
            if let Some(&(item_num, ref item)) = flagged_map.get(&entry.message_id) {
                displayed_flagged.insert(item_num);
                prompt.push_str("\n╔══════════════════════════════════════════════════════════════════════════════╗\n");
                prompt.push_str(&format!(
                    "║ >>> [FLAGGED ITEM #{}] <<< @{}: \"{}\"\n",
                    item_num,
                    entry.author_name,
                    entry.content.trim()
                ));
                prompt.push_str(&format!(
                    "║ 🎯 TARGET MESSAGE BEING ANALYZED (Line [{}], Channel: #{}, Author ID: {})\n",
                    line_no, entry.channel_name, entry.author_id
                ));
                if !item.telemetry_chunk.is_empty() {
                    prompt.push_str(&format!("║ ↳ Telemetry: {}\n", item.telemetry_chunk.trim()));
                }
                prompt.push_str("╚══════════════════════════════════════════════════════════════════════════════╝\n\n");
            } else {
                let short_c = Self::safe_truncate(&entry.content, 90);
                let rep_str = if let Some(ref r) = entry.reply_to {
                    format!(" (replying to {})", r)
                } else {
                    String::new()
                };
                prompt.push_str(&format!(
                    "[{}] #{} | @{}: \"{}\"{}\n",
                    line_no,
                    entry.channel_name,
                    entry.author_name,
                    short_c.trim(),
                    rep_str
                ));
            }
        }

        // Guarantee any unplaced flagged item is displayed:
        for (idx, item) in batch.iter().enumerate() {
            let item_num = idx + 1;
            if !displayed_flagged.contains(&item_num) {
                prompt.push_str("\n╔══════════════════════════════════════════════════════════════════════════════╗\n");
                prompt.push_str(&format!(
                    "║ >>> [FLAGGED ITEM #{}] <<< @{}: \"{}\"\n",
                    item_num,
                    item.author_name,
                    item.trimmed_content.trim()
                ));
                prompt.push_str(&format!(
                    "║ 🎯 TARGET MESSAGE BEING ANALYZED (Channel: #{}, Author ID: {})\n",
                    item.channel_name, item.author_id
                ));
                if !item.telemetry_chunk.is_empty() {
                    prompt.push_str(&format!("║ ↳ Telemetry: {}\n", item.telemetry_chunk.trim()));
                }
                prompt.push_str("╚══════════════════════════════════════════════════════════════════════════════╝\n\n");
            }
        }

        prompt.push_str("\n══════════════════════════════════════════════════════════════════════════════\n");
        prompt.push_str("MODERATION EVALUATION TASK (MANDATORY 3-QUESTION REASONING):\n");
        prompt.push_str("For each flagged item or pattern match, ask:\n");
        prompt.push_str("1. Это оскорбляет человека? (Does it target or insult a real person?)\n");
        prompt.push_str("2. Может ли это быть шуткой на сервере где шутят почти всегда? (Could this be a joke/banter/irony on a server where people joke constantly? If so -> ALLOW!)\n");
        prompt.push_str("3. Является ли это грубым нарушением? (Is it a severe violation like real phishing/scam, token stealer, doxxing, death wishes, or racial slurs?)\n");
        prompt.push_str("Autonomously gauge the danger level and output your decision block for EVERY flagged item:\n\n");
        for idx in 0..count {
            let item_num = idx + 1;
            let target_hint = if let Some(req) = batch.get(idx) {
                format!(" -> TARGET TO EVALUATE: @{}: \"{}\"", req.author_name, Self::safe_truncate(&req.trimmed_content, 80))
            } else {
                String::new()
            };
            prompt.push_str(&format!(
                "[ITEM {}]{}\nVERDICT: [ALLOW|SUSPICIOUS|DELETE]\nRULE: [Crit|Major|Minor/Mild|None]\nMUTE_MINUTES: [0|1|15|30|60|120|1440]\nREASON: [concise rationale]\n\n",
                item_num,
                target_hint
            ));
        }

        prompt
    }

    pub async fn check_message(&self, ctx: &MessageContext<'_>) -> ModerationVerdict {
        let preliminary = self.check_message_pipeline(ctx).await;
        match preliminary {
            ModerationVerdict::DeleteConfirmed { .. } => {
                self.consult_supreme_arbiter(ctx, &preliminary).await
            }
            ModerationVerdict::FlagSuspicious { mute_minutes, .. } if mute_minutes > 0 => {
                self.consult_supreme_arbiter(ctx, &preliminary).await
            }
            other => other,
        }
    }

    pub async fn check_message_pipeline(&self, ctx: &MessageContext<'_>) -> ModerationVerdict {
        let trimmed = ctx.content.trim();

        // ── 0. FAST GATE: Local instant bypass (0ms, 0 API) ───────────────────
        if Self::is_fast_whitelisted(trimmed) {
            return ModerationVerdict::Allow;
        }

        let lower_trimmed = trimmed.to_lowercase();
        if self.dynamic_whitelist.read().unwrap().contains(&lower_trimmed) {
            println!("   ↳ [DYNAMIC WHITELIST PASS] Message '{}' matches learned safe phrase -> ALLOW (0ms, 0 tokens)", Self::safe_truncate(trimmed, 40));
            return ModerationVerdict::Allow;
        }

        // Fetch author's recent messages in this channel within 60 seconds
        let author_past_msgs = self.get_author_recent_context(ctx.channel_id, ctx.author_id, ctx.timestamp_unix);
        let has_author_context = !author_past_msgs.is_empty();
        let combined_text = if has_author_context {
            format!("{}\n{}", author_past_msgs.join("\n"), trimmed)
        } else {
            trimmed.to_string()
        };

        // ── 0.5. LOCAL VECTOR SHIELD: Qdrant 120,000 Scam Vectors + MiniLM (<5ms, 0 API tokens) ──
        let vector_res = self.check_vector_engine(trimmed).await;
        let vector_score = vector_res.as_ref().map(|r| r.score).unwrap_or(0.0);
        let mut vector_trigger_info: Option<String> = None;
        let mut is_vector_suspicious = false;

        if let Some(ref res) = vector_res {
            let cat_lower = res.category.to_lowercase();
            let is_hard_scam = res.is_scam && (res.score >= 0.55 || cat_lower.contains("scam") || cat_lower.contains("phish") || cat_lower.contains("fraud") || cat_lower == "local_custom_scam");
            if is_hard_scam {
                is_vector_suspicious = true;
                vector_trigger_info = Some(format!(
                    "Vector DB Match: '{}' (Cat: {}, Sim: {:.1}%)",
                    Self::safe_truncate(&res.matched_text, 50),
                    res.category,
                    res.score * 100.0
                ));
                println!(
                    "\n🎯 [QDRANT VECTOR MATCH -> ESCALATING TO LLM] Score: {:.4} (Threshold: {:.2}) | Cat: {} | Matched: '{}' | Msg: '{}'",
                    res.score, res.threshold, res.category, res.matched_text, trimmed
                );
            }
        }

        if !is_vector_suspicious && has_author_context {
            if let Some(res) = self.check_vector_engine(&combined_text).await {
                let cat_lower = res.category.to_lowercase();
                let is_hard_scam = res.is_scam && (res.score >= 0.55 || cat_lower.contains("scam") || cat_lower.contains("phish") || cat_lower.contains("fraud") || cat_lower == "local_custom_scam");
                if is_hard_scam {
                    is_vector_suspicious = true;
                    vector_trigger_info = Some(format!(
                        "Split-Message Vector Match: '{}' (Cat: {}, Sim: {:.1}%)",
                        Self::safe_truncate(&res.matched_text, 50),
                        res.category,
                        res.score * 100.0
                    ));
                    println!(
                        "\n🎯 [QDRANT SPLIT VECTOR MATCH -> ESCALATING TO LLM] Score: {:.4} (Threshold: {:.2}) | Cat: {} | Matched: '{}' | Msg: '{}'",
                        res.score, res.threshold, res.category, res.matched_text, combined_text
                    );
                }
            }
        }

        // ── 1. TIER 1: OpenAI Moderation (omni-moderation-latest, $0) ────────
        let inputs: Vec<String> = if has_author_context {
            vec![trimmed.to_string(), combined_text.clone()]
        } else {
            vec![trimmed.to_string()]
        };

        let meta = BatchLogEntry {
            message_id: ctx.message_id,
            channel_id: ctx.channel_id,
            channel_name: ctx.channel_name.clone().unwrap_or_else(|| "general".to_string()),
            author_id: ctx.author_id,
            author_name: ctx.author_name.to_string(),
            content: trimmed.to_string(),
            timestamp_unix: ctx.timestamp_unix,
            reply_to: ctx.reply_to.map(|(a, _, _, t)| format!("@{}: {}", a, Self::safe_truncate(t, 50))),
        };

        let (scores_list, batch_transcript) = if let Some(openai_key) = &self.openai_key {
            self.check_openai_batch(openai_key, inputs, Some(meta)).await
        } else {
            (vec![], Arc::new(vec![]))
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

        let has_link = Self::contains_suspicious_link(trimmed);
        let has_suspicious_kw = Self::contains_suspicious_keywords(trimmed);
        let has_insult = Self::contains_target_insult(trimmed);
        let is_vector_suspicious = is_vector_suspicious || (vector_score >= 0.70 && vector_res.as_ref().map(|r| r.is_scam).unwrap_or(false));

        // ── Native SIMD + Sliding Window + Levenshtein Profanity & Threat Scan ──
        let profanity_hit = self.profanity_engine.scan(trimmed);
        let has_profanity = profanity_hit.is_some();
        let has_severe_harm_keyword = has_severe_harm_keyword
            || profanity_hit.as_ref().map(|p| p.is_severe_root && (p.matched_rule.contains("убей") || p.matched_rule.contains("сдох") || p.matched_rule.contains("пристрел") || p.matched_rule.contains("зареж") || p.matched_rule.contains("kill") || p.matched_rule.contains("kys"))).unwrap_or(false);

        let mut pattern_notes: Vec<String> = Vec::new();
        if let Some(ref v_info) = vector_trigger_info {
            pattern_notes.push(v_info.clone());
        }
        if let Some(ref p_hit) = profanity_hit {
            println!(
                "   🛡️ [NATIVE PROFANITY/THREAT ENGINE] Hit rule '{}' on token '{}' (obfuscated: {}, fuzzy: {}, severe: {})",
                p_hit.matched_rule, p_hit.detected_token, p_hit.is_obfuscated, p_hit.is_fuzzy, p_hit.is_severe_root
            );
            pattern_notes.push(format!("Profanity/Threat Pattern: '{}'", p_hit.matched_rule));
        }
        if has_link {
            pattern_notes.push("Suspicious link/URL".to_string());
        }
        if has_suspicious_kw {
            pattern_notes.push("Scam/cheat keyword".to_string());
        }
        if has_insult {
            pattern_notes.push("Targeted insult".to_string());
        }
        if has_severe_harm_keyword {
            pattern_notes.push("Severe harm keyword".to_string());
        }
        if has_dox_threat {
            pattern_notes.push("Doxxing/threat pattern".to_string());
        }
        if has_slur {
            pattern_notes.push("Slur/hate pattern".to_string());
        }
        let pattern_alert_str = if !pattern_notes.is_empty() {
            Some(pattern_notes.join("; "))
        } else {
            None
        };
        let is_any_pattern_triggered = !pattern_notes.is_empty() || is_vector_suspicious;

        // 1A. Clear clean content -> Instant ALLOW only if:
        // - NO suspicious link
        // - NO suspicious crypto/nitro/scam/cheat keywords
        // - NO targeted insults
        // - NO native profanity/threat patterns
        // - NO moderate vector similarity from Qdrant
        // - NO severe harm keywords, provocative bait, dox threats, slurs, or game shield evasion
        if !has_link
            && !has_suspicious_kw
            && !has_insult
            && !has_profanity
            && !is_vector_suspicious
            && !is_any_pattern_triggered
            && !has_severe_harm_keyword
            && !has_provocative_bait
            && !has_dox_threat
            && !has_slur
            && !is_game_shield
            && !is_shut_up
            && (max_score < OPENAI_SAFE_THRESHOLD || scores_list.is_empty())
        {
            println!("   ↳ [SAFE CHAT ALLOW] Clean message ('{}') -> ALLOW (0 tokens spent)", Self::safe_truncate(trimmed, 40));
            return ModerationVerdict::Allow;
        }

        // 1A-2. ROBLOX & GAMING PVP / HUNTING BANTER:
        // On this server, phrases like 'I will find you and kill you', 'найду тебя и убью в роблоксе'
        // are standard in-game hunting trashtalk unless paired with real-world PII/stalking or slurs.
        if Self::is_game_hunting_or_pvp_threat(trimmed) && !has_slur && !Self::is_explicit_real_world_dox_threat(trimmed) && !has_link && !has_suspicious_kw && !is_vector_suspicious && !has_insult && !is_any_pattern_triggered {
            println!("   ↳ [ROBLOX HUNTING ALLOW] In-game hunting trashtalk ('{}') -> ALLOW (0 tokens spent)", trimmed);
            return ModerationVerdict::Allow;
        }

        // 1A-3. THEATRICAL HYPERBOLE & PLAYFUL POST-IRONIC BANTER:
        if Self::is_theatrical_hyperbole(trimmed) && !has_slur && !has_dox_threat && !has_link && !has_suspicious_kw && !is_vector_suspicious && !has_insult {
            println!("   ↳ [THEATRICAL HYPERBOLE ALLOW] Playful post-ironic banter ('{}') -> ALLOW (0 tokens spent)", trimmed);
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
            || is_game_shield
            || has_link
            || is_vector_suspicious
            || has_profanity
            || is_any_pattern_triggered;

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
                let telemetry_chunk = Self::format_telemetry_chunk(
                    max_score,
                    severe_score,
                    &top_cat,
                    is_game_shield,
                    is_meta,
                    is_pvp_callout,
                    if has_author_context { Some(combined_text.as_str()) } else { None },
                    ctx.reply_to,
                    pattern_alert_str.as_deref(),
                );

                match self.evaluate_via_groq_batch(
                    ctx.message_id,
                    ctx.channel_id,
                    ctx.channel_name.clone().unwrap_or_else(|| "general".to_string()),
                    ctx.author_name.to_string(),
                    ctx.author_id,
                    trimmed.to_string(),
                    telemetry_chunk,
                    is_hardcore_or_drama,
                    batch_transcript.clone(),
                    history.clone(),
                ).await {
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
                            if Self::is_direct_death_wish(trimmed) && !is_meta {
                                println!("   🚨 [DEATH WISH GUARD] Overriding LLM ALLOW for direct death wish ('{}') -> SUSPICIOUS(30m)", trimmed);
                                return ModerationVerdict::FlagSuspicious {
                                    reason: format!("Direct death wish/suicide incitement: \"{}\"", trimmed),
                                    score: if max_score > 0.3 { max_score } else { 0.8 },
                                    category: "harassment/threatening".to_string(),
                                    model_used: format!("Death Wish Guard ({})", model_used),
                                    rule_violated: "Crit (Death Wishes)".to_string(),
                                    mute_minutes: 30,
                                };
                            }
                            let is_split_real_threat = (combined_lower.contains("find you") || combined_lower.contains("find u") || combined_lower.contains("найду"))
                                && (combined_lower.contains("shoot") || combined_lower.contains("пристрелю") || combined_lower.contains("stab") || combined_lower.contains("зарежу"))
                                && !is_meta
                                && !Self::is_game_hunting_or_pvp_threat(&combined_text);
                            if is_split_real_threat {
                                println!("   🚨 [SPLIT THREAT GUARD] Overriding LLM ALLOW for author-split physical threat ('{}') -> DeleteConfirmed(120m)", combined_text);
                                return ModerationVerdict::DeleteConfirmed {
                                    reason: format!("Split physical violence/stalking threat detected across messages: \"{}\"", combined_text),
                                    score: 0.95,
                                    category: "violence".to_string(),
                                    model_used: format!("Split Threat Guard ({})", model_used),
                                    rule_violated: "Major (Threats/Harm)".to_string(),
                                    mute_minutes: 120,
                                };
                            }
                            let has_extreme_harm = EXTREME_REAL_HARM_KEYWORDS.iter().any(|k| lower.contains(k));
                            if has_extreme_harm && !is_meta && !Self::is_theatrical_hyperbole(trimmed) {
                                println!("   🚨 [EXTREME HARM GUARD] Overriding LLM ALLOW for extreme violence threat ('{}') -> DeleteConfirmed(120m)", trimmed);
                                return ModerationVerdict::DeleteConfirmed {
                                    reason: format!("Extreme real-world violence threat detected: \"{}\"", trimmed),
                                    score: if max_score > 0.5 { max_score } else { 0.95 },
                                    category: "violence".to_string(),
                                    model_used: format!("Extreme Harm Guard ({})", model_used),
                                    rule_violated: "Major (Threats/Harm)".to_string(),
                                    mute_minutes: 120,
                                };
                            }
                            if has_dox_threat && !is_meta && Self::is_explicit_real_world_dox_threat(trimmed) {
                                println!("   🚨 [DOX THREAT GUARD] Overriding LLM ALLOW for explicit doxx/extortion threat ('{}') -> DeleteConfirmed(120m)", trimmed);
                                return ModerationVerdict::DeleteConfirmed {
                                    reason: format!("Doxxing, extortion or threat to leak private personal information detected: \"{}\"", trimmed),
                                    score: if max_score > 0.5 { max_score } else { 0.95 },
                                    category: "harassment/threatening".to_string(),
                                    model_used: format!("Dox Threat Guard ({})", model_used),
                                    rule_violated: "Major (Threats/Doxx)".to_string(),
                                    mute_minutes: 120,
                                };
                            }
                            println!("   ✅ [ALLOW] Message allowed by LLM.");
                            self.add_to_whitelist(trimmed).await;
                            return ModerationVerdict::Allow;
                        } else if is_meta && !is_directed {
                            println!("   🛡️ [META GUARD] Overriding LLM {} on undirected meta-discussion / quote to ALLOW.", decision.verdict);
                            self.add_to_whitelist(trimmed).await;
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
                            self.add_to_whitelist(trimmed).await;
                            return ModerationVerdict::Allow;
                        } else if Self::is_standalone_profanity(trimmed) && !has_severe_harm_keyword && !has_dox_threat && !has_slur && !is_game_shield {
                            println!("   🛡️ [BANTER GUARD] Overriding LLM {} on standalone profanity ('{}') to ALLOW.", decision.verdict, trimmed);
                            self.add_to_whitelist(trimmed).await;
                            return ModerationVerdict::Allow;
                        } else if Self::is_theatrical_hyperbole(trimmed) && !has_dox_threat && !has_slur && !is_game_shield {
                            println!("   🎭 [POST-IRONY GUARD] Overriding LLM {} on theatrical hyperbole ('{}') to ALLOW.", decision.verdict, trimmed);
                            self.add_to_whitelist(trimmed).await;
                            return ModerationVerdict::Allow;
                        } else if Self::is_dox_meta_talk(trimmed) && !has_slur && !has_dox_threat {
                            println!("   🛡️ [DOXX META GUARD] Overriding LLM {} on doxx meta-talk/observation ('{}') to ALLOW.", decision.verdict, trimmed);
                            self.add_to_whitelist(trimmed).await;
                            return ModerationVerdict::Allow;
                        } else if Self::is_3rd_party_dev_or_game_critique(trimmed) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                            println!("   🛡️ [DEV CRITIQUE GUARD] Overriding LLM {} on developer/game critique ('{}') to ALLOW.", decision.verdict, trimmed);
                            self.add_to_whitelist(trimmed).await;
                            return ModerationVerdict::Allow;
                        } else if (Self::is_drama_or_gossip(trimmed) || decision.rule.to_lowercase().contains("drama") || decision.reason.to_lowercase().contains("drama incitement")) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                            println!("   🛡️ [DRAMA / GOSSIP GUARD] Overriding LLM {} on gossip / drama rumor ('{}') to ALLOW.", decision.verdict, trimmed);
                            self.add_to_whitelist(trimmed).await;
                            return ModerationVerdict::Allow;
                        } else if Self::is_game_hunting_or_pvp_threat(trimmed) && !has_slur && !has_dox_threat {
                            println!("   🎮 [ROBLOX HUNTING GUARD] Overriding LLM {} on in-game hunting banter ('{}') to ALLOW.", decision.verdict, trimmed);
                            self.add_to_whitelist(trimmed).await;
                            return ModerationVerdict::Allow;
                        } else if decision.rule.to_lowercase().contains("dox")
                            && !decision.rule.to_lowercase().contains("threat")
                            && !has_severe_harm_keyword
                            && !decision.reason.to_lowercase().contains("threat")
                            && !Self::is_explicit_real_world_dox_threat(trimmed) {
                            println!("   🛡️ [DOXX GUARD] Overriding LLM {} on non-explicit doxx rule ('{}') to ALLOW.", decision.verdict, trimmed);
                            self.add_to_whitelist(trimmed).await;
                            return ModerationVerdict::Allow;
                        } else if max_score <= 0.20 && !has_slur && !has_provocative_bait && !has_severe_harm_keyword && !Self::is_explicit_real_world_dox_threat(trimmed) {
                            if decision.rule.contains("Minor") || decision.rule.contains("Mild") || decision.rule.to_lowercase().contains("drama") || decision.rule.to_lowercase().contains("harassment") || decision.mute_minutes <= 15 {
                                println!("   🛡️ [LOW TOXICITY GUARD] Overriding LLM {} on low-toxicity message (score {:.2} <= 0.20, rule '{}') to ALLOW.", decision.verdict, max_score, decision.rule);
                                self.add_to_whitelist(trimmed).await;
                                return ModerationVerdict::Allow;
                            }
                        } else if decision.verdict.contains("DELETE") {
                            if has_provocative_bait && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                                println!("   ⚠️ [PROVOCATIVE BAIT GUARD] Overriding LLM DELETE on provocative bait ('{}') -> SUSPICIOUS(1m)", trimmed);
                                return ModerationVerdict::FlagSuspicious {
                                    reason: format!("Provocative gender bait / trolling: \"{}\"", trimmed),
                                    score: if max_score > 0.3 { max_score } else { 0.5 },
                                    category: "harassment".to_string(),
                                    model_used: format!("Bait Guard ({})", model_used),
                                    rule_violated: "Minor/Mild (Provocative Bait)".to_string(),
                                    mute_minutes: 1,
                                };
                            }
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
                            if is_vector_suspicious || decision.reason.to_lowercase().contains("scam") || decision.rule.to_lowercase().contains("scam") {
                                let model_label = if model_used.contains("120b") {
                                    format!("OpenAI + {} (120B Deep Drama Arbiter)", model_used)
                                } else if model_used.contains("20b") {
                                    format!("OpenAI + {} (20B Safety Arbiter)", model_used)
                                } else {
                                    format!("OpenAI + {} Guard", model_used)
                                };
                                println!("   🚨 [SCAM PURGE] Confirmed scam pattern in flagged message -> DELETE({}m)", if decision.mute_minutes > 0 { decision.mute_minutes } else { 120 });
                                return ModerationVerdict::DeleteConfirmed {
                                    reason: format!("Scam / phishing link: {}", decision.reason),
                                    score: if max_score > 0.5 { max_score } else { 0.95 },
                                    category: "scam".to_string(),
                                    model_used: model_label,
                                    rule_violated: "Major (Scam/Phishing)".to_string(),
                                    mute_minutes: if decision.mute_minutes > 0 { decision.mute_minutes } else { 120 },
                                };
                            }
                            let mut effective_mute = if has_provocative_bait && !has_slur && !has_dox_threat && !has_severe_harm_keyword { 1 } else { decision.mute_minutes };
                            let mut effective_rule = if has_provocative_bait && !has_slur && !has_dox_threat && !has_severe_harm_keyword { "Minor/Mild (Provocative Bait)".to_string() } else { decision.rule };

                            // Prevent false Crit 1440m on casual slang like 'u get loads of hoes'
                            if (effective_rule.contains("Crit") || effective_mute >= 1440) && !has_slur {
                                let lower_msg = trimmed.to_lowercase();
                                if lower_msg.contains("hoes") || lower_msg.contains("hoe") || lower_msg.contains("thot") || lower_msg.contains("simp") {
                                    if lower_msg.contains("get loads of hoes") || lower_msg.contains("loads of hoes") || lower_msg.contains("no hoes") || lower_msg.contains("bros before hoes") || lower_msg.contains("got hoes") || lower_msg.contains("hoes mad") {
                                        println!("   🛡️ [SLANG BANTER GUARD] Overriding false Crit on casual slang ('{}') to ALLOW.", trimmed);
                                        self.add_to_whitelist(trimmed).await;
                                        return ModerationVerdict::Allow;
                                    } else {
                                        effective_mute = 10;
                                        effective_rule = "Minor/Mod (Slang)".to_string();
                                    }
                                }
                            }
                            println!("   ⚠️ [AI VERDICT: SUSPICIOUS] Flagged for mod review + auto-timeout: {}m (Rule: {})", effective_mute, effective_rule);
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
                                rule_violated: effective_rule,
                                mute_minutes: effective_mute,
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
        if Self::is_3rd_party_dev_or_game_critique(trimmed) && !has_severe_harm_keyword && !has_dox_threat && !has_slur && !is_vector_suspicious && !is_any_pattern_triggered && max_score < 0.75 {
            println!("   ↳ [DEV CRITIQUE PRE-FILTER] 3rd-party dev / game critique (score {:.2}) -> ALLOW (0 tokens spent)", max_score);
            return ModerationVerdict::Allow;
        }
        if Self::is_dox_meta_talk(trimmed) && !has_dox_threat && !has_slur && !is_vector_suspicious && !is_any_pattern_triggered && max_score < 0.75 {
            println!("   ↳ [DOXX META PRE-FILTER] Meta-talk about doxxing (score {:.2}) -> ALLOW (0 tokens spent)", max_score);
            return ModerationVerdict::Allow;
        }
        if Self::is_drama_or_gossip(trimmed) && !has_severe_harm_keyword && !has_dox_threat && !has_slur && !is_vector_suspicious && !is_any_pattern_triggered && max_score < 0.75 {
            println!("   ↳ [DRAMA PRE-FILTER] Chat gossip / drama rumor (score {:.2}) -> ALLOW (0 tokens spent)", max_score);
            return ModerationVerdict::Allow;
        }

        // ONLY bypass if it's general non-violent gaming frustration (e.g. "fuck this lag")
        if !is_directed
            && !has_link
            && !has_suspicious_kw
            && !has_insult
            && !is_vector_suspicious
            && !is_any_pattern_triggered
            && !is_violent_category
            && !has_severe_harm_keyword
            && !has_provocative_bait
            && !has_dox_threat
            && !has_slur
            && !is_game_shield
            && max_score < 0.60
        {
            println!("   ↳ [PRE-FILTER] General gaming frustration / non-directed (score {:.2}) -> ALLOW (0 tokens spent)", max_score);
            return ModerationVerdict::Allow;
        }

        if self.groq_keys.is_empty() && self.gemini_keys.is_empty() {
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
            if is_vector_suspicious {
                return ModerationVerdict::DeleteConfirmed {
                    reason: vector_trigger_info.clone().unwrap_or_else(|| "Local Vector / Scam Pattern Match".to_string()),
                    score: if vector_score > 0.1 { vector_score } else { 0.85 },
                    category: "vector_db/scam".to_string(),
                    model_used: "Local Vector Shield Fallback".to_string(),
                    rule_violated: "Major (Scam/Phishing)".to_string(),
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
        let telemetry_chunk = Self::format_telemetry_chunk(
            max_score,
            severe_score,
            &top_cat,
            is_game_shield,
            is_meta,
            is_pvp_callout,
            if has_author_context { Some(combined_text.as_str()) } else { None },
            ctx.reply_to,
            pattern_alert_str.as_deref(),
        );

        match self.evaluate_via_groq_batch(
            ctx.message_id,
            ctx.channel_id,
            ctx.channel_name.clone().unwrap_or_else(|| "general".to_string()),
            ctx.author_name.to_string(),
            ctx.author_id,
            trimmed.to_string(),
            telemetry_chunk,
            is_hardcore_or_drama,
            batch_transcript.clone(),
            history.clone(),
        ).await {
            Ok((decision, model_used, elapsed_ms)) => {
                println!(
                    "   ⚡ [AI RESPONSE] Model: {} (took {}ms) | Verdict: {} | Rule: {} | Mute: {}m | Reason: \"{}\"",
                    model_used, elapsed_ms, decision.verdict, decision.rule, decision.mute_minutes, decision.reason
                );
                if decision.verdict.contains("DELETE") || (has_slur && !is_meta) {
                    if has_provocative_bait && !has_slur && !has_dox_threat && !has_severe_harm_keyword && !is_meta {
                        println!("   ⚠️ [PROVOCATIVE BAIT GUARD] Overriding LLM DELETE on provocative bait ('{}') -> SUSPICIOUS(1m)", trimmed);
                        return ModerationVerdict::FlagSuspicious {
                            reason: format!("Provocative gender bait / trolling: \"{}\"", trimmed),
                            score: if max_score > 0.3 { max_score } else { 0.5 },
                            category: "harassment".to_string(),
                            model_used: format!("Bait Guard ({})", model_used),
                            rule_violated: "Minor/Mild (Provocative Bait)".to_string(),
                            mute_minutes: 1,
                        };
                    }
                    if is_meta && !is_directed {
                        println!("   🛡️ [META GUARD] Overriding LLM {} on undirected meta-discussion / quote to ALLOW.", decision.verdict);
                        self.add_to_whitelist(trimmed).await;
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
                        self.add_to_whitelist(trimmed).await;
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_standalone_profanity(trimmed) && !has_severe_harm_keyword && !has_dox_threat && !has_slur && !is_game_shield {
                        println!("   🛡️ [BANTER GUARD] Overriding LLM DELETE on standalone profanity ('{}') to ALLOW.", trimmed);
                        self.add_to_whitelist(trimmed).await;
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_theatrical_hyperbole(trimmed) && !has_dox_threat && !has_slur && !is_game_shield {
                        println!("   🎭 [POST-IRONY GUARD] Overriding LLM DELETE on theatrical hyperbole ('{}') to ALLOW.", trimmed);
                        self.add_to_whitelist(trimmed).await;
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_dox_meta_talk(trimmed) && !has_slur && !has_dox_threat {
                        println!("   🛡️ [DOXX META GUARD] Overriding LLM DELETE on doxx meta-talk/observation to ALLOW.");
                        self.add_to_whitelist(trimmed).await;
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_3rd_party_dev_or_game_critique(trimmed) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                        println!("   🛡️ [DEV CRITIQUE GUARD] Overriding LLM DELETE on developer/game critique to ALLOW.");
                        self.add_to_whitelist(trimmed).await;
                        return ModerationVerdict::Allow;
                    }
                    if (Self::is_drama_or_gossip(trimmed) || decision.rule.to_lowercase().contains("drama") || decision.reason.to_lowercase().contains("drama incitement")) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                        println!("   🛡️ [DRAMA / GOSSIP GUARD] Overriding LLM DELETE on gossip / drama rumor to ALLOW.");
                        self.add_to_whitelist(trimmed).await;
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_game_hunting_or_pvp_threat(trimmed) && !has_slur && !has_dox_threat {
                        println!("   🎮 [ROBLOX HUNTING GUARD] Overriding LLM DELETE on in-game hunting banter ('{}') to ALLOW.", trimmed);
                        self.add_to_whitelist(trimmed).await;
                        return ModerationVerdict::Allow;
                    }
                    if decision.rule.to_lowercase().contains("dox")
                        && !decision.rule.to_lowercase().contains("threat")
                        && !has_severe_harm_keyword
                        && !decision.reason.to_lowercase().contains("threat")
                        && !Self::is_explicit_real_world_dox_threat(trimmed) {
                        println!("   🛡️ [DOXX GUARD] Overriding LLM DELETE on non-explicit doxx rule ('{}') to ALLOW.", trimmed);
                        self.add_to_whitelist(trimmed).await;
                        return ModerationVerdict::Allow;
                    }
                    if max_score <= 0.20
                        && !has_link
                        && !has_suspicious_kw
                        && !has_insult
                        && !is_vector_suspicious
                        && !has_slur
                        && !has_provocative_bait
                        && !has_severe_harm_keyword
                        && !Self::is_explicit_real_world_dox_threat(trimmed)
                    {
                        if decision.rule.contains("Minor") || decision.rule.contains("Mild") || decision.rule.to_lowercase().contains("drama") || decision.rule.to_lowercase().contains("harassment") || decision.mute_minutes <= 15 {
                            println!("   🛡️ [LOW TOXICITY GUARD] Overriding LLM DELETE on low-toxicity message (score {:.2} <= 0.20, rule '{}') to ALLOW.", max_score, decision.rule);
                            self.add_to_whitelist(trimmed).await;
                            return ModerationVerdict::Allow;
                        }
                    }
                    let mut effective_mute = if has_slur { 1440 } else { decision.mute_minutes };
                    let mut effective_rule = if has_slur { "Crit (Slurs)".to_string() } else { decision.rule };

                    // Prevent false Crit 1440m on casual slang like 'u get loads of hoes'
                    if (effective_rule.contains("Crit") || effective_mute >= 1440) && !has_slur {
                        let lower_msg = trimmed.to_lowercase();
                        if lower_msg.contains("hoes") || lower_msg.contains("hoe") || lower_msg.contains("thot") || lower_msg.contains("simp") {
                            if lower_msg.contains("get loads of hoes") || lower_msg.contains("loads of hoes") || lower_msg.contains("no hoes") || lower_msg.contains("bros before hoes") || lower_msg.contains("got hoes") || lower_msg.contains("hoes mad") {
                                println!("   🛡️ [SLANG BANTER GUARD] Overriding false Crit on casual slang ('{}') to ALLOW.", trimmed);
                                self.add_to_whitelist(trimmed).await;
                                return ModerationVerdict::Allow;
                            } else {
                                effective_mute = 10;
                                effective_rule = "Minor/Mod (Slang)".to_string();
                            }
                        }
                    }
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
                        self.add_to_whitelist(trimmed).await;
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
                        self.add_to_whitelist(trimmed).await;
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_standalone_profanity(trimmed) && !has_severe_harm_keyword && !has_dox_threat && !has_slur && !is_game_shield {
                        println!("   🛡️ [BANTER GUARD] Overriding LLM SUSPICIOUS on standalone profanity ('{}') to ALLOW.", trimmed);
                        self.add_to_whitelist(trimmed).await;
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_theatrical_hyperbole(trimmed) && !has_dox_threat && !has_slur && !is_game_shield {
                        println!("   🎭 [POST-IRONY GUARD] Overriding LLM SUSPICIOUS on theatrical hyperbole ('{}') to ALLOW.", trimmed);
                        self.add_to_whitelist(trimmed).await;
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_dox_meta_talk(trimmed) && !has_slur && !has_dox_threat {
                        println!("   🛡️ [DOXX META GUARD] Overriding LLM SUSPICIOUS on doxx meta-talk/observation to ALLOW.");
                        self.add_to_whitelist(trimmed).await;
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_3rd_party_dev_or_game_critique(trimmed) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                        println!("   🛡️ [DEV CRITIQUE GUARD] Overriding LLM SUSPICIOUS on developer/game critique to ALLOW.");
                        self.add_to_whitelist(trimmed).await;
                        return ModerationVerdict::Allow;
                    }
                    if (Self::is_drama_or_gossip(trimmed) || decision.rule.to_lowercase().contains("drama") || decision.reason.to_lowercase().contains("drama incitement")) && !has_slur && !has_dox_threat && !has_severe_harm_keyword {
                        println!("   🛡️ [DRAMA / GOSSIP GUARD] Overriding LLM SUSPICIOUS on gossip / drama rumor to ALLOW.");
                        self.add_to_whitelist(trimmed).await;
                        return ModerationVerdict::Allow;
                    }
                    if Self::is_game_hunting_or_pvp_threat(trimmed) && !has_slur && !has_dox_threat {
                        println!("   🎮 [ROBLOX HUNTING GUARD] Overriding LLM SUSPICIOUS on in-game hunting banter ('{}') to ALLOW.", trimmed);
                        self.add_to_whitelist(trimmed).await;
                        return ModerationVerdict::Allow;
                    }
                    if decision.rule.to_lowercase().contains("dox") && !Self::is_explicit_real_world_dox_threat(trimmed) {
                        println!("   🛡️ [DOXX GUARD] Overriding LLM SUSPICIOUS on non-explicit doxx rule ('{}') to ALLOW.", trimmed);
                        self.add_to_whitelist(trimmed).await;
                        return ModerationVerdict::Allow;
                    }
                    if max_score <= 0.20 && !has_slur && !has_provocative_bait && !has_severe_harm_keyword && !Self::is_explicit_real_world_dox_threat(trimmed) {
                        if decision.rule.contains("Minor") || decision.rule.contains("Mild") || decision.rule.to_lowercase().contains("drama") || decision.rule.to_lowercase().contains("harassment") || decision.mute_minutes <= 15 {
                            println!("   🛡️ [LOW TOXICITY GUARD] Overriding LLM SUSPICIOUS on low-toxicity message (score {:.2} <= 0.20, rule '{}') to ALLOW.", max_score, decision.rule);
                            self.add_to_whitelist(trimmed).await;
                            return ModerationVerdict::Allow;
                        }
                    }
                    if is_vector_suspicious || decision.reason.to_lowercase().contains("scam") || decision.rule.to_lowercase().contains("scam") {
                        let model_label = if model_used.contains("120b") {
                            format!("{} (120B Deep Drama Arbiter)", model_used)
                        } else if model_used.contains("20b") {
                            format!("{} (20B Safety Arbiter)", model_used)
                        } else {
                            format!("{} (Fast Context)", model_used)
                        };
                        println!("   🚨 [SCAM PURGE] Confirmed scam pattern in flagged message -> DELETE({}m)", if decision.mute_minutes > 0 { decision.mute_minutes } else { 120 });
                        return ModerationVerdict::DeleteConfirmed {
                            reason: format!("Scam / phishing link: {}", decision.reason),
                            score: if max_score > 0.5 { max_score } else { 0.95 },
                            category: "scam".to_string(),
                            model_used: model_label,
                            rule_violated: "Major (Scam/Phishing)".to_string(),
                            mute_minutes: if decision.mute_minutes > 0 { decision.mute_minutes } else { 120 },
                        };
                    }
                    let effective_mute = if has_provocative_bait && !has_slur && !has_dox_threat && !has_severe_harm_keyword { 1 } else { decision.mute_minutes };
                    let effective_rule = if has_provocative_bait && !has_slur && !has_dox_threat && !has_severe_harm_keyword { "Minor/Mild (Provocative Bait)".to_string() } else { decision.rule };
                    println!("   ⚠️ [AI VERDICT: SUSPICIOUS] Flagged grey-zone violation! Mute: {}m (Rule: {})", effective_mute, effective_rule);
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
                        rule_violated: effective_rule,
                        mute_minutes: effective_mute,
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
                    if Self::is_direct_death_wish(trimmed) && !is_meta {
                        println!("   🚨 [DEATH WISH GUARD] Overriding LLM ALLOW for direct death wish ('{}') -> SUSPICIOUS(30m)", trimmed);
                        return ModerationVerdict::FlagSuspicious {
                            reason: format!("Direct death wish/suicide incitement: \"{}\"", trimmed),
                            score: if max_score > 0.3 { max_score } else { 0.8 },
                            category: "harassment/threatening".to_string(),
                            model_used: format!("Death Wish Guard ({})", model_used),
                            rule_violated: "Crit (Death Wishes)".to_string(),
                            mute_minutes: 30,
                        };
                    }
                    let is_split_real_threat = (combined_lower.contains("find you") || combined_lower.contains("find u") || combined_lower.contains("найду"))
                        && (combined_lower.contains("shoot") || combined_lower.contains("пристрелю") || combined_lower.contains("stab") || combined_lower.contains("зарежу"))
                        && !is_meta
                        && !Self::is_game_hunting_or_pvp_threat(&combined_text);
                    if is_split_real_threat {
                        println!("   🚨 [SPLIT THREAT GUARD] Overriding LLM ALLOW for author-split physical threat ('{}') -> DeleteConfirmed(120m)", combined_text);
                        return ModerationVerdict::DeleteConfirmed {
                            reason: format!("Split physical violence/stalking threat detected across messages: \"{}\"", combined_text),
                            score: 0.95,
                            category: "violence".to_string(),
                            model_used: format!("Split Threat Guard ({})", model_used),
                            rule_violated: "Major (Threats/Harm)".to_string(),
                            mute_minutes: 120,
                        };
                    }
                    let has_extreme_harm = EXTREME_REAL_HARM_KEYWORDS.iter().any(|k| lower.contains(k));
                    if has_extreme_harm && !is_meta && !Self::is_theatrical_hyperbole(trimmed) {
                        println!("   🚨 [EXTREME HARM GUARD] Overriding LLM ALLOW for extreme violence threat ('{}') -> DeleteConfirmed(120m)", trimmed);
                        return ModerationVerdict::DeleteConfirmed {
                            reason: format!("Extreme real-world violence threat detected: \"{}\"", trimmed),
                            score: if max_score > 0.5 { max_score } else { 0.95 },
                            category: "violence".to_string(),
                            model_used: format!("Extreme Harm Guard ({})", model_used),
                            rule_violated: "Major (Threats/Harm)".to_string(),
                            mute_minutes: 120,
                        };
                    }
                    println!("   ✅ [ALLOW] Grey-zone message allowed by LLM.");
                    self.add_to_whitelist(trimmed).await;
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
                if Self::is_game_hunting_or_pvp_threat(trimmed) && !has_slur && !has_dox_threat {
                    println!("   🎮 [ROBLOX HUNTING GUARD] Grey-zone failover fallback on in-game hunting banter ('{}') -> ALLOW.", trimmed);
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
                if (lower.contains("kill all") || lower.contains("убить всех") || (lower.contains("kill") && (lower.contains("blacks") || lower.contains("jews") || lower.contains("gays") || lower.contains("trans")))) && !is_meta {
                    return ModerationVerdict::DeleteConfirmed {
                        reason: format!("Hate speech inciting violence against protected group: \"{}\"", trimmed),
                        score: 0.99,
                        category: "hate".to_string(),
                        model_used: "Hate Speech Guard (Failover)".to_string(),
                        rule_violated: "Crit (Hate Speech)".to_string(),
                        mute_minutes: 1440,
                    };
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

    pub async fn check_openai_batch(
        &self,
        api_key: &str,
        inputs: Vec<String>,
        meta: Option<BatchLogEntry>,
    ) -> (Vec<OpenAiScores>, Arc<Vec<BatchLogEntry>>) {
        let expected_len = inputs.len();
        if let Some(ref tx) = self.batch_tx {
            let (resp_tx, resp_rx) = oneshot::channel();
            let default_meta = BatchLogEntry {
                message_id: 0,
                channel_id: 0,
                channel_name: "general".to_string(),
                author_id: 0,
                author_name: "unknown".to_string(),
                content: inputs.first().cloned().unwrap_or_default(),
                timestamp_unix: 0,
                reply_to: None,
            };
            let req = OpenAiBatchRequest {
                inputs: inputs.clone(),
                meta: meta.clone().unwrap_or(default_meta),
                sender: resp_tx,
            };
            if tx.send(req).is_ok() {
                if let Ok(res) = resp_rx.await {
                    return res;
                }
            }
        }

        let str_refs: Vec<&str> = inputs.iter().map(|s| s.as_str()).collect();
        let fallback_transcript = Arc::new(meta.into_iter().collect::<Vec<_>>());
        match self.call_openai_moderation(api_key, &str_refs).await {
            Ok(scores) => (scores, fallback_transcript),
            Err(_) => (vec![OpenAiScores::default(); expected_len], fallback_transcript),
        }
    }

    pub async fn evaluate_via_groq_batch(
        &self,
        message_id: u64,
        channel_id: u64,
        channel_name: String,
        author_name: String,
        author_id: u64,
        trimmed_content: String,
        telemetry_chunk: String,
        is_hardcore: bool,
        batch_transcript: Arc<Vec<BatchLogEntry>>,
        channel_history: Vec<ChatEntry>,
    ) -> Result<(GroqDecision, String, u128), String> {
        if let Some(ref tx) = self.groq_batch_tx {
            let (resp_tx, resp_rx) = oneshot::channel();
            let req = GroqBatchItemRequest {
                message_id,
                channel_id,
                channel_name: channel_name.clone(),
                author_name: author_name.clone(),
                author_id,
                trimmed_content: trimmed_content.clone(),
                telemetry_chunk: telemetry_chunk.clone(),
                is_hardcore,
                batch_transcript: batch_transcript.clone(),
                channel_history: channel_history.clone(),
                sender: resp_tx,
            };
            if tx.send(req).is_ok() {
                if let Ok(res) = resp_rx.await {
                    return res;
                }
            }
        }

        let preferred_model = if is_hardcore {
            &self.groq_deep_model
        } else {
            &self.groq_fast_model
        };
        let single_item = GroqBatchItemRequest {
            message_id,
            channel_id,
            channel_name,
            author_name,
            author_id,
            trimmed_content,
            telemetry_chunk,
            is_hardcore,
            batch_transcript,
            channel_history,
            sender: oneshot::channel().0,
        };
        let prompt = Self::build_batch_transcript_prompt(&[single_item]);
        self.call_groq_failover(preferred_model, SERVER_RULES_BATCH_SYSTEM_PROMPT, &prompt).await
    }

    async fn run_batch_worker(
        mut rx: mpsc::UnboundedReceiver<OpenAiBatchRequest>,
        http_client: reqwest::Client,
        openai_key: Option<String>,
        batch_size: usize,
        batch_wait: Duration,
    ) {
        let mut pending: Vec<OpenAiBatchRequest> = Vec::new();
        let mut total_inputs: usize = 0;

        while let Some(first_req) = rx.recv().await {
            total_inputs += first_req.inputs.len();
            pending.push(first_req);

            if total_inputs >= batch_size || batch_wait.is_zero() {
                Self::flush_batch(&mut pending, &mut total_inputs, &http_client, openai_key.as_deref()).await;
                continue;
            }

            let deadline = tokio::time::Instant::now() + batch_wait;

            while !pending.is_empty() {
                tokio::select! {
                    maybe_req = rx.recv() => {
                        match maybe_req {
                            Some(req) => {
                                total_inputs += req.inputs.len();
                                pending.push(req);
                                if total_inputs >= batch_size {
                                    Self::flush_batch(&mut pending, &mut total_inputs, &http_client, openai_key.as_deref()).await;
                                    break;
                                }
                            }
                            None => {
                                Self::flush_batch(&mut pending, &mut total_inputs, &http_client, openai_key.as_deref()).await;
                                return;
                            }
                        }
                    }
                    _ = tokio::time::sleep_until(deadline) => {
                        Self::flush_batch(&mut pending, &mut total_inputs, &http_client, openai_key.as_deref()).await;
                        break;
                    }
                }
            }
        }
    }

    async fn flush_batch(
        pending: &mut Vec<OpenAiBatchRequest>,
        total_inputs: &mut usize,
        http_client: &reqwest::Client,
        openai_key: Option<&str>,
    ) {
        if pending.is_empty() {
            return;
        }

        let batch = std::mem::take(pending);
        *total_inputs = 0;

        let batch_transcript: Arc<Vec<BatchLogEntry>> = Arc::new(
            batch.iter().map(|r| r.meta.clone()).collect()
        );

        let api_key = match openai_key {
            Some(k) if !k.is_empty() => k,
            _ => {
                for req in batch {
                    let default_scores = vec![OpenAiScores::default(); req.inputs.len()];
                    let _ = req.sender.send((default_scores, batch_transcript.clone()));
                }
                return;
            }
        };

        let mut flat_inputs: Vec<&str> = Vec::new();
        let mut slice_lens: Vec<usize> = Vec::with_capacity(batch.len());

        for req in &batch {
            slice_lens.push(req.inputs.len());
            for inp in &req.inputs {
                flat_inputs.push(inp.as_str());
            }
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let in_cooldown = now < OPENAI_COOLDOWN_UNTIL.load(Ordering::Relaxed);

        if !in_cooldown && (batch.len() > 1 || flat_inputs.len() > 1) {
            println!(
                "   📦 [OPENAI BATCH FLUSH] Moderating {} messages ({} inputs in single array) in 1 API request",
                batch.len(),
                flat_inputs.len()
            );
        }

        let scores_res = Self::call_openai_moderation_static(http_client, api_key, &flat_inputs).await;
        let all_scores = match scores_res {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[OPENAI BATCH ERROR] Status/Network error: {}. Falling back to default.", e);
                vec![OpenAiScores::default(); flat_inputs.len()]
            }
        };

        let mut cursor = 0;
        for (req, len) in batch.into_iter().zip(slice_lens.into_iter()) {
            let end = (cursor + len).min(all_scores.len());
            let sub_scores = if cursor < all_scores.len() {
                all_scores[cursor..end].to_vec()
            } else {
                vec![OpenAiScores::default(); len]
            };
            cursor = end;
            let _ = req.sender.send((sub_scores, batch_transcript.clone()));
        }
    }

    pub async fn call_openai_moderation(&self, api_key: &str, texts: &[&str]) -> Result<Vec<OpenAiScores>, reqwest::Error> {
        Self::call_openai_moderation_static(&self.http_client, api_key, texts).await
    }

    pub async fn call_openai_moderation_static(http_client: &reqwest::Client, api_key: &str, texts: &[&str]) -> Result<Vec<OpenAiScores>, reqwest::Error> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if now < OPENAI_COOLDOWN_UNTIL.load(Ordering::Relaxed) {
            return Ok(vec![OpenAiScores::default(); texts.len()]);
        }

        let req_body = OpenAiBatchModRequest {
            model: "omni-moderation-latest",
            input: texts.to_vec(),
        };

        let resp = http_client
            .post("https://api.openai.com/v1/moderations")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&req_body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            if status.as_u16() == 429 {
                let cooldown_secs = 60;
                OPENAI_COOLDOWN_UNTIL.store(now + cooldown_secs, Ordering::Relaxed);
                eprintln!("[OPENAI API] 429 Rate Limit hit. Backing off OpenAI requests for {}s (falling back to native engine + LLM router).", cooldown_secs);
            } else {
                eprintln!("[OPENAI API ERROR] Status {}: {}", status, err_text);
            }
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

    pub async fn call_gemini_static(
        http_client: &reqwest::Client,
        api_key: &str,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: u32,
    ) -> Result<(String, u128), String> {
        let start_time = std::time::Instant::now();
        // Zero key leak: pass key strictly in encrypted HTTP header (x-goog-api-key)
        // so proxy operators and network sniffers can NEVER see the key in URL parameters
        let base_url = get_env_var("GEMINI_API_ENDPOINT")
            .unwrap_or_else(|| "https://generativelanguage.googleapis.com".to_string());
        let url = format!(
            "{}/v1beta/models/{}:generateContent",
            base_url.trim_end_matches('/'),
            model
        );

        let system_instruction = if !system_prompt.trim().is_empty() {
            Some(GeminiContent {
                role: None,
                parts: vec![GeminiPart { text: system_prompt }],
            })
        } else {
            None
        };

        let req_body = GeminiRequest {
            system_instruction,
            contents: vec![GeminiContent {
                role: Some("user"),
                parts: vec![GeminiPart { text: user_prompt }],
            }],
            generation_config: GeminiGenConfig {
                temperature: 0.0,
                max_output_tokens: max_tokens,
            },
        };

        let resp = http_client
            .post(&url)
            .timeout(std::time::Duration::from_secs(16))
            .header("Content-Type", "application/json")
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
            .header("x-goog-api-key", api_key)
            .json(&req_body)
            .send()
            .await
            .map_err(|e| format!("Gemini network error: {}", e))?;

        let status = resp.status();
        let raw_text = resp.text().await.map_err(|e| format!("Gemini response read error: {}", e))?;

        if !status.is_success() {
            return Err(format!("Gemini HTTP {}: {}", status, raw_text));
        }

        let body: GeminiResponse = serde_json::from_str(&raw_text)
            .map_err(|e| format!("Gemini JSON decode error: {} (raw: {})", e, raw_text))?;

        if let Some(err) = body.error {
            return Err(format!("Gemini API error: {}", err.message));
        }

        if let Some(cand) = body.candidates.first() {
            if let Some(content) = &cand.content {
                let text = content.parts.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join("");
                let elapsed_ms = start_time.elapsed().as_millis();
                return Ok((text, elapsed_ms));
            }
        }

        Err("Gemini returned empty candidates".to_string())
    }

    pub async fn call_gemini_failover_static(
        http_client: &reqwest::Client,
        gemini_keys: &[String],
        gemini_counter: &AtomicUsize,
        preferred_model: &str,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: u32,
    ) -> Result<(String, String, u128), String> {
        let total_keys = gemini_keys.len();
        if total_keys == 0 {
            return Err("No Gemini keys configured".to_string());
        }

        let safe_preferred = if preferred_model.contains("3.8") || preferred_model.contains("pro") {
            "gemini-3.5-flash-lite"
        } else {
            preferred_model
        };

        let mut models_to_try = vec![safe_preferred];
        for candidate in &[
            "gemini-3.5-flash-lite",
            "gemini-3.1-flash-lite",
            "gemini-flash-lite-latest",
            "gemini-2.5-flash-lite",
            "gemma-4-31b-it",
            "gemini-2.0-flash",
            "gemini-1.5-flash",
        ] {
            if !models_to_try.contains(candidate) {
                models_to_try.push(candidate);
            }
        }

        let start_idx = gemini_counter.fetch_add(1, Ordering::Relaxed) % total_keys;
        for target_model in models_to_try {
            for i in 0..total_keys {
                let idx = (start_idx + i) % total_keys;
                let key = &gemini_keys[idx];
                match Self::call_gemini_static(http_client, key, target_model, system_prompt, user_prompt, max_tokens).await {
                    Ok((text, elapsed_ms)) => return Ok((text, target_model.to_string(), elapsed_ms)),
                    Err(e) => {
                        eprintln!("[GEMINI FAILOVER] Key #{} model '{}' error: {}. Trying fallback...", idx + 1, target_model, e);
                        continue;
                    }
                }
            }
        }
        Err("All Gemini keys and models exhausted".to_string())
    }

    pub async fn call_nvidia_failover_static(
        http_client: &reqwest::Client,
        nvidia_keys: &[String],
        nvidia_counter: &AtomicUsize,
        preferred_model: &str,
        nvidia_api_endpoint: &str,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: u32,
    ) -> Result<(String, String, u128), String> {
        let total_keys = nvidia_keys.len();
        if total_keys == 0 {
            return Err("No NVIDIA keys configured".to_string());
        }

        let mut models_to_try = vec![preferred_model];
        for candidate in &[
            "nvidia/nemotron-3-ultra-550b-a55b",
            "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
            "meta/llama-3.2-11b-vision-instruct",
            "nvidia/nemotron-3-super-120b-a12b",
            "openai/gpt-oss-20b",
        ] {
            if !models_to_try.contains(candidate) {
                models_to_try.push(candidate);
            }
        }

        let start_idx = nvidia_counter.fetch_add(1, Ordering::Relaxed) % total_keys;
        for target_model in models_to_try {
            for i in 0..total_keys {
                let idx = (start_idx + i) % total_keys;
                let key = &nvidia_keys[idx];

                let start_time = std::time::Instant::now();
                let req_body = GroqChatRequest {
                    model: target_model.to_string(),
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
                    temperature: 0.1,
                };

                let resp_res = http_client
                    .post(nvidia_api_endpoint)
                    .header("Authorization", format!("Bearer {}", key))
                    .header("Content-Type", "application/json")
                    .json(&req_body)
                    .timeout(Duration::from_secs(15))
                    .send()
                    .await;

                match resp_res {
                    Ok(resp) => {
                        let status = resp.status();
                        if status.is_success() {
                            if let Ok(raw_json) = resp.text().await {
                                if let Ok(body) = serde_json::from_str::<GroqChatResponse>(&raw_json) {
                                    if let Some(choice) = body.choices.first() {
                                        let elapsed_ms = start_time.elapsed().as_millis();
                                        let text_out = if !choice.message.content.trim().is_empty() {
                                            choice.message.content.clone()
                                        } else if let Some(ref r) = choice.message.reasoning_content {
                                            r.clone()
                                        } else if let Some(ref r) = choice.message.reasoning {
                                            r.clone()
                                        } else {
                                            choice.message.content.clone()
                                        };
                                        return Ok((text_out, target_model.to_string(), elapsed_ms));
                                    }
                                }
                            }
                        } else {
                            let err_text = resp.text().await.unwrap_or_default();
                            eprintln!("[NVIDIA FAILOVER] Key #{} model '{}' error: HTTP {}: {}. Trying fallback...", idx + 1, target_model, status, err_text);
                        }
                    }
                    Err(e) => {
                        eprintln!("[NVIDIA FAILOVER] Key #{} model '{}' network error: {}. Trying fallback...", idx + 1, target_model, e);
                    }
                }
            }
        }
        Err("All NVIDIA keys and models exhausted".to_string())
    }

    pub fn parse_supreme_decision(text: &str) -> SupremeDecision {
        let mut verdict = SupremeVerdict::Confirm;
        let mut reason = String::new();

        for line in text.lines() {
            let normalized = line.replace('*', "").replace('`', "").replace('#', "").replace('>', "").trim().to_string();
            let upper = normalized.to_uppercase();
            if let Some(rest) = upper.strip_prefix("VERDICT:") {
                let v = rest.trim().trim_matches(|c: char| c == '[' || c == ']' || c == '"' || c == '\'');
                if v.contains("ALLOW") {
                    verdict = SupremeVerdict::Allow;
                } else if v.contains("CONFIRM") || v.contains("DELETE") || v.contains("MUTE") {
                    verdict = SupremeVerdict::Confirm;
                }
            } else if let Some(_rest) = upper.strip_prefix("REASON:") {
                let r = normalized["REASON:".len()..].trim().trim_matches(|c: char| c == '[' || c == ']' || c == '"' || c == '\'');
                reason = r.to_string();
            }
        }

        if reason.is_empty() {
            reason = text.chars().take(120).collect();
        }

        let upper_text = text.to_uppercase();
        if upper_text.contains("VERDICT: ALLOW") || upper_text.starts_with("ALLOW") || (upper_text.contains("[ALLOW]") && !upper_text.contains("[CONFIRM]")) {
            verdict = SupremeVerdict::Allow;
        }

        SupremeDecision { verdict, reason }
    }

    pub async fn consult_supreme_arbiter(
        &self,
        ctx: &MessageContext<'_>,
        preliminary: &ModerationVerdict,
    ) -> ModerationVerdict {
        if cfg!(test) && get_env_var("TEST_RUN_SUPREME").is_none() {
            return preliminary.clone();
        }

        let (preliminary_reason, orig_model, rule_violated, mute_minutes, is_delete) = match preliminary {
            ModerationVerdict::DeleteConfirmed { reason, model_used, rule_violated, mute_minutes, .. } => {
                (reason.clone(), model_used.clone(), rule_violated.clone(), *mute_minutes, true)
            }
            ModerationVerdict::FlagSuspicious { reason, model_used, rule_violated, mute_minutes, .. } if *mute_minutes > 0 => {
                (reason.clone(), model_used.clone(), rule_violated.clone(), *mute_minutes, false)
            }
            _ => return preliminary.clone(),
        };

        let has_nvidia = !self.nvidia_keys.is_empty();
        let has_gemini = !self.gemini_keys.is_empty();

        if !has_nvidia && !has_gemini {
            return preliminary.clone();
        }

        let trimmed = ctx.content.trim();

        println!(
            "\n⚖️ [SUPREME ARBITER] Reviewing proposed punishment for @{} (Action: {}, Mute: {}m, Rule: '{}', Reason: '{}')",
            ctx.author_name,
            if is_delete { "DELETE" } else { "MUTE" },
            mute_minutes,
            rule_violated,
            preliminary_reason
        );

        let history = self.get_context_snapshot(ctx.channel_id);
        let mut history_str = String::new();
        for entry in history.iter().rev().take(30).collect::<Vec<_>>().into_iter().rev() {
            history_str.push_str(&format!("@{}: \"{}\"\n", entry.author_name, entry.content));
        }

        let supreme_system_prompt = "You are the Supreme Court & Chief Moderation Arbiter for a Discord gaming community. \
            Your sole purpose is to serve as the final sanity check and PREVENT FALSE POSITIVES against innocent users. \
            You review messages that lower-tier AI filters flagged for deletion or user timeouts. \
            You must rigorously distinguish between genuine malicious attacks (phishing, doxxing, malware, hate slurs, real death threats) \
            versus harmless Discord/gaming banter, idioms, jokes, hyperbole, quotes, and gamer slang. \
            When in doubt between innocent humor vs real malice, err on the side of ALLOW.";

        let supreme_user_prompt = format!(
            "══════════════════════════════════════════════════════════════════════════════\n\
            SUPREME MODERATION AUDIT:\n\
            A lower-level AI filter has flagged a Discord message for {action} and a {mute_minutes}-minute timeout.\n\n\
            FLAGGED MESSAGE:\n\
            Author: @{author_name} (ID: {author_id})\n\
            Channel: #{channel_name}\n\
            Message Content: \"{content}\"\n\n\
            PRELIMINARY MODERATION DETAILS:\n\
            - Flagged By: {model_used}\n\
            - Rule: {rule_violated}\n\
            - Flag Reason: \"{preliminary_reason}\"\n\
            - Proposed Action: {action} ({mute_minutes} min timeout)\n\n\
            RECENT CHANNEL CONTEXT (Previous messages in #{channel_name}):\n\
            {history_str}\n\
            ══════════════════════════════════════════════════════════════════════════════\n\
            DECISION GUIDELINES:\n\
            - Overrule to ALLOW if the message is:\n\
              * Common English or Russian idioms ('kill time', 'i'm dead', 'shoot me an email')\n\
              * Gaming callouts or hyperbole ('kill him', 'shoot them', 'we gonna kill you', 'i will find you in roblox')\n\
              * Banter, dramatic exaggeration, playful warnings ('say yo one more time and ur done for')\n\
              * Inside jokes, sarcasm, memes, self-deprecation, spoilers, or quotes\n\
            - Confirm (CONFIRM) ONLY if the message is:\n\
              * Actual scam, credential theft, steam trade fraud, or token drainer\n\
              * Real-world doxxing / leaking private personal data (real names, physical addresses, phone numbers)\n\
              * Genuine hate speech or racial slurs directed at protected groups\n\
              * Explicit real-world death threat with genuine IRL malice (e.g., 'i will come to your house and slit your throat')\n\n\
            OUTPUT FORMAT (EXACTLY 2 LINES):\n\
            VERDICT: [ALLOW | CONFIRM]\n\
            REASON: [concise 1-sentence rationale]",
            action = if is_delete { "DELETION" } else { "TIMEOUT" },
            mute_minutes = mute_minutes,
            author_name = ctx.author_name,
            author_id = ctx.author_id,
            channel_name = ctx.channel_name.as_deref().unwrap_or("general"),
            content = trimmed,
            model_used = orig_model,
            rule_violated = rule_violated,
            preliminary_reason = preliminary_reason,
            history_str = if history_str.is_empty() { "(no prior messages)".to_string() } else { history_str },
        );

        let mut supreme_res: Option<(SupremeDecision, String, u128)> = None;

        // 1. Try NVIDIA NIM API first if configured
        if has_nvidia {
            match Self::call_nvidia_failover_static(
                &self.http_client,
                &self.nvidia_keys,
                &self.nvidia_counter,
                &self.nvidia_model,
                &self.nvidia_api_endpoint,
                supreme_system_prompt,
                &supreme_user_prompt,
                400,
            ).await {
                Ok((raw_text, model, elapsed)) => {
                    let decision = Self::parse_supreme_decision(&raw_text);
                    supreme_res = Some((decision, format!("NVIDIA Supreme ({})", model), elapsed));
                }
                Err(e) => {
                    eprintln!("   ⚠️ [SUPREME ARBITER] NVIDIA NIM call failed: {}. Trying fallback...", e);
                }
            }
        }

        // 2. Fallback to Gemini Deep model if NVIDIA was not configured or failed
        if supreme_res.is_none() && has_gemini {
            let gemini_model = &self.gemini_deep_model;
            match Self::call_gemini_failover_static(
                &self.http_client,
                &self.gemini_keys,
                &self.gemini_counter,
                gemini_model,
                supreme_system_prompt,
                &supreme_user_prompt,
                400,
            ).await {
                Ok((raw_text, model, elapsed)) => {
                    let decision = Self::parse_supreme_decision(&raw_text);
                    supreme_res = Some((decision, format!("Gemini Supreme ({})", model), elapsed));
                }
                Err(e) => {
                    eprintln!("   ⚠️ [SUPREME ARBITER] Gemini fallback failed: {}", e);
                }
            }
        }

        if let Some((decision, arbiter_model, elapsed_ms)) = supreme_res {
            println!(
                "   ⚖️ [SUPREME ARBITER RESULT] Model: {} (took {}ms) | Verdict: {:?} | Reason: \"{}\"",
                arbiter_model, elapsed_ms, decision.verdict, decision.reason
            );

            match decision.verdict {
                SupremeVerdict::Allow => {
                    println!(
                        "   🛡️ [SUPREME OVERRULE] {} overruled preliminary {} ({}) for @{} -> ALLOW! Reason: \"{}\"",
                        arbiter_model,
                        if is_delete { "DELETE" } else { "MUTE" },
                        rule_violated,
                        ctx.author_name,
                        decision.reason
                    );
                    self.add_to_whitelist(trimmed).await;
                    ModerationVerdict::Allow
                }
                SupremeVerdict::Confirm => {
                    println!(
                        "   🚨 [SUPREME CONFIRMED] {} confirmed punishment for @{} (Rule: {}).",
                        arbiter_model,
                        ctx.author_name,
                        rule_violated
                    );
                    match preliminary {
                        ModerationVerdict::DeleteConfirmed { reason, score, category, model_used, rule_violated, mute_minutes } => {
                            ModerationVerdict::DeleteConfirmed {
                                reason: reason.clone(),
                                score: *score,
                                category: category.clone(),
                                model_used: format!("{} + {}", model_used, arbiter_model),
                                rule_violated: rule_violated.clone(),
                                mute_minutes: *mute_minutes,
                            }
                        }
                        ModerationVerdict::FlagSuspicious { reason, score, category, model_used, rule_violated, mute_minutes } => {
                            ModerationVerdict::FlagSuspicious {
                                reason: reason.clone(),
                                score: *score,
                                category: category.clone(),
                                model_used: format!("{} + {}", model_used, arbiter_model),
                                rule_violated: rule_violated.clone(),
                                mute_minutes: *mute_minutes,
                            }
                        }
                        other => other.clone(),
                    }
                }
            }
        } else {
            preliminary.clone()
        }
    }

    async fn call_groq_failover(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<(GroqDecision, String, u128), String> {
        // Try Groq cluster first (120B reasoning model or fast 27B model)
        if !self.groq_keys.is_empty() {
            let max_tokens = if model.contains("gpt-oss") { 1024 } else { 200 };
            match Self::call_groq_failover_static(
                &self.http_client,
                &self.groq_keys,
                &self.groq_counter,
                model,
                system_prompt,
                user_prompt,
                max_tokens,
            ).await {
                Ok((raw_text, reasoning, model_used, elapsed_ms)) => {
                    if let Ok(decision) = Self::parse_single_decision_from_text(&raw_text, reasoning.as_deref()) {
                        return Ok((decision, model_used, elapsed_ms));
                    }
                }
                Err(e) => {
                    eprintln!("[LLM ARBITER] Groq error: {}. Falling back to Gemini...", e);
                }
            }
        }

        // Fallback to Gemini if configured
        if !self.gemini_keys.is_empty() {
            let preferred_gemini = if model.contains("gpt-oss") || model.contains("pro") {
                &self.gemini_deep_model
            } else {
                &self.gemini_fast_model
            };
            let max_tokens = if preferred_gemini.contains("pro") { 1024 } else { 250 };
            match Self::call_gemini_failover_static(
                &self.http_client,
                &self.gemini_keys,
                &self.gemini_counter,
                preferred_gemini,
                system_prompt,
                user_prompt,
                max_tokens,
            ).await {
                Ok((raw_text, model_used, elapsed_ms)) => {
                    if let Ok(decision) = Self::parse_single_decision_from_text(&raw_text, None) {
                        return Ok((decision, format!("gemini:{}", model_used), elapsed_ms));
                    }
                }
                Err(e) => {
                    eprintln!("[LLM ARBITER] Gemini failover error: {}", e);
                }
            }
        }

        Err("All Groq and Gemini models exhausted".to_string())
    }

    pub async fn call_groq_failover_static(
        http_client: &reqwest::Client,
        groq_keys: &[String],
        groq_counter: &AtomicUsize,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: u32,
    ) -> Result<(String, Option<String>, String, u128), String> {
        let total_keys = groq_keys.len();
        if total_keys == 0 {
            return Err("No Groq keys configured".to_string());
        }

        let mut models_to_try = vec![model];
        for candidate in &[
            "openai/gpt-oss-120b",
            "qwen/qwen3.8-27b",
            "openai/gpt-oss-20b",
        ] {
            if !models_to_try.contains(candidate) {
                models_to_try.push(candidate);
            }
        }

        let start_idx = groq_counter.fetch_add(1, Ordering::Relaxed) % total_keys;
        for target_model in models_to_try {
            for i in 0..total_keys {
                let idx = (start_idx + i) % total_keys;
                let key = &groq_keys[idx];
                match Self::execute_groq_call_raw_static(http_client, key, target_model, system_prompt, user_prompt, max_tokens).await {
                    Ok((content, reasoning, elapsed_ms)) => return Ok((content, reasoning, target_model.to_string(), elapsed_ms)),
                    Err(e) => {
                        eprintln!("[GROQ FAILOVER] Key {} model '{}' error: {}. Trying fallback...", idx, target_model, e);
                        continue;
                    }
                }
            }
        }
        Err("All Groq keys and models exhausted".to_string())
    }

    #[allow(dead_code)]
    async fn execute_groq_call(
        &self,
        api_key: &str,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<(GroqDecision, u128), String> {
        let max_tokens = if model.contains("gpt-oss") { 1024 } else { 85 };
        let (raw_text, reasoning, elapsed_ms) = Self::execute_groq_call_raw_static(
            &self.http_client,
            api_key,
            model,
            system_prompt,
            user_prompt,
            max_tokens,
        ).await?;

        let decision = Self::parse_single_decision_from_text(&raw_text, reasoning.as_deref())?;
        Ok((decision, elapsed_ms))
    }

    async fn execute_groq_call_raw_static(
        http_client: &reqwest::Client,
        api_key: &str,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: u32,
    ) -> Result<(String, Option<String>, u128), String> {
        let start_time = std::time::Instant::now();
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

        let resp = http_client
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
            Ok((choice.message.content.clone(), choice.message.reasoning.clone(), elapsed_ms))
        } else {
            Err("Empty choices in response".to_string())
        }
    }

    pub fn parse_single_decision_from_text(text: &str, reasoning: Option<&str>) -> Result<GroqDecision, String> {
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
            if let Some(r) = reasoning {
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

        Ok(GroqDecision {
            verdict,
            rule,
            mute_minutes,
            reason,
        })
    }

    pub fn extract_item_id(upper: &str) -> Option<usize> {
        let clean = upper.replace('*', "").replace('`', "").replace('#', "");
        let clean = clean.trim();
        for prefix in &["[ITEM", "ITEM", "[MESSAGE", "MESSAGE", "[MSG", "MSG"] {
            if let Some(idx) = clean.find(prefix) {
                let rest = &clean[idx + prefix.len()..];
                let rest = rest.trim_start_matches(|c: char| c == ':' || c == '#' || c == ' ' || c == '[' || c == ']' || c == '-' || c == '.');
                let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(id) = num_str.parse::<usize>() {
                    if id > 0 {
                        return Some(id);
                    }
                }
            }
        }
        None
    }

    pub fn parse_batch_decisions(
        text: &str,
        reasoning: Option<&str>,
        item_count: usize,
    ) -> HashMap<usize, GroqDecision> {
        let mut map: HashMap<usize, GroqDecision> = HashMap::new();
        if item_count == 0 {
            return map;
        }

        let full_text = if let Some(r) = reasoning {
            format!("{}\n{}", text, r)
        } else {
            text.to_string()
        };

        let mut current_id: Option<usize> = if item_count == 1 { Some(1) } else { None };
        let mut cur_lines: Vec<String> = Vec::new();

        let flush_item = |id_opt: Option<usize>, lines: &[String], out: &mut HashMap<usize, GroqDecision>| {
            if let Some(id) = id_opt {
                if !lines.is_empty() {
                    let chunk = lines.join("\n");
                    if let Ok(decision) = Self::parse_single_decision_from_text(&chunk, None) {
                        out.insert(id, decision);
                    }
                }
            }
        };

        for line in full_text.lines() {
            let clean = line.replace('*', "").replace('`', "").replace('#', "").trim().to_string();
            let upper = clean.to_uppercase();

            if let Some(new_id) = Self::extract_item_id(&upper) {
                flush_item(current_id, &cur_lines, &mut map);
                cur_lines.clear();
                current_id = Some(new_id);
                continue;
            }

            cur_lines.push(clean);
        }
        flush_item(current_id, &cur_lines, &mut map);

        if item_count == 1 && !map.contains_key(&1) {
            if let Ok(decision) = Self::parse_single_decision_from_text(text, reasoning) {
                map.insert(1, decision);
            }
        }

        for i in 1..=item_count {
            map.entry(i).or_insert_with(|| GroqDecision {
                verdict: "ALLOW".to_string(),
                rule: "None".to_string(),
                mute_minutes: 0,
                reason: "Safe chat banter".to_string(),
            });
        }

        map
    }

    async fn run_groq_batch_worker(
        mut rx: mpsc::UnboundedReceiver<GroqBatchItemRequest>,
        http_client: reqwest::Client,
        groq_keys: Vec<String>,
        groq_fast_model: String,
        groq_deep_model: String,
        groq_counter: Arc<AtomicUsize>,
        gemini_keys: Vec<String>,
        gemini_counter: Arc<AtomicUsize>,
        gemini_fast_model: String,
        gemini_deep_model: String,
        batch_size: usize,
        batch_wait: Duration,
    ) {
        let mut pending: Vec<GroqBatchItemRequest> = Vec::new();

        while let Some(first_req) = rx.recv().await {
            pending.push(first_req);

            if pending.len() >= batch_size || batch_wait.is_zero() {
                Self::flush_groq_batch(
                    &mut pending,
                    &http_client,
                    &groq_keys,
                    &groq_fast_model,
                    &groq_deep_model,
                    &groq_counter,
                    &gemini_keys,
                    &gemini_counter,
                    &gemini_fast_model,
                    &gemini_deep_model,
                ).await;
                continue;
            }

            let deadline = tokio::time::Instant::now() + batch_wait;

            while !pending.is_empty() {
                tokio::select! {
                    maybe_req = rx.recv() => {
                        match maybe_req {
                            Some(req) => {
                                pending.push(req);
                                if pending.len() >= batch_size {
                                    Self::flush_groq_batch(
                                        &mut pending,
                                        &http_client,
                                        &groq_keys,
                                        &groq_fast_model,
                                        &groq_deep_model,
                                        &groq_counter,
                                        &gemini_keys,
                                        &gemini_counter,
                                        &gemini_fast_model,
                                        &gemini_deep_model,
                                    ).await;
                                    break;
                                }
                            }
                            None => {
                                Self::flush_groq_batch(
                                    &mut pending,
                                    &http_client,
                                    &groq_keys,
                                    &groq_fast_model,
                                    &groq_deep_model,
                                    &groq_counter,
                                    &gemini_keys,
                                    &gemini_counter,
                                    &gemini_fast_model,
                                    &gemini_deep_model,
                                ).await;
                                return;
                            }
                        }
                    }
                    _ = tokio::time::sleep_until(deadline) => {
                        Self::flush_groq_batch(
                            &mut pending,
                            &http_client,
                            &groq_keys,
                            &groq_fast_model,
                            &groq_deep_model,
                            &groq_counter,
                            &gemini_keys,
                            &gemini_counter,
                            &gemini_fast_model,
                            &gemini_deep_model,
                        ).await;
                        break;
                    }
                }
            }
        }
    }

    async fn flush_groq_batch(
        pending: &mut Vec<GroqBatchItemRequest>,
        http_client: &reqwest::Client,
        groq_keys: &[String],
        groq_fast_model: &str,
        groq_deep_model: &str,
        groq_counter: &AtomicUsize,
        gemini_keys: &[String],
        gemini_counter: &AtomicUsize,
        gemini_fast_model: &str,
        gemini_deep_model: &str,
    ) {
        if pending.is_empty() {
            return;
        }

        let batch = std::mem::take(pending);
        let count = batch.len();

        if groq_keys.is_empty() && gemini_keys.is_empty() {
            for item in batch {
                let _ = item.sender.send(Err("No Gemini or Groq keys configured".to_string()));
            }
            return;
        }

        let has_hardcore = batch.iter().any(|item| item.is_hardcore);
        let combined_user_prompt = Self::build_batch_transcript_prompt(&batch);

        // Try Groq cluster first (120B reasoning model or fast 27B model)
        let mut eval_result: Option<(String, Option<String>, String, u128)> = None;

        if !groq_keys.is_empty() {
            let preferred_groq = if has_hardcore {
                groq_deep_model
            } else {
                groq_fast_model
            };

            println!(
                "\n📦 [GROQ BATCH FLUSH] Evaluating {} flagged messages with chronological batch transcript! (Model: {}, Hardcore/Deep: {})",
                count, preferred_groq, has_hardcore
            );

            let base_tokens = if preferred_groq.contains("gpt-oss") { 800 } else { 200 };
            let max_tokens = ((count * base_tokens).max(800)).min(4096) as u32;

            match Self::call_groq_failover_static(
                http_client,
                groq_keys,
                groq_counter,
                preferred_groq,
                SERVER_RULES_BATCH_SYSTEM_PROMPT,
                &combined_user_prompt,
                max_tokens,
            ).await {
                Ok(res) => {
                    eval_result = Some(res);
                }
                Err(e) => {
                    eprintln!("[GROQ BATCH ERROR] Groq failed across all keys: {}. Falling back to Gemini...", e);
                }
            }
        }

        // If Groq was not used or failed, fallback to Gemini
        if eval_result.is_none() && !gemini_keys.is_empty() {
            let preferred_gemini = if has_hardcore {
                gemini_deep_model
            } else {
                gemini_fast_model
            };
            let max_tokens = ((count * 250).max(512)).min(4096) as u32;

            println!(
                "\n✨ [GEMINI BATCH FLUSH] Evaluating {} flagged messages with chronological batch transcript via Gemini! (Model: {}, Hardcore/Deep: {})",
                count, preferred_gemini, has_hardcore
            );

            match Self::call_gemini_failover_static(
                http_client,
                gemini_keys,
                gemini_counter,
                preferred_gemini,
                SERVER_RULES_BATCH_SYSTEM_PROMPT,
                &combined_user_prompt,
                max_tokens,
            ).await {
                Ok((raw_text, model_used, elapsed_ms)) => {
                    eval_result = Some((raw_text, None, format!("gemini:{}", model_used), elapsed_ms));
                }
                Err(e) => {
                    eprintln!("⚠️ [GEMINI BATCH ERROR] Gemini failover failed: {}", e);
                }
            }
        }

        match eval_result {
            Some((raw_text, reasoning, model_used, elapsed_ms)) => {
                let mut decisions_map = Self::parse_batch_decisions(&raw_text, reasoning.as_deref(), count);
                println!(
                    "⚡ [LLM BATCH RESULTS] Evaluated {} items in {}ms (Model: {}):",
                    count, elapsed_ms, model_used
                );

                for (idx, item) in batch.into_iter().enumerate() {
                    let item_num = idx + 1;
                    let decision = decisions_map.remove(&item_num).unwrap_or_else(|| GroqDecision {
                        verdict: "ALLOW".to_string(),
                        rule: "None".to_string(),
                        mute_minutes: 0,
                        reason: "Safe chat banter".to_string(),
                    });

                    println!(
                        "   ↳ Item #{} (@{} [{}] in #{}): [{}] Rule: {} | Mute: {}m | Reason: \"{}\"",
                        item_num,
                        item.author_name,
                        item.author_id,
                        item.channel_name,
                        decision.verdict,
                        decision.rule,
                        decision.mute_minutes,
                        decision.reason
                    );
                    if decision.verdict != "ALLOW" {
                        println!("      ⚠️ Caught text: \"{}\"", Self::safe_truncate(&item.trimmed_content, 60));
                    }

                    let _ = item.sender.send(Ok((decision, model_used.clone(), elapsed_ms)));
                }
            }
            None => {
                let err_msg = "Both Gemini and Groq evaluation failed or were unavailable".to_string();
                for item in batch {
                    let _ = item.sender.send(Err(err_msg.clone()));
                }
            }
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
        assert!(AiModerator::is_theatrical_hyperbole("say yo one more time and ur done for"));
        assert!(AiModerator::is_theatrical_hyperbole("say that again and you're cooked"));
        assert!(AiModerator::is_theatrical_hyperbole("тебе хана"));
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
    async fn test_check_message_say_yo_done_for() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 11,
            timestamp_unix: 1727376000,
            author_name: "voidyxkm",
            author_id: 1494872075142299761,
            author_nick: Some("voidyxkm".to_string()),
            account_age_days: Some(30),
            server_member_days: Some(10),
            roles_count: 1,
            content: "say yo one more time and ur done for",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'say yo one more time and ur done for': {:?}\n", verdict);
        assert!(matches!(verdict, ModerationVerdict::Allow), "Expected 'say yo one more time and ur done for' to be ALLOW, got {:?}", verdict);
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

    #[test]
    fn test_is_early_access_query_unit() {
        // Direct phrase matches (English)
        assert!(AiModerator::is_early_access_query("how to get ea"));
        assert!(AiModerator::is_early_access_query("how to get ea?"));
        assert!(AiModerator::is_early_access_query("get ea"));
        assert!(AiModerator::is_early_access_query("obtain ea"));
        assert!(AiModerator::is_early_access_query("how do i get ea"));
        assert!(AiModerator::is_early_access_query("how can i get ea"));
        assert!(AiModerator::is_early_access_query("where to get ea"));
        assert!(AiModerator::is_early_access_query("where can i get ea?"));
        assert!(AiModerator::is_early_access_query("can i get ea"));
        assert!(AiModerator::is_early_access_query("can i have ea?"));
        assert!(AiModerator::is_early_access_query("how to obtain ea"));
        assert!(AiModerator::is_early_access_query("how do i obtain ea"));
        assert!(AiModerator::is_early_access_query("how to get early access"));
        assert!(AiModerator::is_early_access_query("how do i get early access?"));
        assert!(AiModerator::is_early_access_query("how can i get early access"));
        assert!(AiModerator::is_early_access_query("get early access"));
        assert!(AiModerator::is_early_access_query("obtain early access"));
        assert!(AiModerator::is_early_access_query("how to get into ea"));
        assert!(AiModerator::is_early_access_query("how to join early access"));
        assert!(AiModerator::is_early_access_query("how to become a tester"));
        assert!(AiModerator::is_early_access_query("how to get tester"));
        assert!(AiModerator::is_early_access_query("how to get tester role"));
        assert!(AiModerator::is_early_access_query("is there a way to get ea?"));
        assert!(AiModerator::is_early_access_query("yo anyone know how to get ea"));
        assert!(AiModerator::is_early_access_query("i want ea"));
        assert!(AiModerator::is_early_access_query("give me ea please"));

        // Russian queries
        assert!(AiModerator::is_early_access_query("как получить ea"));
        assert!(AiModerator::is_early_access_query("как получить еа?"));
        assert!(AiModerator::is_early_access_query("как получить early access"));
        assert!(AiModerator::is_early_access_query("где взять ea"));
        assert!(AiModerator::is_early_access_query("как попасть в ea"));
        assert!(AiModerator::is_early_access_query("как стать тестером"));
        // Plea / Asking variations ("please ea", "pls ea", etc.)
        assert!(AiModerator::is_early_access_query("please ea"));
        assert!(AiModerator::is_early_access_query("pls ea"));
        assert!(AiModerator::is_early_access_query("plz ea"));
        assert!(AiModerator::is_early_access_query("plzz ea"));
        assert!(AiModerator::is_early_access_query("plsss ea"));
        assert!(AiModerator::is_early_access_query("ea please"));
        assert!(AiModerator::is_early_access_query("ea pls"));
        assert!(AiModerator::is_early_access_query("ea plz"));
        assert!(AiModerator::is_early_access_query("pls give ea"));
        assert!(AiModerator::is_early_access_query("give me ea please"));
        assert!(AiModerator::is_early_access_query("give me ea pls"));
        assert!(AiModerator::is_early_access_query("please early access"));
        assert!(AiModerator::is_early_access_query("early access pls"));
        assert!(AiModerator::is_early_access_query("пожалуйста ea"));
        assert!(AiModerator::is_early_access_query("пж ea"));
        assert!(AiModerator::is_early_access_query("ea пж"));
        assert!(AiModerator::is_early_access_query("дай ea пж"));
        assert!(AiModerator::is_early_access_query("дайте ea пожалуйста"));
        assert!(AiModerator::is_early_access_query("скиньте ea пж"));
        assert!(AiModerator::is_early_access_query("хочу early access"));

        // Repetition, bypass and typo resilience tests (Regex-hardened)
        assert!(AiModerator::is_early_access_query("i want early accesss"));
        assert!(AiModerator::is_early_access_query("i want early access"));
        assert!(AiModerator::is_early_access_query("wanna early access"));
        assert!(AiModerator::is_early_access_query("ea plssss"));
        assert!(AiModerator::is_early_access_query("e.a pls"));
        assert!(AiModerator::is_early_access_query("e a pls"));
        assert!(AiModerator::is_early_access_query("how to get e.a"));
        assert!(AiModerator::is_early_access_query("can i get e-a"));
        assert!(AiModerator::is_early_access_query("plzzz ea"));

        // Negative cases (must NOT trigger)
        assert!(!AiModerator::is_early_access_query("i have ea"));
        assert!(!AiModerator::is_early_access_query("ea is so cool"));
        assert!(!AiModerator::is_early_access_query("ea sports it's in the game"));
        assert!(!AiModerator::is_early_access_query("we beat the boss easily")); // "ea" in "beat", "easily"
        assert!(!AiModerator::is_early_access_query("clean your room"));
        assert!(!AiModerator::is_early_access_query("what did you eat?"));
        assert!(!AiModerator::is_early_access_query("how to reach level 10")); // "ea" in "reach"
        assert!(!AiModerator::is_early_access_query("how to deal with this boss")); // "ea" in "deal"
        assert!(!AiModerator::is_early_access_query("how to speak clear")); // "ea" in "speak"
        assert!(!AiModerator::is_early_access_query("hello everyone"));
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

    #[test]
    fn test_game_hunting_or_pvp_threat_unit() {
        assert!(AiModerator::is_game_hunting_or_pvp_threat("I will find you and kill you"));
        assert!(AiModerator::is_game_hunting_or_pvp_threat("i will find you and kill you in roblox"));
        assert!(AiModerator::is_game_hunting_or_pvp_threat("im gonna find u and kill u"));
        assert!(AiModerator::is_game_hunting_or_pvp_threat("найду тебя и убью в роблоксе"));
        assert!(AiModerator::is_game_hunting_or_pvp_threat("найду тебя на сервере и убью"));
        assert!(AiModerator::is_game_hunting_or_pvp_threat("i'm sending current aba wb after u"));
        assert!(AiModerator::is_game_hunting_or_pvp_threat("We gonna kill you man"));

        // If real-world address/IRL indicators exist, it's NOT just game hunting!
        assert!(!AiModerator::is_game_hunting_or_pvp_threat("I will find your house in real life and kill you"));
        assert!(!AiModerator::is_game_hunting_or_pvp_threat("найду твой адрес и убью"));
    }

    #[test]
    fn test_explicit_real_world_dox_threat_unit() {
        assert!(AiModerator::is_explicit_real_world_dox_threat("I will leak your address and phone number"));
        assert!(AiModerator::is_explicit_real_world_dox_threat("солью твой домашний адрес"));
        assert!(AiModerator::is_explicit_real_world_dox_threat("знаю где ты живешь"));

        // Meta talk is NOT a doxx threat
        assert!(!AiModerator::is_explicit_real_world_dox_threat("Its basically a doxx soo yeah"));
        assert!(!AiModerator::is_explicit_real_world_dox_threat("это деанон"));

        // In-game context is NOT a real-world doxx threat
        assert!(!AiModerator::is_explicit_real_world_dox_threat("i will doxx you in roblox"));
    }

    #[tokio::test]
    async fn test_check_message_roblox_find_and_kill() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 116,
            timestamp_unix: 1727376000,
            author_name: "RobloxWarrior",
            author_id: 55555,
            author_nick: Some("RobloxWarrior".to_string()),
            account_age_days: Some(60),
            server_member_days: Some(30),
            roles_count: 2,
            content: "I will find you and kill you in roblox",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'I will find you and kill you in roblox': {:?}\n", verdict);
        assert!(
            matches!(verdict, ModerationVerdict::Allow),
            "Expected 'I will find you and kill you in roblox' to be ALLOW, got {:?}",
            verdict
        );
    }

    #[tokio::test]
    async fn test_check_message_find_and_kill_standalone() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 117,
            timestamp_unix: 1727376000,
            author_name: "RobloxWarrior",
            author_id: 55555,
            author_nick: Some("RobloxWarrior".to_string()),
            account_age_days: Some(60),
            server_member_days: Some(30),
            roles_count: 2,
            content: "I will find you and kill you",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'I will find you and kill you': {:?}\n", verdict);
        assert!(
            matches!(verdict, ModerationVerdict::Allow),
            "Expected 'I will find you and kill you' to be ALLOW, got {:?}",
            verdict
        );
    }

    #[tokio::test]
    async fn test_check_message_find_and_kill_russian_roblox() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 118,
            timestamp_unix: 1727376000,
            author_name: "RobloxWarriorRu",
            author_id: 55556,
            author_nick: Some("RobloxWarriorRu".to_string()),
            account_age_days: Some(60),
            server_member_days: Some(30),
            roles_count: 2,
            content: "найду тебя и убью в роблоксе",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'найду тебя и убью в роблоксе': {:?}\n", verdict);
        assert!(
            matches!(verdict, ModerationVerdict::Allow),
            "Expected 'найду тебя и убью в роблоксе' to be ALLOW, got {:?}",
            verdict
        );
    }

    #[tokio::test]
    async fn test_check_message_real_life_house_threat() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(123456789),
            guild_name: Some("Gaming Arena".to_string()),
            channel_id: 1,
            channel_name: Some("lounge".to_string()),
            message_id: 119,
            timestamp_unix: 1727376000,
            author_name: "RealStalker",
            author_id: 77777,
            author_nick: Some("RealStalker".to_string()),
            account_age_days: Some(60),
            server_member_days: Some(30),
            roles_count: 2,
            content: "I will find your house in real life and kill you",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'I will find your house in real life and kill you': {:?}\n", verdict);
        assert!(
            matches!(verdict, ModerationVerdict::DeleteConfirmed { .. } | ModerationVerdict::FlagSuspicious { .. }),
            "Expected real-life house threat to be caught, got {:?}",
            verdict
        );
    }

    #[tokio::test]
    async fn test_openai_batcher_concurrent_dispatch() {
        let client = reqwest::Client::new();
        let moderator = std::sync::Arc::new(AiModerator::new(client));

        let mut handles = Vec::new();
        for i in 0..10 {
            let mod_clone = moderator.clone();
            let text = format!("batch test message {}", i);
            handles.push(tokio::spawn(async move {
                let meta = BatchLogEntry {
                    message_id: 1000 + i as u64,
                    channel_id: 1,
                    channel_name: "general".to_string(),
                    author_id: 2000 + i as u64,
                    author_name: format!("user{}", i),
                    content: text.clone(),
                    timestamp_unix: 1727376000 + i as i64,
                    reply_to: None,
                };
                mod_clone.check_openai_batch("test_key", vec![text], Some(meta)).await
            }));
        }

        for h in handles {
            let (res, transcript) = h.await.unwrap();
            assert_eq!(res.len(), 1);
            assert!(!transcript.is_empty());
        }
    }

    #[tokio::test]
    async fn test_openai_batcher_multi_input_slice() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let (res, _transcript) = moderator.check_openai_batch("test_key", vec!["hello".to_string(), "world".to_string()], None).await;
        assert_eq!(res.len(), 2);
    }

    #[test]
    fn test_build_batch_transcript_prompt_inlining() {
        let transcript = std::sync::Arc::new(vec![
            BatchLogEntry {
                message_id: 101,
                channel_id: 1,
                channel_name: "general".to_string(),
                author_id: 1001,
                author_name: "Alice".to_string(),
                content: "did anyone finish the homework?".to_string(),
                timestamp_unix: 100,
                reply_to: None,
            },
            BatchLogEntry {
                message_id: 102,
                channel_id: 1,
                channel_name: "general".to_string(),
                author_id: 1002,
                author_name: "Troll".to_string(),
                content: "kys you idiot".to_string(),
                timestamp_unix: 105,
                reply_to: Some("@Alice: homework".to_string()),
            },
            BatchLogEntry {
                message_id: 103,
                channel_id: 1,
                channel_name: "general".to_string(),
                author_id: 1003,
                author_name: "Charlie".to_string(),
                content: "whoa calm down".to_string(),
                timestamp_unix: 110,
                reply_to: None,
            },
        ]);

        let (tx, _) = tokio::sync::oneshot::channel();
        let flagged_item = GroqBatchItemRequest {
            message_id: 102,
            channel_id: 1,
            channel_name: "general".to_string(),
            author_name: "Troll".to_string(),
            author_id: 1002,
            trimmed_content: "kys you idiot".to_string(),
            telemetry_chunk: "OpenAI Score: 0.92 (harassment) | Severe: 0.89".to_string(),
            is_hardcore: true,
            batch_transcript: transcript,
            channel_history: Vec::new(),
            sender: tx,
        };

        let prompt = AiModerator::build_batch_transcript_prompt(&[flagged_item]);
        println!("\n=== GENERATED BATCH TRANSCRIPT PROMPT ===\n{}\n=========================================\n", prompt);

        assert!(prompt.contains(">>> [FLAGGED ITEM #1] <<< @Troll: \"kys you idiot\""));
        assert!(prompt.contains("@Alice: \"did anyone finish the homework?\""));
        assert!(prompt.contains("@Charlie: \"whoa calm down\""));
        assert!(prompt.contains("[ITEM 1]"));
        assert!(prompt.contains("VERDICT: [ALLOW|SUSPICIOUS|DELETE]"));
    }

    #[test]
    fn test_extract_item_id() {
        assert_eq!(AiModerator::extract_item_id("[ITEM 1]"), Some(1));
        assert_eq!(AiModerator::extract_item_id("[ITEM 2] (Channel: #general)"), Some(2));
        assert_eq!(AiModerator::extract_item_id("ITEM 3:"), Some(3));
        assert_eq!(AiModerator::extract_item_id("### ITEM #4: [ALLOW]"), Some(4));
        assert_eq!(AiModerator::extract_item_id("[MESSAGE 5]"), Some(5));
        assert_eq!(AiModerator::extract_item_id("VERDICT: ALLOW"), None);
    }

    #[test]
    fn test_parse_batch_decisions() {
        let sample_output = "\
[ITEM 1]
VERDICT: ALLOW
RULE: None
MUTE_MINUTES: 0
REASON: Roblox hunting banter

[ITEM 2]
VERDICT: DELETE
RULE: Crit (Slurs)
MUTE_MINUTES: 1440
REASON: Racial slur detected
";
        let decisions = AiModerator::parse_batch_decisions(sample_output, None, 2);
        assert_eq!(decisions.len(), 2);

        let d1 = decisions.get(&1).unwrap();
        assert_eq!(d1.verdict, "ALLOW");
        assert_eq!(d1.rule, "None");
        assert_eq!(d1.mute_minutes, 0);

        let d2 = decisions.get(&2).unwrap();
        assert_eq!(d2.verdict, "DELETE");
        assert_eq!(d2.rule, "Crit (Slurs)");
        assert_eq!(d2.mute_minutes, 1440);
    }

    #[test]
    fn test_parse_single_decision_fallback() {
        let sample_single = "\
VERDICT: ALLOW
RULE: None
MUTE_MINUTES: 0
REASON: Friendly banter
";
        let decisions = AiModerator::parse_batch_decisions(sample_single, None, 1);
        assert_eq!(decisions.len(), 1);
        let d = decisions.get(&1).unwrap();
        assert_eq!(d.verdict, "ALLOW");
        assert_eq!(d.rule, "None");
        assert_eq!(d.mute_minutes, 0);
    }

    #[tokio::test]
    async fn test_check_message_local_vector_scam() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(1),
            guild_name: Some("Test Guild".to_string()),
            channel_id: 100,
            channel_name: Some("general".to_string()),
            message_id: 200,
            timestamp_unix: 1700000000,
            author_name: "Scammer",
            author_id: 300,
            author_nick: None,
            account_age_days: Some(1),
            server_member_days: Some(0),
            roles_count: 0,
            content: "fr3333 d!sc0rd n!tr000 c1ick h3r3 n0w",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for 'fr3333 d!sc0rd n!tr000 c1ick h3r3 n0w': {:?}\n", verdict);
        assert!(
            matches!(verdict, ModerationVerdict::DeleteConfirmed { .. }),
            "Expected DeleteConfirmed from vector DB for obfuscated nitro scam, got {:?}",
            verdict
        );
    }

    #[tokio::test]
    async fn test_check_message_local_vector_clean() {
        let client = reqwest::Client::new();
        let moderator = AiModerator::new(client);
        let ctx = MessageContext {
            guild_id: Some(1),
            guild_name: Some("Test Guild".to_string()),
            channel_id: 100,
            channel_name: Some("general".to_string()),
            message_id: 201,
            timestamp_unix: 1700000001,
            author_name: "GoodUser",
            author_id: 301,
            author_nick: None,
            account_age_days: Some(30),
            server_member_days: Some(10),
            roles_count: 1,
            content: "Hey everyone, who wants to play basketball or counter-strike tonight?",
            reply_to: None,
            mentions: &[],
            attachments_info: &[],
        };
        let verdict = moderator.check_message(&ctx).await;
        println!("\n>>> LIVE TEST VERDICT for clean gaming chat: {:?}\n", verdict);
        assert!(
            matches!(verdict, ModerationVerdict::Allow),
            "Expected ALLOW for clean gaming chat, got {:?}",
            verdict
        );
    }

    #[test]
    fn test_parse_supreme_decision() {
        let allow_res = "VERDICT: ALLOW\nREASON: Standard Roblox hunting/PVP trashtalk with no real world threat.";
        let d1 = AiModerator::parse_supreme_decision(allow_res);
        assert_eq!(d1.verdict, SupremeVerdict::Allow);
        assert!(d1.reason.contains("Roblox"));

        let confirm_res = "VERDICT: CONFIRM\nREASON: Explicit real-world death threat with genuine malice.";
        let d2 = AiModerator::parse_supreme_decision(confirm_res);
        assert_eq!(d2.verdict, SupremeVerdict::Confirm);
        assert!(d2.reason.contains("death threat"));

        let markdown_res = "**VERDICT**: `ALLOW`\n**REASON**: Casual English idiom ('kill time') harmlessly used.";
        let d3 = AiModerator::parse_supreme_decision(markdown_res);
        assert_eq!(d3.verdict, SupremeVerdict::Allow);

        let bracket_confirm = "VERDICT: [CONFIRM]\nREASON: [Known steam phishing credential harvester]";
        let d4 = AiModerator::parse_supreme_decision(bracket_confirm);
        assert_eq!(d4.verdict, SupremeVerdict::Confirm);
    }
}



