//! Memory bookkeeping: normalising notes so duplicates collapse, choosing
//! which notes to inject into a prompt, and parsing what the extraction pass
//! hands back.

use crate::model::{Memory, ProposedMemory, valid_kind};

/// Words that carry no retrieval signal. Kept short on purpose: an aggressive
/// stop list hurts more than it helps at this scale.
const STOP_WORDS: [&str; 24] = [
    "the", "and", "for", "with", "that", "this", "you", "your", "are", "was", "what", "when",
    "how", "does", "did", "have", "has", "can", "could", "would", "about", "from", "into", "not",
];

/// Canonical form used for deduplication: lowercase, letters, digits and single
/// spaces only.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_space = true;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim().to_string()
}

fn tokens(text: &str) -> Vec<String> {
    normalize(text)
        .split(' ')
        .filter(|word| word.len() > 2 && !STOP_WORDS.contains(word))
        .map(str::to_string)
        .collect()
}

/// How relevant one note is to the sentence being answered. Overlap on its own
/// favours long notes, so it is divided by the square root of the note length;
/// notes that were useful before get a small nudge.
pub fn relevance(memory: &Memory, query: &str, now: chrono::DateTime<chrono::Utc>) -> f64 {
    let query_tokens = tokens(query);
    let memory_tokens = tokens(&memory.text);
    if query_tokens.is_empty() || memory_tokens.is_empty() {
        return 0.0;
    }
    let hits = query_tokens
        .iter()
        .filter(|token| memory_tokens.contains(token))
        .count();
    let mut score = hits as f64 / (memory_tokens.len() as f64).sqrt();

    if memory.use_count > 0 {
        score += 0.05;
        if let Ok(used_at) = chrono::DateTime::parse_from_rfc3339(&memory.last_used_at) {
            let days = (now - used_at.with_timezone(&chrono::Utc)).num_days();
            if days >= 0 && days < 14 {
                score += 0.1;
            }
        }
    }
    score
}

/// Picks the notes that go into the prompt: pinned ones always, then the most
/// relevant of the rest, stopping at a character budget so a large memory
/// never crowds out the conversation.
pub fn select_for_prompt(
    memories: &[Memory],
    query: &str,
    budget_chars: usize,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<Memory> {
    let mut chosen: Vec<Memory> = Vec::new();
    let mut used = 0usize;

    let take = |memory: &Memory, used: &mut usize, chosen: &mut Vec<Memory>| -> bool {
        let cost = memory.text.chars().count() + memory.kind.chars().count() + 8;
        if *used + cost > budget_chars && !chosen.is_empty() {
            return false;
        }
        *used += cost;
        chosen.push(memory.clone());
        true
    };

    for memory in memories.iter().filter(|m| m.pinned && m.enabled) {
        take(memory, &mut used, &mut chosen);
    }

    let mut rest: Vec<&Memory> = memories.iter().filter(|m| m.enabled && !m.pinned).collect();
    rest.sort_by(|a, b| {
        let sa = relevance(a, query, now);
        let sb = relevance(b, query, now);
        sb.partial_cmp(&sa)
            .unwrap_or(std::cmp::Ordering::Equal)
            // Ties fall back to the newest note, which is the more specific one.
            .then(b.id.cmp(&a.id))
    });
    for memory in rest {
        if !take(memory, &mut used, &mut chosen) {
            break;
        }
    }
    chosen
}

/// Phrases that mean "do not keep this", whatever the model proposes.
const SECRET_HINTS: [&str; 6] = [
    "password",
    "passphrase",
    "api key",
    "apikey",
    "token",
    "secret",
];

fn looks_like_a_secret(text: &str) -> bool {
    let lower = text.to_lowercase();
    SECRET_HINTS.iter().any(|hint| lower.contains(hint))
}

/// Reads the extraction reply: models like to wrap JSON in prose or fences, so
/// the array is located rather than assumed.
pub fn parse_extraction(raw: &str) -> Vec<ProposedMemory> {
    let Some(start) = raw.find('[') else {
        return Vec::new();
    };
    let Some(end) = raw.rfind(']') else {
        return Vec::new();
    };
    if end < start {
        return Vec::new();
    }
    let slice = &raw[start..=end];
    let Ok(items) = serde_json::from_str::<Vec<ProposedMemory>>(slice) else {
        return Vec::new();
    };

    let mut out: Vec<ProposedMemory> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for item in items {
        let text = item.text.trim().trim_matches('"').trim().to_string();
        if text.len() < 4 || text.chars().count() > 300 || looks_like_a_secret(&text) {
            continue;
        }
        let kind = if valid_kind(&item.kind) {
            item.kind.to_lowercase()
        } else {
            "fact".to_string()
        };
        let key = normalize(&text);
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(ProposedMemory { text, kind });
        if out.len() == 6 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory(id: i64, text: &str) -> Memory {
        Memory {
            id,
            text: text.into(),
            kind: "fact".into(),
            source: "user".into(),
            pinned: false,
            enabled: true,
            created_at: "2026-01-01T00:00:00Z".into(),
            last_used_at: String::new(),
            use_count: 0,
        }
    }

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-06-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    #[test]
    fn normalize_collapses_punctuation_and_case() {
        assert_eq!(
            normalize("  Prefers   TEA, not coffee!  "),
            "prefers tea not coffee"
        );
        assert_eq!(normalize("---"), "");
        assert_eq!(normalize("A/b"), "a b");
    }

    #[test]
    fn relevance_favours_the_matching_note() {
        let tea = memory(1, "Prefers tea over coffee.");
        let rust = memory(2, "Building a Rust dashboard.");
        let question = "What should I drink while I work on the dashboard?";
        assert!(relevance(&rust, question, now()) > relevance(&tea, question, now()));
        assert_eq!(relevance(&tea, "", now()), 0.0);
    }

    #[test]
    fn pinned_notes_always_make_it_in_and_come_first() {
        let pinned = Memory {
            pinned: true,
            ..memory(1, "Allergic to peanuts.")
        };
        let other = memory(2, "Building a Rust dashboard.");
        let chosen = select_for_prompt(&[pinned, other], "zzz unrelated", 400, now());
        assert!(
            chosen.iter().any(|m| m.text == "Allergic to peanuts."),
            "the pinned note must be present"
        );
        assert_eq!(
            chosen[0].text, "Allergic to peanuts.",
            "the pinned note comes first"
        );
    }

    #[test]
    fn selection_respects_the_budget_and_relevance_order() {
        let notes: Vec<Memory> = (0..20)
            .map(|i| memory(i, &format!("Note number {i} about the dashboard project.")))
            .collect();
        let chosen = select_for_prompt(&notes, "tell me about the dashboard project", 200, now());
        assert!(!chosen.is_empty());
        let spent: usize = chosen
            .iter()
            .map(|m| m.text.chars().count() + m.kind.chars().count() + 8)
            .sum();
        assert!(spent <= 200 + 60, "budget overrun: {spent}");
    }

    #[test]
    fn disabled_notes_are_never_injected() {
        let off = Memory {
            enabled: false,
            pinned: true,
            ..memory(1, "Old and wrong.")
        };
        assert!(select_for_prompt(&[off], "anything", 400, now()).is_empty());
    }

    #[test]
    fn extraction_parsing_tolerates_fences_and_prose() {
        let raw = "Sure! Here you go:\n```json\n[{\"text\": \"Prefers tea.\", \"kind\": \"preference\"}]\n```";
        let got = parse_extraction(raw);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "Prefers tea.");
        assert_eq!(got[0].kind, "preference");
    }

    #[test]
    fn extraction_parsing_drops_secrets_duplicates_and_junk() {
        let raw = r#"[
            {"text": "My API key is sk-abc123", "kind": "fact"},
            {"text": "Prefers tea.", "kind": "preference"},
            {"text": " prefers TEA! ", "kind": "preference"},
            {"text": "x", "kind": "fact"},
            {"text": "Sister is called Maya.", "kind": "made-up-kind"}
        ]"#;
        let got = parse_extraction(raw);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].text, "Prefers tea.");
        assert_eq!(got[1].text, "Sister is called Maya.");
        assert_eq!(got[1].kind, "fact", "unknown kinds fall back to fact");
    }

    #[test]
    fn extraction_parsing_survives_bad_json() {
        assert!(parse_extraction("I could not find anything.").is_empty());
        assert!(parse_extraction("[{").is_empty());
    }
}
