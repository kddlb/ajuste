//! Decoding of GET_DEVICE_STATE and info responses.
//!
//! All offsets are into the response payload (report ID stripped): `[type, cmd, status, …]`.

use crate::{ProtoError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct DongleState {
    pub headset_connected: bool,
}

impl DongleState {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let b = *data.get(3).ok_or(ProtoError::Short(data.len()))?;
        Ok(DongleState { headset_connected: b == 1 })
    }
}

/// Decoded `F1 21` response. Byte meanings are from Adjust Pro's parser; fields marked
/// "unverified" have not been confirmed on hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct HeadsetState {
    pub battery_pct: u8,
    pub boom_mic_attached: bool,
    pub muted: bool,
    pub eq_slot: u8,
    pub lighting_slot: u8,
    pub chat_volume: u8,
    pub game_volume: u8,
    pub volume_state: u8,
    pub bt_mode: u8,
    pub hall_sensor: u8,
    pub sidetone_on: bool,
    pub sidetone_raw: u8,
    pub bt_connection: u8,
    pub enc_on: bool,
    pub power_state: u8,
    /// b26–27 LE; looks like battery millivolts (unconfirmed).
    pub battery_mv: u16,
}

pub const HEADSET_STATE_MIN_LEN: usize = 28;

impl HeadsetState {
    pub fn parse(d: &[u8]) -> Result<Self> {
        if d.len() < HEADSET_STATE_MIN_LEN {
            return Err(ProtoError::Short(d.len()));
        }
        Ok(HeadsetState {
            // Verified on hardware: 1 = boom attached (Adjust Pro's naming suggested the opposite).
            boom_mic_attached: d[3] == 1,
            // Verified on hardware: flipping the boom up (mute) sets byte 4 and the hall sensor
            // (byte 13) to 1.
            muted: d[4] == 1,
            eq_slot: d[5],
            // 0 = LEDs off (lighting button or SetLights).
            lighting_slot: d[6],
            chat_volume: d[7],
            game_volume: d[9],
            volume_state: d[11],
            bt_mode: d[12],
            hall_sensor: d[13],
            battery_pct: d[14],
            sidetone_on: d[15] == 1,
            sidetone_raw: d[16],
            bt_connection: d[18],
            enc_on: d[19] == 1,
            power_state: d[20],
            battery_mv: u16::from_le_bytes([d[26], d[27]]),
        })
    }

    pub fn sidetone_pct(&self) -> u8 {
        crate::sidetone::raw_to_pct(self.sidetone_raw)
    }
}

/// Info strings gathered from the factory getters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Info {
    pub fw_version: Option<String>,
    pub serial: Option<String>,
    pub edition_id: Option<u8>,
    pub bt_addr: Option<String>,
}

pub fn parse_fw_version(d: &[u8]) -> Result<String> {
    let v = d.get(3..6).ok_or(ProtoError::Short(d.len()))?;
    Ok(format!("{}.{}.{}", v[0], v[1], v[2]))
}

/// NUL-terminated ASCII starting at byte 3 (20 chars on real hardware).
pub fn parse_serial_half(d: &[u8]) -> Result<String> {
    let v = d.get(3..).ok_or(ProtoError::Short(d.len()))?;
    Ok(v.iter().take_while(|&&b| b != 0).map(|&b| b as char).collect())
}

pub fn parse_edition(d: &[u8]) -> Result<u8> {
    d.get(3).copied().ok_or(ProtoError::Short(d.len()))
}

pub fn parse_bt_addr(d: &[u8]) -> Result<String> {
    let v = d.get(3..9).ok_or(ProtoError::Short(d.len()))?;
    Ok(v.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headset_state() {
        let mut d = [0u8; 63];
        d[..3].copy_from_slice(&[0xF1, 0x21, 0]);
        d[3] = 1; // boom attached
        d[4] = 0; // not muted
        d[5] = 2;
        d[14] = 87;
        d[15] = 1;
        d[16] = 70;
        d[19] = 1;
        let s = HeadsetState::parse(&d).unwrap();
        assert!(s.boom_mic_attached && !s.muted && s.sidetone_on && s.enc_on);
        assert_eq!((s.eq_slot, s.battery_pct, s.sidetone_pct()), (2, 87, 40));
        assert!(HeadsetState::parse(&d[..10]).is_err());
    }
}
