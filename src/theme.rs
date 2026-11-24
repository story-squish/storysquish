use std::fmt;

use eframe::egui::Color32;
use serde::{Deserialize, Serialize};

use crate::model::Status;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub dark: bool,
    pub board: Color32,
    pub header: Color32,
    pub header_text: Color32,
    pub ink: Color32,
    pub muted: Color32,
    pub card: Color32,
    pub line: Color32,
    pub sidebar: Color32,
    pub inspector: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub danger: Color32,
    pub act: Color32,
    /// Badge fills in `Status::ALL` order.
    pub statuses: [Color32; 5],
}

impl Palette {
    pub fn status(&self, status: Status) -> Color32 {
        let index = Status::ALL
            .iter()
            .position(|&s| s == status)
            .unwrap_or_default();
        self.statuses[index]
    }

    pub fn field_background(&self) -> Color32 {
        if self.dark { self.card } else { Color32::WHITE }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ColorScheme {
    #[default]
    Parchment,
    Linen,
    Sage,
    Harbor,
    Rosewood,
    Lavender,
    Slate,
    Midnight,
    Ember,
    ForestNight,
}

impl ColorScheme {
    pub const ALL: [Self; 10] = [
        Self::Parchment,
        Self::Linen,
        Self::Sage,
        Self::Harbor,
        Self::Rosewood,
        Self::Lavender,
        Self::Slate,
        Self::Midnight,
        Self::Ember,
        Self::ForestNight,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Parchment => "Parchment",
            Self::Linen => "Linen",
            Self::Sage => "Sage",
            Self::Harbor => "Harbor",
            Self::Rosewood => "Rosewood",
            Self::Lavender => "Lavender",
            Self::Slate => "Slate",
            Self::Midnight => "Midnight",
            Self::Ember => "Ember",
            Self::ForestNight => "Forest Night",
        }
    }

    #[expect(
        clippy::unreadable_literal,
        reason = "one flat table of #rrggbb color codes reads best"
    )]
    pub fn palette(self) -> Palette {
        match self {
            Self::Parchment => light(
                [0xc9b79c, 0x3a2c1e, 0xf4ead6, 0x2b2118, 0x7a6a52, 0xfdfaf2],
                [0xd8cdb6, 0xf2ecdd, 0xfaf6ec, 0x8a5a2b, 0xb04030, 0xcfe0e8],
                [0xcfe0e8, 0xe8dcc0, 0xf3e0c0, 0xd5ecd0, 0xe2e2e2],
            ),
            Self::Linen => light(
                [0xd6d3cc, 0x33363b, 0xf1efe9, 0x24262a, 0x6b6e74, 0xffffff],
                [0xd9d6cf, 0xefede8, 0xf7f6f2, 0x3f6e8c, 0xb3413a, 0xdbe6ee],
                [0xdbe6ee, 0xece4d2, 0xf2dfc8, 0xd8ead6, 0xe4e4e4],
            ),
            Self::Sage => light(
                [0xa9b8a0, 0x2f3d2c, 0xeef2e6, 0x1f2a1d, 0x5d6b58, 0xfbfcf7],
                [0xcdd6c4, 0xe9eee2, 0xf4f7ef, 0x4c7a3d, 0xa8453a, 0xd6e4d0],
                [0xd3e2ea, 0xe8e2c4, 0xf0dcc0, 0xcfe8c8, 0xe0e2dc],
            ),
            Self::Harbor => light(
                [0x9fb4c7, 0x1e2f40, 0xe8f0f7, 0x18232e, 0x586a7c, 0xfbfdff],
                [0xcbd7e2, 0xe6edf4, 0xf2f6fa, 0x2f6690, 0xb03f3f, 0xd3e3f0],
                [0xd3e3f0, 0xe6e0cc, 0xf0dcc4, 0xd2eadb, 0xe1e4e8],
            ),
            Self::Rosewood => light(
                [0xc7a9a6, 0x3d2428, 0xf7e9ea, 0x2e1a1d, 0x7a5c60, 0xfffafa],
                [0xe2cfd0, 0xf4e9e9, 0xfbf4f4, 0x9c3d54, 0xb3402e, 0xe9d6e4],
                [0xdcdcef, 0xefe0cc, 0xf5d9cc, 0xd9ead3, 0xe6e1e2],
            ),
            Self::Lavender => light(
                [0xb3aac8, 0x2c2640, 0xeeeaf7, 0x221d30, 0x655d7a, 0xfdfcff],
                [0xd8d2e6, 0xece8f4, 0xf6f4fa, 0x6a4fa3, 0xb0434a, 0xdcd6f0],
                [0xd6dcf2, 0xece2cf, 0xf2dccb, 0xd6ebd8, 0xe3e1e8],
            ),
            Self::Slate => dark(
                [0x2b3036, 0x1b1e22, 0xe6e8eb, 0xe4e6e9, 0x9aa1a9, 0x3a4048],
                [0x4c535c, 0x23272c, 0x262a30, 0x6ea8d8, 0xc0503f, 0x2f4a5c],
                [0x2f4a5c, 0x524a33, 0x5c4630, 0x2f5238, 0x474b51],
            ),
            Self::Midnight => dark(
                [0x141b2d, 0x0b1020, 0xdde4f5, 0xe1e7f5, 0x8b96b3, 0x1f2840],
                [0x323d5c, 0x111829, 0x151d31, 0x8ca8ff, 0xc4574b, 0x2a3a66],
                [0x2a3a66, 0x4d4530, 0x5a4030, 0x24503e, 0x3a3f4f],
            ),
            Self::Ember => dark(
                [0x2a201b, 0x17110e, 0xf2e3d3, 0xf0e2d2, 0xab9682, 0x3a2d26],
                [0x54433a, 0x211915, 0x251c18, 0xe08a4a, 0xc44a3a, 0x3d4650],
                [0x3a4a58, 0x5a4a2e, 0x66442c, 0x3a5030, 0x4a423d],
            ),
            Self::ForestNight => dark(
                [0x1c2620, 0x101712, 0xdfeadf, 0xe0ebe0, 0x92a596, 0x28352c],
                [0x3b4b3f, 0x172019, 0x1a241d, 0x7cc48a, 0xbf5646, 0x2d4a4f],
                [0x2d4a4f, 0x4f4a2c, 0x5a442c, 0x2f5a35, 0x3c4540],
            ),
        }
    }
}

impl fmt::Display for ColorScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// `surfaces`: board, header, header text, ink, muted, card.
/// `chrome`: line, sidebar, inspector, accent, danger, act badge.
fn light(surfaces: [u32; 6], chrome: [u32; 6], statuses: [u32; 5]) -> Palette {
    build(false, surfaces, chrome, statuses)
}

fn dark(surfaces: [u32; 6], chrome: [u32; 6], statuses: [u32; 5]) -> Palette {
    build(true, surfaces, chrome, statuses)
}

fn build(dark: bool, surfaces: [u32; 6], chrome: [u32; 6], statuses: [u32; 5]) -> Palette {
    let [board, header, header_text, ink, muted, card] = surfaces.map(hex);
    let [line, sidebar, inspector, accent, danger, act] = chrome.map(hex);
    Palette {
        dark,
        board,
        header,
        header_text,
        ink,
        muted,
        card,
        line,
        sidebar,
        inspector,
        accent,
        // Dark schemes use light accents, which need dark text on top.
        on_accent: if dark { header } else { Color32::WHITE },
        danger,
        act,
        statuses: statuses.map(hex),
    }
}

const fn hex(rgb: u32) -> Color32 {
    let [_, r, g, b] = rgb.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Color32) -> f32 {
        let channel = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (hi, lo) = {
            let (la, lb) = (luminance(a), luminance(b));
            if la > lb { (la, lb) } else { (lb, la) }
        };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn every_scheme_has_a_distinct_name() {
        let names: std::collections::BTreeSet<_> =
            ColorScheme::ALL.iter().map(|s| s.name()).collect();
        assert_eq!(names.len(), 10);
    }

    #[test]
    fn text_is_readable_on_every_surface() {
        for scheme in ColorScheme::ALL {
            let p = scheme.palette();
            let pairs = [
                ("ink on card", p.ink, p.card),
                ("ink on sidebar", p.ink, p.sidebar),
                ("ink on inspector", p.ink, p.inspector),
                ("header text on header", p.header_text, p.header),
                ("on-accent on accent", p.on_accent, p.accent),
                ("ink on act badge", p.ink, p.act),
                ("muted on card", p.muted, p.card),
                ("white on danger", Color32::WHITE, p.danger),
            ];
            for (what, fg, bg) in pairs {
                let ratio = contrast(fg, bg);
                assert!(ratio >= 4.0, "{scheme}: {what} contrast {ratio:.2}");
            }
            for status in Status::ALL {
                let ratio = contrast(p.ink, p.status(status));
                assert!(
                    ratio >= 4.5,
                    "{scheme}: ink on {status} contrast {ratio:.2}"
                );
            }
        }
    }

    #[test]
    fn parchment_is_the_default() {
        assert_eq!(ColorScheme::default(), ColorScheme::Parchment);
    }
}
