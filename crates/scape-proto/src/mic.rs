//! Microphone EQ: sent as a dB magnitude table, not as biquad coefficients.

use crate::biquad::{self, SampleRate};
use crate::command::Command;
use crate::eq::{EqPoint, EqProfile, FilterType};

pub const DB_MIN: f32 = -20.0;
pub const DB_MAX: f32 = 18.0;
/// Points per chunk report.
pub const CHUNK_LEN: usize = 8;
/// Table resolution used by the app (31.25 Hz steps up to 8 kHz).
pub const TABLE_LEN: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicType {
    CupMicWb = 1,
    BoomMicWb = 2,
    CupMicSwb = 3,
    BoomMicSwb = 4,
}

impl MicType {
    /// Number of 8-value chunks the device expects for this mic type.
    pub fn chunks(self) -> u8 {
        match self {
            MicType::CupMicSwb | MicType::BoomMicSwb => 48,
            _ => 32,
        }
    }
}

pub fn clamp_db(v: f32) -> f32 {
    v.clamp(DB_MIN, DB_MAX)
}

/// Frequencies of the table: 31.25 × (k + 1) Hz.
pub fn table_freq(k: usize) -> f64 {
    31.25 * (k as f64 + 1.0)
}

/// Evaluate the PEQ chain at 96 kHz (Q floored at 0.5), convert to dB, clamp to −20..18.
pub fn magnitude_table(profile: &EqProfile) -> [f32; TABLE_LEN] {
    let pts: Vec<EqPoint> = profile
        .points
        .iter()
        .map(|p| EqPoint {
            q: p.q.max(0.5),
            ..*p
        })
        .collect();
    std::array::from_fn(
        |k| clamp_db(biquad::chain_db(&pts, table_freq(k), SampleRate::S96k) as f32),
    )
}

/// Chunk reports for the table, followed by a commit (save) report.
pub fn apply_commands(profile: &EqProfile, mic: MicType) -> Vec<Command> {
    let table = magnitude_table(profile);
    let mut cmds: Vec<Command> = table
        .chunks(CHUNK_LEN)
        .enumerate()
        .map(|(i, c)| Command::SetMicChunk {
            mic,
            chunk: i as u8,
            values: c.try_into().unwrap(),
        })
        .collect();
    cmds.push(Command::CommitMic { mic });
    cmds
}

/// Parse a GET_MIC_EQ response payload: 8 × f32 at bytes 4..36.
pub fn parse_chunk(data: &[u8]) -> Option<[f32; CHUNK_LEN]> {
    let b = data.get(4..4 + 4 * CHUNK_LEN)?;
    Some(std::array::from_fn(|i| {
        f32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap())
    }))
}

pub struct MicPreset {
    pub name: &'static str,
    pub points: &'static [EqPoint],
}

pub const MIC_PRESETS: &[MicPreset] = &[
    MicPreset {
        name: "Clear",
        points: &[
            EqPoint {
                filter: FilterType::LowShelf,
                freq: 200,
                gain: 0.0,
                q: 0.5,
            },
            EqPoint {
                filter: FilterType::LowShelf,
                freq: 1000,
                gain: 0.0,
                q: 0.5,
            },
        ],
    },
    MicPreset {
        name: "Bass",
        points: &[
            EqPoint {
                filter: FilterType::LowPass,
                freq: 1000,
                gain: 4.0,
                q: 0.7,
            },
            EqPoint {
                filter: FilterType::LowShelf,
                freq: 200,
                gain: 8.0,
                q: 2.0,
            },
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_table_is_zero() {
        let t = magnitude_table(&EqProfile::default());
        assert!(t.iter().all(|&v| v == 0.0));
        let c = apply_commands(&EqProfile::default(), MicType::CupMicWb);
        assert_eq!(c.len(), 33);
        assert_eq!(
            c.last(),
            Some(&Command::CommitMic {
                mic: MicType::CupMicWb
            })
        );
    }

    #[test]
    fn shelf_boost_shows_in_table() {
        let p = EqProfile {
            points: vec![EqPoint {
                filter: FilterType::LowShelf,
                freq: 200,
                gain: 8.0,
                q: 0.7,
            }],
        };
        let t = magnitude_table(&p);
        assert!((t[0] - 8.0).abs() < 0.5, "low end ≈ +8 dB, got {}", t[0]);
        assert!(t[255].abs() < 0.2, "8 kHz ≈ 0 dB, got {}", t[255]);
    }
}
