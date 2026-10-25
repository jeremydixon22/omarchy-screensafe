//! The theme palette packs draw with, read from a theme's colors.toml.

use crate::expr::parse_hex;

pub const CUSTOM_COLOR: u32 = 1 << 24;

/// A color choice is numeric data: 0 = original palette, 1..=N = theme swatch,
/// CUSTOM_COLOR + 24-bit RGB = custom. Gaps/fractions/non-finite values are invalid.
pub fn valid_choice(value: f64) -> bool {
    value.is_finite()
        && value.fract() == 0.0
        && ((0.0..=NAMES.len() as f64).contains(&value) || (CUSTOM_COLOR as f64..=(2 * CUSTOM_COLOR - 1) as f64).contains(&value))
}

fn linear(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn srgb(v: f64) -> f64 {
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}
fn luminance(c: [f64; 3]) -> f64 {
    c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
}

/// Tint by moving along black -> chosen hue -> white in linear light. Matching
/// relative luminance preserves light/dark detail and contrast against the background,
/// even for a dark blue, bright yellow, black or white selection.
fn tint(original: [f64; 4], selected: [f64; 4]) -> [f64; 4] {
    let target = luminance([linear(original[0]), linear(original[1]), linear(original[2])]);
    let ink = [linear(selected[0]), linear(selected[1]), linear(selected[2])];
    let middle = luminance(ink);
    let mut out = [0.0, 0.0, 0.0, original[3]];
    for i in 0..3 {
        let value = if target <= middle && middle > 0.0 {
            ink[i] * target / middle
        } else if middle < 1.0 {
            ink[i] + (1.0 - ink[i]) * (target - middle) / (1.0 - middle)
        } else {
            target
        };
        out[i] = srgb(value.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    }
    out
}

/// The color names every pack can use, with Omarchy's default (Tokyo Night) values for themes that leave one out.
/// A missing name falls back to another name's value where the theme has one (see `fallback`).
pub const NAMES: &[(&str, &str)] = &[
    ("background", "#1a1b26"),
    ("foreground", "#a9b1d6"),
    ("accent", "#7aa2f7"),
    ("muted", "#414868"),
    ("selection", "#292e42"),
    ("red", "#f7768e"),
    ("orange", "#eb927b"),
    ("yellow", "#e0af68"),
    ("green", "#9ece6a"),
    ("cyan", "#449dab"),
    ("blue", "#7aa2f7"),
    ("magenta", "#ad8ee6"),
    ("brown", "#75493d"),
    ("bright_foreground", "#c0caf5"),
    ("bright_red", "#ff7a93"),
    ("bright_yellow", "#ff9e64"),
    ("bright_green", "#b9f27c"),
    ("bright_cyan", "#0db9d7"),
    ("bright_blue", "#7da6ff"),
    ("bright_magenta", "#bb9af7"),
    ("dark_background", "#13141c"),
    ("darker_background", "#0e0e14"),
    ("lighter_background", "#24283b"),
    ("dark_foreground", "#565f89"),
    ("light_foreground", "#b4bee6"),
    ("black", "#000000"),
    ("white", "#ffffff"),
];

/// The name whose value stands in for a missing one.
fn fallback(name: &str) -> Option<&'static str> {
    Some(match name {
        "accent" => "blue",
        "muted" | "dark_foreground" => "foreground",
        "selection" => "lighter_background",
        "bright_foreground" | "light_foreground" => "foreground",
        "bright_red" => "red",
        "bright_yellow" => "yellow",
        "bright_green" => "green",
        "bright_cyan" => "cyan",
        "bright_blue" => "blue",
        "bright_magenta" => "magenta",
        "orange" => "yellow",
        "dark_background" | "darker_background" | "lighter_background" => "background",
        _ => return None,
    })
}

#[derive(Clone, Debug)]
pub struct Palette {
    pub colors: Vec<(&'static str, [f64; 4])>,
    /// Whether the theme is a light one: its `mode`, or, without one, whether its background is mostly light.
    pub light: bool,
}

impl Palette {
    pub fn choice(&self, value: f64) -> Option<[f64; 4]> {
        if !valid_choice(value) || value == 0.0 {
            return None;
        }
        let value = value as u32;
        if value < CUSTOM_COLOR {
            return self.colors.get(value as usize - 1).map(|(_, c)| *c);
        }
        let rgb = value - CUSTOM_COLOR;
        Some([((rgb >> 16) & 255) as f64 / 255.0, ((rgb >> 8) & 255) as f64 / 255.0, (rgb & 255) as f64 / 255.0, 1.0])
    }

    pub fn recolored(&self, value: f64) -> Self {
        let Some(ink) = self.choice(value) else {
            return self.clone();
        };
        let colors = self
            .colors
            .iter()
            .map(|(name, color)| {
                // Preserve the canvas and absolute black/white. Other inks retain their luminance.
                let c = if name.ends_with("background") || matches!(*name, "black" | "white") { *color } else { tint(*color, ink) };
                (*name, c)
            })
            .collect();
        Self { colors, light: self.light }
    }
    pub fn default_theme() -> Palette {
        Palette::from_toml("").expect("an empty palette parses")
    }

    /// Reads the known names from a colors.toml; anything else in it is ignored, and black and white are fixed.
    pub fn from_toml(text: &str) -> Result<Palette, String> {
        let table: toml::Table = text.parse().map_err(|e: toml::de::Error| format!("colors.toml: {}", e.message()))?;
        let given = |name: &str| -> Option<[f64; 4]> {
            if name == "black" || name == "white" {
                return None;
            }
            table.get(name).and_then(|v| v.as_str()).and_then(parse_hex)
        };
        let mut colors = Vec::with_capacity(NAMES.len());
        for (name, default) in NAMES {
            let value = given(name).or_else(|| fallback(name).and_then(given)).unwrap_or_else(|| parse_hex(default).expect("default colors are valid"));
            colors.push((*name, value));
        }
        let light = match table.get("mode").and_then(|v| v.as_str()) {
            Some("light") => true,
            Some("dark") => false,
            _ => {
                let bg = colors.iter().find(|(n, _)| *n == "background").map(|(_, c)| *c).unwrap_or([0.0; 4]);
                0.2126 * bg[0] + 0.7152 * bg[1] + 0.0722 * bg[2] > 0.5
            }
        };
        Ok(Palette { colors, light })
    }

    #[cfg(test)]
    pub fn get(&self, name: &str) -> Option<[f64; 4]> {
        self.colors.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_choices_are_exact_bounded_numbers_and_keep_theme_swatches_live() {
        let p = Palette::from_toml("accent = \"#123456\"\n").unwrap();
        assert!(p.choice(0.0).is_none());
        assert_eq!(p.choice(3.0), p.get("accent"));
        assert_eq!(p.choice((CUSTOM_COLOR + 0xff0000) as f64), Some([1.0, 0.0, 0.0, 1.0]));
        for value in [-1.0, 1.5, 28.0, 1000.0, (CUSTOM_COLOR - 1) as f64, (2 * CUSTOM_COLOR) as f64, f64::NAN, f64::INFINITY] {
            assert!(!valid_choice(value));
            assert!(p.choice(value).is_none());
        }
        assert_eq!(p.recolored(0.0).colors, p.colors);
    }

    #[test]
    fn recoloring_preserves_luminance_alpha_and_canvas_for_extreme_hues() {
        for theme in ["", "mode=\"light\"\nbackground=\"#ffffff\"\nforeground=\"#1b1d20\"\n"] {
            let original = Palette::from_toml(theme).unwrap();
            for rgb in [0, 0xffffff, 0x0000ff, 0xffff00, 0xff0000, 0x003344, 0x858585] {
                let changed = original.recolored((CUSTOM_COLOR + rgb) as f64);
                assert_eq!(changed.light, original.light);
                for ((name, a), (_, b)) in original.colors.iter().zip(&changed.colors) {
                    assert!(b.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
                    assert_eq!(a[3], b[3]);
                    if name.ends_with("background") || matches!(*name, "black" | "white") {
                        assert_eq!(a, b);
                    }
                    let y = |c: &[f64; 4]| luminance([linear(c[0]), linear(c[1]), linear(c[2])]);
                    assert!((y(a) - y(b)).abs() < 1e-10, "lost contrast: {name}/{rgb:x}");
                }
            }
        }
    }

    #[test]
    fn reads_theme_and_falls_back() {
        let p = Palette::from_toml("background = \"#130F18\"\nred = \"#F0503A\"\naccent = \"bogus\"\nblack = \"#111111\"\n[x]\ny = 1\n").unwrap();
        assert_eq!(p.get("background").unwrap()[0], 0x13 as f64 / 255.0);
        assert_eq!(p.get("bright_red"), p.get("red"));
        assert_eq!(p.get("dark_background"), p.get("background"));
        assert_eq!(p.get("accent").unwrap(), parse_hex("#7aa2f7").unwrap());
        assert_eq!(p.get("black").unwrap(), [0.0, 0.0, 0.0, 1.0]);
        assert!(p.get("nonexistent").is_none());
        assert_eq!(Palette::default_theme().colors.len(), NAMES.len());
        assert!(Palette::from_toml("= broken").is_err());
        // Light or dark: as the theme says, or else by its background.
        assert!(!Palette::default_theme().light);
        assert!(Palette::from_toml("mode = \"light\"\nbackground = \"#000000\"\n").unwrap().light);
        assert!(!Palette::from_toml("mode = \"dark\"\nbackground = \"#ffffff\"\n").unwrap().light);
        assert!(Palette::from_toml("background = \"#f2f0e5\"\n").unwrap().light);
        assert!(!Palette::from_toml("background = \"#1a1b26\"\n").unwrap().light);
    }
}
