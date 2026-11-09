//! Headset parametric EQ (device type 0xA7): profile model, wire layout, built-in presets.

use crate::biquad::{self, Coefs, SampleRate};
use crate::command::Command;
use crate::{PAYLOAD_LEN, ProtoError, Result};

/// Longest preset UID (in bytes) that fits the COMMIT_EQ payload.
pub const MAX_PRESET_UID_LEN: usize = PAYLOAD_LEN - 9;

pub const MAX_POINTS: usize = 5;
pub const SLOTS: u8 = 3;
pub const GAIN_RANGE: (f32, f32) = (-10.0, 10.0);
pub const FREQ_RANGE: (u16, u16) = (20, 20_000);
pub const Q_RANGE: (f32, f32) = (0.5, 10.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum FilterType {
    Peak = 0,
    LowShelf = 1,
    HighShelf = 2,
    LowPass = 3,
    HighPass = 4,
    Notch = 5,
    BandPass = 6,
}

impl FilterType {
    /// The three types the Adjust Pro editor offers.
    pub const EDITOR: [FilterType; 3] = [
        FilterType::Peak,
        FilterType::LowShelf,
        FilterType::HighShelf,
    ];

    pub fn from_u8(v: u8) -> Option<Self> {
        use FilterType::*;
        Some(match v {
            0 => Peak,
            1 => LowShelf,
            2 => HighShelf,
            3 => LowPass,
            4 => HighPass,
            5 => Notch,
            6 => BandPass,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            FilterType::Peak => "Peak",
            FilterType::LowShelf => "Low shelf",
            FilterType::HighShelf => "High shelf",
            FilterType::LowPass => "Low pass",
            FilterType::HighPass => "High pass",
            FilterType::Notch => "Notch",
            FilterType::BandPass => "Band pass",
        }
    }
}

/// Commit mode for COMMIT_EQ (0xA7 0x03).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitMode {
    /// Apply now without persisting (used while dragging in the editor).
    Live = 1,
    /// Persist to the slot.
    Save = 2,
    /// Erase the slot.
    Erase = 3,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EqPoint {
    pub filter: FilterType,
    pub freq: u16,
    pub gain: f32,
    pub q: f32,
}

impl EqPoint {
    pub fn new(filter: FilterType, freq: u16, gain: f32, q: f32) -> Self {
        EqPoint {
            filter,
            freq,
            gain,
            q,
        }
    }

    /// Clamp to the limits the Adjust Pro editor enforces.
    pub fn clamped(self) -> Self {
        EqPoint {
            filter: self.filter,
            freq: self.freq.clamp(FREQ_RANGE.0, FREQ_RANGE.1),
            gain: self.gain.clamp(GAIN_RANGE.0, GAIN_RANGE.1),
            q: self.q.clamp(Q_RANGE.0, Q_RANGE.1),
        }
    }

    fn write(&self, out: &mut Vec<u8>, enabled: bool) {
        out.push(enabled as u8);
        out.push(self.filter as u8);
        out.extend_from_slice(&self.freq.to_le_bytes());
        out.extend_from_slice(&self.gain.to_le_bytes());
        out.extend_from_slice(&self.q.to_le_bytes());
    }
}

/// Up to [`MAX_POINTS`] filters. An empty profile is flat.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EqProfile {
    pub points: Vec<EqPoint>,
}

impl EqProfile {
    pub fn validate(&self) -> Result<()> {
        if self.points.len() > MAX_POINTS {
            return Err(ProtoError::OutOfRange("more than 5 EQ points"));
        }
        for p in &self.points {
            if *p != p.clamped() {
                return Err(ProtoError::OutOfRange(
                    "EQ point outside freq/gain/Q limits",
                ));
            }
        }
        Ok(())
    }

    /// Payload bytes after `A7 01`: header + 5 × 12-byte points (61 bytes).
    pub fn settings_bytes(&self, profile_id: u8) -> Vec<u8> {
        let mut out = Vec::with_capacity(61);
        out.push(profile_id << 4 | SampleRate::MASK_ALL);
        let unused = EqPoint::new(FilterType::Peak, 0x20, 0.0, 0.5);
        let mut written = self.points.len();
        for p in &self.points {
            p.write(&mut out, true);
        }
        if self.points.is_empty() {
            // Flat profile: one enabled placeholder at 22 kHz (0x55F0).
            EqPoint::new(FilterType::Peak, 22_000, 0.0, 0.5).write(&mut out, true);
            written = 1;
        }
        for _ in written..MAX_POINTS {
            unused.write(&mut out, false);
        }
        out
    }

    /// Parse a GET_EQ_SETTINGS response payload (`A7 04 <hdr> <points…>`).
    /// Returns `(profile_id, sample_rate_mask, profile)`. Profile id 0 means "empty slot".
    pub fn parse_settings(data: &[u8]) -> Result<(u8, u8, EqProfile)> {
        if data.len() < 3 + 12 * MAX_POINTS {
            return Err(ProtoError::Short(data.len()));
        }
        let hdr = data[2];
        let mut points = Vec::new();
        for i in 0..MAX_POINTS {
            let p = &data[3 + 12 * i..3 + 12 * (i + 1)];
            let enabled = p[0] != 0;
            let freq = u16::from_le_bytes([p[2], p[3]]);
            if !enabled || freq > FREQ_RANGE.1 {
                continue;
            }
            let Some(filter) = FilterType::from_u8(p[1]) else {
                continue;
            };
            let gain = f32::from_le_bytes(p[4..8].try_into().unwrap());
            let q = f32::from_le_bytes(p[8..12].try_into().unwrap());
            points.push(EqPoint {
                filter,
                freq,
                gain,
                q,
            });
        }
        Ok((hdr >> 4, hdr & 0x0F, EqProfile { points }))
    }

    /// Parse a GET_EQ_SETTINGS metadata response (`A7 04 ?? <len> <utf8 preset id>`).
    pub fn parse_metadata(data: &[u8]) -> Result<String> {
        let len = *data.get(3).ok_or(ProtoError::Short(data.len()))? as usize;
        let s = data.get(4..4 + len).ok_or(ProtoError::Short(data.len()))?;
        Ok(String::from_utf8_lossy(s)
            .trim_end_matches('\0')
            .to_string())
    }

    /// Coefficient reports, in the app's order: per point, then per sample rate.
    pub fn coef_commands(&self, profile_id: u8) -> Vec<Command> {
        if self.points.is_empty() {
            return SampleRate::ALL
                .iter()
                .map(|&rate| Command::SetEqCoef {
                    profile_id,
                    rate,
                    index: 0,
                    coefs: Coefs::passthrough(rate),
                })
                .collect();
        }
        let mut out = Vec::new();
        for (i, p) in self.points.iter().enumerate() {
            for rate in SampleRate::ALL {
                out.push(Command::SetEqCoef {
                    profile_id,
                    rate,
                    index: i as u8,
                    coefs: biquad::design(p, rate),
                });
            }
        }
        out
    }

    /// Full sequence the app runs in `setSoundProfile`: settings → coefficients → commit.
    pub fn apply_commands(
        &self,
        slot: u8,
        mode: CommitMode,
        preset_uid: &str,
    ) -> Result<Vec<Command>> {
        self.validate()?;
        check_slot(slot)?;
        // Checked up front: failing at COMMIT_EQ would leave settings and coefficients
        // already sent but uncommitted.
        if preset_uid.len() > MAX_PRESET_UID_LEN {
            return Err(ProtoError::OutOfRange("preset uid too long (max 54 bytes)"));
        }
        let mut cmds = vec![Command::SetEqSettings {
            profile_id: slot,
            profile: self.clone(),
        }];
        cmds.extend(self.coef_commands(slot));
        cmds.push(Command::CommitEq {
            profile_id: slot,
            mode,
            preset_uid: preset_uid.to_string(),
        });
        Ok(cmds)
    }
}

pub fn check_slot(slot: u8) -> Result<()> {
    if (1..=SLOTS).contains(&slot) {
        Ok(())
    } else {
        Err(ProtoError::OutOfRange("EQ slot must be 1..=3"))
    }
}

/// Factory presets shipped in Adjust Pro.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    pub uid: &'static str,
    pub points: &'static [EqPoint],
}

impl Preset {
    pub fn profile(&self) -> EqProfile {
        EqProfile {
            points: self.points.to_vec(),
        }
    }
}

const fn pt(filter: FilterType, freq: u16, gain: f32, q: f32) -> EqPoint {
    EqPoint {
        filter,
        freq,
        gain,
        q,
    }
}

pub const UID_BALANCE: &str = "7d1401c3-fcbc-6c29-a2t2-eba4ae4d6bd8";
pub const UID_CLARITY: &str = "7d1401c3-fcbc-4c29-a212-eba4ae4d6bd8";
pub const UID_DEPTH: &str = "61a47fd7-2d55-4d25-96cc-f7935f93eea3";

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Balance",
        uid: UID_BALANCE,
        points: &[],
    },
    Preset {
        name: "Clarity",
        uid: UID_CLARITY,
        points: &[
            pt(FilterType::LowShelf, 265, -2.3, 0.5),
            pt(FilterType::Peak, 380, 2.2, 1.45),
            pt(FilterType::HighShelf, 1470, 1.2, 1.133),
        ],
    },
    Preset {
        name: "Depth",
        uid: UID_DEPTH,
        points: &[
            pt(FilterType::LowShelf, 120, 2.5, 0.975),
            pt(FilterType::Peak, 260, -2.0, 1.75),
        ],
    },
];

pub fn preset(name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

pub fn preset_by_uid(uid: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.uid == uid)
}
