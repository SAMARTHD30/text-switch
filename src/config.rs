use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The key you tap (after typing a trigger) to expand it. The presets are keys
/// that are inert on their own, so expansion never collides with Enter (send),
/// Tab, or Space in chat / coding tools. `Custom` holds the `rdev::Key` debug
/// name of any key the user captured, so they can bind whatever they like. The
/// concrete `rdev::Key` mapping lives in the hook (keeps this module free of the
/// input-backend dependency).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ActivationKey {
    #[default]
    RightCtrl,
    RightShift,
    Insert,
    ScrollLock,
    Pause,
    F9,
    /// A user-captured key, stored as its `rdev::Key` debug name (e.g. "F10").
    Custom(String),
}

impl ActivationKey {
    /// The preset choices, in the order shown in the editor dropdown.
    pub const ALL: [ActivationKey; 6] = [
        ActivationKey::RightCtrl,
        ActivationKey::RightShift,
        ActivationKey::Insert,
        ActivationKey::ScrollLock,
        ActivationKey::Pause,
        ActivationKey::F9,
    ];

    /// Stable id written to the TOML file. Custom keys are namespaced with a
    /// `custom:` prefix so they never collide with a preset id.
    pub fn id(&self) -> String {
        match self {
            ActivationKey::RightCtrl => "right_ctrl".to_string(),
            ActivationKey::RightShift => "right_shift".to_string(),
            ActivationKey::Insert => "insert".to_string(),
            ActivationKey::ScrollLock => "scroll_lock".to_string(),
            ActivationKey::Pause => "pause".to_string(),
            ActivationKey::F9 => "f9".to_string(),
            ActivationKey::Custom(name) => format!("custom:{name}"),
        }
    }

    /// Human-friendly label for the editor dropdown.
    pub fn label(&self) -> String {
        match self {
            ActivationKey::RightCtrl => "Right Ctrl".to_string(),
            ActivationKey::RightShift => "Right Shift".to_string(),
            ActivationKey::Insert => "Insert".to_string(),
            ActivationKey::ScrollLock => "Scroll Lock".to_string(),
            ActivationKey::Pause => "Pause / Break".to_string(),
            ActivationKey::F9 => "F9".to_string(),
            ActivationKey::Custom(name) => pretty_key_name(name),
        }
    }

    /// Resolve an id from the file; unknown ids fall back to the default.
    pub fn from_id(s: &str) -> ActivationKey {
        if let Some(name) = s.strip_prefix("custom:") {
            return ActivationKey::Custom(name.to_string());
        }
        ActivationKey::ALL
            .into_iter()
            .find(|k| k.id() == s)
            .unwrap_or_default()
    }
}

/// Turn an `rdev::Key` debug name into a readable label by splitting camelCase
/// ("ControlRight" -> "Control Right", "ScrollLock" -> "Scroll Lock", "F10" -> "F10").
fn pretty_key_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    for (i, c) in name.chars().enumerate() {
        if i > 0 && c.is_uppercase() {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// Result of an editor-initiated "press any key" capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureResult {
    /// The user pressed a key to bind as the activation key.
    Key(ActivationKey),
    /// The user pressed Escape to cancel capture.
    Cancelled,
}

/// Editor color theme. Persisted so the choice survives restarts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    /// Stable id written to the TOML file.
    pub fn id(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Light => "light",
        }
    }

    /// The other theme (for a toggle button).
    pub fn toggled(self) -> Theme {
        match self {
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::Dark,
        }
    }

    /// Human-friendly label.
    pub fn label(self) -> &'static str {
        match self {
            Theme::Dark => "Dark",
            Theme::Light => "Light",
        }
    }

    /// Resolve an id from the file; anything unknown falls back to the default.
    pub fn from_id(s: &str) -> Theme {
        match s {
            "light" => Theme::Light,
            _ => Theme::Dark,
        }
    }
}

/// Engine configuration: trigger word -> replacement text, plus user settings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Config {
    pub map: HashMap<String, String>,
    pub activation: ActivationKey,
    pub theme: Theme,
}

/// An ordered trigger/replacement pair, exactly as it appears in the file.
/// The editor works with these (order + duplicates preserved); the engine
/// collapses them into a `Config` map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchEntry {
    pub trigger: String,
    pub replace: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RawConfig {
    // Scalar keys must precede the `[[match]]` tables for valid TOML; serde
    // serializes struct fields in declaration order, so keep these first.
    #[serde(default)]
    activation: Option<String>,
    #[serde(default)]
    theme: Option<String>,
    #[serde(default, rename = "match")]
    matches: Vec<MatchEntry>,
}

/// Everything the editor loads from the triggers file: settings + ordered entries.
#[derive(Debug, Default)]
pub struct FileData {
    pub activation: ActivationKey,
    pub theme: Theme,
    pub entries: Vec<MatchEntry>,
}

/// Parse TOML text into settings plus an ordered list of entries (file order
/// preserved). Returns Err(message) if the text is not valid TOML.
pub fn parse_file(text: &str) -> Result<FileData, String> {
    let raw: RawConfig = toml::from_str(text).map_err(|e| e.to_string())?;
    Ok(FileData {
        activation: raw
            .activation
            .as_deref()
            .map(ActivationKey::from_id)
            .unwrap_or_default(),
        theme: raw.theme.as_deref().map(Theme::from_id).unwrap_or_default(),
        entries: raw.matches,
    })
}

/// Parse TOML text into an ordered list of entries (settings discarded).
pub fn parse_entries(text: &str) -> Result<Vec<MatchEntry>, String> {
    Ok(parse_file(text)?.entries)
}

/// Build the engine `Config` from ordered entries.
/// - Empty / whitespace-only triggers are skipped.
/// - On duplicate triggers, the last definition wins.
pub fn entries_to_config(entries: &[MatchEntry]) -> Config {
    let mut map = HashMap::new();
    for e in entries {
        let trigger = e.trigger.trim().to_string();
        if trigger.is_empty() {
            continue;
        }
        map.insert(trigger, e.replace.clone());
    }
    Config {
        map,
        ..Default::default()
    }
}

/// Serialize the settings + ordered entries back to TOML text for disk.
pub fn entries_to_toml(activation: &ActivationKey, theme: Theme, entries: &[MatchEntry]) -> String {
    let raw = RawConfig {
        activation: Some(activation.id()),
        theme: Some(theme.id().to_string()),
        matches: entries.to_vec(),
    };
    toml::to_string_pretty(&raw).unwrap_or_default()
}

/// Parse the TOML text of a triggers file straight into an engine `Config`.
pub fn parse_config(text: &str) -> Result<Config, String> {
    let fd = parse_file(text)?;
    let mut cfg = entries_to_config(&fd.entries);
    cfg.activation = fd.activation;
    cfg.theme = fd.theme;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(trigger: &str, replace: &str) -> MatchEntry {
        MatchEntry {
            trigger: trigger.to_string(),
            replace: replace.to_string(),
        }
    }

    #[test]
    fn parses_a_single_match() {
        let cfg = parse_config(
            r#"
            [[match]]
            trigger = "cli"
            replace = "hello world"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.map.get("cli"), Some(&"hello world".to_string()));
        assert_eq!(cfg.map.len(), 1);
    }

    #[test]
    fn empty_file_is_valid_and_empty() {
        let cfg = parse_config("").unwrap();
        assert!(cfg.map.is_empty());
    }

    #[test]
    fn skips_empty_or_whitespace_triggers() {
        let cfg = parse_config(
            r#"
            [[match]]
            trigger = "   "
            replace = "ignored"

            [[match]]
            trigger = "ok"
            replace = "kept"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.map.len(), 1);
        assert_eq!(cfg.map.get("ok"), Some(&"kept".to_string()));
    }

    #[test]
    fn duplicate_trigger_last_wins() {
        let cfg = parse_config(
            r#"
            [[match]]
            trigger = "x"
            replace = "first"

            [[match]]
            trigger = "x"
            replace = "second"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.map.get("x"), Some(&"second".to_string()));
    }

    #[test]
    fn invalid_toml_returns_err() {
        assert!(parse_config("this is = = not toml").is_err());
    }

    #[test]
    fn parse_entries_preserves_order() {
        let entries = parse_entries(
            r#"
            [[match]]
            trigger = "b"
            replace = "1"

            [[match]]
            trigger = "a"
            replace = "2"
            "#,
        )
        .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].trigger, "b");
        assert_eq!(entries[1].trigger, "a");
    }

    #[test]
    fn toml_round_trips_including_multiline() {
        let original = vec![
            entry("cli", "single line"),
            entry("fixbug", "line one\nline two\nline three"),
        ];
        let text = entries_to_toml(&ActivationKey::default(), Theme::default(), &original);
        let parsed = parse_entries(&text).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn settings_round_trip_through_toml() {
        let text = entries_to_toml(&ActivationKey::F9, Theme::Light, &[entry("cli", "x")]);
        let fd = parse_file(&text).unwrap();
        assert_eq!(fd.activation, ActivationKey::F9);
        assert_eq!(fd.theme, Theme::Light);
        assert_eq!(fd.entries.len(), 1);
    }

    #[test]
    fn custom_activation_key_round_trips_through_toml() {
        let custom = ActivationKey::Custom("F10".to_string());
        let text = entries_to_toml(&custom, Theme::Dark, &[entry("cli", "x")]);
        let fd = parse_file(&text).unwrap();
        assert_eq!(fd.activation, custom);
    }

    #[test]
    fn custom_id_is_namespaced_and_parses_back() {
        let custom = ActivationKey::Custom("ControlRight".to_string());
        assert_eq!(custom.id(), "custom:ControlRight");
        assert_eq!(
            ActivationKey::from_id("custom:ControlRight"),
            ActivationKey::Custom("ControlRight".to_string())
        );
    }

    #[test]
    fn custom_label_splits_camel_case() {
        assert_eq!(
            ActivationKey::Custom("ScrollLock".into()).label(),
            "Scroll Lock"
        );
        assert_eq!(ActivationKey::Custom("F10".into()).label(), "F10");
        assert_eq!(
            ActivationKey::Custom("ControlRight".into()).label(),
            "Control Right"
        );
    }

    #[test]
    fn missing_or_unknown_activation_defaults_to_right_ctrl() {
        // No activation key in the file.
        let cfg = parse_config(
            r#"[[match]]
            trigger = "cli"
            replace = "x""#,
        )
        .unwrap();
        assert_eq!(cfg.activation, ActivationKey::RightCtrl);

        // Unknown id falls back to the default rather than erroring.
        assert_eq!(ActivationKey::from_id("bogus"), ActivationKey::RightCtrl);
    }

    #[test]
    fn entries_to_config_collapses_to_map() {
        let entries = vec![
            entry("a", "first"),
            entry("a", "second"),
            entry("  ", "skip"),
        ];
        let cfg = entries_to_config(&entries);
        assert_eq!(cfg.map.len(), 1);
        assert_eq!(cfg.map.get("a"), Some(&"second".to_string()));
    }
}
