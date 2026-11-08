//! The allow-listed set of commands this crate can encode.

use crate::biquad::{Coefs, SampleRate};
use crate::device_type::{DONGLE, EFFECT, EQ, HEADSET};
use crate::eq::{self, CommitMode, EqProfile};
use crate::mic::{self, MicType};
use crate::sidetone::SidetoneOp;
use crate::{PAYLOAD_LEN, ProtoError, REPORT_ID, REPORT_LEN, Result};

/// Which end of the radio link a factory/info command is addressed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Dongle,
    Headset,
}

impl Target {
    pub fn type_byte(self) -> u8 {
        match self {
            Target::Dongle => DONGLE,
            Target::Headset => HEADSET,
        }
    }
}

/// Read-only info getters (factory command space).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoField {
    FwVersion = 0x01,
    SerialMsb = 0x02,
    SerialLsb = 0x03,
    EditionId = 0x04,
    BtAddr = 0x05,
}

mod cmd {
    pub const GET_DEVICE_STATE: u8 = 0x21;
    pub const SIDE_TONE_CONTROL: u8 = 0x34;
    pub const ENC_CONTROL: u8 = 0x36;
    pub const SET_EQ_SETTINGS: u8 = 0x01;
    pub const SET_EQ_COEF: u8 = 0x02;
    pub const COMMIT_EQ: u8 = 0x03;
    pub const GET_EQ_SETTINGS: u8 = 0x04;
    pub const GET_EQ_COEF: u8 = 0x05;
    pub const SELECT_EQ_MODE: u8 = 0x07;
    pub const SET_MIC_EQ: u8 = 0x11;
    pub const COMMIT_MIC_EQ: u8 = 0x12;
    pub const GET_MIC_EQ: u8 = 0x13;
    pub const SELECT_EFFECT_SLOT: u8 = 0x04;
    pub const READ_EFFECT: u8 = 0x05;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// `11 21` / `F1 21`.
    GetState(Target),
    /// `11|F1 01..05`.
    GetInfo(Target, InfoField),
    /// `F1 36 <0|1>` — environmental noise cancellation (mic).
    SetEnc(bool),
    /// `F1 34 <op> <amount>`.
    Sidetone { op: SidetoneOp, amount: u8 },
    /// `A4 04 <0|1>` — lighting slot 0 = LEDs off, 1 = the stored effect (Adjust Pro's
    /// `turnLight`). The only lighting command this crate can encode.
    SetLights(bool),
    /// `A4 05 <slot> <offHi> <offLo>` — read stored effect data (read-only). Offset 0 returns
    /// the 42-byte metadata header at reply offset 10.
    ReadEffect { slot: u8, offset: u16 },
    /// `A7 07 <slot>` — no response.
    SelectEq { slot: u8 },
    /// `A7 04 <slot> <0|1>` (slot 4/5 = factory copies of 2/3).
    GetEq { slot: u8, metadata: bool },
    /// `A7 05 <slot> <rate> <index>`.
    GetEqCoef { slot: u8, rate: SampleRate, index: u8 },
    /// `A7 01 <id<<4|7> <5 points>`.
    SetEqSettings { profile_id: u8, profile: EqProfile },
    /// `A7 02 <id> <rate> <index> b0 b1 b2 a1 a2`.
    SetEqCoef { profile_id: u8, rate: SampleRate, index: u8, coefs: Coefs },
    /// `A7 03 <id> <mode> 00000000 <len> <uid>`.
    CommitEq { profile_id: u8, mode: CommitMode, preset_uid: String },
    /// `A7 11 <mic> <chunk> <8×f32>`.
    SetMicChunk { mic: MicType, chunk: u8, values: [f32; 8] },
    /// `A7 12 01 <mic>`.
    CommitMic { mic: MicType },
    /// `A7 12 02 01`.
    EraseMic,
    /// `A7 13 <mic> <chunk>`.
    GetMicChunk { mic: MicType, chunk: u8 },
}

impl Command {
    pub fn type_byte(&self) -> u8 {
        match self {
            Command::GetState(t) | Command::GetInfo(t, _) => t.type_byte(),
            Command::SetEnc(_) | Command::Sidetone { .. } => HEADSET,
            Command::SetLights(_) | Command::ReadEffect { .. } => EFFECT,
            _ => EQ,
        }
    }

    pub fn cmd_byte(&self) -> u8 {
        match self {
            Command::GetState(_) => cmd::GET_DEVICE_STATE,
            Command::GetInfo(_, f) => *f as u8,
            Command::SetEnc(_) => cmd::ENC_CONTROL,
            Command::Sidetone { .. } => cmd::SIDE_TONE_CONTROL,
            Command::SetLights(_) => cmd::SELECT_EFFECT_SLOT,
            Command::ReadEffect { .. } => cmd::READ_EFFECT,
            Command::SelectEq { .. } => cmd::SELECT_EQ_MODE,
            Command::GetEq { .. } => cmd::GET_EQ_SETTINGS,
            Command::GetEqCoef { .. } => cmd::GET_EQ_COEF,
            Command::SetEqSettings { .. } => cmd::SET_EQ_SETTINGS,
            Command::SetEqCoef { .. } => cmd::SET_EQ_COEF,
            Command::CommitEq { .. } => cmd::COMMIT_EQ,
            Command::SetMicChunk { .. } => cmd::SET_MIC_EQ,
            Command::CommitMic { .. } | Command::EraseMic => cmd::COMMIT_MIC_EQ,
            Command::GetMicChunk { .. } => cmd::GET_MIC_EQ,
        }
    }

    /// Whether the device answers with an input report. SELECT_EQ_MODE does not.
    pub fn expects_response(&self) -> bool {
        !matches!(self, Command::SelectEq { .. })
    }

    /// Whether the command changes device state (used for `--dry-run` / confirmation UX).
    pub fn is_write(&self) -> bool {
        !matches!(
            self,
            Command::GetState(_)
                | Command::GetInfo(..)
                | Command::GetEq { .. }
                | Command::GetEqCoef { .. }
                | Command::GetMicChunk { .. }
                | Command::ReadEffect { .. }
        )
    }

    /// Payload (without report ID), unpadded.
    pub fn payload(&self) -> Result<Vec<u8>> {
        let mut p = vec![self.type_byte(), self.cmd_byte()];
        match self {
            Command::GetState(_) | Command::GetInfo(..) => {}
            Command::SetEnc(on) | Command::SetLights(on) => p.push(*on as u8),
            Command::ReadEffect { slot, offset } => {
                p.push(*slot);
                p.extend(offset.to_be_bytes());
            }
            Command::Sidetone { op, amount } => p.extend([*op as u8, *amount]),
            Command::SelectEq { slot } => {
                eq::check_slot(*slot)?;
                p.push(*slot);
            }
            Command::GetEq { slot, metadata } => {
                if !(1..=5).contains(slot) {
                    return Err(ProtoError::OutOfRange("GET_EQ slot must be 1..=5"));
                }
                p.extend([*slot, *metadata as u8]);
            }
            Command::GetEqCoef { slot, rate, index } => p.extend([*slot, rate.code(), *index]),
            Command::SetEqSettings { profile_id, profile } => {
                eq::check_slot(*profile_id)?;
                profile.validate()?;
                p.extend(profile.settings_bytes(*profile_id));
            }
            Command::SetEqCoef { profile_id, rate, index, coefs } => {
                eq::check_slot(*profile_id)?;
                if *index as usize >= eq::MAX_POINTS {
                    return Err(ProtoError::OutOfRange("coef index"));
                }
                p.extend([*profile_id, rate.code(), *index]);
                for v in coefs.b.iter().chain(coefs.a.iter()) {
                    p.extend(v.to_le_bytes());
                }
            }
            Command::CommitEq { profile_id, mode, preset_uid } => {
                eq::check_slot(*profile_id)?;
                let uid = preset_uid.as_bytes();
                if uid.len() > PAYLOAD_LEN - 9 {
                    return Err(ProtoError::OutOfRange("preset uid too long"));
                }
                p.extend([*profile_id, *mode as u8]);
                p.extend(0f32.to_le_bytes());
                p.push(uid.len() as u8);
                p.extend(uid);
            }
            Command::SetMicChunk { mic, chunk, values } => {
                if *chunk >= mic.chunks() {
                    return Err(ProtoError::OutOfRange("mic chunk index"));
                }
                p.extend([*mic as u8, *chunk]);
                for v in values {
                    p.extend(mic::clamp_db(*v).to_le_bytes());
                }
            }
            Command::CommitMic { mic } => p.extend([1, *mic as u8]),
            Command::EraseMic => p.extend([2, 1]),
            Command::GetMicChunk { mic, chunk } => p.extend([*mic as u8, *chunk]),
        }
        debug_assert!(p.len() <= PAYLOAD_LEN);
        Ok(p)
    }

    /// Full hidraw report: `02 <payload>` zero-padded to 64 bytes.
    pub fn encode(&self) -> Result<[u8; REPORT_LEN]> {
        let p = self.payload()?;
        let mut out = [0u8; REPORT_LEN];
        out[0] = REPORT_ID;
        out[1..1 + p.len()].copy_from_slice(&p);
        Ok(out)
    }

    /// Does an input-report payload (without report ID) belong to this command?
    pub fn matches(&self, data: &[u8]) -> bool {
        data.len() >= 2 && data[0] == self.type_byte() && data[1] == self.cmd_byte()
    }

    /// Validate a response payload (without report ID). GET_EQ_SETTINGS carries data in the
    /// status byte, so only type/cmd are checked for it.
    pub fn check_response<'a>(&self, data: &'a [u8]) -> Result<&'a [u8]> {
        if data.len() < 3 {
            return Err(ProtoError::Short(data.len()));
        }
        if !self.matches(data) {
            return Err(ProtoError::Mismatch {
                want_type: self.type_byte(),
                want_cmd: self.cmd_byte(),
                got_type: data[0],
                got_cmd: data[1],
            });
        }
        if !matches!(self, Command::GetEq { .. }) && data[2] != 0 {
            return Err(ProtoError::Status(data[2]));
        }
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eq::{EqPoint, FilterType, PRESETS};

    fn all_samples() -> Vec<Command> {
        let mut v = vec![
            Command::GetState(Target::Dongle),
            Command::GetState(Target::Headset),
            Command::SetEnc(true),
            Command::SetLights(true),
            Command::SetLights(false),
            Command::ReadEffect { slot: 1, offset: 0 },
            Command::Sidetone { op: SidetoneOp::Enable, amount: 1 },
            Command::SelectEq { slot: 2 },
            Command::GetEq { slot: 1, metadata: false },
            Command::GetEqCoef { slot: 1, rate: SampleRate::S48k, index: 0 },
            Command::CommitEq { profile_id: 1, mode: CommitMode::Live, preset_uid: "x".into() },
            Command::SetMicChunk { mic: MicType::CupMicWb, chunk: 0, values: [0.0; 8] },
            Command::CommitMic { mic: MicType::CupMicWb },
            Command::EraseMic,
            Command::GetMicChunk { mic: MicType::CupMicWb, chunk: 0 },
        ];
        for t in [Target::Dongle, Target::Headset] {
            for f in [InfoField::FwVersion, InfoField::SerialMsb, InfoField::SerialLsb, InfoField::EditionId, InfoField::BtAddr] {
                v.push(Command::GetInfo(t, f));
            }
        }
        for p in PRESETS {
            v.extend(p.profile().apply_commands(3, CommitMode::Save, p.uid).unwrap());
        }
        v
    }

    #[test]
    fn never_encodes_dangerous_commands() {
        for c in all_samples() {
            let r = c.encode().unwrap();
            let (t, cmd) = (r[1], r[2]);
            assert!(t != 0xB5 && t != 0x15, "firmware type in {c:?}");
            assert!(t != 0xA5, "lighting stream in {c:?}");
            // Only the slot select (on/off) is allowed from the lighting command set.
            assert!(t != 0xA4 || (cmd == 0x04 && r[3] <= 1) || cmd == 0x05, "lighting upload in {c:?}");
            if t == DONGLE || t == HEADSET {
                assert!(![0xF0, 0xF1, 0xF2, 0x15].contains(&cmd), "forbidden factory cmd in {c:?}");
            }
        }
    }

    #[test]
    fn simple_encodings() {
        assert_eq!(&Command::GetState(Target::Dongle).encode().unwrap()[..4], &[0x02, 0x11, 0x21, 0]);
        assert_eq!(&Command::GetState(Target::Headset).encode().unwrap()[..4], &[0x02, 0xF1, 0x21, 0]);
        assert_eq!(&Command::SetEnc(true).encode().unwrap()[..4], &[0x02, 0xF1, 0x36, 1]);
        assert_eq!(
            &Command::Sidetone { op: SidetoneOp::VolUp, amount: 10 }.encode().unwrap()[..5],
            &[0x02, 0xF1, 0x34, 2, 10]
        );
        assert_eq!(&Command::SetLights(false).encode().unwrap()[..4], &[0x02, 0xA4, 0x04, 0]);
        assert_eq!(&Command::SelectEq { slot: 3 }.encode().unwrap()[..4], &[0x02, 0xA7, 0x07, 3]);
        assert!(Command::SelectEq { slot: 0 }.encode().is_err());
        assert!(Command::SelectEq { slot: 4 }.encode().is_err());
        assert_eq!(Command::EraseMic.payload().unwrap(), vec![0xA7, 0x12, 2, 1]);
    }

    #[test]
    fn eq_settings_layout() {
        let prof = EqProfile { points: vec![EqPoint::new(FilterType::Peak, 1000, 3.0, 1.0)] };
        let p = Command::SetEqSettings { profile_id: 2, profile: prof.clone() }.payload().unwrap();
        assert_eq!(p.len(), 63);
        assert_eq!(&p[..3], &[0xA7, 0x01, 0x27]);
        assert_eq!(&p[3..7], &[1, 0, 0xE8, 0x03]);
        assert_eq!(&p[7..11], &3.0f32.to_le_bytes());
        assert_eq!(&p[11..15], &1.0f32.to_le_bytes());
        // Unused points
        assert_eq!(&p[15..19], &[0, 0, 0x20, 0x00]);
        assert_eq!(&p[23..27], &0.5f32.to_le_bytes());

        // Round trip through the GET parser (response mirrors the SET layout).
        let (id, mask, back) = EqProfile::parse_settings(&p).unwrap();
        assert_eq!((id, mask), (2, 7));
        assert_eq!(back, prof);

        // Flat profile writes a 22 kHz placeholder and parses back as empty.
        let flat = Command::SetEqSettings { profile_id: 1, profile: EqProfile::default() }.payload().unwrap();
        assert_eq!(&flat[3..7], &[1, 0, 0xF0, 0x55]);
        assert_eq!(EqProfile::parse_settings(&flat).unwrap().2, EqProfile::default());
    }

    #[test]
    fn commit_layout() {
        let p = Command::CommitEq { profile_id: 1, mode: CommitMode::Save, preset_uid: "abc".into() }.payload().unwrap();
        assert_eq!(p, vec![0xA7, 0x03, 1, 2, 0, 0, 0, 0, 3, b'a', b'b', b'c']);
    }

    #[test]
    fn response_checks() {
        let c = Command::SetEnc(true);
        assert!(c.check_response(&[0xF1, 0x36, 0]).is_ok());
        assert_eq!(c.check_response(&[0xF1, 0x36, 1]), Err(ProtoError::Status(1)));
        assert!(matches!(c.check_response(&[0xF1, 0x21, 0]), Err(ProtoError::Mismatch { .. })));
        // GET_EQ ignores byte 2.
        assert!(Command::GetEq { slot: 1, metadata: false }.check_response(&[0xA7, 0x04, 0x17]).is_ok());
    }
}
