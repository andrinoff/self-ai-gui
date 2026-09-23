//! Runtime configuration, read from the environment.

use std::path::PathBuf;

/// All knobs are environment variables so the binary stays deployable as one
/// file with no config format of its own.
pub struct Config {
    pub addr: String,
    pub db_path: PathBuf,
    /// OpenAI-compatible base URL, e.g. https://api.openai.com/v1 or
    /// http://127.0.0.1:11434/v1 for Ollama.
    pub base_url: String,
    pub api_key: Option<String>,
    pub default_model: String,
    /// Models offered in the UI. Empty means "ask the upstream for its list".
    pub models: Vec<String>,
    /// Whether conversations are mined for durable notes.
    pub memory_enabled: bool,
    /// User turns between automatic memory extractions.
    pub memory_every: usize,
    /// Characters of memory that may be injected into one prompt.
    pub memory_budget: usize,
    /// Model used for extraction; falls back to the conversation's model.
    pub memory_model: Option<String>,
    /// Overrides the built-in persona.
    pub system_prompt: Option<String>,
    /// How many past messages are replayed to the model.
    pub history_messages: usize,
    /// Characters of past messages that are replayed.
    pub history_budget: usize,
    /// Name used in the persona. Empty means "the user".
    pub user_name: String,
    /// Seconds to wait for the upstream before giving up.
    pub timeout_secs: u64,
}

impl Config {
    pub fn from_env() -> Self {
        let data_dir = env_or("SELF_DATA_DIR", "./data");
        Self {
            addr: env_or("SELF_ADDR", "127.0.0.1:8080"),
            db_path: PathBuf::from(data_dir).join("self.db"),
            base_url: env_or("SELF_BASE_URL", "https://api.openai.com/v1"),
            api_key: first_env(&["SELF_API_KEY", "OPENAI_API_KEY"]),
            default_model: env_or("SELF_MODEL", "gpt-4o-mini"),
            models: env_list("SELF_MODELS"),
            memory_enabled: env_flag("SELF_MEMORY", true),
            memory_every: env_num("SELF_MEMORY_EVERY", 2),
            memory_budget: env_num("SELF_MEMORY_BUDGET", 1200),
            memory_model: first_env(&["SELF_MEMORY_MODEL"]),
            system_prompt: first_env(&["SELF_SYSTEM_PROMPT"]),
            history_messages: env_num("SELF_HISTORY_MESSAGES", 24),
            history_budget: env_num("SELF_HISTORY_BUDGET", 24_000),
            user_name: env_or("SELF_USER_NAME", ""),
            timeout_secs: env_num("SELF_TIMEOUT", 120) as u64,
        }
    }

    /// The name to use in the persona: the configured one, or a neutral stand-in.
    pub fn person(&self) -> String {
        if self.user_name.trim().is_empty() {
            "the user".to_string()
        } else {
            self.user_name.trim().to_string()
        }
    }

    /// What the browser is allowed to know: no keys, just shapes and defaults.
    pub fn public(&self) -> serde_json::Value {
        serde_json::json!({
            "baseUrl": self.base_url,
            "hasKey": self.api_key.is_some(),
            "defaultModel": self.default_model,
            "pinnedModels": self.models,
            "memoryEnabled": self.memory_enabled,
            "memoryEvery": self.memory_every,
            "memoryBudget": self.memory_budget,
            "historyMessages": self.history_messages,
            "person": self.person(),
        })
    }
}

fn first_env(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| std::env::var(name).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_or(name: &str, fallback: &str) -> String {
    first_env(&[name]).unwrap_or_else(|| fallback.to_string())
}

fn env_list(name: &str) -> Vec<String> {
    first_env(&[name])
        .map(|raw| {
            raw.split(',')
                .map(|part| part.trim().to_string())
                .filter(|part| !part.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn env_flag(name: &str, fallback: bool) -> bool {
    match first_env(&[name]) {
        Some(raw) => !matches!(
            raw.to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        None => fallback,
    }
}

fn env_num<T: std::str::FromStr>(name: &str, fallback: T) -> T {
    first_env(&[name])
        .and_then(|raw| raw.parse::<T>().ok())
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_accept_the_usual_spellings() {
        // SAFETY: single-threaded test process; no other thread reads the env here.
        unsafe { std::env::set_var("SELF_TEST_FLAG", "off") };
        assert!(!env_flag("SELF_TEST_FLAG", true));
        unsafe { std::env::set_var("SELF_TEST_FLAG", "yes") };
        assert!(env_flag("SELF_TEST_FLAG", false));
        unsafe { std::env::remove_var("SELF_TEST_FLAG") };
        assert!(env_flag("SELF_TEST_FLAG", true));
    }

    #[test]
    fn lists_split_on_commas_and_drop_blanks() {
        unsafe { std::env::set_var("SELF_TEST_LIST", " a , b ,, c ") };
        assert_eq!(env_list("SELF_TEST_LIST"), vec!["a", "b", "c"]);
        unsafe { std::env::remove_var("SELF_TEST_LIST") };
        assert!(env_list("SELF_TEST_LIST").is_empty());
    }

    #[test]
    fn person_falls_back_to_a_neutral_name() {
        let mut cfg = Config::from_env();
        cfg.user_name = "   ".into();
        assert_eq!(cfg.person(), "the user");
        cfg.user_name = "Drew".into();
        assert_eq!(cfg.person(), "Drew");
    }
}
