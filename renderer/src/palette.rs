//! The theme palette packs draw with, read from a theme's colors.toml.

use crate::expr::parse_hex;

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
