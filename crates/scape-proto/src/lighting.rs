//! Read-only decoding of the stored lighting effect (`A4 05 <slot> 00 00`).
//!
//! The reply payload is `A4 05 <status> <len> A5 5A <frames> FF <fps> <seconds> <metadata…>`,
//! where the metadata (42 bytes from offset 10) is
//! `[preset_id, mode, speed, (r g b)…, brightness, 0…]`. Verified on hardware with "Sunset".

/// Offset of the metadata in the reply payload (Adjust Pro `METADATA_OFFSET`).
pub const METADATA_OFFSET: usize = 10;
/// Adjust Pro `METADATA_LENGHT`.
pub const METADATA_LEN: usize = 42;

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Effect {
    pub frames: u8,
    pub fps: u8,
    pub seconds: u8,
    pub preset_id: u8,
    pub mode: u8,
    pub speed: u8,
    pub colors: Vec<[u8; 3]>,
    pub brightness: u8,
}

impl Effect {
    /// Parse a READ_EFFECT reply payload (report ID stripped). `None` if there is no
    /// recognisable effect header (e.g. an empty slot).
    pub fn parse(d: &[u8]) -> Option<Effect> {
        if d.get(4..6)? != [0xA5, 0x5A] {
            return None;
        }
        let m = d.get(METADATA_OFFSET..METADATA_OFFSET + METADATA_LEN)?;
        let mode = m[1];
        // Layout per Adjust Pro's effect classes: `[preset, mode, speed, colours…, extras…, brightness]`.
        let n = color_count(mode);
        let colors: Vec<[u8; 3]> = m[3..3 + 3 * n]
            .chunks(3)
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        let extras = match mode {
            8 => 1, // two-colour fade repeats the speed
            9 => 4, // radius: up/top/down wave + freq
            _ => 0,
        };
        let brightness = m[3 + 3 * n + extras];
        Some(Effect {
            frames: d[6],
            fps: d[8],
            seconds: d[9],
            preset_id: m[0],
            mode,
            speed: m[2],
            colors,
            brightness,
        })
    }

    pub fn preset_name(&self) -> Option<&'static str> {
        preset_name(self.preset_id)
    }

    pub fn mode_name(&self) -> &'static str {
        MODES.get(self.mode as usize).copied().unwrap_or("unknown")
    }

    /// e.g. "Sunset", falling back to the mode name for custom effects.
    pub fn label(&self) -> String {
        self.preset_name()
            .map(str::to_string)
            .unwrap_or_else(|| format!("Custom {}", self.mode_name()))
    }
}

/// Colours stored per mode (Adjust Pro `colorNumControl`).
pub fn color_count(mode: u8) -> usize {
    match mode {
        0 | 6 | 11 => 6,        // shift, lava lamp, torrent
        2 | 7 | 8 => 2,         // breathe, wave ripple, two-colour fade
        1 | 3 | 4 | 5 | 9 => 1, // still, meshify, fade in, instant, radius
        _ => 0,                 // off / unknown
    }
}

/// Animation modes (Adjust Pro `tn`).
pub const MODES: [&str; 12] = [
    "shift",
    "still",
    "breathe",
    "meshify",
    "fade in",
    "instant",
    "lava lamp",
    "wave ripple",
    "two-colour fade",
    "radius",
    "off",
    "torrent",
];

/// Built-in preset IDs (Adjust Pro `$n`) → display name.
pub fn preset_name(id: u8) -> Option<&'static str> {
    Some(match id {
        0 | 46 => "Northern lights",
        1 | 47 => "Summer sky",
        2 | 48 => "Sunset",
        3 | 50 => "Mystic night",
        4 | 49 => "Emerald lake",
        5 | 45 | 51 | 35 => "Radiant dawn",
        6 | 7 | 34 | 44 => "Fractal blue",
        16 => "Bonfire",
        17 => "Ocean",
        18 | 33 | 43 => "Pine",
        19 => "Twilight forest",
        20 => "Skyward",
        21 => "Misty fade",
        8 | 41 => "White",
        32 | 42 | 11 => "Campfire",
        9 | 10 => "Gray",
        12 => "Glistening ice",
        22 => "Rustling leaves",
        13 => "Starfall",
        23 => "Cedar smoke",
        24 => "Maple",
        25 => "Deep sea",
        14 => "Pink sapphire",
        15 => "Lunar mist",
        26 => "Nautical",
        27 => "Shoreline",
        28 => "Endless woodland",
        29 => "Azure horizon",
        30 => "Flowing spark",
        31 => "RGB off",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real reply captured from the user's headset (Sunset).
    const SUNSET: [u8; 63] = [
        0xa4, 0x05, 0x00, 0x3b, 0xa5, 0x5a, 0x4e, 0xff, 0x14, 0x04, 0x02, 0x00, 0x4b, 0xc6, 0x00,
        0x00, 0xc6, 0x00, 0x00, 0xe5, 0x5f, 0x00, 0xe5, 0x5f, 0x00, 0xc1, 0x00, 0x16, 0xf4, 0x00,
        0xa3, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc6, 0x00, 0x00, 0xc6, 0x00, 0x00, 0xc6, 0x00,
        0x00, 0xd6, 0x30,
    ];

    #[test]
    fn decodes_sunset() {
        let e = Effect::parse(&SUNSET).unwrap();
        assert_eq!(e.label(), "Sunset");
        assert_eq!(e.mode_name(), "shift");
        assert_eq!(
            (e.frames, e.fps, e.seconds, e.speed, e.brightness),
            (78, 20, 4, 75, 100)
        );
        assert_eq!(
            e.colors,
            vec![
                [0xc6, 0, 0],
                [0xc6, 0, 0],
                [0xe5, 0x5f, 0],
                [0xe5, 0x5f, 0],
                [0xc1, 0, 0x16],
                [0xf4, 0, 0xa3]
            ]
        );
    }

    /// Real reply: "Pink sapphire" two-colour fade (#FF0059 / #000000).
    const PINK_SAPPHIRE: [u8; 31] = [
        0xa4, 0x05, 0x00, 0x3b, 0xa5, 0x5a, 0x50, 0xff, 0x14, 0x04, 0x0e, 0x08, 0x4b, 0xff, 0x00,
        0x59, 0x00, 0x00, 0x00, 0x4b, 0x64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    #[test]
    fn decodes_two_colour_fade() {
        let mut d = PINK_SAPPHIRE.to_vec();
        d.resize(63, 0);
        let e = Effect::parse(&d).unwrap();
        assert_eq!(e.label(), "Pink sapphire");
        assert_eq!(e.mode_name(), "two-colour fade");
        assert_eq!(e.colors, vec![[0xff, 0, 0x59], [0, 0, 0]]);
        assert_eq!((e.speed, e.brightness, e.frames), (75, 100, 80));
    }

    #[test]
    fn empty_slot() {
        assert!(Effect::parse(&[0xa4, 0x05, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0]).is_none());
    }
}
