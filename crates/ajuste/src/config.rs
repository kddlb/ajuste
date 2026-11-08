//! `~/.config/ajuste/config.toml`.

use std::path::PathBuf;

use scape_proto::EqPoint;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Notify when battery drops to this percentage.
    pub low_battery: u8,
    /// Second, more urgent notification.
    pub critical_battery: u8,
    /// Start with only the tray icon.
    pub start_hidden: bool,
    /// Headset EQ presets saved from the editor.
    pub presets: Vec<UserPreset>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserPreset {
    pub name: String,
    pub uid: String,
    pub points: Vec<EqPoint>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            low_battery: 20,
            critical_battery: 10,
            start_hidden: false,
            presets: Vec::new(),
        }
    }
}

fn path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "ajuste").map(|d| d.config_dir().join("config.toml"))
}

impl Config {
    pub fn load() -> Config {
        let Some(p) = path() else {
            return Config::default();
        };
        match std::fs::read_to_string(&p) {
            Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
                log::warn!("ignoring invalid {}: {e}", p.display());
                Config::default()
            }),
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        let res = p
            .parent()
            .map(std::fs::create_dir_all)
            .transpose()
            .map_err(anyhow::Error::from)
            .and_then(|_| Ok(toml::to_string_pretty(self)?))
            .and_then(|s| Ok(std::fs::write(&p, s)?));
        if let Err(e) = res {
            log::warn!("could not save {}: {e}", p.display());
        }
    }
}
