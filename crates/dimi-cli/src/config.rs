//! Configuration loading for the dimi CLI — the slice-6 flat `config.toml`
//! schema.
//!
//! The TS side keeps the real schema in `apps/dimi/src/tui/config.ts`
//! (`TuiConfig` / per-section keys). Slice 6 only needs the four fields the
//! app shell consumes (model / work_dir / wire / theme); fields are optional
//! and a missing file, a partial table, or an unparseable file all fall back
//! to `Config::default()` (every field `None`). The TOML surface is stable:
//! `model = "..."`, `work_dir = "..."`, `wire = "..."`, `theme = "..."` at
//! the top level, mirroring the corresponding `AppState`/`TuiConfig` keys.

use std::path::{Path, PathBuf};

use dimi_engine::types::ProviderConfig;

/// Slice-6 app configuration.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    /// Active model id (welcome panel + footer model slot + engine provider).
    pub model: Option<String>,
    /// Working directory (welcome panel + footer cwd slot + tool cwd).
    pub work_dir: Option<String>,
    /// `wire.jsonl` path to cold-rebuild the transcript at startup.
    pub wire: Option<String>,
    /// Theme name (`dark` / `light`). Slice 6 always runs dark; the field is
    /// parsed for forward compat and reserved for the theme-switch slice.
    pub theme: Option<String>,
    /// OpenAI-compatible provider base URL (slice 6a: engine direct connect).
    pub base_url: Option<String>,
    /// Provider API key (slice 6a: engine direct connect).
    pub api_key: Option<String>,
    /// Optional reasoning effort forwarded to the provider (slice 6a).
    pub thinking_effort: Option<String>,
}

impl Config {
    /// The engine [`ProviderConfig`], present when model + base_url + api_key
    /// are all configured (slice 6a). `None` → the backend surfaces a clear
    /// "provider not configured" status instead of a dead turn.
    pub fn provider_config(&self) -> Option<ProviderConfig> {
        let model = self.model.as_ref()?;
        let base_url = self.base_url.as_ref()?;
        let api_key = self.api_key.as_ref()?;
        Some(ProviderConfig {
            base_url: base_url.clone(),
            api_key: api_key.clone(),
            model: model.clone(),
            thinking_effort: self.thinking_effort.clone(),
        })
    }
}

/// Load configuration from a TOML file. Missing/unreadable/unparseable input
/// falls back to [`Config::default`] — the CLI must still start.
pub fn load_config(path: &Path) -> Config {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Config::default();
    };
    toml::from_str(&content).unwrap_or_default()
}

/// The default config path: `~/.dimi/config.toml` (falls back to a relative
/// `.dimi/config.toml` when `$HOME` is unset, e.g. in sandboxes).
pub fn default_config_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".dimi").join("config.toml"))
        .unwrap_or_else(|| PathBuf::from(".dimi/config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{name}_{}", std::process::id()))
    }

    #[test]
    fn load_config_reads_full_toml() {
        let dir = temp_dir("dimi_cli_config_full");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            r#"
            model = "claude-sonnet-4-5"
            work_dir = "/tmp/proj"
            wire = "/tmp/wire.jsonl"
            theme = "dark"
            base_url = "https://api.example.com/v1"
            api_key = "sk-abc"
            thinking_effort = "high"
            "#,
        )
        .unwrap();

        let config = load_config(&path);
        assert_eq!(config.model.as_deref(), Some("claude-sonnet-4-5"));
        assert_eq!(config.work_dir.as_deref(), Some("/tmp/proj"));
        assert_eq!(config.wire.as_deref(), Some("/tmp/wire.jsonl"));
        assert_eq!(config.theme.as_deref(), Some("dark"));
        assert_eq!(
            config.base_url.as_deref(),
            Some("https://api.example.com/v1")
        );
        assert_eq!(config.api_key.as_deref(), Some("sk-abc"));
        assert_eq!(config.thinking_effort.as_deref(), Some("high"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_config_partial_table_defaults_missing_fields() {
        let dir = temp_dir("dimi_cli_config_partial");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "model = \"gpt-5\"\n").unwrap();

        let config = load_config(&path);
        assert_eq!(config.model.as_deref(), Some("gpt-5"));
        assert!(config.work_dir.is_none());
        assert!(config.wire.is_none());
        assert!(config.theme.is_none());
        assert!(config.base_url.is_none());
        assert!(config.api_key.is_none());
        assert!(config.thinking_effort.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_config_missing_file_returns_defaults() {
        let config = load_config(Path::new("/nonexistent/dimi/config.toml"));
        assert!(config.model.is_none());
        assert!(config.work_dir.is_none());
        assert!(config.wire.is_none());
        assert!(config.theme.is_none());
        assert!(config.base_url.is_none());
        assert!(config.api_key.is_none());
    }

    #[test]
    fn load_config_invalid_toml_returns_defaults() {
        let dir = temp_dir("dimi_cli_config_bad");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "model = [unterminated").unwrap();

        let config = load_config(&path);
        assert!(config.model.is_none());
        assert!(config.wire.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn default_config_path_points_at_home() {
        let home = std::env::var("HOME").ok();
        let path = default_config_path();
        if let Some(home) = home {
            assert!(path.starts_with(home), "path: {}", path.display());
        }
        assert_eq!(
            path.file_name().and_then(|s| s.to_str()),
            Some("config.toml")
        );
    }
}
