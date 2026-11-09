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
    /// Which desktop notifications to show.
    pub notifications: Notifications,
}

/// Per-category notification switches (`[notifications]` in config.toml). The "headset"
/// ones fire only for changes made on the headset itself, not from ajuste.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Notifications {
    pub connection: bool,
    pub battery: bool,
    pub mute: bool,
    pub enc: bool,
    pub eq_slot: bool,
    pub lights: bool,
    pub boom: bool,
}

impl Default for Notifications {
    fn default() -> Self {
        Notifications {
            connection: true,
            battery: true,
            mute: true,
            enc: true,
            eq_slot: true,
            lights: true,
            boom: true,
        }
    }
}

impl Notifications {
    pub fn enabled(&self, kind: crate::notify::Kind) -> bool {
        use crate::notify::Kind::*;
        match kind {
            Connection => self.connection,
            Battery => self.battery,
            Mute => self.mute,
            Enc => self.enc,
            EqSlot => self.eq_slot,
            Lights => self.lights,
            Boom => self.boom,
        }
    }

    pub fn set(&mut self, kind: crate::notify::Kind, on: bool) {
        use crate::notify::Kind::*;
        *match kind {
            Connection => &mut self.connection,
            Battery => &mut self.battery,
            Mute => &mut self.mute,
            Enc => &mut self.enc,
            EqSlot => &mut self.eq_slot,
            Lights => &mut self.lights,
            Boom => &mut self.boom,
        } = on;
    }
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
            notifications: Notifications::default(),
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

    /// Apply `f` and persist it without clobbering the file: the change is applied on top of
    /// what is on disk *now*, so hand edits made while ajuste runs survive (and are picked up).
    /// If the file exists but doesn't parse, it is left untouched and an error is returned;
    /// the change still applies in memory for this session.
    pub fn update(&mut self, f: impl FnOnce(&mut Config)) -> Result<(), String> {
        let Some(p) = path() else {
            f(self);
            return Ok(());
        };
        let mut base = match std::fs::read_to_string(&p) {
            Ok(s) => match toml::from_str::<Config>(&s) {
                Ok(disk) => disk,
                Err(e) => {
                    f(self);
                    return Err(format!(
                        "{} has errors, so ajuste won't overwrite it (fix or delete it): {e}",
                        p.display()
                    ));
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => self.clone(),
            Err(e) => {
                f(self);
                return Err(format!("could not read {}: {e}", p.display()));
            }
        };
        f(&mut base);
        *self = base;
        let res = p
            .parent()
            .map(std::fs::create_dir_all)
            .transpose()
            .map_err(anyhow::Error::from)
            .and_then(|_| Ok(toml::to_string_pretty(self)?))
            .and_then(|s| {
                // Write-then-rename so a crash mid-write can't truncate the file.
                let tmp = p.with_extension("toml.tmp");
                std::fs::write(&tmp, s)?;
                std::fs::rename(&tmp, &p)?;
                Ok(())
            });
        res.map_err(|e| format!("could not save {}: {e}", p.display()))
    }
}
