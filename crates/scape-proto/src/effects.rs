//! Lighting effect rendering and upload framing, ported from Adjust Pro's effect classes.
//!
//! The headset plays a pre-rendered loop: 12 LEDs × RGB per frame at 20 fps. The app renders
//! the frames client-side and uploads `header ++ frames` into a lighting slot:
//!
//! ```text
//! A4 01 <slot> <sum8> <lenLo> <lenHi>     SET_EFFECT   (sum8 = byte sum of the data, mod 256)
//! A4 02 <n≤60> <n data bytes, 0-padded>   SEND_EFFECT  × ceil(len / 60)
//! A4 03                                   COMMIT_EFFECT
//! A4 04 <slot>                            SELECT_EFFECT_SLOT
//! ```
//!
//! Data = `A5 5A <frames> FF <fps> <seconds> <metadata, zero-padded to 42>` ++ frames, where
//! metadata = `[preset_id, mode, speed, colours…, extras…, brightness]`.
//!
//! Every renderer here is checked byte-for-byte against uploads captured from Adjust Pro
//! (`tests/effects.rs`). The arithmetic deliberately mirrors the JavaScript (f64 maths, JS
//! `Math.round`, tinycolor's 0–255 round trip) so rounding matches exactly.

use crate::command::Command;
use crate::lighting::METADATA_LEN;

pub const LEDS: usize = 12;
pub const FPS: u8 = 20;
/// Bytes per `A4 02` chunk (Adjust Pro `MAX_EFFECT_PACKAGE_SIZE`).
pub const CHUNK: usize = 60;
/// The slot Adjust Pro writes the headset's effect into.
pub const SLOT: u8 = 1;

pub type Rgb = [u8; 3];

/// JavaScript `Math.round`: round half towards +∞.
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// One channel through tinycolor: `bound01(v, 255) * 255`, small values rounded, then
/// `toRgb()` rounds.
fn tc_channel(v: f64) -> u8 {
    let n = v.clamp(0.0, 255.0);
    let mut c = if (n - 255.0).abs() < 0.000_001 {
        255.0
    } else {
        (n % 255.0) / 255.0 * 255.0
    };
    if c < 1.0 {
        c = js_round(c);
    }
    js_round(c) as u8
}

/// `tinycolor.mix(a, b, amount)`, amount in percent.
fn mix(a: Rgb, b: Rgb, amount: f64) -> Rgb {
    // `n = 0 === n ? 0 : n || 50` — NaN would become 50; never happens with our inputs.
    let p = amount / 100.0;
    std::array::from_fn(|i| {
        let (x, y) = (f64::from(a[i]), f64::from(b[i]));
        tc_channel((y - x) * p + x)
    })
}

/// `changeColorViaBrightness`.
fn dim(c: Rgb, brightness: u8) -> Rgb {
    c.map(|v| js_round(f64::from(v) * (f64::from(brightness) / 100.0)) as u8)
}

/// Loop length in seconds for a speed (10–100): `11 − ⌊speed / 10⌋`.
pub fn duration_secs(speed: u8) -> u8 {
    11 - speed.clamp(10, 100) / 10
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    /// Solid colour (speed is fixed at 100 → 1 s loop).
    Still(Rgb),
    /// Six colours spread over the ring, rotating through six transitions.
    Shift([Rgb; 6]),
    /// Two colours fading around the ring.
    TwoColorFade(Rgb, Rgb),
}

impl Pattern {
    pub fn mode(&self) -> u8 {
        match self {
            Pattern::Shift(_) => 0,
            Pattern::Still(_) => 1,
            Pattern::TwoColorFade(..) => 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectSpec {
    /// Adjust Pro preset id stored in the metadata (only used for display / read-back).
    pub preset_id: u8,
    pub pattern: Pattern,
    /// 10–100; ignored for `Still`.
    pub speed: u8,
    /// 0–100.
    pub brightness: u8,
}

impl EffectSpec {
    fn effective_speed(&self) -> u8 {
        match self.pattern {
            Pattern::Still(_) => 100,
            _ => self.speed,
        }
    }

    pub fn duration_secs(&self) -> u8 {
        duration_secs(self.effective_speed())
    }

    /// `[mode, speed, colours…, extras…, brightness]` (the app's `getMetadata()`).
    fn metadata(&self) -> Vec<u8> {
        let speed = self.effective_speed();
        let mut m = vec![self.pattern.mode(), speed];
        match &self.pattern {
            Pattern::Still(c) => m.extend(c),
            Pattern::Shift(cs) => cs.iter().for_each(|c| m.extend(c)),
            Pattern::TwoColorFade(a, b) => {
                m.extend(a);
                m.extend(b);
                m.push(speed);
            }
        }
        m.push(self.brightness);
        m
    }

    /// Rendered frames, `LEDS * 3` bytes each.
    pub fn render(&self) -> Vec<u8> {
        let secs = usize::from(self.duration_secs());
        let total = secs * usize::from(FPS);
        match &self.pattern {
            Pattern::Still(c) => {
                let c = dim(*c, self.brightness);
                (0..total * LEDS * 3).map(|i| c[i % 3]).collect()
            }
            Pattern::Shift(cs) => render_shift(&cs.map(|c| dim(c, self.brightness)), total),
            Pattern::TwoColorFade(a, b) => {
                render_two_color_fade(dim(*a, self.brightness), dim(*b, self.brightness), total)
            }
        }
    }

    /// `header ++ frames`, exactly as uploaded.
    pub fn upload_data(&self) -> Vec<u8> {
        let frames = self.render();
        let mut d = vec![
            0xA5,
            0x5A,
            (frames.len() / (LEDS * 3)) as u8,
            0xFF,
            FPS,
            self.duration_secs(),
        ];
        let mut meta = vec![self.preset_id];
        meta.extend(self.metadata());
        meta.resize(METADATA_LEN, 0);
        d.extend(meta);
        d.extend(frames);
        d
    }

    /// The full upload sequence into [`SLOT`] (SET, SEND × n, COMMIT, SELECT), as Adjust Pro
    /// sends it for the headset.
    pub fn upload_commands(&self) -> Vec<Command> {
        let data = self.upload_data();
        let sum = data.iter().fold(0u8, |a, &b| a.wrapping_add(b));
        let mut cmds = vec![Command::SetEffect {
            slot: SLOT,
            checksum: sum,
            len: data.len() as u16,
        }];
        cmds.extend(
            data.chunks(CHUNK)
                .map(|c| Command::SendEffect { data: c.to_vec() }),
        );
        cmds.push(Command::CommitEffect);
        cmds.push(Command::SetLights(true));
        cmds
    }
}

/// Shift's colour order after each transition (Adjust Pro `a = [4,5,3,0,1,2]`).
const SHIFT_PERM: [usize; 6] = [4, 5, 3, 0, 1, 2];

fn shift_transition(t: &[Rgb; 6]) -> [Rgb; 6] {
    std::array::from_fn(|n| t[SHIFT_PERM[n]])
}

/// `generateLedColors`: spread the six colours over the ring, blending between neighbours.
fn shift_led_colors(e: &[Rgb; 6]) -> [Rgb; LEDS] {
    let n = e.len() as f64 / LEDS as f64;
    let mut r = 0usize;
    std::array::from_fn(|i| {
        let mut l = i as f64 * n - r as f64;
        if l > 1.0 {
            r += 1;
            l = i as f64 * n - r as f64;
        }
        let a = e[r];
        let s = if r + 1 < e.len() { e[r + 1] } else { e[0] };
        mix(a, s, 100.0 * l)
    })
}

fn render_shift(colors: &[Rgb; 6], _total: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut t = *colors;
    // `Math.round(durationSeconds * frameRate / a.length)` frames per transition.
    let per = js_round(_total as f64 / SHIFT_PERM.len() as f64) as usize;
    for _ in 0..SHIFT_PERM.len() {
        let from = shift_led_colors(&t);
        let to = shift_led_colors(&shift_transition(&t));
        for f in 0..per {
            let l = f as f64 / per as f64;
            for led in 0..LEDS {
                out.extend(mix(from[led], to[led], 100.0 * l));
            }
        }
        t = shift_transition(&t);
    }
    out
}

fn render_two_color_fade(main: Rgb, second: Rgb, total: usize) -> Vec<u8> {
    let leds = LEDS as f64;
    // Base gradient, doubled so the window can slide past the end.
    let mut base = Vec::with_capacity(LEDS * 6);
    for l in 0..LEDS {
        base.extend(mix(
            main,
            second,
            100.0 * (l as f64 / (leds - 1.0) * std::f64::consts::PI).sin(),
        ));
    }
    let mut out = base.clone();
    base.extend_from_within(..);
    let step = 2.0 * std::f64::consts::PI / total as f64;
    for o in 1..total {
        let x = step * o as f64;
        let r = if x < std::f64::consts::PI / 2.0 {
            (leds / 4.0 * x.sin()).abs()
        } else if x < std::f64::consts::PI {
            (leds / 4.0 * (2.0 - x.sin())).abs()
        } else if x < 3.0 * std::f64::consts::PI / 2.0 {
            leds / 2.0 + (leds / 4.0 * x.sin()).abs()
        } else {
            leds / 2.0 + (leds / 4.0 * (2.0 + x.sin())).abs()
        };
        let l = r.floor() as usize;
        let a = 100.0 * r % 100.0;
        for e in (0..LEDS * 3).step_by(3) {
            let (i, j) = (e + 3 * l, e + 3 * (l + 1));
            for k in 0..3 {
                // Past the end, JS reads `undefined` → NaN, which a Uint8Array stores as 0.
                match (base.get(i + k), base.get(j + k)) {
                    (Some(&x), Some(&y)) => {
                        let v = (f64::from(x) * (100.0 - a) + f64::from(y) * a) / 100.0;
                        out.push(js_round(v) as u8);
                    }
                    _ => out.push(0),
                }
            }
        }
    }
    out
}

/// A built-in Adjust Pro preset that uses one of the verified renderers.
#[derive(Debug, Clone, Copy)]
pub struct Preset {
    pub name: &'static str,
    pub id: u8,
    pub speed: u8,
    kind: PresetKind,
}

#[derive(Debug, Clone, Copy)]
enum PresetKind {
    Still(&'static str),
    Shift([&'static str; 6]),
    TwoColorFade(&'static str, &'static str),
}

const fn rgb(hex: &str) -> Rgb {
    let b = hex.as_bytes();
    const fn nib(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => c - b'A' + 10,
        }
    }
    [
        nib(b[1]) << 4 | nib(b[2]),
        nib(b[3]) << 4 | nib(b[4]),
        nib(b[5]) << 4 | nib(b[6]),
    ]
}

impl Preset {
    pub fn pattern(&self) -> Pattern {
        match self.kind {
            PresetKind::Still(c) => Pattern::Still(rgb(c)),
            PresetKind::Shift(cs) => Pattern::Shift(cs.map(rgb)),
            PresetKind::TwoColorFade(a, b) => Pattern::TwoColorFade(rgb(a), rgb(b)),
        }
    }

    pub fn spec(&self, brightness: u8) -> EffectSpec {
        EffectSpec {
            preset_id: self.id,
            pattern: self.pattern(),
            speed: self.speed,
            brightness,
        }
    }

    pub fn mode_name(&self) -> &'static str {
        crate::lighting::MODES[usize::from(self.pattern().mode())]
    }
}

const fn p(name: &'static str, id: u8, speed: u8, kind: PresetKind) -> Preset {
    Preset {
        name,
        id,
        speed,
        kind,
    }
}

/// Adjust Pro's "Fractal themes" for these effect types (palettes, ids and speeds from the app).
pub const PRESETS: &[Preset] = &[
    p(
        "Northern lights",
        0,
        40,
        PresetKind::Shift([
            "#0074C5", "#6E00FA", "#00FAEB", "#C940BD", "#95FDE4", "#00F03C",
        ]),
    ),
    p(
        "Summer sky",
        1,
        35,
        PresetKind::Shift([
            "#FFF4A8", "#FA9A00", "#FA1500", "#B040C9", "#959EFD", "#FFFFFF",
        ]),
    ),
    p(
        "Sunset",
        2,
        75,
        PresetKind::Shift([
            "#C60000", "#C60000", "#E55F00", "#E55F00", "#C10016", "#F400A3",
        ]),
    ),
    p(
        "Mystic night",
        3,
        60,
        PresetKind::Shift([
            "#560055", "#9A00FA", "#FA00CC", "#431447", "#AC03D2", "#3C0042",
        ]),
    ),
    p(
        "Emerald lake",
        4,
        75,
        PresetKind::Shift([
            "#00600E", "#00560A", "#007A28", "#54CE6C", "#027D1E", "#004C10",
        ]),
    ),
    p(
        "Radiant dawn",
        5,
        45,
        PresetKind::Shift([
            "#FFA25B", "#FF9123", "#FF9122", "#FFC35B", "#FFA023", "#FFC35B",
        ]),
    ),
    p("Fractal blue", 6, 100, PresetKind::Still("#0074C5")),
    p("Bonfire", 16, 100, PresetKind::Still("#C11900")),
    p("Ocean", 17, 100, PresetKind::Still("#ACCBE6")),
    p("Pine", 18, 100, PresetKind::Still("#005711")),
    p(
        "Pink sapphire",
        14,
        75,
        PresetKind::TwoColorFade("#FF0059", "#000000"),
    ),
    p(
        "Lunar mist",
        15,
        75,
        PresetKind::TwoColorFade("#9B9B9B", "#000000"),
    ),
    p(
        "Nautical",
        26,
        92,
        PresetKind::TwoColorFade("#1C0880", "#FFFFFF"),
    ),
    p(
        "Shoreline",
        27,
        92,
        PresetKind::TwoColorFade("#FF630F", "#C63800"),
    ),
    p(
        "Endless woodland",
        28,
        100,
        PresetKind::TwoColorFade("#42FF65", "#007014"),
    ),
    p(
        "Azure horizon",
        29,
        100,
        PresetKind::TwoColorFade("#EE7C3E", "#DBDBDB"),
    ),
    p(
        "Flowing spark",
        30,
        100,
        PresetKind::TwoColorFade("#E50000", "#00B746"),
    ),
];

pub fn preset(name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_ids_match_names() {
        for p in PRESETS {
            assert_eq!(
                crate::lighting::preset_name(p.id).map(str::to_lowercase),
                Some(p.name.to_lowercase()),
                "{p:?}"
            );
            let d = p.spec(100).upload_data();
            assert_eq!(d.len(), 48 + usize::from(d[2]) * LEDS * 3, "{}", p.name);
        }
    }

    #[test]
    fn rgb_const() {
        assert_eq!(rgb("#0074C5"), [0x00, 0x74, 0xc5]);
        assert_eq!(rgb("#ffffff"), [255, 255, 255]);
    }
}
