//! Our renderers must reproduce, byte for byte, the uploads Adjust Pro sent to a real headset
//! (captured from WebHID; `fixtures/*.bin` are the concatenated `A4 02` payloads).

use scape_proto::effects::{EffectSpec, Pattern};

fn hex(c: &str) -> [u8; 3] {
    let v = u32::from_str_radix(c.trim_start_matches('#'), 16).unwrap();
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

fn check(spec: EffectSpec, fixture: &[u8]) {
    let ours = spec.upload_data();
    assert_eq!(ours.len(), fixture.len(), "length");
    if let Some(i) = ours.iter().zip(fixture).position(|(a, b)| a != b) {
        let (frame, at) = if i < 48 {
            (None, i)
        } else {
            (Some((i - 48) / 36), (i - 48) % 36)
        };
        panic!(
            "first difference at byte {i} (frame {frame:?}, offset {at}): ours {:02x?} vs app {:02x?}",
            &ours[i.saturating_sub(3)..(i + 6).min(ours.len())],
            &fixture[i.saturating_sub(3)..(i + 6).min(fixture.len())]
        );
    }
    // Checksum the app sent in A4 01 for these uploads.
    let sum = ours.iter().fold(0u8, |a, &b| a.wrapping_add(b));
    let expected = fixture.iter().fold(0u8, |a, &b| a.wrapping_add(b));
    assert_eq!(sum, expected);
}

#[test]
fn still_fractal_blue() {
    check(
        EffectSpec {
            preset_id: 6,
            pattern: Pattern::Still(hex("#0074C5")),
            speed: 100,
            brightness: 100,
        },
        include_bytes!("fixtures/still_fractal_blue.bin"),
    );
}

#[test]
fn shift_sunset() {
    let c = [
        "#C60000", "#C60000", "#E55F00", "#E55F00", "#C10016", "#F400A3",
    ]
    .map(hex);
    check(
        EffectSpec {
            preset_id: 2,
            pattern: Pattern::Shift(c),
            speed: 75,
            brightness: 100,
        },
        include_bytes!("fixtures/shift_sunset.bin"),
    );
}

#[test]
fn two_color_fade_pink_sapphire() {
    check(
        EffectSpec {
            preset_id: 14,
            pattern: Pattern::TwoColorFade(hex("#FF0059"), hex("#000000")),
            speed: 75,
            brightness: 100,
        },
        include_bytes!("fixtures/twocolor_pink_sapphire.bin"),
    );
}

#[test]
fn upload_framing() {
    let spec = EffectSpec {
        preset_id: 6,
        pattern: Pattern::Still(hex("#0074C5")),
        speed: 100,
        brightness: 100,
    };
    let cmds = spec.upload_commands();
    let first = cmds[0].encode().unwrap();
    // Captured: A4 01 01 9f 00 03 (slot 1, sum 0x9f, len 768).
    assert_eq!(&first[1..7], &[0xA4, 0x01, 0x01, 0x9f, 0x00, 0x03]);
    assert_eq!(cmds.len(), 1 + 768 / 60 + 1 + 2);
    let second = cmds[1].encode().unwrap();
    assert_eq!(
        &second[1..10],
        &[0xA4, 0x02, 0x3c, 0xa5, 0x5a, 0x14, 0xff, 0x14, 0x01]
    );
    let tail: Vec<_> = cmds[cmds.len() - 2..]
        .iter()
        .map(|c| c.encode().unwrap()[1..4].to_vec())
        .collect();
    assert_eq!(tail, vec![vec![0xA4, 0x03, 0x00], vec![0xA4, 0x04, 0x01]]);
}
