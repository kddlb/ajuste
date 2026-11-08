//! Main window: status header + Device / EQ tabs.

use gpui_kit::component::button::Button;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme, Disableable, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use scape_proto::effects;

use crate::eq_editor::EqEditor;
use crate::model::{DeviceModel, Link};

pub struct MainView {
    model: Entity<DeviceModel>,
    tab: usize,
    sidetone: Entity<SliderState>,
    brightness: Entity<SliderState>,
    headset_eq: Entity<EqEditor>,
    _subs: Vec<Subscription>,
}

impl MainView {
    pub fn new(model: Entity<DeviceModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sidetone = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(100.0)
                .step(10.0)
                .default_value(0.0)
        });
        let brightness = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(100.0)
                .step(5.0)
                .default_value(100.0)
        });
        let headset_eq = cx.new(|cx| EqEditor::new(model.clone(), cx));

        let subs = vec![
            // Keep the sidetone slider in sync with polled state unless the user is dragging.
            cx.observe_in(&model, window, |this, model, window, cx| {
                if let Some(h) = model.read(cx).headset() {
                    let pct = h.sidetone_pct() as f32;
                    if this.sidetone.read(cx).value().start() != pct {
                        this.sidetone
                            .update(cx, |s, cx| s.set_value(pct, window, cx));
                    }
                }
                if let Some(e) = &model.read(cx).effect {
                    let b = f32::from(e.brightness);
                    if this.brightness.read(cx).value().start() != b {
                        this.brightness
                            .update(cx, |s, cx| s.set_value(b, window, cx));
                    }
                }
                cx.notify();
            }),
            cx.subscribe(&sidetone, |this, _, ev: &SliderEvent, cx| {
                if let SliderEvent::Release(v) = ev {
                    let pct = v.start().round() as u8;
                    this.model
                        .update(cx, |m, cx| m.set_sidetone(true, Some(pct), cx));
                }
            }),
            // Re-upload the current preset at the new brightness when the slider is released.
            cx.subscribe(&brightness, |this, _, ev: &SliderEvent, cx| {
                if let SliderEvent::Release(v) = ev {
                    let b = v.start().round() as u8;
                    this.model.update(cx, |m, cx| {
                        let preset = m
                            .effect
                            .as_ref()
                            .and_then(|e| effects::PRESETS.iter().find(|p| p.id == e.preset_id));
                        if let Some(p) = preset {
                            m.set_effect(p.spec(b), cx);
                        }
                    });
                }
            }),
        ];
        MainView {
            model,
            tab: 0,
            sidetone,
            brightness,
            headset_eq,
            _subs: subs,
        }
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let m = self.model.read(cx);
        let theme = cx.theme();
        let (title, subtitle) = match &m.link {
            Link::Absent(reason) => ("Not connected".to_string(), reason.clone()),
            Link::HeadsetAway(kind) => (
                "Headset off or out of range".into(),
                format!("{kind:?} found"),
            ),
            Link::Ready(kind, h) => (
                format!("Battery {}%", h.battery_pct),
                format!(
                    "{:?} · mic {}{} · EQ slot {}",
                    kind,
                    if h.boom_mic_attached {
                        "boom"
                    } else {
                        "built-in"
                    },
                    if h.muted { " (muted)" } else { "" },
                    h.eq_slot
                ),
            ),
        };
        let battery = m.headset().map(|h| h.battery_pct);
        let color = match battery {
            Some(b) if b <= m.config.critical_battery => theme.danger,
            Some(b) if b <= m.config.low_battery => theme.warning,
            _ => theme.success,
        };
        let mut col = v_flex()
            .gap_1()
            .p_4()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(subtitle),
            );
        if let Some(b) = battery {
            col = col.child(Progress::new("battery").value(b as f32).color(color));
        }
        if let Some(err) = &m.last_error {
            col = col.child(div().text_sm().text_color(theme.danger).child(err.clone()));
        }
        col
    }

    fn render_presets(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let m = self.model.read(cx);
        let live = m.headset().is_some();
        let uploading = m.uploading;
        let current = m.effect.as_ref().map(|e| e.preset_id);
        let known = current.is_some_and(|id| effects::PRESETS.iter().any(|p| p.id == id));
        let muted = cx.theme().muted_foreground;
        let mut grid = h_flex().gap_1().flex_wrap();
        for p in effects::PRESETS {
            grid = grid.child(
                Button::new(SharedString::from(format!("light-{}", p.id)))
                    .label(p.name)
                    .xsmall()
                    .selected(current == Some(p.id))
                    .disabled(!live || uploading)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let b = this.brightness.read(cx).value().start().round() as u8;
                        this.model.update(cx, |m, cx| m.set_effect(p.spec(b), cx));
                    })),
            );
        }
        v_flex()
            .gap_2()
            .child(grid)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .w(px(70.))
                            .text_xs()
                            .text_color(muted)
                            .child("Brightness"),
                    )
                    .child(
                        Slider::new(&self.brightness)
                            .disabled(!live || uploading || !known)
                            .flex_1(),
                    )
                    .child(div().w(px(44.)).text_sm().text_right().child(format!(
                        "{}%",
                        self.brightness.read(cx).value().start().round()
                    ))),
            )
            .when(uploading, |d| {
                d.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("Uploading effect… (about 4 s)"),
                )
            })
    }

    fn render_device(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let m = self.model.read(cx);
        let h = m.headset().copied();
        let live = h.is_some();
        let enc = h.is_some_and(|h| h.enc_on);
        let lights = h.is_some_and(|h| h.lighting_slot != 0);
        let effect = m.effect.clone();
        let st_on = h.is_some_and(|h| h.sidetone_on);
        let muted = cx.theme().muted_foreground;
        let section = |title: &'static str, desc: &'static str| {
            v_flex()
                .gap_0p5()
                .flex_1()
                .child(div().font_weight(FontWeight::MEDIUM).child(title))
                .child(div().text_xs().text_color(muted).child(desc))
        };
        let active_slot = h.map(|h| h.eq_slot).unwrap_or(1).clamp(1, 3);

        v_flex()
            .gap_5()
            .child(
                h_flex()
                    .gap_4()
                    .child(section(
                        "Noise cancellation",
                        "ENC filters background noise from your mic.",
                    ))
                    .child(
                        Switch::new("enc")
                            .checked(enc)
                            .disabled(!live)
                            .on_click(cx.listener(|this, on: &bool, _, cx| {
                                let on = *on;
                                this.model.update(cx, |m, cx| m.set_enc(on, cx));
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_4()
                    .child(
                        v_flex()
                            .gap_0p5()
                            .flex_1()
                            .child(div().font_weight(FontWeight::MEDIUM).child("Lights"))
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(div().text_xs().text_color(muted).child(match &effect {
                                        Some(e) => format!("{} · {}", e.label(), e.mode_name()),
                                        None => "Headset LEDs (stored effect)".into(),
                                    }))
                                    .children(effect.iter().flat_map(|e| e.colors.clone()).map(
                                        |[r, g, b]| {
                                            div()
                                                .size(px(10.))
                                                .rounded_full()
                                                .bg(gpui_kit::rgb(u32::from_be_bytes([0, r, g, b])))
                                        },
                                    )),
                            ),
                    )
                    .child(
                        Switch::new("lights")
                            .checked(lights)
                            .disabled(!live)
                            .on_click(cx.listener(|this, on: &bool, _, cx| {
                                let on = *on;
                                this.model.update(cx, |m, cx| m.set_lights(on, cx));
                            })),
                    ),
            )
            .child(self.render_presets(cx))
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_4()
                            .child(section(
                                "Sidetone",
                                "Hear your own voice through the headset.",
                            ))
                            .child(
                                Switch::new("sidetone")
                                    .checked(st_on)
                                    .disabled(!live)
                                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                                        let on = *on;
                                        this.model.update(cx, |m, cx| m.set_sidetone(on, None, cx));
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Slider::new(&self.sidetone)
                                    .disabled(!live || !st_on)
                                    .flex_1(),
                            )
                            .child(
                                div()
                                    .w(px(44.))
                                    .text_sm()
                                    .text_right()
                                    .child(format!("{}%", h.map_or(0, |h| h.sidetone_pct()))),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .gap_4()
                    .child(section(
                        "Active EQ",
                        "Pick which of the three stored EQ slots plays.",
                    ))
                    .child(
                        TabBar::new("active-eq")
                            .segmented()
                            .selected_index(active_slot as usize - 1)
                            .children((1..=3).map(|s| Tab::new().label(format!("{s}"))))
                            .on_click(cx.listener(|this, i: &usize, _, cx| {
                                let slot = *i as u8 + 1;
                                this.model.update(cx, |m, cx| m.select_eq(slot, cx));
                            })),
                    ),
            )
    }
}

impl Render for MainView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body: AnyElement = match self.tab {
            0 => self.render_device(cx).into_any_element(),
            _ => self.headset_eq.clone().into_any_element(),
        };
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_header(cx))
            .child(
                div().px_4().pt_3().child(
                    TabBar::new("main-tabs")
                        .underline()
                        .selected_index(self.tab)
                        .child(Tab::new().label("Device"))
                        .child(Tab::new().label("EQ"))
                        .on_click(cx.listener(|this, i: &usize, _, cx| {
                            this.tab = *i;
                            cx.notify();
                        })),
                ),
            )
            .child(
                div()
                    .id("body")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_4()
                    .child(body),
            )
    }
}
