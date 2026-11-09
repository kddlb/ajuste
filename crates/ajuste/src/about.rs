//! About dialog: version, project info and the connected devices' firmware.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::*;

use crate::model::DeviceModel;

pub const REPO_URL: &str = "https://github.com/kddlb/ajuste";

pub fn version() -> String {
    let hash = env!("AJUSTE_GIT_HASH");
    if hash.is_empty() {
        env!("CARGO_PKG_VERSION").to_string()
    } else {
        format!("{} ({hash})", env!("CARGO_PKG_VERSION"))
    }
}

fn row(label: &'static str, value: String, muted: Hsla) -> impl IntoElement {
    h_flex()
        .gap_3()
        .child(div().w(px(110.)).text_sm().text_color(muted).child(label))
        .child(div().text_sm().child(value))
}

/// Dialog body. Cheap to rebuild: it only reads already-cached model state.
pub fn content(model: &DeviceModel, cx: &App) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    let mut devices = v_flex().gap_1();
    let fmt = |i: &scape_proto::Info| {
        format!(
            "fw {} · S/N {}",
            i.fw_version.as_deref().unwrap_or("?"),
            i.serial.as_deref().unwrap_or("?")
        )
    };
    match (&model.dongle_info, &model.headset_info) {
        (None, None) => {
            devices = devices.child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("No headset connected."),
            )
        }
        (d, h) => {
            if let Some(d) = d {
                devices = devices.child(row("Dongle", fmt(d), muted));
            }
            if let Some(h) = h {
                devices = devices.child(row("Headset", fmt(h), muted));
            }
        }
    }

    v_flex()
        .gap_4()
        .child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xl()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Ajuste"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(format!("Version {}", version())),
                )
                .child(div().text_sm().child("Made by Kevin López Brante")),
        )
        .child(div().text_sm().child(
            "Native Linux control for the Fractal Scape wireless headset: battery, EQ, \
             lighting, sidetone and noise cancellation.",
        ))
        .child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(muted)
                        .child("CONNECTED DEVICES"),
                )
                .child(devices),
        )
        .child(div().text_xs().text_color(muted).child(
            "Unofficial project, not affiliated with or endorsed by Fractal Design. \
                 Licensed under MIT OR Apache-2.0.",
        ))
        .child(
            h_flex().child(
                Button::new("about-repo")
                    .label("Source code on GitHub")
                    .small()
                    .ghost()
                    .on_click(|_, _, cx| cx.open_url(REPO_URL)),
            ),
        )
}
