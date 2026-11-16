use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The Chinese that replies are written in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplyVariant {
    /// Traditional characters with Taiwanese wording.
    #[default]
    #[serde(rename = "zh-Hant-TW")]
    Taiwan,
    /// Simplified characters with mainland wording.
    #[serde(rename = "zh-Hans")]
    Simplified,
}

impl ReplyVariant {
    /// The variant's name in its own script, for the settings.
    pub fn label(self) -> &'static str {
        match self {
            ReplyVariant::Taiwan => "繁體中文（台灣）",
            ReplyVariant::Simplified => "简体中文",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Any OpenAI-compatible base URL, ending before /chat/completions.
    pub api_base: String,
    /// Left empty for local servers such as Ollama.
    pub api_key: String,
    pub model: String,
    pub reply_variant: ReplyVariant,
    pub overlay_x: f32,
    pub overlay_y: f32,
    pub overlay_width: f32,
    pub overlay_height: f32,
    pub overlay_opacity: f32,
    pub font_size: f32,
    pub max_lines: usize,
    pub show_original: bool,
    /// Name of the chat tab the overlay sits on and follows; empty to place it by hand.
    pub anchor_tab: String,
    /// Hide the locked overlay while another window is in front of WoW.
    pub hide_when_unfocused: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_base: "http://localhost:11434/v1".into(),
            api_key: String::new(),
            model: "hf.co/tencent/HY-MT1.5-7B-GGUF:Q4_K_M".into(),
            reply_variant: ReplyVariant::default(),
            overlay_x: 20.0,
            overlay_y: 420.0,
            overlay_width: 520.0,
            overlay_height: 260.0,
            overlay_opacity: 0.55,
            font_size: 15.0,
            max_lines: 100,
            show_original: false,
            anchor_tab: String::new(),
            hide_when_unfocused: true,
        }
    }
}

/// config.json lives next to the executable so the app stays portable. For a macOS app
/// bundle that means next to `Translate.app`, not inside it, so rebuilding or replacing
/// the app keeps the settings.
pub fn path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(config_dir(&exe)?.join("config.json")))
        .unwrap_or_else(|| PathBuf::from("config.json"))
}

fn config_dir(exe: &Path) -> Option<&Path> {
    let dir = exe.parent()?;
    let bundle = dir.parent().and_then(Path::parent).filter(|bundle| {
        dir.ends_with("Contents/MacOS") && bundle.extension().is_some_and(|extension| extension == "app")
    });
    bundle.and_then(Path::parent).or(Some(dir))
}

pub fn load() -> Config {
    match std::fs::read_to_string(path()) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => {
            let config = Config::default();
            let _ = save(&config);
            config
        }
    }
}

pub fn save(config: &Config) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(config).expect("config serializes");
    std::fs::write(path(), text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_files_reply_in_taiwanese() {
        let old: Config = serde_json::from_str(r#"{"api_base":"http://localhost:1234/v1","model":"m"}"#).unwrap();
        assert_eq!(old.reply_variant, ReplyVariant::Taiwan);
        assert_eq!(old.api_base, "http://localhost:1234/v1");
        let simplified: Config = serde_json::from_str(r#"{"reply_variant":"zh-Hans"}"#).unwrap();
        assert_eq!(simplified.reply_variant, ReplyVariant::Simplified);
        assert!(serde_json::to_string(&Config::default()).unwrap().contains(r#""reply_variant":"zh-Hant-TW""#));
    }

    #[test]
    fn config_sits_beside_the_program_or_its_app_bundle() {
        let dir = |exe: &str| config_dir(Path::new(exe)).unwrap().to_str().unwrap().to_owned();
        assert_eq!(dir("/Applications/wow-translate/translate-companion"), "/Applications/wow-translate");
        assert_eq!(
            dir("/Applications/wow-translate/Translate.app/Contents/MacOS/translate-companion"),
            "/Applications/wow-translate"
        );
        assert_eq!(dir("/opt/Contents/MacOS/translate-companion"), "/opt/Contents/MacOS");
    }
}
