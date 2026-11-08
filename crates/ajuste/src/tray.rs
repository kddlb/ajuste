//! StatusNotifierItem tray icon (KDE, GNOME with AppIndicator extension, etc.).

use ksni::blocking::TrayMethods;
use ksni::menu::{CheckmarkItem, RadioGroup, RadioItem, StandardItem};
use ksni::{MenuItem, ToolTip};

/// Requests from the tray to the GPUI app.
#[derive(Debug, Clone)]
pub enum AppMsg {
    OpenWindow,
    SetEnc(bool),
    SetLights(bool),
    SelectEq(u8),
    Quit,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrayState {
    pub dongle: bool,
    pub headset: bool,
    pub battery: Option<u8>,
    pub enc: bool,
    pub lights: bool,
    pub eq_slot: u8,
}

pub struct ScapeTray {
    pub state: TrayState,
    tx: async_channel::Sender<AppMsg>,
}

impl ScapeTray {
    fn send(&self, m: AppMsg) {
        let _ = self.tx.send_blocking(m);
    }

    fn summary(&self) -> String {
        match (&self.state.dongle, &self.state.headset, self.state.battery) {
            (false, _, _) => "Dongle not found".into(),
            (true, false, _) => "Headset not connected".into(),
            (true, true, Some(b)) => format!("Battery {b}%"),
            _ => "Connected".into(),
        }
    }
}

impl ksni::Tray for ScapeTray {
    fn id(&self) -> String {
        "ajuste".into()
    }

    fn title(&self) -> String {
        format!("Fractal Scape — {}", self.summary())
    }

    fn icon_name(&self) -> String {
        match self.state.battery {
            _ if !self.state.headset => "audio-headset".into(),
            Some(b) if b <= 10 => "battery-caution".into(),
            Some(b) if b <= 20 => "battery-low".into(),
            _ => "audio-headset".into(),
        }
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip { title: "Fractal Scape".into(), description: self.summary(), ..Default::default() }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(AppMsg::OpenWindow);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let live = self.state.headset;
        vec![
            StandardItem {
                label: self.summary(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Open ajuste".into(),
                icon_name: "audio-headset".into(),
                activate: Box::new(|t: &mut Self| t.send(AppMsg::OpenWindow)),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Noise cancellation (ENC)".into(),
                enabled: live,
                checked: self.state.enc,
                activate: Box::new(|t: &mut Self| t.send(AppMsg::SetEnc(!t.state.enc))),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Lights".into(),
                enabled: live,
                checked: self.state.lights,
                activate: Box::new(|t: &mut Self| t.send(AppMsg::SetLights(!t.state.lights))),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            RadioGroup {
                selected: self.state.eq_slot.saturating_sub(1) as usize,
                select: Box::new(|t: &mut Self, i| t.send(AppMsg::SelectEq(i as u8 + 1))),
                options: (1..=3)
                    .map(|i| RadioItem { label: format!("EQ slot {i}"), enabled: live, ..Default::default() })
                    .collect(),
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut Self| t.send(AppMsg::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub type TrayHandle = ksni::blocking::Handle<ScapeTray>;

pub fn spawn(tx: async_channel::Sender<AppMsg>) -> Option<TrayHandle> {
    match (ScapeTray { state: TrayState::default(), tx }).spawn() {
        Ok(h) => Some(h),
        Err(e) => {
            log::warn!("tray unavailable ({e}); closing the window will quit");
            None
        }
    }
}
