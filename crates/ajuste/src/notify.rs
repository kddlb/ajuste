//! Desktop notifications for low battery and headset connection changes.

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

/// Fire-and-forget; notify-rust talks D-Bus synchronously, so keep it off the UI thread.
pub fn show(summary: String, urgent: bool) {
    std::thread::spawn(move || {
        let mut n = notify_rust::Notification::new();
        n.appname("ajuste")
            .summary(&summary)
            .icon("audio-headphones");
        if urgent {
            n.urgency(notify_rust::Urgency::Critical);
        }
        if let Err(e) = n.show() {
            log::warn!("notification failed: {e}");
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
}
