//! Sidetone (mic monitoring). The device only exposes relative volume steps.
//!
//! Raw volume is 0 or 55..=100 in steps of 5; the app shows it as `(raw − 50) × 2` percent.

use crate::command::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidetoneOp {
    Disable = 0,
    Enable = 1,
    VolUp = 2,
    VolDown = 3,
}

/// Largest single step the app sends.
const MAX_STEP: u8 = 50;

pub fn raw_to_pct(raw: u8) -> u8 {
    if raw <= 50 {
        0
    } else {
        ((raw - 50) * 2).min(100)
    }
}

/// Percent → raw, snapped to the 10 % grid the device supports.
pub fn pct_to_raw(pct: u8) -> u8 {
    let steps = (pct.min(100) as u32 + 5) / 10;
    if steps == 0 { 0 } else { 50 + 5 * steps as u8 }
}

/// Commands to move sidetone volume from `current_raw` to `target_pct`.
pub fn plan_volume(current_raw: u8, target_pct: u8) -> Vec<Command> {
    let target = pct_to_raw(target_pct);
    let mut cur = current_raw;
    let mut out = Vec::new();
    if cur == 0 && target > 0 {
        out.push(Command::Sidetone {
            op: SidetoneOp::VolUp,
            amount: MAX_STEP,
        });
        cur = MAX_STEP;
    }
    let (op, mut delta) = if target >= cur {
        (SidetoneOp::VolUp, target - cur)
    } else {
        (SidetoneOp::VolDown, cur - target)
    };
    // The app splits amounts ≥ 51 into 50 + remainder.
    while delta > 0 {
        let step = delta.min(MAX_STEP);
        out.push(Command::Sidetone { op, amount: step });
        delta -= step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        assert_eq!(raw_to_pct(0), 0);
        assert_eq!(raw_to_pct(55), 10);
        assert_eq!(raw_to_pct(100), 100);
        assert_eq!(pct_to_raw(0), 0);
        assert_eq!(pct_to_raw(4), 0);
        assert_eq!(pct_to_raw(10), 55);
        assert_eq!(pct_to_raw(47), 75);
        assert_eq!(pct_to_raw(100), 100);
    }

    #[test]
    fn plans() {
        use SidetoneOp::*;
        let s = |op, amount| Command::Sidetone { op, amount };
        assert_eq!(plan_volume(70, 60), vec![s(VolUp, 10)]);
        assert_eq!(plan_volume(70, 30), vec![s(VolDown, 5)]);
        assert_eq!(plan_volume(0, 50), vec![s(VolUp, 50), s(VolUp, 25)]);
        assert_eq!(plan_volume(100, 0), vec![s(VolDown, 50), s(VolDown, 50)]);
        assert!(plan_volume(75, 50).is_empty());
    }
}
