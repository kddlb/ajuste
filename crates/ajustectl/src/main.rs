use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use scape_hid::{Direction, Scape};
use scape_proto::eq::{self, CommitMode, EqProfile};
use scape_proto::{Command, Target, hex};

#[derive(Parser)]
#[command(
    name = "ajustectl",
    version,
    about = "Control the Fractal Scape headset over hidraw"
)]
struct Cli {
    /// Print every raw report sent/received.
    #[arg(long, global = true)]
    hex: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show battery, mic, EQ slot, sidetone and ENC state.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Firmware versions, serials and BT address.
    Info,
    /// Print status every 1.5 s until interrupted.
    Watch,
    /// Environmental noise cancellation on the mic.
    Enc { state: OnOff },
    /// Headset LEDs on (stored effect) or off.
    Lights { state: OnOff },
    /// Upload a lighting preset into slot 1 (see `lights-presets`).
    LightsSet {
        /// Preset name, e.g. "Sunset" or "Fractal blue".
        preset: String,
        /// LED brightness 0–100.
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u8).range(0..=100))]
        brightness: u8,
        /// Override the preset's speed (10–100; ignored for still colours).
        #[arg(long, value_parser = clap::value_parser!(u8).range(10..=100))]
        speed: Option<u8>,
        #[arg(long)]
        dry_run: bool,
    },
    /// List lighting presets ajuste can upload.
    LightsPresets,
    /// Dump stored lighting effect metadata (read-only).
    LightsShow {
        #[arg(default_value_t = 1)]
        slot: u8,
    },
    /// Sidetone (hear your own mic).
    Sidetone {
        state: OnOff,
        /// Level 0–100 % (snapped to 10 % steps).
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
        level: Option<u8>,
    },
    /// Headset EQ.
    #[command(subcommand)]
    Eq(EqCmd),
}

#[derive(Subcommand)]
enum EqCmd {
    /// List built-in presets.
    Presets,
    /// Read one slot (or all three) from the headset.
    Show { slot: Option<u8> },
    /// Make a slot active.
    Select {
        slot: u8,
        #[arg(long)]
        dry_run: bool,
    },
    /// Write a preset or TOML profile into a slot (live preview unless --save).
    Apply {
        slot: u8,
        #[command(flatten)]
        src: ProfileSource,
        /// Persist to the slot instead of a live (non-persistent) update.
        #[arg(long)]
        save: bool,
        /// Preset ID stored with the slot (defaults to the preset's, or one derived from the file name).
        #[arg(long)]
        uid: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(clap::Args)]
#[group(required = true, multiple = false)]
struct ProfileSource {
    /// Built-in preset name.
    #[arg(long)]
    preset: Option<String>,
    /// TOML file with `[[points]] filter = "peak", freq = 1000, gain = 3.0, q = 1.0`.
    #[arg(long)]
    file: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum OnOff {
    On,
    Off,
}

impl OnOff {
    fn on(self) -> bool {
        matches!(self, OnOff::On)
    }
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let cli = Cli::parse();

    // Dry runs never touch the device.
    if let Some(cmds) = dry_run_commands(&cli.cmd)? {
        for c in cmds {
            println!("{}", hex(&c.encode()?));
        }
        return Ok(());
    }

    let mut dev = Scape::open()?;
    if cli.hex {
        dev.set_trace(Some(Box::new(|dir, bytes| {
            let arrow = if dir == Direction::Out { ">>" } else { "<<" };
            eprintln!("{arrow} {}", hex(bytes));
        })));
    }

    match cli.cmd {
        Cmd::Status { json } => print_status(&dev, json)?,
        Cmd::Info => {
            println!("device: {:?} at {}", dev.kind(), dev.path());
            if dev.kind() == scape_hid::DeviceKind::Dongle {
                println!("dongle:  {:?}", dev.info(Target::Dongle)?);
            }
            if dev.headset_connected()? {
                println!("headset: {:?}", dev.info(Target::Headset)?);
            } else {
                println!("headset: not connected");
            }
        }
        Cmd::Watch => loop {
            print_status(&dev, false)?;
            println!();
            std::thread::sleep(scape_hid::POLL_INTERVAL);
        },
        Cmd::Enc { state } => {
            dev.set_enc(state.on())?;
            confirm(&dev, |s| s.enc_on == state.on(), "ENC")?;
        }
        Cmd::Lights { state } => {
            dev.set_lights(state.on())?;
            confirm(&dev, |s| (s.lighting_slot != 0) == state.on(), "lights")?;
        }
        Cmd::LightsSet {
            preset,
            brightness,
            speed,
            ..
        } => {
            let spec = lights_spec(&preset, brightness, speed)?;
            let n = spec.upload_commands().len();
            dev.upload_effect(&spec)?;
            println!("uploaded {preset} ({n} reports); reading back…");
            match scape_proto::lighting::Effect::parse(&dev.read_effect_raw(1)?) {
                Some(e) => println!("slot 1 now: {} ({})", e.label(), e.mode_name()),
                None => bail!("slot 1 has no readable effect after upload"),
            }
        }
        Cmd::LightsPresets => {
            use scape_proto::effects::{PRESETS, Pattern};
            for p in PRESETS {
                let cols: Vec<String> = match p.pattern() {
                    Pattern::Still(c) => vec![c],
                    Pattern::Shift(cs) => cs.to_vec(),
                    Pattern::TwoColorFade(a, b) => vec![a, b],
                }
                .iter()
                .map(|c| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]))
                .collect();
                println!(
                    "{:<17} {:<16} speed {:>3}  {}",
                    p.name,
                    p.mode_name(),
                    p.speed,
                    cols.join(" ")
                );
            }
        }
        Cmd::LightsShow { slot } => {
            let d = dev.read_effect_raw(slot)?;
            match scape_proto::lighting::Effect::parse(&d) {
                Some(e) => {
                    println!(
                        "slot {slot}: {} ({}, speed {}, brightness {})",
                        e.label(),
                        e.mode_name(),
                        e.speed,
                        e.brightness
                    );
                    let cols: Vec<String> = e
                        .colors
                        .iter()
                        .map(|c| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]))
                        .collect();
                    println!("  colours: {}", cols.join(" "));
                    println!(
                        "  {} frames @ {} fps, {} s loop",
                        e.frames, e.fps, e.seconds
                    );
                }
                None => println!(
                    "slot {slot}: no stored effect ({})",
                    hex(&d[..12.min(d.len())])
                ),
            }
        }
        Cmd::Sidetone { state, level } => {
            dev.set_sidetone(state.on(), level)?;
            confirm(&dev, |s| s.sidetone_on == state.on(), "sidetone")?;
        }
        Cmd::Eq(EqCmd::Presets) => {
            for p in eq::PRESETS {
                println!("{} ({})", p.name, p.uid);
                print_profile(&p.profile());
            }
        }
        Cmd::Eq(EqCmd::Show { slot }) => {
            let slots: Vec<u8> = slot
                .map(|s| vec![s])
                .unwrap_or_else(|| (1..=eq::SLOTS).collect());
            for s in slots {
                let e = dev.read_eq(s)?;
                let name = e
                    .preset_uid
                    .as_deref()
                    .and_then(eq::preset_by_uid)
                    .map(|p| p.name);
                println!(
                    "slot {s}: preset {}{}",
                    name.or(e.preset_uid.as_deref()).unwrap_or("(none)"),
                    if e.factory_copy {
                        " [factory copy]"
                    } else {
                        ""
                    }
                );
                print_profile(&e.profile);
            }
        }
        Cmd::Eq(EqCmd::Select { slot, .. }) => {
            dev.select_eq(slot)?;
            std::thread::sleep(Duration::from_millis(300));
            confirm(&dev, |s| s.eq_slot == slot, "EQ slot")?;
        }
        Cmd::Eq(EqCmd::Apply {
            slot,
            src,
            save,
            uid: u,
            ..
        }) => {
            let (profile, uid) = load_profile(&src)?;
            let uid = u.unwrap_or(uid);
            let mode = if save {
                CommitMode::Save
            } else {
                CommitMode::Live
            };
            dev.apply_eq(slot, &profile, mode, &uid)?;
            println!(
                "applied to slot {slot} ({})",
                if save { "saved" } else { "live, not persisted" }
            );
        }
    }
    Ok(())
}

fn dry_run_commands(cmd: &Cmd) -> Result<Option<Vec<Command>>> {
    Ok(match cmd {
        Cmd::Eq(EqCmd::Select {
            slot,
            dry_run: true,
        }) => Some(vec![Command::SelectEq { slot: *slot }]),
        Cmd::LightsSet {
            preset,
            brightness,
            speed,
            dry_run: true,
        } => Some(lights_spec(preset, *brightness, *speed)?.upload_commands()),
        Cmd::Eq(EqCmd::Apply {
            slot,
            src,
            save,
            uid: u,
            dry_run: true,
        }) => {
            let (p, uid) = load_profile(src)?;
            let uid = u.clone().unwrap_or(uid);
            let mode = if *save {
                CommitMode::Save
            } else {
                CommitMode::Live
            };
            Some(p.apply_commands(*slot, mode, &uid)?)
        }
        _ => None,
    })
}

fn lights_spec(
    name: &str,
    brightness: u8,
    speed: Option<u8>,
) -> Result<scape_proto::effects::EffectSpec> {
    let p = scape_proto::effects::preset(name).with_context(|| {
        format!("unknown lighting preset {name:?}; see `ajustectl lights-presets`")
    })?;
    let mut spec = p.spec(brightness);
    if let Some(s) = speed {
        spec.speed = s;
    }
    Ok(spec)
}

/// Load a headset EQ profile; returns the preset UID to store with it.
fn load_profile(src: &ProfileSource) -> Result<(EqProfile, String)> {
    if let Some(name) = &src.preset {
        let p = eq::preset(name)
            .with_context(|| format!("unknown preset {name:?}; see `ajustectl eq presets`"))?;
        return Ok((p.profile(), p.uid.to_string()));
    }
    let path = src.file.as_ref().unwrap();
    let p = read_toml(path)?;
    p.validate()?;
    // Stable per-file UID so the slot shows a custom preset.
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok((
        p,
        truncate_bytes(format!("custom-{stem}"), eq::MAX_PRESET_UID_LEN),
    ))
}

/// Cut `s` to at most `max` bytes without splitting a UTF-8 character.
fn truncate_bytes(mut s: String, max: usize) -> String {
    if s.len() > max {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
    s
}

fn read_toml(path: &PathBuf) -> Result<EqProfile> {
    let s = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&s).with_context(|| format!("parsing {}", path.display()))
}

fn print_profile(p: &EqProfile) {
    if p.points.is_empty() {
        println!("  (flat)");
    }
    for pt in &p.points {
        println!(
            "  {:<10} {:>5} Hz  {:+5.1} dB  Q {:.3}",
            pt.filter.label(),
            pt.freq,
            pt.gain,
            pt.q
        );
    }
}

fn print_status(dev: &Scape, json: bool) -> Result<()> {
    let st = dev.status()?;
    if json {
        println!(
            "{}",
            serde_json::json!({ "device": format!("{:?}", st.kind), "headset": st.headset })
        );
        return Ok(());
    }
    println!("device:    {:?}", st.kind);
    let Some(h) = st.headset else {
        println!("headset:   not connected");
        return Ok(());
    };
    println!("battery:   {}%", h.battery_pct);
    println!(
        "mic:       {}{}",
        if h.boom_mic_attached {
            "boom attached"
        } else {
            "boom detached"
        },
        if h.muted { ", MUTED" } else { "" }
    );
    println!("eq slot:   {}", h.eq_slot);
    println!(
        "sidetone:  {} ({}%, raw {})",
        if h.sidetone_on { "on" } else { "off" },
        h.sidetone_pct(),
        h.sidetone_raw
    );
    println!("enc:       {}", if h.enc_on { "on" } else { "off" });
    println!(
        "lights:    {}",
        if h.lighting_slot != 0 { "on" } else { "off" }
    );
    println!("battery V: {} mV (unconfirmed)", h.battery_mv);
    println!(
        "raw:       chat={} game={} vol_state={} bt_mode={} hall={} bt_conn={} power={}",
        h.chat_volume,
        h.game_volume,
        h.volume_state,
        h.bt_mode,
        h.hall_sensor,
        h.bt_connection,
        h.power_state
    );
    Ok(())
}

/// Re-read state after a setter and report whether it took effect. Over USB the state can
/// trail the command by ~70 ms (ENC), so poll for up to ~0.5 s before giving up.
fn confirm(dev: &Scape, ok: impl Fn(&scape_proto::HeadsetState) -> bool, what: &str) -> Result<()> {
    let mut st = dev.status()?.headset.context("headset disconnected")?;
    for _ in 0..10 {
        if ok(&st) {
            println!("{what}: confirmed by device state");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
        st = dev.status()?.headset.context("headset disconnected")?;
    }
    if ok(&st) {
        println!("{what}: confirmed by device state");
        Ok(())
    } else {
        bail!("{what}: command sent but device state did not change ({st:?})")
    }
}

#[cfg(test)]
mod tests {
    use super::truncate_bytes;

    #[test]
    fn truncates_on_char_boundary() {
        let s = format!("{}ñ", "a".repeat(53)); // 55 bytes
        let t = truncate_bytes(s, 54);
        assert_eq!(t, "a".repeat(53));
        assert_eq!(truncate_bytes("short".into(), 54), "short");
    }
}
