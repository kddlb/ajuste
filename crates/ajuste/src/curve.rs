//! Frequency-response plot for an EQ profile.

use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use scape_proto::biquad::{self, SampleRate};
use scape_proto::EqPoint;

const F_MIN: f64 = 20.0;
const F_MAX: f64 = 20_000.0;
const DB_RANGE: f64 = 12.0;
const SAMPLES: usize = 160;

fn x_of(f: f64, b: &Bounds<Pixels>) -> Pixels {
    let t = (f / F_MIN).ln() / (F_MAX / F_MIN).ln();
    b.origin.x + b.size.width * t as f32
}

fn y_of(db: f64, b: &Bounds<Pixels>) -> Pixels {
    let t = (1.0 - (db.clamp(-DB_RANGE, DB_RANGE) + DB_RANGE) / (2.0 * DB_RANGE)) as f32;
    b.origin.y + b.size.height * t
}

/// A canvas drawing the summed biquad magnitude (48 kHz) with dB and decade grid lines.
pub fn response_curve(points: Vec<EqPoint>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let line = theme.primary;
    let grid = theme.border;
    let zero = theme.muted_foreground.opacity(0.5);

    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let hline = |db: f64, color: Hsla, window: &mut Window| {
                let y = y_of(db, &bounds);
                window.paint_quad(fill(
                    Bounds::new(point(bounds.origin.x, y), size(bounds.size.width, px(1.))),
                    color,
                ));
            };
            for db in [-12.0, -6.0, 6.0, 12.0] {
                hline(db, grid, window);
            }
            hline(0.0, zero, window);
            for f in [100.0, 1_000.0, 10_000.0] {
                let x = x_of(f, &bounds);
                window.paint_quad(fill(
                    Bounds::new(point(x, bounds.origin.y), size(px(1.), bounds.size.height)),
                    grid,
                ));
            }

            let mut path = PathBuilder::stroke(px(2.));
            for i in 0..=SAMPLES {
                let f = F_MIN * (F_MAX / F_MIN).powf(i as f64 / SAMPLES as f64);
                let db = if points.is_empty() { 0.0 } else { biquad::chain_db(&points, f, SampleRate::S48k) };
                let p = point(x_of(f, &bounds), y_of(db, &bounds));
                if i == 0 { path.move_to(p) } else { path.line_to(p) }
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, line);
            }
        },
    )
    .w_full()
    .h(px(160.))
}
