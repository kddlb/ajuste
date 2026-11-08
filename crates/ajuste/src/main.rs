//! ajuste — Fractal Scape control panel with tray icon and battery alerts.

mod config;
mod curve;
mod eq_editor;
mod main_view;
mod model;
mod notify;
mod tray;

use gpui_kit::*;

use crate::config::Config;
use crate::main_view::MainView;
use crate::model::DeviceModel;
use crate::tray::AppMsg;

struct Args {
    hidden: bool,
    fake_battery: Option<u8>,
}

fn parse_args() -> Args {
    let mut args = Args {
        hidden: false,
        fake_battery: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--hidden" => args.hidden = true,
            "--fake-battery" => args.fake_battery = it.next().and_then(|v| v.parse().ok()),
            "-h" | "--help" => {
                println!("ajuste [--hidden] [--fake-battery PCT]");
                std::process::exit(0);
            }
            other => eprintln!("ignoring unknown argument {other:?}"),
        }
    }
    args
}

/// Handle to the (single) main window, if open.
#[derive(Default)]
struct MainWindow(Option<AnyWindowHandle>);
impl Global for MainWindow {}

fn open_main(model: Entity<DeviceModel>, cx: &mut App) {
    if let Some(h) = cx.global::<MainWindow>().0
        && h.update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(600.), px(760.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("ajuste — Fractal Scape".into()),
            ..Default::default()
        }),
        app_id: Some("ajuste".into()),
        ..Default::default()
    };
    match gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| MainView::new(model, window, cx))
    }) {
        Ok((handle, _)) => cx.global_mut::<MainWindow>().0 = Some(handle),
        Err(e) => log::error!("failed to open window: {e}"),
    }
}

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,ajuste=info,scape_hid=info"),
    )
    .init();
    let args = parse_args();
    let config = Config::load();
    let hidden = args.hidden || config.start_hidden;

    let (worker, events) = scape_hid::Worker::spawn();
    let (app_tx, app_rx) = async_channel::unbounded::<AppMsg>();
    let tray = tray::spawn(app_tx);
    let has_tray = tray.is_some();

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            cx.set_global(MainWindow::default());

            let model =
                cx.new(|cx| DeviceModel::new(worker, events, config, tray, args.fake_battery, cx));

            // Without a tray there is no way back once the window closes, so quit then.
            cx.on_window_closed(move |cx, _| {
                if !has_tray && cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            // Tray → app messages.
            let m = model.clone();
            cx.spawn(async move |cx| {
                while let Ok(msg) = app_rx.recv().await {
                    let m = m.clone();
                    cx.update(|cx| match msg {
                        AppMsg::OpenWindow => open_main(m, cx),
                        AppMsg::SetEnc(on) => m.update(cx, |m, cx| m.set_enc(on, cx)),
                        AppMsg::SetLights(on) => m.update(cx, |m, cx| m.set_lights(on, cx)),
                        AppMsg::SelectEq(slot) => m.update(cx, |m, cx| m.select_eq(slot, cx)),
                        AppMsg::Quit => cx.quit(),
                    });
                }
            })
            .detach();

            if !hidden || !has_tray {
                open_main(model, cx);
            }
        });
}
