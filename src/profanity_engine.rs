use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

const MAX_CANDIDATES: usize = 128;
const MAX_TOKEN_LEN: usize = 28;

/// Hardcoded fallback roots in case rust_dict.txt is missing
const CORE_SEVERE_ROOTS: &[&str] = &[
    "хуй", "хуя", "хуе", "хуи", "хую", "хуем", "хуйня", "хуета", "хуевы",
    "пизд", "пиздец", "пизда", "пиздо", "пизди",
    "ебат", "ебал", "ебан", "ебло", "ебуч", "еблив", "заеб", "выеб", "доеб", "уеб", "долбоеб", "долбоёб",
    "бляд", "блят", "бля",
    "сука", "сучк", "сучар", "ссук",
    "пидор", "пидарас", "пидорас", "педик",
    "гондон", "гандон",
    "убейся", "сдохни", "сдохнуть", "пристрелю", "зарежу", "повеситься", "вскройся",
    "kys", "kill yourself", "slit your", "hang yourself",
];

#[derive(Clone, Debug)]
pub struct Candidate {
    pub text: String,
    pub obfuscated: bool,
}

#[derive(Clone, Debug)]
pub struct ProfanityMatchResult {
    pub matched_rule: String,
    pub detected_token: String,
    pub is_obfuscated: bool,
    pub is_fuzzy: bool,
    pub is_severe_root: bool,
}

pub struct ProfanityEngine {
    dict: HashSet<String>,
    dict_by_len: HashMap<usize, Vec<String>>,
    clean_words: HashSet<String>,
    severe_roots: Vec<String>,
}

impl Default for ProfanityEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ProfanityEngine {
    fn resolve_file_path(candidates: &[&str]) -> Option<PathBuf> {
        for candidate in candidates {
            let p = PathBuf::from(candidate);
            if p.exists() {
                return Some(p);
            }
            if let Ok(exe) = std::env::current_exe() {
                if let Some(parent) = exe.parent() {
                    let candidate_path = parent.join(candidate);
                    if candidate_path.exists() {
                        return Some(candidate_path);
                    }
                }
            }
        }
        None
    }

    pub fn new() -> Self {
        let mut dict = HashSet::new();

        // 1. Try loading profanity dictionary from multiple standard portable paths
        let paths = [
            "rust_dict.txt",
            "vector_engine/rust_dict.txt",
            "../rust_dict.txt",
        ];

        let mut loaded_from_file = false;
        if let Some(p) = Self::resolve_file_path(&paths) {
            if let Ok(file) = File::open(&p) {
                let reader = BufReader::new(file);
                for line in reader.lines().flatten() {
                    let trimmed = line.trim().to_lowercase();
                    if !trimmed.is_empty() && !trimmed.starts_with('#') {
                        dict.insert(trimmed);
                    }
                }
                if !dict.is_empty() {
                    loaded_from_file = true;
                    println!(
                        "   🛡️ [PROFANITY ENGINE] Ingested {} profanity/threat rules from '{}'",
                        dict.len(),
                        p.display()
                    );
                }
            }
        }

        // 2. If file missing or incomplete, ensure core roots are present
        for root in CORE_SEVERE_ROOTS {
            dict.insert(root.to_string());
        }

        if !loaded_from_file {
            println!(
                "   ⚠️ [PROFANITY ENGINE] rust_dict.txt not found, using embedded core roots ({} rules)",
                dict.len()
            );
        }

        // 3. Bucket bad words by length for instant O(1) Damerau-Levenshtein candidates
        let mut dict_by_len: HashMap<usize, Vec<String>> = HashMap::new();
        for word in &dict {
            if word.len() >= 3 && word.len() <= MAX_TOKEN_LEN {
                dict_by_len.entry(word.len()).or_default().push(word.clone());
            }
        }

        // 4. Ingest clean dictionary words from whitelist to suppress false-positive fuzzy matches
        let mut clean_words = HashSet::new();
        let clean_paths = [
            "whitelist.txt",
            "whitelists/profanity_destroyer_whitelist.txt",
            "vector_engine/whitelist.txt",
            "../whitelist.txt",
        ];
        if let Some(cp) = Self::resolve_file_path(&clean_paths) {
            if let Ok(file) = File::open(&cp) {
                let reader = BufReader::new(file);
                for line in reader.lines().flatten() {
                    let w = line.trim().to_lowercase();
                    if !w.is_empty() && !dict.contains(&w) {
                        clean_words.insert(w);
                    }
                }
                if !clean_words.is_empty() {
                    println!(
                        "   📖 [PROFANITY ENGINE] Loaded {} clean dictionary words to guard against fuzzy false positives from '{}'",
                        clean_words.len(),
                        cp.display()
                    );
                }
            }
        }

        let severe_roots = CORE_SEVERE_ROOTS.iter().map(|s| s.to_string()).collect();

        Self {
            dict,
            dict_by_len,
            clean_words,
            severe_roots,
        }
    }

    /// Fast scan of incoming text using SIMD-style normalization + candidate chunk extraction + fuzzy matching
    pub fn scan(&self, text: &str) -> Option<ProfanityMatchResult> {
        if text.trim().is_empty() {
            return None;
        }

        let lower = text.to_lowercase();

        // Step 1: Check severe root substrings directly on normalized lower text
        for root in &self.severe_roots {
            if lower.contains(root) {
                return Some(ProfanityMatchResult {
                    matched_rule: root.clone(),
                    detected_token: root.clone(),
                    is_obfuscated: false,
                    is_fuzzy: false,
                    is_severe_root: true,
                });
            }
        }

        // Step 2: Extract multi-level candidates (including spaced-out joins like "п о ш е л", "f u c k", "s c a m")
        let candidates = self.extract_candidates(text);

        // Step 3: Exact candidate match against the dictionary
        for cand in &candidates {
            if self.dict.contains(&cand.text) {
                return Some(ProfanityMatchResult {
                    matched_rule: cand.text.clone(),
                    detected_token: cand.text.clone(),
                    is_obfuscated: cand.obfuscated,
                    is_fuzzy: false,
                    is_severe_root: self.severe_roots.iter().any(|r| cand.text.contains(r)),
                });
            }

            // Also check severe roots as substring of candidate
            for root in &self.severe_roots {
                if cand.text.contains(root) {
                    return Some(ProfanityMatchResult {
                        matched_rule: root.clone(),
                        detected_token: cand.text.clone(),
                        is_obfuscated: cand.obfuscated,
                        is_fuzzy: false,
                        is_severe_root: true,
                    });
                }
            }
        }

        // Step 4: Fuzzy match via Damerau-Levenshtein with clean-word suppression and boundary pruning
        for cand in &candidates {
            if let Some(matched) = self.fuzzy_match(&cand.text, cand.obfuscated) {
                return Some(ProfanityMatchResult {
                    matched_rule: matched,
                    detected_token: cand.text.clone(),
                    is_obfuscated: true,
                    is_fuzzy: true,
                    is_severe_root: false,
                });
            }
        }

        None
    }

    /// Extract obfuscation candidates:
    /// - Strips zero-width chars
    /// - Normalizes base, Latin-leet, and Cyrillic-homoglyph variants
    /// - Collapses repeated characters (e.g. "сууукааа" -> "сука")
    /// - Merges sliding chunks for spaced-out evasions (e.g. "п о ш е л   н а х у й" -> "пошел", "нахуй")
    fn extract_candidates(&self, text: &str) -> Vec<Candidate> {
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();

        let clean_base = strip_invisible_and_lower(text);
        let cyrillic_variant = to_cyrillic(&clean_base);
        let latin_variant = to_latin(&clean_base);

        let mut variants = vec![(clean_base, false)];
        if cyrillic_variant != variants[0].0 {
            variants.push((cyrillic_variant, true));
        }
        if latin_variant != variants[0].0 && variants.get(1).map(|v| v.0 != latin_variant).unwrap_or(true) {
            variants.push((latin_variant, true));
        }

        for (variant_text, is_transformed) in variants {
            let mut chunks = Vec::new();

            for raw_chunk in variant_text.split_whitespace() {
                let clean: String = raw_chunk.chars().filter(|c| c.is_alphanumeric()).collect();
                if clean.is_empty() || clean.len() > MAX_TOKEN_LEN {
                    continue;
                }

                let collapsed_single = collapse_repeats(&clean, 1);
                let collapsed_double = collapse_repeats(&clean, 2);

                if clean.len() >= 2 && seen.insert(clean.clone()) {
                    candidates.push(Candidate {
                        text: clean.clone(),
                        obfuscated: is_transformed,
                    });
                }

                if collapsed_single != clean && collapsed_single.len() >= 2 && seen.insert(collapsed_single.clone()) {
                    candidates.push(Candidate {
                        text: collapsed_single.clone(),
                        obfuscated: true,
                    });
                }

                if collapsed_double != clean && collapsed_double != collapsed_single && collapsed_double.len() >= 2 && seen.insert(collapsed_double.clone()) {
                    candidates.push(Candidate {
                        text: collapsed_double.clone(),
                        obfuscated: true,
                    });
                }

                chunks.push(collapsed_single);
                if candidates.len() >= MAX_CANDIDATES {
                    return candidates;
                }
            }

            // Sliding window merge for spaced-out characters (e.g. "п о ш е л   н а х у й", "s c a m", "f u c k", "k y s")
            // ONLY merge when tokens are single characters (or 1-2 char fragments) and NOT existing clean words
            for start in 0..chunks.len() {
                if chunks[start].chars().count() > 2 || self.clean_words.contains(&chunks[start]) {
                    continue;
                }

                let mut combined = chunks[start].clone();
                let mut single_count = if chunks[start].chars().count() == 1 { 1 } else { 0 };

                for end in (start + 1)..usize::min(start + 8, chunks.len()) {
                    if chunks[end].chars().count() > 2 || self.clean_words.contains(&chunks[end]) {
                        break;
                    }

                    combined.push_str(&chunks[end]);
                    if chunks[end].chars().count() == 1 {
                        single_count += 1;
                    }

                    if combined.len() > MAX_TOKEN_LEN {
                        break;
                    }

                    // Only valid spaced-out evasion if at least 2 single characters were merged
                    if single_count >= 2 && combined.chars().count() >= 3 && seen.insert(combined.clone()) {
                        candidates.push(Candidate {
                            text: combined.clone(),
                            obfuscated: true,
                        });
                        if candidates.len() >= MAX_CANDIDATES {
                            return candidates;
                        }
                    }
                }
            }
        }

        candidates
    }

    /// Fast Damerau-Levenshtein fuzzy matching with length-bucketed pruning
    /// and clean-word false-positive suppression (identical to profanity-destroyer logic)
    fn fuzzy_match(&self, token: &str, is_obfuscated: bool) -> Option<String> {
        // Words shorter than 5 chars MUST NEVER be fuzzy matched!
        // For length <= 4, only exact match is safe (otherwise "for" -> "fkr", "new" -> "nfw", "hear" -> "hoer").
        if token.chars().count() < 5 || token.len() > MAX_TOKEN_LEN {
            return None;
        }

        // Suppress fuzzy matching on clean dictionary words unconditionally!
        if self.clean_words.contains(token) {
            return None;
        }

        let max_dist = if token.chars().count() <= 5 { 1 } else { 2 };
        let min_len = token.len().saturating_sub(max_dist);
        let max_len = token.len() + max_dist;

        let token_chars: Vec<char> = token.chars().collect();
        let (first_char, last_char) = (*token_chars.first()?, *token_chars.last()?);

        for len in min_len..=max_len {
            if let Some(bucket) = self.dict_by_len.get(&len) {
                for bad in bucket {
                    let bad_chars: Vec<char> = bad.chars().collect();
                    if bad_chars.is_empty() {
                        continue;
                    }

                    // Fast boundary pruning: first AND last char must match
                    if first_char != bad_chars[0] || last_char != bad_chars[bad_chars.len() - 1] {
                        continue;
                    }

                    if let Some(dist) = damerau_levenshtein_chars(&token_chars, &bad_chars, max_dist) {
                        if dist <= 1 {
                            return Some(bad.clone());
                        }
                        if dist == 2 && is_obfuscated && token.chars().count() >= 7 {
                            return Some(bad.clone());
                        }
                    }
                }
            }
        }

        None
    }
}

/// Base strip of zero-width and invisible format characters
fn strip_invisible_and_lower(text: &str) -> String {
    text.chars()
        .filter(|&ch| !matches!(ch, '\u{200B}'..='\u{200D}' | '\u{FEFF}' | '\u{00AD}' | '\u{2060}'))
        .collect::<String>()
        .to_lowercase()
}

/// Maps Latin visual lookalikes and leet digits to Cyrillic
fn to_cyrillic(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            'a' => 'а',
            'c' => 'с',
            'e' => 'е',
            'o' => 'о',
            'p' => 'р',
            'x' => 'х',
            'y' => 'у',
            'k' => 'к',
            'm' => 'м',
            't' => 'т',
            'b' => 'в',
            '4' => 'а',
            '3' => 'е',
            '0' => 'о',
            _ => ch,
        })
        .collect()
}

/// Maps Cyrillic visual lookalikes and leet symbols to Latin
fn to_latin(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            'а' => 'a',
            'с' => 'c',
            'е' | 'ё' => 'e',
            'о' => 'o',
            'р' => 'p',
            'х' => 'x',
            'у' => 'y',
            'к' => 'k',
            'м' => 'm',
            'т' => 't',
            '@' | '4' => 'a',
            '3' => 'e',
            '1' | '!' | '|' => 'i',
            '0' => 'o',
            '$' | '5' => 's',
            '7' | '+' => 't',
            '8' => 'b',
            '2' => 'z',
            _ => ch,
        })
        .collect()
}

/// Collapse repeated characters (3+ identical consecutive characters down to max_run limit)
fn collapse_repeats(text: &str, max_run: usize) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev: Option<char> = None;
    let mut count = 0;

    for ch in text.chars() {
        if prev == Some(ch) {
            count += 1;
            if count <= max_run {
                out.push(ch);
            }
        } else {
            prev = Some(ch);
            count = 1;
            out.push(ch);
        }
    }

    out
}

/// Damerau-Levenshtein distance calculation over unicode chars
fn damerau_levenshtein_chars(a: &[char], b: &[char], max_dist: usize) -> Option<usize> {
    let a_len = a.len();
    let b_len = b.len();

    if a_len.abs_diff(b_len) > max_dist {
        return None;
    }

    let mut prev_prev = vec![0usize; b_len + 1];
    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0usize; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        let mut row_min = curr[0];

        for j in 1..=b_len {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };

            let deletion = prev[j] + 1;
            let insertion = curr[j - 1] + 1;
            let substitution = prev[j - 1] + cost;
            let mut value = deletion.min(insertion).min(substitution);

            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                value = value.min(prev_prev[j - 2] + 1);
            }

            curr[j] = value;
            row_min = row_min.min(value);
        }

        if row_min > max_dist {
            return None;
        }

        std::mem::swap(&mut prev_prev, &mut prev);
        std::mem::swap(&mut prev, &mut curr);
    }

    let dist = prev[b_len];
    if dist <= max_dist {
        Some(dist)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spaced_out_profanity() {
        let engine = ProfanityEngine::new();
        let res = engine.scan("п о ш е л   н а х у й");
        assert!(res.is_some(), "Should catch spaced-out Russian profanity");

        let res_en = engine.scan("f u c k   y o u");
        assert!(res_en.is_some(), "Should catch spaced-out English profanity");
    }

    #[test]
    fn test_leet_and_repeats() {
        let engine = ProfanityEngine::new();
        let res = engine.scan("сууууукааа");
        assert!(res.is_some(), "Should catch collapsed repeat profanity");

        let res_cyka = engine.scan("cyka");
        assert!(res_cyka.is_some(), "Should catch Latin homoglyph cyka");

        let res_kys = engine.scan("k   y   s");
        assert!(res_kys.is_some(), "Should catch spaced out kys");
    }

    #[test]
    fn test_false_positive_suppression() {
        let engine = ProfanityEngine::new();
        let innocent_phrases = [
            "3rd or 4th for ea 10th or 11th for public",
            "You get ea by winning a giveaway an event or handpicked for being active",
            "I’m not wrong am i",
            "can i hear it",
            "Only 2 bs",
            "How is auxluvy famous",
            "He has the role",
            "ive been making so much new music",
            "lead dev for neighbors and the corner",
            "i dont even know how to feel",
            "I got so lucky",
            "what's bf?",
            "Bro how did you alr forget 😭",
            "Sadly",
            "Fr",
            "Wassup",
            "Nope",
        ];

        for phrase in &innocent_phrases {
            let res = engine.scan(phrase);
            assert!(
                res.is_none(),
                "Innocent phrase '{}' must NOT trigger profanity engine, but hit: {:?}",
                phrase,
                res
            );
        }
    }
}

