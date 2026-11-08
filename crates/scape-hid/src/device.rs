use std::time::Duration;

use scape_proto::eq::{self, CommitMode, EqProfile};
use scape_proto::mic::{self, MicType};
use scape_proto::sidetone::{self, SidetoneOp};
use scape_proto::state::{self, DongleState, HeadsetState, Info};
use scape_proto::{Command, InfoField, Target};

use crate::transport::{DeviceKind, Trace, Transport};
use crate::{DEFAULT_TIMEOUT, Error, Result};

/// Snapshot published by polling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Status {
    pub kind: DeviceKind,
    /// `None` when the dongle reports no headset on the radio link.
    pub headset: Option<HeadsetState>,
}

/// One EQ slot as read from the headset.
#[derive(Debug, Clone, PartialEq)]
pub struct EqSlot {
    pub slot: u8,
    pub profile: EqProfile,
    pub preset_uid: Option<String>,
    /// True if the slot was empty and the factory copy (slot + 2) was read instead.
    pub factory_copy: bool,
}

/// High-level, blocking API over a [`Transport`].
pub struct Scape {
    t: Transport,
    timeout: Duration,
}

impl Scape {
    pub fn open() -> Result<Self> {
        Ok(Scape {
            t: Transport::open()?,
            timeout: DEFAULT_TIMEOUT,
        })
    }

    pub fn kind(&self) -> DeviceKind {
        self.t.kind
    }

    pub fn path(&self) -> &str {
        &self.t.path
    }

    pub fn set_trace(&mut self, trace: Option<Trace>) {
        self.t.set_trace(trace);
    }

    pub fn send(&self, cmd: &Command) -> Result<Option<Vec<u8>>> {
        self.t.transact(cmd, self.timeout)
    }

    fn send_reply(&self, cmd: &Command) -> Result<Vec<u8>> {
        Ok(self.send(cmd)?.expect("command expects a response"))
    }

    pub fn run(&self, cmds: &[Command]) -> Result<()> {
        for c in cmds {
            self.send(c)?;
        }
        Ok(())
    }

    pub fn headset_connected(&self) -> Result<bool> {
        match self.t.kind {
            DeviceKind::WiredHeadset => Ok(true),
            DeviceKind::Dongle => Ok(DongleState::parse(
                &self.send_reply(&Command::GetState(Target::Dongle))?,
            )?
            .headset_connected),
        }
    }

    fn require_headset(&self) -> Result<()> {
        if self.headset_connected()? {
            Ok(())
        } else {
            Err(Error::HeadsetAway)
        }
    }

    pub fn status(&self) -> Result<Status> {
        let headset = if self.headset_connected()? {
            Some(HeadsetState::parse(
                &self.send_reply(&Command::GetState(Target::Headset))?,
            )?)
        } else {
            None
        };
        Ok(Status {
            kind: self.t.kind,
            headset,
        })
    }

    /// Raw `F1 21` payload, for `--hex` debugging.
    pub fn headset_state_raw(&self) -> Result<Vec<u8>> {
        self.require_headset()?;
        self.send_reply(&Command::GetState(Target::Headset))
    }

    pub fn info(&self, target: Target) -> Result<Info> {
        let get = |f| self.send_reply(&Command::GetInfo(target, f));
        let msb = get(InfoField::SerialMsb)
            .and_then(|d| Ok(state::parse_serial_half(&d)?))
            .ok();
        let lsb = get(InfoField::SerialLsb)
            .and_then(|d| Ok(state::parse_serial_half(&d)?))
            .ok();
        let info = Info {
            fw_version: get(InfoField::FwVersion)
                .and_then(|d| Ok(state::parse_fw_version(&d)?))
                .ok(),
            // On hardware the dongle returns the full serial for both halves and the headset
            // leaves the second one empty, so only append it when it adds something.
            serial: match (msb.filter(|s| !s.is_empty()), lsb.filter(|s| !s.is_empty())) {
                (Some(a), Some(b)) if a != b => Some(format!("{a}{b}")),
                (Some(a), _) | (None, Some(a)) => Some(a),
                (None, None) => None,
            },
            edition_id: get(InfoField::EditionId)
                .and_then(|d| Ok(state::parse_edition(&d)?))
                .ok(),
            bt_addr: if target == Target::Headset {
                get(InfoField::BtAddr)
                    .and_then(|d| Ok(state::parse_bt_addr(&d)?))
                    .ok()
            } else {
                None
            },
        };
        Ok(info)
    }

    pub fn set_lights(&self, on: bool) -> Result<()> {
        self.require_headset()?;
        self.send(&Command::SetLights(on)).map(drop)
    }

    /// Raw reply to `A4 05 <slot> 00 00` (stored effect metadata at offset 10).
    pub fn read_effect_raw(&self, slot: u8) -> Result<Vec<u8>> {
        self.require_headset()?;
        self.send_reply(&Command::ReadEffect { slot, offset: 0 })
    }

    /// Decoded effect stored in a lighting slot (read-only).
    pub fn read_effect(&self, slot: u8) -> Result<Option<scape_proto::lighting::Effect>> {
        Ok(scape_proto::lighting::Effect::parse(
            &self.read_effect_raw(slot)?,
        ))
    }

    /// Render and upload a lighting effect into slot 1, then select it (as Adjust Pro does).
    pub fn upload_effect(&self, spec: &scape_proto::effects::EffectSpec) -> Result<()> {
        self.require_headset()?;
        self.run(&spec.upload_commands())
    }

    pub fn set_enc(&self, on: bool) -> Result<()> {
        self.require_headset()?;
        self.send(&Command::SetEnc(on)).map(drop)
    }

    /// Turn sidetone on/off and optionally move it to `level_pct` (snapped to 10 %).
    pub fn set_sidetone(&self, on: bool, level_pct: Option<u8>) -> Result<()> {
        self.require_headset()?;
        let st = HeadsetState::parse(&self.send_reply(&Command::GetState(Target::Headset))?)?;
        if !on {
            return self
                .send(&Command::Sidetone {
                    op: SidetoneOp::Disable,
                    amount: 0,
                })
                .map(drop);
        }
        let mut raw = st.sidetone_raw;
        if !st.sidetone_on {
            // Response is `F1 34 00 <state> <vol>`.
            let r = self.send_reply(&Command::Sidetone {
                op: SidetoneOp::Enable,
                amount: 1,
            })?; // app sends `F1 34 01 01`
            raw = r.get(4).copied().unwrap_or(raw);
        }
        if let Some(pct) = level_pct {
            self.run(&sidetone::plan_volume(raw, pct))?;
        }
        Ok(())
    }

    /// Activate EQ slot 1..=3. The device sends no reply; confirm via [`Scape::status`].
    pub fn select_eq(&self, slot: u8) -> Result<()> {
        self.require_headset()?;
        self.send(&Command::SelectEq { slot }).map(drop)
    }

    pub fn read_eq(&self, slot: u8) -> Result<EqSlot> {
        eq::check_slot(slot)?;
        self.require_headset()?;
        let mut src = slot;
        let mut data = self.send_reply(&Command::GetEq {
            slot,
            metadata: false,
        })?;
        let mut factory_copy = false;
        // Same fallback as Adjust Pro: empty user slot 2/3 → read factory copy 4/5.
        if (slot == 2 || slot == 3) && data.get(3) == Some(&0) {
            src = slot + 2;
            data = self.send_reply(&Command::GetEq {
                slot: src,
                metadata: false,
            })?;
            factory_copy = true;
        }
        let (_, _, profile) = EqProfile::parse_settings(&data)?;
        let preset_uid = self
            .send_reply(&Command::GetEq {
                slot: src,
                metadata: true,
            })
            .ok()
            .and_then(|d| EqProfile::parse_metadata(&d).ok())
            .filter(|s| !s.is_empty());
        Ok(EqSlot {
            slot,
            profile,
            preset_uid,
            factory_copy,
        })
    }

    pub fn apply_eq(
        &self,
        slot: u8,
        profile: &EqProfile,
        mode: CommitMode,
        preset_uid: &str,
    ) -> Result<()> {
        let cmds = profile.apply_commands(slot, mode, preset_uid)?;
        self.require_headset()?;
        self.run(&cmds)
    }

    pub fn apply_mic_eq(&self, profile: &EqProfile, mic: MicType) -> Result<()> {
        profile.validate()?;
        self.require_headset()?;
        self.run(&mic::apply_commands(profile, mic))
    }

    pub fn erase_mic_eq(&self, mic: MicType) -> Result<()> {
        self.require_headset()?;
        self.send(&Command::EraseMic { mic }).map(drop)
    }

    /// Read back the stored mic magnitude table (dB values).
    pub fn read_mic_table(&self, mic: MicType) -> Result<Vec<f32>> {
        self.require_headset()?;
        let mut out = Vec::with_capacity(mic::TABLE_LEN);
        for chunk in 0..mic.chunks() {
            let d = self.send_reply(&Command::GetMicChunk { mic, chunk })?;
            out.extend(mic::parse_chunk(&d).ok_or(scape_proto::ProtoError::Short(d.len()))?);
        }
        Ok(out)
    }
}
