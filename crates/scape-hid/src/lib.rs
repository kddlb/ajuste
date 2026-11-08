//! hidraw transport for the Fractal Scape plus a polling background worker.

mod device;
mod transport;
mod worker;

pub use device::{EqSlot, Scape, Status};
pub use transport::{DeviceKind, Direction, Trace, Transport};
pub use worker::{DeviceEvent, Worker, WorkerHandle};

use std::time::Duration;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);
pub const POLL_INTERVAL: Duration = Duration::from_millis(1500);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no Fractal Scape dongle or headset found")]
    NotFound,
    #[error(
        "permission denied opening {0}. Install packaging/70-fractal-scape.rules into /etc/udev/rules.d/ and replug the dongle"
    )]
    Permission(String),
    #[error("headset is not connected to the dongle")]
    HeadsetAway,
    #[error("timed out waiting for a reply to {0}")]
    Timeout(String),
    #[error("device is disconnected")]
    Disconnected,
    #[error(transparent)]
    Proto(#[from] scape_proto::ProtoError),
    #[error("HID error: {0}")]
    Hid(#[from] hidapi::HidError),
}

pub type Result<T> = std::result::Result<T, Error>;
