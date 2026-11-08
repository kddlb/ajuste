//! Pure (no I/O) encoder/decoder for the Fractal Scape "P911" vendor HID protocol.
//!
//! Every outgoing report is built from [`Command`]. Commands that could brick or wipe the
//! device (firmware upgrade 0xB5/0x15, FACTORY_RESET 0xF2, INITIALIZE/SHUTDOWN, lighting
//! uploads, WDL opt-in) intentionally have **no** representation here.
//!
//! See `FINDINGS.md` at the repository root for the reverse-engineered layout.

pub mod biquad;
pub mod command;
pub mod eq;
pub mod mic;
pub mod sidetone;
pub mod state;

pub use biquad::{Coefs, SampleRate};
pub use command::{Command, InfoField, Target};
pub use eq::{CommitMode, EqPoint, EqProfile, FilterType};
pub use mic::MicType;
pub use sidetone::SidetoneOp;
pub use state::{DongleState, HeadsetState, Info};

/// HID report ID of the P911 channel (vendor usage page 0xFF00).
pub const REPORT_ID: u8 = 0x02;
/// Payload length of report 0x02 (excluding the report ID byte).
pub const PAYLOAD_LEN: usize = 63;
/// Full length on hidraw: report ID + payload.
pub const REPORT_LEN: usize = PAYLOAD_LEN + 1;

pub const VID_FRACTAL: u16 = 0x36BC;
pub const PID_DONGLE: u16 = 0x0001;
pub const PID_HEADSET_USB: u16 = 0x2001;
pub const USAGE_PAGE_P911: u16 = 0xFF00;

/// Device-type byte (first payload byte).
pub mod device_type {
    pub const DONGLE: u8 = 0x11;
    pub const HEADSET: u8 = 0xF1;
    pub const EQ: u8 = 0xA7;
    pub const EFFECT: u8 = 0xA4;
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ProtoError {
    #[error("value out of range: {0}")]
    OutOfRange(&'static str),
    #[error("response too short ({0} bytes)")]
    Short(usize),
    #[error("response is for {got_type:#04x}/{got_cmd:#04x}, expected {want_type:#04x}/{want_cmd:#04x}")]
    Mismatch { want_type: u8, want_cmd: u8, got_type: u8, got_cmd: u8 },
    #[error("device returned status {0:#04x}")]
    Status(u8),
}

pub type Result<T> = std::result::Result<T, ProtoError>;

/// Format bytes as space-separated hex, for logs and `--hex` output.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}
