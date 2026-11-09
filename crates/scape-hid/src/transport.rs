use std::time::{Duration, Instant};

use hidapi::{HidApi, HidDevice};
use scape_proto::{
    Command, PID_DONGLE, PID_HEADSET_USB, REPORT_ID, REPORT_LEN, USAGE_PAGE_P911, VID_FRACTAL, hex,
};

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    /// 2.4 GHz dongle; headset commands are forwarded over the radio link.
    Dongle,
    /// Headset plugged in directly over USB.
    WiredHeadset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Out,
    In,
}

/// Optional hook to observe raw traffic (for `--hex` / debug views).
pub type Trace = Box<dyn Fn(Direction, &[u8]) + Send>;

pub struct Transport {
    dev: HidDevice,
    pub kind: DeviceKind,
    pub path: String,
    trace: Option<Trace>,
}

impl Transport {
    /// Find and open the P911 interface (vendor usage page 0xFF00).
    pub fn open() -> Result<Self> {
        let api = HidApi::new()?;
        let info = api
            .device_list()
            .filter(|d| {
                d.vendor_id() == VID_FRACTAL
                    && matches!(d.product_id(), PID_DONGLE | PID_HEADSET_USB)
            })
            .find(|d| d.usage_page() == USAGE_PAGE_P911)
            .ok_or(Error::NotFound)?;
        let path = info.path().to_string_lossy().into_owned();
        let kind = if info.product_id() == PID_DONGLE {
            DeviceKind::Dongle
        } else {
            DeviceKind::WiredHeadset
        };

        let dev = info.open_device(&api).map_err(|e| {
            // hidapi flattens errno; check directly so we can give the udev hint.
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
            {
                Err(io) if io.kind() == std::io::ErrorKind::PermissionDenied => {
                    Error::Permission(path.clone())
                }
                _ => Error::Hid(e),
            }
        })?;
        log::info!("opened {kind:?} at {path}");
        Ok(Transport {
            dev,
            kind,
            path,
            trace: None,
        })
    }

    pub fn set_trace(&mut self, trace: Option<Trace>) {
        self.trace = trace;
    }

    fn drain(&self) -> Result<()> {
        let mut buf = [0u8; REPORT_LEN];
        while self.dev.read_timeout(&mut buf, 0)? > 0 {}
        Ok(())
    }

    /// Send one command and, if it has a reply, wait for the matching input report.
    /// Returns the reply payload (report ID stripped), already status-checked.
    pub fn transact(&self, cmd: &Command, timeout: Duration) -> Result<Option<Vec<u8>>> {
        let report = cmd.encode()?;
        self.drain()?;
        if let Some(t) = &self.trace {
            t(Direction::Out, &report);
        }
        self.dev.write(&report)?;
        if !cmd.expects_response() {
            return Ok(None);
        }

        let deadline = Instant::now() + timeout;
        let mut buf = [0u8; REPORT_LEN];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Timeout(hex(&report[1..4])));
            }
            let n = self
                .dev
                .read_timeout(&mut buf, left.as_millis().max(1) as i32)?;
            if n == 0 {
                continue;
            }
            let rep = &buf[..n];
            if let Some(t) = &self.trace {
                t(Direction::In, rep);
            }
            // Ignore consumer (0x0C) / telephony (0x05) reports and stale replies.
            if rep[0] != REPORT_ID || !cmd.matches(&rep[1..]) {
                log::debug!("ignoring unrelated report: {}", hex(rep));
                continue;
            }
            cmd.check_response(&rep[1..])?;
            return Ok(Some(rep[1..].to_vec()));
        }
    }
}
