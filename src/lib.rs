pub mod config;
pub mod editor;
pub mod expander;
pub mod hook;
pub mod matcher;
pub mod single_instance;

use std::fs;
use std::path::PathBuf;

/// Default config file: %APPDATA%\TextSwitch\triggers.toml
pub fn config_path() -> PathBuf {
    let mut dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    dir.push("TextSwitch");
    dir.push("triggers.toml");
    dir
}

fn legacy_config_path() -> PathBuf {
    let mut dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    dir.push("word-replacement-tool");
    dir.push("triggers.toml");
    dir
}

/// Ensure the config file exists; create it with a commented example if missing.
/// Returns the file's text contents.
pub fn ensure_and_read_config() -> String {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if !path.exists() {
        let legacy_path = legacy_config_path();
        if legacy_path.exists() {
            let _ = fs::copy(&legacy_path, &path);
        } else {
            let example = "# TextSwitch config\n\
                       # Type the trigger, then tap Right Ctrl to expand.\n\
                       # Triggers must be lowercase letters/digits (a-z, 0-9).\n\n\
                       [[match]]\n\
                       trigger = \"cli\"\n\
                       replace = \"\"\"\n\
                       Act as a senior CLI engineer. Output exact commands,\n\
                       explain each flag briefly, and warn about destructive ops.\n\
                       \"\"\"\n";
            let _ = fs::write(&path, example);
        }
    }
    fs::read_to_string(&path).unwrap_or_default()
}
