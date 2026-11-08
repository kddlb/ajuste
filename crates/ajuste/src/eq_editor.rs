//! Parametric EQ editor for the headset's three EQ slots.

use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderScale, SliderState};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme, Disableable, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::*;
use scape_proto::eq::{self, CommitMode, EqPoint, EqProfile, FilterType};

use crate::config::UserPreset;
use crate::curve::response_curve;
use crate::model::DeviceModel;

/// Debounce for live (non-persistent) previews while dragging.
const LIVE_DEBOUNCE: Duration = Duration::from_millis(150);

struct Band {
    freq: Entity<SliderState>,
    gain: Entity<SliderState>,
    q: Entity<SliderState>,
}

pub struct EqEditor {
    model: Entity<DeviceModel>,
    slot: u8,
    profile: EqProfile,
    /// UID of the preset the profile came from; cleared once edited.
    preset_uid: Option<String>,
    status: Option<String>,
    loading: bool,
    bands: Vec<Band>,
    live_task: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl EqEditor {
    pub fn new(model: Entity<DeviceModel>, cx: &mut Context<Self>) -> Self {
        let mut this = EqEditor {
            model,
            slot: 1,
            profile: EqProfile::default(),
            preset_uid: None,
            status: None,
            loading: false,
            bands: Vec::new(),
            live_task: None,
            _subs: Vec::new(),
        };
        this.load_slot(1, false, cx);
        this
    }

    fn worker(&self, cx: &App) -> scape_hid::WorkerHandle {
        self.model.read(cx).worker.clone()
    }

    fn connected(&self, cx: &App) -> bool {
        self.model.read(cx).headset().is_some()
    }

    /// Read a slot from the headset. With `reapply`, also push the stored profile back as a
    /// live update, undoing any live previews still playing.
    fn load_slot(&mut self, slot: u8, reapply: bool, cx: &mut Context<Self>) {
        self.slot = slot;
        self.loading = true;
        self.status = None;
        let worker = self.worker(cx);
        cx.spawn(async move |this, cx| {
            let r = worker
                .call(move |d| {
                    let s = d.read_eq(slot)?;
                    if reapply {
                        let uid = s.preset_uid.clone().unwrap_or_default();
                        d.apply_eq(slot, &s.profile, CommitMode::Live, &uid)?;
                    }
                    Ok(s)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                if this.slot != slot {
                    return;
                }
                match r {
                    Ok(s) => {
                        if s.factory_copy {
                            this.status =
                                Some("Slot is empty; showing the factory default.".into());
                        }
                        this.set_profile(s.profile, s.preset_uid, cx);
                    }
                    Err(e) => {
                        this.status = Some(format!("Could not read slot {slot}: {e}"));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn set_profile(&mut self, profile: EqProfile, uid: Option<String>, cx: &mut Context<Self>) {
        self.profile = profile;
        self.preset_uid = uid;
        self.rebuild_bands(cx);
        cx.notify();
    }

    fn rebuild_bands(&mut self, cx: &mut Context<Self>) {
        self._subs.clear();
        self.bands = self
            .profile
            .points
            .clone()
            .into_iter()
            .enumerate()
            .map(|(i, p)| {
                let mk = |min: f32, max: f32, step: f32, scale, v: f32, cx: &mut Context<Self>| {
                    cx.new(|_| {
                        SliderState::new()
                            .min(min)
                            .max(max)
                            .step(step)
                            .scale(scale)
                            .default_value(v)
                    })
                };
                let (gmin, gmax) = eq::GAIN_RANGE;
                let band = Band {
                    freq: mk(
                        20.0,
                        20_000.0,
                        1.0,
                        SliderScale::Logarithmic,
                        p.freq as f32,
                        cx,
                    ),
                    gain: mk(gmin, gmax, 0.1, SliderScale::Linear, p.gain, cx),
                    q: mk(0.5, 10.0, 0.01, SliderScale::Logarithmic, p.q, cx),
                };
                for (which, s) in [(0, &band.freq), (1, &band.gain), (2, &band.q)] {
                    self._subs
                        .push(cx.subscribe(s, move |this, _, ev: &SliderEvent, cx| {
                            let (SliderEvent::Change(v) | SliderEvent::Release(v)) = ev;
                            let v = v.start();
                            if let Some(pt) = this.profile.points.get_mut(i) {
                                match which {
                                    0 => pt.freq = v.round() as u16,
                                    1 => pt.gain = (v * 10.0).round() / 10.0,
                                    _ => pt.q = (v * 100.0).round() / 100.0,
                                }
                                *pt = pt.clamped();
                            }
                            this.edited(cx);
                        }));
                }
                band
            })
            .collect();
    }

    /// Called after any user edit.
    fn edited(&mut self, cx: &mut Context<Self>) {
        self.preset_uid = None;
        if self.connected(cx) {
            self.schedule_live(cx);
        }
        cx.notify();
    }

    fn schedule_live(&mut self, cx: &mut Context<Self>) {
        let worker = self.worker(cx);
        let (slot, profile) = (self.slot, self.profile.clone());
        let active = self.model.read(cx).headset().map(|h| h.eq_slot);
        // Dropping the previous task cancels it: only the last edit in the window is sent.
        self.live_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LIVE_DEBOUNCE).await;
            let r = worker
                .call(move |d| {
                    if active != Some(slot) {
                        d.select_eq(slot)?;
                    }
                    d.apply_eq(slot, &profile, CommitMode::Live, "live")
                })
                .await;
            if let Err(e) = r {
                let _ = this.update(cx, |this, cx| {
                    this.status = Some(format!("Live preview failed: {e}"));
                    cx.notify();
                });
            }
        }));
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let worker = self.worker(cx);
        let profile = self.profile.clone();
        let slot = self.slot;
        let uid = self
            .preset_uid
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        self.preset_uid = Some(uid.clone());
        self.status = Some("Saving…".into());
        cx.spawn(async move |this, cx| {
            let r = worker
                .call(move |d| d.apply_eq(slot, &profile, CommitMode::Save, &uid))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.status = Some(match r {
                    Ok(()) => "Saved to headset.".into(),
                    Err(e) => format!("Save failed: {e}"),
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn save_as_preset(&mut self, cx: &mut Context<Self>) {
        let points = self.profile.points.clone();
        let uid = uuid::Uuid::new_v4().to_string();
        self.preset_uid = Some(uid.clone());
        self.model.update(cx, |m, _| {
            let name = format!("Custom {}", m.config.presets.len() + 1);
            m.config.presets.push(UserPreset { name, uid, points });
            m.config.save();
        });
        cx.notify();
    }

    fn add_band(&mut self, cx: &mut Context<Self>) {
        if self.profile.points.len() >= eq::MAX_POINTS {
            return;
        }
        self.profile
            .points
            .push(EqPoint::new(FilterType::Peak, 1000, 0.0, 1.0));
        self.rebuild_bands(cx);
        self.edited(cx);
    }

    fn remove_band(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.profile.points.len() {
            self.profile.points.remove(i);
            self.rebuild_bands(cx);
            self.edited(cx);
        }
    }

    fn set_filter(&mut self, i: usize, f: FilterType, cx: &mut Context<Self>) {
        if let Some(p) = self.profile.points.get_mut(i) {
            p.filter = f;
            self.edited(cx);
        }
    }

    fn render_presets(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = h_flex().gap_1().flex_wrap();
        let current = self.preset_uid.clone();
        {
            {
                for p in eq::PRESETS {
                    let profile = p.profile();
                    let uid = p.uid.to_string();
                    row = row.child(
                        Button::new(SharedString::from(format!("preset-{}", p.name)))
                            .label(p.name)
                            .small()
                            .selected(current.as_deref() == Some(p.uid))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_profile(profile.clone(), Some(uid.clone()), cx);
                                if this.connected(cx) {
                                    this.schedule_live(cx);
                                }
                            })),
                    );
                }
                for p in self.model.read(cx).config.presets.clone() {
                    let profile = EqProfile {
                        points: p.points.clone(),
                    };
                    let selected = current.as_deref() == Some(p.uid.as_str());
                    row = row.child(
                        Button::new(SharedString::from(format!("user-{}", p.uid)))
                            .label(p.name.clone())
                            .small()
                            .selected(selected)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_profile(profile.clone(), Some(p.uid.clone()), cx);
                                if this.connected(cx) {
                                    this.schedule_live(cx);
                                }
                            })),
                    );
                }
            }
        }
        row
    }

    fn render_band(&self, i: usize, band: &Band, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.profile.points[i];
        let muted = cx.theme().muted_foreground;
        let types: &[FilterType] = &FilterType::EDITOR;
        let mut type_row = h_flex().gap_1();
        for &t in types {
            type_row = type_row.child(
                Button::new(SharedString::from(format!("band-{i}-type-{}", t as u8)))
                    .label(t.label())
                    .xsmall()
                    .selected(p.filter == t)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_filter(i, t, cx))),
            );
        }
        let row = |label: &'static str, value: String, s: &Entity<SliderState>| {
            h_flex()
                .gap_2()
                .child(div().w(px(44.)).text_sm().text_color(muted).child(label))
                .child(Slider::new(s).flex_1())
                .child(div().w(px(72.)).text_sm().text_right().child(value))
        };
        v_flex()
            .gap_1()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!("Band {}", i + 1)),
                            )
                            .child(type_row),
                    )
                    .child(
                        Button::new(SharedString::from(format!("band-{i}-remove")))
                            .label("Remove")
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(move |this, _, _, cx| this.remove_band(i, cx))),
                    ),
            )
            .child(row("Freq", format!("{} Hz", p.freq), &band.freq))
            .child(row("Gain", format!("{:+.1} dB", p.gain), &band.gain))
            .child(row("Q", format!("{:.2}", p.q), &band.q))
    }
}

impl Render for EqEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let connected = self.connected(cx);
        let mut root = v_flex().gap_3();

        {
            let active = self.model.read(cx).headset().map(|h| h.eq_slot);
            root = root.child(
                h_flex()
                    .gap_2()
                    .child(
                        TabBar::new("eq-slot-tabs")
                            .segmented()
                            .selected_index(self.slot as usize - 1)
                            .children((1..=eq::SLOTS).map(|s| {
                                Tab::new().label(if active == Some(s) {
                                    format!("Slot {s} ●")
                                } else {
                                    format!("Slot {s}")
                                })
                            }))
                            .on_click(cx.listener(|this, i: &usize, _, cx| {
                                this.load_slot(*i as u8 + 1, false, cx)
                            })),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("● = active on headset"),
                    ),
            );
        }

        root = root.child(self.render_presets(cx)).child(
            div()
                .rounded_md()
                .bg(cx.theme().secondary)
                .p_2()
                .child(response_curve(self.profile.points.clone(), cx)),
        );

        let bands: Vec<_> = (0..self.bands.len())
            .map(|i| {
                let band = &self.bands[i];
                self.render_band(i, band, cx).into_any_element()
            })
            .collect();
        root = root.children(bands);

        let can_add = self.profile.points.len() < eq::MAX_POINTS;
        let mut actions = h_flex()
            .gap_2()
            .child(
                Button::new("add-band")
                    .label("Add band")
                    .small()
                    .disabled(!can_add)
                    .on_click(cx.listener(|this, _, _, cx| this.add_band(cx))),
            )
            .child(div().flex_1())
            .child(
                Button::new("save")
                    .primary()
                    .small()
                    .label(format!("Save to slot {}", self.slot))
                    .disabled(!connected || self.loading)
                    .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
            );
        actions = actions
            .child(
                Button::new("save-preset")
                    .small()
                    .label("Save as preset")
                    .on_click(cx.listener(|this, _, _, cx| this.save_as_preset(cx))),
            )
            .child(
                Button::new("revert")
                    .small()
                    .ghost()
                    .label("Revert")
                    .disabled(!connected)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let slot = this.slot;
                        this.live_task = None;
                        this.load_slot(slot, true, cx)
                    })),
            );
        root = root.child(actions);

        let hint = if self.loading {
            Some("Reading slot…".to_string())
        } else if connected {
            Some("Edits play live on the headset; they are kept only after Save.".to_string())
        } else {
            None
        };
        for line in [self.status.clone(), hint].into_iter().flatten() {
            root = root.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(line),
            );
        }
        root
    }
}
