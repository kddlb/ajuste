//! App-wide device state, shared by the window, tray and notifier.

use gpui_kit::*;
use scape_hid::{DeviceEvent, DeviceKind, Scape, WorkerHandle};
use scape_proto::HeadsetState;

use crate::config::Config;
use crate::notify::{self, BatteryAlerts};
use crate::tray::{TrayHandle, TrayState};

#[derive(Debug, Clone, PartialEq)]
pub enum Link {
    /// No dongle/headset found (or no permission); carries the reason.
    Absent(String),
    /// Dongle present, headset off or out of range.
    HeadsetAway(DeviceKind),
    Ready(DeviceKind, HeadsetState),
}

pub struct DeviceModel {
    pub link: Link,
    pub worker: WorkerHandle,
    pub config: Config,
    pub last_error: Option<String>,
    /// Number of in-flight device jobs (for spinners).
    pub busy: usize,
    tray: Option<TrayHandle>,
    alerts: BatteryAlerts,
    fake_battery: Option<u8>,
    _events: Task<()>,
}

impl DeviceModel {
    pub fn new(
        worker: WorkerHandle,
        events: async_channel::Receiver<DeviceEvent>,
        config: Config,
        tray: Option<TrayHandle>,
        fake_battery: Option<u8>,
        cx: &mut Context<Self>,
    ) -> Self {
        let task = cx.spawn(async move |this, cx| {
            while let Ok(ev) = events.recv().await {
                if this.update(cx, |m, cx| m.on_event(ev, cx)).is_err() {
                    break;
                }
            }
        });
        DeviceModel {
            link: Link::Absent("searching…".into()),
            worker,
            config,
            last_error: None,
            busy: 0,
            tray,
            alerts: BatteryAlerts::default(),
            fake_battery,
            _events: task,
        }
    }

    pub fn headset(&self) -> Option<&HeadsetState> {
        match &self.link {
            Link::Ready(_, h) => Some(h),
            _ => None,
        }
    }

    fn on_event(&mut self, ev: DeviceEvent, cx: &mut Context<Self>) {
        let was_ready = self.headset().is_some();
        self.link = match ev {
            DeviceEvent::Connected { kind, .. } => Link::HeadsetAway(kind),
            DeviceEvent::Disconnected { reason } => Link::Absent(reason),
            DeviceEvent::Status(s) => match s.headset {
                Some(mut h) => {
                    if let Some(b) = self.fake_battery {
                        h.battery_pct = b;
                    }
                    Link::Ready(s.kind, h)
                }
                None => Link::HeadsetAway(s.kind),
            },
        };

        let ready = self.headset().is_some();
        if ready != was_ready && !matches!(self.link, Link::Absent(_)) {
            notify::show(if ready { "Headset connected".into() } else { "Headset disconnected".into() }, false);
        }
        if let Some(h) = self.headset()
            && let Some((msg, urgent)) =
                self.alerts.update(h.battery_pct, self.config.low_battery, self.config.critical_battery)
        {
            notify::show(msg, urgent);
        }
        self.sync_tray();
        cx.notify();
    }

    fn sync_tray(&self) {
        let Some(tray) = &self.tray else { return };
        let st = TrayState {
            dongle: !matches!(self.link, Link::Absent(_)),
            headset: self.headset().is_some(),
            battery: self.headset().map(|h| h.battery_pct),
            enc: self.headset().is_some_and(|h| h.enc_on),
            lights: self.headset().is_some_and(|h| h.lighting_slot != 0),
            eq_slot: self.headset().map_or(0, |h| h.eq_slot),
        };
        tray.update(|t| {
            if t.state != st {
                t.state = st;
            }
        });
    }

    /// Run a blocking device operation on the worker; errors land in `last_error`.
    pub fn run<T, F, D>(&mut self, cx: &mut Context<Self>, f: F, done: D)
    where
        T: Send + 'static,
        F: FnOnce(&Scape) -> scape_hid::Result<T> + Send + 'static,
        D: FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    {
        let worker = self.worker.clone();
        self.busy += 1;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let r = worker.call(f).await;
            let _ = this.update(cx, |m, cx| {
                m.busy -= 1;
                match r {
                    Ok(v) => {
                        m.last_error = None;
                        done(m, v, cx);
                    }
                    Err(e) => m.last_error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn exec<F>(&mut self, cx: &mut Context<Self>, f: F)
    where
        F: FnOnce(&Scape) -> scape_hid::Result<()> + Send + 'static,
    {
        self.run(cx, f, |_, _, _| {});
    }

    pub fn set_lights(&mut self, on: bool, cx: &mut Context<Self>) {
        self.exec(cx, move |d| d.set_lights(on));
    }

    pub fn set_enc(&mut self, on: bool, cx: &mut Context<Self>) {
        self.exec(cx, move |d| d.set_enc(on));
    }

    pub fn select_eq(&mut self, slot: u8, cx: &mut Context<Self>) {
        self.exec(cx, move |d| d.select_eq(slot));
    }

    pub fn set_sidetone(&mut self, on: bool, level: Option<u8>, cx: &mut Context<Self>) {
        self.exec(cx, move |d| d.set_sidetone(on, level));
    }
}
