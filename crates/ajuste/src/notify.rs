//! Desktop notifications: battery, connection, and changes the wearer makes on the headset.

/// Edge-triggered battery alerts with hysteresis.
#[derive(Debug, Default)]
pub struct BatteryAlerts {
    warned_low: bool,
    warned_critical: bool,
}

/// Re-arm once the battery is this many points above the low threshold (i.e. charging).
const REARM_MARGIN: u8 = 5;

impl BatteryAlerts {
    /// Returns the message to show, if a threshold was just crossed.
    pub fn update(&mut self, pct: u8, low: u8, critical: u8) -> Option<(String, bool)> {
        if pct > low.saturating_add(REARM_MARGIN) {
            self.warned_low = false;
            self.warned_critical = false;
            return None;
        }
        if pct <= critical && !self.warned_critical {
            self.warned_critical = true;
            self.warned_low = true;
            return Some((format!("Headset battery critical: {pct}%"), true));
        }
        if pct <= low && !self.warned_low {
            self.warned_low = true;
            return Some((format!("Headset battery low: {pct}%"), false));
        }
        None
    }
}

/// Notification categories. Each one replaces its own previous notification, so repeated
/// button presses update a single bubble instead of stacking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Connection,
    Battery,
    Mute,
    Enc,
    EqSlot,
    Lights,
    Boom,
}

/// A state change the wearer made on the headset (buttons, boom, EQ key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Mute(bool),
    Enc(bool),
    EqSlot(u8),
    Lights(bool),
    Boom(bool),
}

impl Change {
    pub fn kind(self) -> Kind {
        match self {
            Change::Mute(_) => Kind::Mute,
            Change::Enc(_) => Kind::Enc,
            Change::EqSlot(_) => Kind::EqSlot,
            Change::Lights(_) => Kind::Lights,
            Change::Boom(_) => Kind::Boom,
        }
    }

    /// Summary and body; `eq_name` is the preset stored in the new EQ slot, if known.
    pub fn message(self, eq_name: Option<&str>) -> (String, Option<String>) {
        let s = |t: &str| t.to_string();
        match self {
            Change::Mute(true) => (s("Microphone muted"), None),
            Change::Mute(false) => (s("Microphone unmuted"), None),
            Change::Enc(on) => (
                format!("Noise cancellation {}", if on { "on" } else { "off" }),
                None,
            ),
            Change::EqSlot(n) => (format!("EQ slot {n}"), eq_name.map(str::to_string)),
            Change::Lights(on) => (format!("Lights {}", if on { "on" } else { "off" }), None),
            Change::Boom(true) => (s("Boom mic attached"), None),
            Change::Boom(false) => (
                s("Boom mic removed"),
                Some(s("Muted. Unmute to use the built-in microphone.")),
            ),
        }
    }
}

/// What changed between two consecutive polls. Unplugging the boom also auto-mutes; that
/// mute is folded into the boom message instead of being reported twice.
pub fn diff(old: &scape_proto::HeadsetState, new: &scape_proto::HeadsetState) -> Vec<Change> {
    let mut out = Vec::new();
    let boom_changed = old.boom_mic_attached != new.boom_mic_attached;
    if boom_changed {
        out.push(Change::Boom(new.boom_mic_attached));
    }
    if old.muted != new.muted && !(boom_changed && !new.boom_mic_attached) {
        out.push(Change::Mute(new.muted));
    }
    if old.enc_on != new.enc_on {
        out.push(Change::Enc(new.enc_on));
    }
    if old.eq_slot != new.eq_slot {
        out.push(Change::EqSlot(new.eq_slot));
    }
    if (old.lighting_slot != 0) != (new.lighting_slot != 0) {
        out.push(Change::Lights(new.lighting_slot != 0));
    }
    out
}

/// Fire-and-forget; notify-rust talks D-Bus synchronously, so keep it off the UI thread.
pub fn show(kind: Kind, summary: String, body: Option<String>, urgent: bool) {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static IDS: OnceLock<Mutex<HashMap<Kind, u32>>> = OnceLock::new();
    let ids = IDS.get_or_init(Default::default);

    std::thread::spawn(move || {
        let mut n = notify_rust::Notification::new();
        n.appname("Ajuste")
            .summary(&summary)
            .icon("audio-headphones");
        if let Some(b) = &body {
            n.body(b);
        }
        if urgent {
            n.urgency(notify_rust::Urgency::Critical);
        } else {
            n.timeout(notify_rust::Timeout::Milliseconds(4000));
        }
        if let Some(&id) = ids.lock().unwrap().get(&kind) {
            n.id(id);
        }
        match n.show() {
            Ok(h) => {
                ids.lock().unwrap().insert(kind, h.id());
            }
            Err(e) => log::warn!("notification failed: {e}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hysteresis() {
        let mut a = BatteryAlerts::default();
        assert!(a.update(50, 20, 10).is_none());
        assert!(a.update(20, 20, 10).is_some());
        assert!(a.update(19, 20, 10).is_none());
        assert!(a.update(10, 20, 10).unwrap().1);
        assert!(a.update(9, 20, 10).is_none());
        assert!(a.update(24, 20, 10).is_none()); // still within margin
        assert!(a.update(30, 20, 10).is_none()); // re-armed
        assert!(a.update(20, 20, 10).is_some());
    }

    fn state() -> scape_proto::HeadsetState {
        let mut d = [0u8; 63];
        d[..3].copy_from_slice(&[0xF1, 0x21, 0]);
        d[3] = 1; // boom attached
        d[5] = 1; // EQ slot 1
        d[6] = 1; // lights on
        d[19] = 1; // ENC on
        scape_proto::HeadsetState::parse(&d).unwrap()
    }

    #[test]
    fn detects_button_changes() {
        let a = state();
        assert!(diff(&a, &a).is_empty());
        let mut b = a;
        b.muted = true;
        b.eq_slot = 2;
        assert_eq!(diff(&a, &b), vec![Change::Mute(true), Change::EqSlot(2)]);
        let mut c = a;
        c.enc_on = false;
        c.lighting_slot = 0;
        assert_eq!(
            diff(&a, &c),
            vec![Change::Enc(false), Change::Lights(false)]
        );
    }

    #[test]
    fn boom_removal_folds_auto_mute() {
        let a = state();
        let mut b = a;
        b.boom_mic_attached = false;
        b.muted = true;
        assert_eq!(diff(&a, &b), vec![Change::Boom(false)]);
        // Plugging back in doesn't unmute; a later unmute is its own change.
        let mut c = b;
        c.boom_mic_attached = true;
        assert_eq!(diff(&b, &c), vec![Change::Boom(true)]);
    }
}
