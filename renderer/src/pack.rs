//! Loading a pack: reading `screensafe.toml` and its files, checking every value, and compiling every expression.
//! Nothing in a pack runs as code; anything this module doesn't recognize is refused rather than ignored.

use std::collections::HashSet;
use std::path::Path;

use crate::expr::{self, Program, Scope, Ty};
use crate::palette::Palette;
use crate::safefs::{self, Dir};

pub const MANIFEST: &str = "screensafe.toml";
pub const FORMAT: i64 = 1;

pub const MANIFEST_LIMIT: u64 = 1024 * 1024;
pub const ART_LIMIT: u64 = 64 * 1024;
pub const ART_ROWS: usize = 200;
pub const ART_COLS: usize = 400;
pub const IMAGE_FILE_LIMIT: u64 = 32 * 1024 * 1024;
pub const FONT_FILE_LIMIT: u64 = 4 * 1024 * 1024;
pub const MAX_FONTS: usize = 4;
pub const PACK_BYTES_LIMIT: u64 = 96 * 1024 * 1024;
pub const BACKGROUNDS_LIMIT: usize = 32;
pub const MAX_SCENES: usize = 64;
pub const MAX_ELEMENTS: usize = 256;
pub const MAX_NESTING: usize = 4;
pub const MAX_COUNT: i64 = 1024;
/// The widest or tallest a sprite sheet may be, and the pixels all of a pack's sheets may hold.
pub const MAX_SHEET_SIDE: u32 = 4096;
pub const SHEET_PIXELS_LIMIT: u64 = 16 * 1024 * 1024;
/// The fewest and most canvas pixels to a stage unit, for `pixels`.
pub const MIN_PIXELS: i64 = 64;
pub const MAX_PIXELS: i64 = 1080;
pub const MAX_SAMPLES: i64 = 4096;
pub const MAX_VERTICES: usize = 4096;
pub const MAX_EDGES: usize = 8192;
pub const MAX_LETS: usize = 64;
pub const MAX_TEXT: usize = 2000;
pub const MAX_SLOTS: usize = 8192;
/// Expression operations a scene may perform per frame.
pub const FRAME_COST_LIMIT: usize = 1_500_000;
/// Line points, mesh edges and text characters a scene may draw per frame.
pub const FRAME_POINTS_LIMIT: usize = 250_000;
pub const MAX_FPS: f64 = 60.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stage {
    /// A frame of this width-to-height ratio, fitted inside the screen.
    Aspect(f64),
    /// The whole screen.
    Screen,
    /// The whole screen as a canvas of chunky pixels, `pixels` of them to a stage unit, scaled up by a whole
    /// number with hard edges.
    Pixels(u32),
}

#[derive(Debug, Default, Clone)]
pub struct Meta {
    pub name: String,
    pub author: String,
    pub license: String,
    pub description: String,
    /// The moment that best shows the pack, in seconds, for its thumbnail in the panel.
    pub preview: Option<f64>,
    /// What someone may change about the pack in the panel, and each setting's value as given.
    pub settings: Vec<Setting>,
}

/// The most settings a pack may offer.
pub const MAX_SETTINGS: usize = 8;

/// A setting a pack offers, with its value: the one given, if it's valid, or else its default.
#[derive(Debug, Clone, PartialEq)]
pub struct Setting {
    pub name: String,
    pub label: String,
    pub kind: SettingKind,
    pub default: f64,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SettingKind {
    /// On or off: a true/false name; values 1 and 0.
    Toggle,
    /// One of a few labelled numbers.
    Choice { labels: Vec<String>, values: Vec<f64> },
    /// A number from `min` to `max` in steps of `step`, shown with `unit`.
    Range { min: f64, max: f64, step: f64, unit: String },
}

impl Setting {
    /// `given`, if it's a value this setting can take (a range's is snapped to its steps); otherwise the default.
    pub fn accept(&self, given: Option<f64>) -> f64 {
        let Some(v) = given.filter(|v| v.is_finite()) else { return self.default };
        match &self.kind {
            SettingKind::Toggle => (v != 0.0) as u8 as f64,
            SettingKind::Choice { values, .. } => values.iter().copied().find(|c| (c - v).abs() < 1e-9).unwrap_or(self.default),
            SettingKind::Range { min, max, step, .. } => {
                if v < *min || v > *max {
                    return self.default;
                }
                (min + ((v - min) / step).round() * step).clamp(*min, *max)
            }
        }
    }
}

/// The slots of the names ScreenSafe itself provides.
#[derive(Debug, Clone, Copy)]
pub struct Builtins {
    pub time: u16,
    pub cycle: u16,
    pub aspect: u16,
    pub screen_aspect: u16,
    pub t: u16,
    pub u: u16,
    pub dur: u16,
    pub i: u16,
    pub n: u16,
    pub k: u16,
    pub s: u16,
    pub j: u16,
    pub m: u16,
    pub e: u16,
    pub ex: u16,
    pub ey: u16,
    pub ez: u16,
}

pub struct Let {
    pub slot: u16,
    pub program: Program,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Format {
    pub width: u8,
    pub zero: bool,
    pub decimals: u8,
}

pub enum Segment {
    Literal(String),
    Value(Program, Format),
}

pub struct Text {
    pub segments: Vec<Segment>,
    pub size: Program,
    pub align: Align,
    pub valign: VAlign,
    pub reveal: Option<Program>,
    pub cursor: Option<Program>,
    pub line_height: f64,
    /// 0 for the built-in font, or one more than an index into `Pack::fonts`.
    pub font: usize,
    /// How much wider or narrower than the font's own the characters are drawn.
    pub stretch: f64,
}

pub enum Points {
    Fixed(Vec<[f64; 2]>),
    /// `lets` are worked out for each sample, before `px` and `py`.
    Sampled {
        samples: u32,
        lets: Vec<Let>,
        px: Program,
        py: Program,
    },
}

pub struct Lines {
    pub points: Points,
    pub width: Program,
    pub closed: bool,
    pub draw: Program,
    pub fill: Option<Program>,
}

pub struct Mesh {
    pub vertices: Vec<[f64; 3]>,
    pub edges: Vec<[u32; 2]>,
    /// A color per edge, given its number `e` and the midpoint (`ex`, `ey`, `ez`) of its two vertices.
    pub edge_color: Option<Program>,
    pub rx: Program,
    pub ry: Program,
    pub rz: Program,
    /// Where the turned mesh is moved to, in mesh units, before it's seen in perspective: a camera's move.
    pub offset: [Program; 3],
    pub size: Program,
    pub distance: Program,
    pub width: Program,
    pub draw: Program,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fit {
    Cover,
    Contain,
}

pub struct Image {
    /// An index into `Pack::images`, or `None` for the theme's backgrounds.
    pub source: Option<usize>,
    pub fit: Fit,
    pub zoom: Program,
    pub pan_x: Program,
    pub pan_y: Program,
    pub index: Program,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Effect {
    Print,
    Decrypt,
    Rain,
    Wipe,
    Fade,
    Random,
}

pub const EFFECTS: &[(&str, Effect)] = &[
    ("print", Effect::Print),
    ("decrypt", Effect::Decrypt),
    ("rain", Effect::Rain),
    ("wipe", Effect::Wipe),
    ("fade", Effect::Fade),
    ("random", Effect::Random),
];

pub struct Ascii {
    /// An index into `Pack::arts`.
    pub art: usize,
    pub effect: Effect,
    pub fit: f64,
    pub colors: Vec<Program>,
    pub progress: Program,
}

/// Frames cut from a sheet of pixel art, drawn with hard pixel edges.
pub struct Sprite {
    /// An index into `Pack::images`.
    pub image: usize,
    pub frame_w: u32,
    pub frame_h: u32,
    /// Frames across the sheet, and frames in all.
    pub columns: u32,
    pub frames: u32,
    pub frame: Program,
    /// The point of the frame, in its pixels from its top left, that goes at the element's origin.
    pub anchor: [f64; 2],
    pub flip: Option<Program>,
    /// Stage units per sprite pixel.
    pub pixel: Program,
    /// Sampled smoothly, at any scale and position, rather than drawn in crisp blocks on whole pixels.
    pub smooth: bool,
}

pub enum Kind {
    /// `sort`: draw the children's copies from the least `depth` to the greatest, rather than in order.
    Group {
        children: Vec<Element>,
        clip: Option<[Program; 4]>,
        sort: bool,
    },
    Text(Text),
    Lines(Lines),
    Mesh(Box<Mesh>),
    Rect {
        w: Program,
        h: Program,
    },
    Image(Image),
    Ascii(Ascii),
    Sprite(Sprite),
}

/// A soft light around what an element draws, as a lit phosphor or a bright line has.
pub struct Glow {
    /// How bright, from 0 (none) to 4.
    pub strength: Program,
    /// How far it spreads, in stage units.
    pub radius: f64,
}

pub struct Element {
    pub kind: Kind,
    pub glow: Option<Glow>,
    pub count: u32,
    /// Whether the element has its own `count`, and so its own i, n and k.
    pub repeats: bool,
    pub lets: Vec<Let>,
    pub visible: Option<Program>,
    pub opacity: Program,
    pub x: Program,
    pub y: Program,
    pub rotate: Program,
    pub scale: Program,
    pub color: Program,
    /// Where a copy goes in its group's drawing order, when the group sorts.
    pub depth: Option<Program>,
}

pub struct Scene {
    pub name: String,
    pub duration: f64,
    pub hold: f64,
    pub fade: f64,
    pub fps: f64,
    pub background: Program,
    pub lets: Vec<Let>,
    pub draw: Vec<Element>,
    pub cost: usize,
    pub points: usize,
}

impl Scene {
    pub fn length(&self) -> f64 {
        self.duration + self.hold
    }
}

pub enum ImageSource {
    File {
        name: String,
        bytes: Vec<u8>,
    },
    /// An image prepared as the pack loads (someone's badge fitted to a sheet, or a sheet in the theme's colors):
    /// straight RGBA, `width` x `height`.
    Pixels {
        name: String,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
}

/// The image files tried, in order, for someone's badge, given the path without its extension.
pub const BADGE_EXTENSIONS: [&str; 4] = ["png", "webp", "jpg", "jpeg"];
/// The largest badge file read.
pub const BADGE_FILE_LIMIT: u64 = 8 * 1024 * 1024;

pub struct Pack {
    pub meta: Meta,
    pub stage: Stage,
    pub scenes: Vec<Scene>,
    pub builtins: Builtins,
    pub initial: Vec<f64>,
    pub images: Vec<ImageSource>,
    pub backgrounds: Vec<(String, Vec<u8>)>,
    pub arts: Vec<Vec<Vec<char>>>,
    /// Fonts the pack brings, by file name, checked as TrueType or OpenType.
    pub fonts: Vec<(String, Vec<u8>)>,
    pub warnings: Vec<String>,
    /// A digest of everything the pack was made from: its manifest and files, the theme's colors and backgrounds it
    /// uses, the art it shows, and this renderer's version. The panel keeps a pack's thumbnail until it changes.
    pub fingerprint: u64,
}

impl Pack {
    pub fn length(&self) -> f64 {
        self.scenes.iter().map(Scene::length).sum()
    }
}

/// What a pack may read besides its own directory.
#[derive(Default)]
pub struct Environment<'a> {
    pub palette: Option<&'a Palette>,
    /// The current theme's directory, for `source = "theme-backgrounds"`.
    pub theme: Option<&'a Path>,
    /// The user's screensaver art, for `source = "branding"`.
    pub branding: Option<&'a Path>,
    /// Someone's badge, without its extension (see [`BADGE_EXTENSIONS`]), for sprites with `source = "badge"`.
    pub badge: Option<&'a Path>,
    /// The values chosen for the pack's settings, by name. Names it doesn't offer, and values it can't take, are
    /// passed over for its defaults.
    pub settings: &'a [(String, f64)],
    /// Art to show when the user's can't be read: Omarchy's logo.
    pub logo: Option<&'a Path>,
}

#[derive(Debug)]
pub struct LoadError(pub String);

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

type Result<T> = std::result::Result<T, LoadError>;

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(LoadError(message.into()))
}

// ---------------------------------------------------------------------------------------------------------------
// Reading TOML tables strictly

struct Fields<'a> {
    table: &'a toml::Table,
    used: HashSet<&'a str>,
    path: String,
}

impl<'a> Fields<'a> {
    fn new(table: &'a toml::Table, path: impl Into<String>) -> Self {
        Fields { table, used: HashSet::new(), path: path.into() }
    }

    fn err<T>(&self, key: &str, message: impl std::fmt::Display) -> Result<T> {
        if key.is_empty() {
            fail(format!("{}: {message}", self.path))
        } else if self.path.is_empty() {
            fail(format!("{key}: {message}"))
        } else {
            fail(format!("{} › {key}: {message}", self.path))
        }
    }

    fn get(&mut self, key: &'a str) -> Option<&'a toml::Value> {
        let (k, v) = self.table.get_key_value(key)?;
        self.used.insert(k.as_str());
        Some(v)
    }

    fn string(&mut self, key: &'a str) -> Result<Option<String>> {
        match self.get(key) {
            None => Ok(None),
            Some(toml::Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => self.err(key, "must be text in quotes"),
        }
    }

    fn text(&mut self, key: &'a str, max: usize) -> Result<String> {
        let s = self.string(key)?.unwrap_or_default();
        if s.chars().count() > max {
            return self.err(key, format!("is longer than {max} characters"));
        }
        if s.chars().any(|c| c.is_control()) {
            return self.err(key, "contains a control character");
        }
        Ok(s)
    }

    fn number(&mut self, key: &'a str, default: f64, min: f64, max: f64) -> Result<f64> {
        let value = match self.get(key) {
            None => return Ok(default),
            Some(toml::Value::Integer(i)) => *i as f64,
            Some(toml::Value::Float(f)) => *f,
            Some(_) => return self.err(key, "must be a number"),
        };
        if !(min..=max).contains(&value) {
            return self.err(key, format!("must be between {min} and {max}"));
        }
        Ok(value)
    }

    fn integer(&mut self, key: &'a str, default: i64, min: i64, max: i64) -> Result<i64> {
        let value = match self.get(key) {
            None => return Ok(default),
            Some(toml::Value::Integer(i)) => *i,
            Some(_) => return self.err(key, "must be a whole number"),
        };
        if !(min..=max).contains(&value) {
            return self.err(key, format!("must be between {min} and {max}"));
        }
        Ok(value)
    }

    fn boolean(&mut self, key: &'a str, default: bool) -> Result<bool> {
        match self.get(key) {
            None => Ok(default),
            Some(toml::Value::Boolean(b)) => Ok(*b),
            Some(_) => self.err(key, "must be true or false"),
        }
    }

    fn choice<T: Copy>(&mut self, key: &'a str, options: &[(&str, T)], default: T) -> Result<T> {
        let Some(s) = self.string(key)? else { return Ok(default) };
        match options.iter().find(|(name, _)| *name == s) {
            Some((_, v)) => Ok(*v),
            None => {
                let names: Vec<&str> = options.iter().map(|(n, _)| *n).collect();
                self.err(key, format!("must be one of: {}", names.join(", ")))
            }
        }
    }

    /// Unquoted numbers and true/false are accepted wherever an expression is.
    fn expr_source(&mut self, key: &'a str) -> Result<Option<String>> {
        match self.get(key) {
            None => Ok(None),
            Some(toml::Value::String(s)) => Ok(Some(s.clone())),
            Some(toml::Value::Integer(i)) => Ok(Some(i.to_string())),
            Some(toml::Value::Float(f)) if f.is_finite() => Ok(Some(format!("{f:?}"))),
            Some(toml::Value::Boolean(b)) => Ok(Some(b.to_string())),
            Some(_) => self.err(key, "must be an expression in quotes, or a number"),
        }
    }

    fn tables(&mut self, key: &'a str) -> Result<Vec<&'a toml::Table>> {
        match self.get(key) {
            None => Ok(Vec::new()),
            Some(toml::Value::Array(items)) => items
                .iter()
                .map(|v| match v {
                    toml::Value::Table(t) => Ok(t),
                    _ => self.err(key, "must be a list of tables, written [[...]]"),
                })
                .collect(),
            Some(_) => self.err(key, "must be a list of tables, written [[...]]"),
        }
    }

    fn strings(&mut self, key: &'a str) -> Result<Vec<String>> {
        match self.get(key) {
            None => Ok(Vec::new()),
            Some(toml::Value::Array(items)) => items
                .iter()
                .map(|v| match v {
                    toml::Value::String(s) => Ok(s.clone()),
                    _ => self.err(key, "must be a list of quoted texts"),
                })
                .collect(),
            Some(_) => self.err(key, "must be a list of quoted texts"),
        }
    }

    fn finish(self) -> Result<()> {
        let mut unknown: Vec<&String> = self.table.keys().filter(|k| !self.used.contains(k.as_str())).collect();
        unknown.sort();
        match unknown.first() {
            None => Ok(()),
            Some(key) => self.err(key, "isn't a setting ScreenSafe knows here"),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Compiling

struct Loader<'a> {
    dir: Dir,
    env: &'a Environment<'a>,
    slots: usize,
    initial: Vec<f64>,
    builtins: Builtins,
    images: Vec<ImageSource>,
    image_names: Vec<String>,
    image_sizes: Vec<(u32, u32)>,
    /// The images used as sprite sheets, and the pixels they hold altogether.
    sheets: Vec<usize>,
    sheet_pixels: u64,
    stage: Stage,
    arts: Vec<Vec<Vec<char>>>,
    art_names: Vec<String>,
    fonts: Vec<(String, Vec<u8>)>,
    font_names: Vec<String>,
    bytes_read: u64,
    elements: usize,
    uses_backgrounds: bool,
    warnings: Vec<String>,
}

/// The cost and drawing volume of one evaluation.
#[derive(Default, Clone, Copy)]
struct Budget {
    cost: usize,
    points: usize,
}

impl Budget {
    fn add(&mut self, other: Budget) {
        self.cost = self.cost.saturating_add(other.cost);
        self.points = self.points.saturating_add(other.points);
    }

    fn times(self, n: usize) -> Budget {
        Budget { cost: self.cost.saturating_mul(n), points: self.points.saturating_mul(n) }
    }
}

impl Loader<'_> {
    fn alloc(&mut self, lanes: usize) -> Result<u16> {
        let slot = self.slots;
        self.slots += lanes;
        if self.slots > MAX_SLOTS {
            return fail(format!("the pack defines more than {MAX_SLOTS} values"));
        }
        self.initial.resize(self.slots, 0.0);
        Ok(slot as u16)
    }

    fn read(&mut self, rel: &str, limit: u64) -> Result<Vec<u8>> {
        let bytes = self.dir.read(rel, limit).map_err(|e| LoadError(e.0))?;
        self.bytes_read += bytes.len() as u64;
        if self.bytes_read > PACK_BYTES_LIMIT {
            return fail(format!("the pack's files add up to more than {} MiB", PACK_BYTES_LIMIT / 1024 / 1024));
        }
        Ok(bytes)
    }

    fn compile(&self, fields: &Fields, key: &str, src: &str, scope: &Scope, ty: Ty, budget: &mut Budget) -> Result<Program> {
        match expr::compile(src, scope, ty) {
            Ok(p) => {
                budget.cost += p.cost();
                Ok(p)
            }
            Err(e) => fields.err(key, format!("{e}\n    {src}{}", caret(e.at))),
        }
    }

    fn expr<'b>(&self, f: &mut Fields<'b>, key: &'b str, default: &str, scope: &Scope, ty: Ty, budget: &mut Budget) -> Result<Program> {
        let src = f.expr_source(key)?.unwrap_or_else(|| default.to_string());
        self.compile(f, key, &src, scope, ty, budget)
    }

    fn optional_expr<'b>(&self, f: &mut Fields<'b>, key: &'b str, scope: &Scope, ty: Ty, budget: &mut Budget) -> Result<Option<Program>> {
        match f.expr_source(key)? {
            None => Ok(None),
            Some(src) => self.compile(f, key, &src, scope, ty, budget).map(Some),
        }
    }

    /// A number, or an expression worked out once as the pack loads, from its params and settings (so a setting
    /// can choose how long a scene lasts): within `min` to `max` either way.
    fn fixed<'b>(&self, f: &mut Fields<'b>, key: &'b str, default: f64, min: f64, max: f64, scope: &Scope) -> Result<f64> {
        let Some(toml::Value::String(src)) = f.get(key) else { return f.number(key, default, min, max) };
        let program = expr::compile(src, scope, Ty::Num).or_else(|e| f.err(key, format!("{e}\n    {src}{}", caret(e.at))))?;
        let value = expr::Machine::new(0).num(&program, &self.initial);
        if !(min..=max).contains(&value) {
            return f.err(key, format!("comes to {value}; it must be between {min} and {max}"));
        }
        Ok(value)
    }

    /// `let = ["name = expression", ...]`: values computed in order, each visible to the ones after it.
    fn lets<'b>(&mut self, f: &mut Fields<'b>, scope: &mut Scope, budget: &mut Budget) -> Result<Vec<Let>> {
        self.lets_in(f, "let", scope, budget)
    }

    /// A list of named values under `key`, as `let` is.
    fn lets_in<'b>(&mut self, f: &mut Fields<'b>, key: &'b str, scope: &mut Scope, budget: &mut Budget) -> Result<Vec<Let>> {
        let lines = f.strings(key)?;
        if lines.len() > MAX_LETS {
            return f.err(key, format!("has more than {MAX_LETS} lines"));
        }
        let mut lets = Vec::new();
        for line in lines {
            let Some((name, src)) = split_let(&line) else {
                return f.err(key, format!("'{line}' must look like: name = expression"));
            };
            if !expr::valid_name(name) {
                return f.err(key, format!("'{name}' isn't a valid name: use lowercase letters, digits and _"));
            }
            if scope.contains(name) {
                return f.err(key, format!("'{name}' is already defined"));
            }
            let program = match expr::compile_any(src, scope) {
                Ok(p) => p,
                Err(e) => return f.err(key, format!("{name}: {e}\n    {src}{}", caret(e.at))),
            };
            budget.cost += program.cost();
            let slot = self.alloc(if program.ty == Ty::Color { 4 } else { 1 })?;
            scope.bind(name, slot, program.ty);
            lets.push(Let { slot, program });
        }
        Ok(lets)
    }

    fn element(&mut self, table: &toml::Table, path: String, scope: &Scope, depth: usize) -> Result<(Element, Budget)> {
        self.elements += 1;
        if self.elements > MAX_ELEMENTS {
            return fail(format!("{path}: a scene can draw at most {MAX_ELEMENTS} elements"));
        }
        let mut f = Fields::new(table, path.clone());
        let kind_name = f.string("kind")?.unwrap_or_default();
        let repeats = table.contains_key("count");
        let count = f.integer("count", 1, 1, MAX_COUNT)? as u32;
        let mut scope = scope.clone();
        let b = self.builtins;
        // i, n and k number this element's copies; an element without a count shares its group's.
        scope.bind("i", b.i, Ty::Num);
        scope.bind("n", b.n, Ty::Num);
        scope.bind("k", b.k, Ty::Num);
        let mut budget = Budget::default();
        let lets = self.lets(&mut f, &mut scope, &mut budget)?;
        let visible = self.optional_expr(&mut f, "visible", &scope, Ty::Bool, &mut budget)?;
        let opacity = self.expr(&mut f, "opacity", "1", &scope, Ty::Num, &mut budget)?;
        let x = self.expr(&mut f, "x", "0", &scope, Ty::Num, &mut budget)?;
        let y = self.expr(&mut f, "y", "0", &scope, Ty::Num, &mut budget)?;
        let rotate = self.expr(&mut f, "rotate", "0", &scope, Ty::Num, &mut budget)?;
        let scale = self.expr(&mut f, "scale", "1", &scope, Ty::Num, &mut budget)?;
        let color = self.expr(&mut f, "color", "foreground", &scope, Ty::Color, &mut budget)?;
        let order = self.optional_expr(&mut f, "depth", &scope, Ty::Num, &mut budget)?;
        let glow = match self.optional_expr(&mut f, "glow", &scope, Ty::Num, &mut budget)? {
            Some(strength) => Some(Glow { strength, radius: f.number("glow_radius", 0.006, 0.0005, 0.05)? }),
            None => {
                if table.contains_key("glow_radius") {
                    return f.err("glow_radius", "only applies with glow");
                }
                None
            }
        };

        let kind = match kind_name.as_str() {
            "group" => {
                if depth >= MAX_NESTING {
                    return f.err("kind", format!("groups can nest at most {MAX_NESTING} deep"));
                }
                let clip = match table.contains_key("clip") {
                    false => None,
                    true => {
                        let parts = f.strings("clip").or_else(|_| f.err("clip", "must be four expressions: [left, top, right, bottom]"))?;
                        if parts.len() != 4 {
                            return f.err("clip", "must be four expressions: [left, top, right, bottom]");
                        }
                        let mut out = Vec::new();
                        for part in &parts {
                            out.push(self.compile(&f, "clip", part, &scope, Ty::Num, &mut budget)?);
                        }
                        let [a, b, c, d]: [Program; 4] = out.try_into().unwrap_or_else(|_| unreachable!("four parts were checked"));
                        Some([a, b, c, d])
                    }
                };
                let mut children = Vec::new();
                for (index, child) in f.tables("draw")?.into_iter().enumerate() {
                    let (element, child_budget) = self.element(child, format!("{path} › draw {}", index + 1), &scope, depth + 1)?;
                    budget.add(child_budget);
                    children.push(element);
                }
                if children.is_empty() {
                    return f.err("draw", "a group needs at least one [[...draw]] element");
                }
                let sort = f.boolean("sort", false)?;
                Kind::Group { children, clip, sort }
            }
            "text" => {
                let raw = f.string("text")?.unwrap_or_default();
                if raw.chars().count() > MAX_TEXT {
                    return f.err("text", format!("is longer than {MAX_TEXT} characters"));
                }
                if raw.chars().any(|c| c.is_control() && c != '\n') {
                    return f.err("text", "contains a control character other than a line break");
                }
                let segments = self.segments(&f, &raw, &scope, &mut budget)?;
                let literal_chars: usize = segments.iter().map(|s| if let Segment::Literal(l) = s { l.chars().count() } else { 24 }).sum();
                budget.points += literal_chars;
                let size = self.expr(&mut f, "size", "0.04", &scope, Ty::Num, &mut budget)?;
                let align = f.choice("align", &[("left", Align::Left), ("center", Align::Center), ("right", Align::Right)], Align::Left)?;
                let valign = f.choice("valign", &[("top", VAlign::Top), ("middle", VAlign::Middle), ("bottom", VAlign::Bottom)], VAlign::Top)?;
                let reveal = self.optional_expr(&mut f, "reveal", &scope, Ty::Num, &mut budget)?;
                let cursor = self.optional_expr(&mut f, "cursor", &scope, Ty::Bool, &mut budget)?;
                let line_height = f.number("line_height", 1.25, 0.5, 4.0)?;
                let font = match f.string("font")? {
                    None => 0,
                    Some(name) => match self.font_names.iter().position(|n| *n == name) {
                        Some(i) => i + 1,
                        None => return f.err("font", format!("'{name}' isn't one of the pack's [fonts]")),
                    },
                };
                let stretch = f.number("stretch", 1.0, 0.25, 4.0)?;
                Kind::Text(Text { segments, size, align, valign, reveal, cursor, line_height, font, stretch })
            }
            "lines" => {
                let width = self.expr(&mut f, "width", "0.003", &scope, Ty::Num, &mut budget)?;
                let closed = f.boolean("closed", false)?;
                let draw = self.expr(&mut f, "draw", "1", &scope, Ty::Num, &mut budget)?;
                let fill = self.optional_expr(&mut f, "fill", &scope, Ty::Color, &mut budget)?;
                let fixed = f.get("points");
                let samples = f.integer("samples", 0, 2, MAX_SAMPLES)?;
                let points = match (fixed, samples) {
                    (Some(value), 0) => {
                        let points = points2(value).ok_or(()).or_else(|_| f.err("points", format!("must be a list of 2 to {MAX_SAMPLES} [x, y] pairs")))?;
                        budget.points += points.len();
                        Points::Fixed(points)
                    }
                    (None, samples) if samples >= 2 => {
                        let mut curve = scope.clone();
                        curve.bind("s", b.s, Ty::Num);
                        curve.bind("j", b.j, Ty::Num);
                        curve.bind("m", b.m, Ty::Num);
                        let mut per_sample = Budget::default();
                        let lets = self.lets_in(&mut f, "sample_let", &mut curve, &mut per_sample)?;
                        let px = self.expr(&mut f, "px", "s", &curve, Ty::Num, &mut per_sample)?;
                        let py = self.expr(&mut f, "py", "0", &curve, Ty::Num, &mut per_sample)?;
                        per_sample.points += 1;
                        budget.add(per_sample.times(samples as usize));
                        Points::Sampled { samples: samples as u32, lets, px, py }
                    }
                    (Some(_), _) => return f.err("samples", "use either points or samples, not both"),
                    _ => return f.err("", "lines need either points = [[x, y], ...] or samples with px and py"),
                };
                Kind::Lines(Lines { points, width, closed, draw, fill })
            }
            "mesh" => {
                let vertices = f
                    .get("vertices")
                    .and_then(points3)
                    .ok_or(())
                    .or_else(|_| f.err("vertices", format!("must be a list of 2 to {MAX_VERTICES} [x, y, z] triples")))?;
                let edges = f
                    .get("edges")
                    .and_then(|v| edges(v, vertices.len()))
                    .ok_or(())
                    .or_else(|_| f.err("edges", format!("must be a list of 1 to {MAX_EDGES} [a, b] pairs of vertex numbers, counting from 0")))?;
                budget.points += edges.len() * 2;
                let rx = self.expr(&mut f, "rx", "0", &scope, Ty::Num, &mut budget)?;
                let ry = self.expr(&mut f, "ry", "0", &scope, Ty::Num, &mut budget)?;
                let rz = self.expr(&mut f, "rz", "0", &scope, Ty::Num, &mut budget)?;
                let offset = [
                    self.expr(&mut f, "offset_x", "0", &scope, Ty::Num, &mut budget)?,
                    self.expr(&mut f, "offset_y", "0", &scope, Ty::Num, &mut budget)?,
                    self.expr(&mut f, "offset_z", "0", &scope, Ty::Num, &mut budget)?,
                ];
                let size = self.expr(&mut f, "size", "0.25", &scope, Ty::Num, &mut budget)?;
                let distance = self.expr(&mut f, "distance", "4", &scope, Ty::Num, &mut budget)?;
                let width = self.expr(&mut f, "width", "0.003", &scope, Ty::Num, &mut budget)?;
                let draw = self.expr(&mut f, "draw", "1", &scope, Ty::Num, &mut budget)?;
                let edge_color = if table.contains_key("edge_color") {
                    let mut per_edge = scope.clone();
                    per_edge.bind("e", b.e, Ty::Num);
                    per_edge.bind("ex", b.ex, Ty::Num);
                    per_edge.bind("ey", b.ey, Ty::Num);
                    per_edge.bind("ez", b.ez, Ty::Num);
                    let mut edge_budget = Budget::default();
                    let program = self.expr(&mut f, "edge_color", "foreground", &per_edge, Ty::Color, &mut edge_budget)?;
                    budget.add(edge_budget.times(edges.len()));
                    Some(program)
                } else {
                    None
                };
                Kind::Mesh(Box::new(Mesh { vertices, edges, edge_color, rx, ry, rz, offset, size, distance, width, draw }))
            }
            "rect" => {
                let w = self.expr(&mut f, "w", "0.1", &scope, Ty::Num, &mut budget)?;
                let h = self.expr(&mut f, "h", "0.1", &scope, Ty::Num, &mut budget)?;
                budget.points += 4;
                Kind::Rect { w, h }
            }
            "image" => {
                let file = f.string("file")?;
                let source = f.string("source")?;
                let source = match (file, source.as_deref()) {
                    (Some(file), None) => Some(self.image(&f, &file)?),
                    (None, Some("theme-backgrounds")) => {
                        self.uses_backgrounds = true;
                        None
                    }
                    (None, Some(_)) => return f.err("source", "the only image source is \"theme-backgrounds\""),
                    (Some(_), Some(_)) => return f.err("source", "use either file or source, not both"),
                    (None, None) => return f.err("", "an image needs file = \"name.webp\" or source = \"theme-backgrounds\""),
                };
                let fit = f.choice("fit", &[("cover", Fit::Cover), ("contain", Fit::Contain)], Fit::Cover)?;
                let zoom = self.expr(&mut f, "zoom", "1", &scope, Ty::Num, &mut budget)?;
                let pan_x = self.expr(&mut f, "pan_x", "0", &scope, Ty::Num, &mut budget)?;
                let pan_y = self.expr(&mut f, "pan_y", "0", &scope, Ty::Num, &mut budget)?;
                let index = self.expr(&mut f, "index", "0", &scope, Ty::Num, &mut budget)?;
                budget.points += 4;
                Kind::Image(Image { source, fit, zoom, pan_x, pan_y, index })
            }
            "ascii" => {
                let art_file = f.string("art")?;
                let source = f.string("source")?;
                let art = match (art_file, source.as_deref()) {
                    (Some(file), None) => self.art(&f, &file)?,
                    (None, Some("branding")) => self.branding()?,
                    (None, Some(_)) => return f.err("source", "the only art source is \"branding\""),
                    (Some(_), Some(_)) => return f.err("source", "use either art or source, not both"),
                    (None, None) => return f.err("", "ascii needs art = \"name.txt\" or source = \"branding\""),
                };
                let effect = f.choice("effect", EFFECTS, Effect::Random)?;
                let fit = f.number("fit", 0.8, 0.05, 1.0)?;
                let mut colors = Vec::new();
                let sources = match f.get("colors") {
                    None => vec!["foreground".to_string()],
                    Some(_) => f.strings("colors")?,
                };
                if sources.is_empty() || sources.len() > 8 {
                    return f.err("colors", "must list 1 to 8 colors");
                }
                for src in &sources {
                    colors.push(self.compile(&f, "colors", src, &scope, Ty::Color, &mut budget)?);
                }
                let progress = self.expr(&mut f, "progress", "u", &scope, Ty::Num, &mut budget)?;
                let cells = self.arts[art].iter().map(Vec::len).sum::<usize>();
                budget.points += cells;
                Kind::Ascii(Ascii { art, effect, fit, colors, progress })
            }
            "sprite" => {
                let Some(file) = f.string("file")? else { return f.err("", "a sprite needs file = \"sheet.png\"") };
                let image = self.image(&f, &file)?;
                let badge = match f.string("source")?.as_deref() {
                    None => false,
                    Some("badge") => true,
                    Some(_) => return f.err("source", "a sprite's only source is \"badge\""),
                };
                let themed = match f.string("palette")?.as_deref() {
                    None => false,
                    Some("theme") => true,
                    Some(_) => return f.err("palette", "a sprite's only palette is \"theme\""),
                };
                // Checked before a badge or themed copy is made, which decodes the image.
                let (sheet_w, sheet_h) = self.image_sizes[image];
                if sheet_w > MAX_SHEET_SIDE || sheet_h > MAX_SHEET_SIDE {
                    return f.err("file", format!("{file} is {sheet_w} x {sheet_h}; a sprite sheet may be at most {MAX_SHEET_SIDE} pixels a side"));
                }
                let image = if badge || themed { self.prepared(&f, image, badge, themed)? } else { image };
                if !self.sheets.contains(&image) {
                    self.sheets.push(image);
                    self.sheet_pixels += sheet_w as u64 * sheet_h as u64;
                    if self.sheet_pixels > SHEET_PIXELS_LIMIT {
                        return f.err("file", format!("the pack's sprite sheets hold more than {SHEET_PIXELS_LIMIT} pixels altogether"));
                    }
                }
                let [frame_w, frame_h] = match f.get("frame_size") {
                    None => [sheet_w, sheet_h],
                    Some(toml::Value::Array(v)) if v.len() == 2 => {
                        let mut out = [0u32; 2];
                        for (o, item) in out.iter_mut().zip(v) {
                            match item {
                                toml::Value::Integer(n) if (1..=MAX_SHEET_SIDE as i64).contains(n) => *o = *n as u32,
                                _ => return f.err("frame_size", format!("must be two whole numbers of pixels, from 1 to {MAX_SHEET_SIDE}: [width, height]")),
                            }
                        }
                        out
                    }
                    Some(_) => return f.err("frame_size", "must be two whole numbers of pixels: [width, height]"),
                };
                let (columns, rows) = (sheet_w / frame_w, sheet_h / frame_h);
                if columns == 0 || rows == 0 {
                    return f.err("frame_size", format!("is larger than the sheet, {sheet_w} x {sheet_h}"));
                }
                let anchor = match f.get("anchor") {
                    None => [0.0, 0.0],
                    Some(toml::Value::Array(v)) if v.len() == 2 => {
                        let mut out = [0.0f64; 2];
                        for (o, item) in out.iter_mut().zip(v) {
                            *o = match item {
                                toml::Value::Integer(n) => *n as f64,
                                toml::Value::Float(x) => *x,
                                _ => return f.err("anchor", "must be two numbers of pixels: [x, y]"),
                            };
                            if !(-(MAX_SHEET_SIDE as f64)..=MAX_SHEET_SIDE as f64).contains(o) {
                                return f.err("anchor", format!("must be within {MAX_SHEET_SIDE} pixels of the frame"));
                            }
                        }
                        out
                    }
                    Some(_) => return f.err("anchor", "must be two numbers of pixels: [x, y]"),
                };
                let frame = self.expr(&mut f, "frame", "0", &scope, Ty::Num, &mut budget)?;
                let flip = self.optional_expr(&mut f, "flip", &scope, Ty::Bool, &mut budget)?;
                let default_pixel = match self.stage {
                    Stage::Pixels(p) => format!("{:?}", 1.0 / p as f64),
                    _ => "0.004".to_string(),
                };
                let pixel = self.expr(&mut f, "pixel", &default_pixel, &scope, Ty::Num, &mut budget)?;
                budget.points += 4;
                let smooth = f.boolean("smooth", false)?;
                Kind::Sprite(Sprite { image, frame_w, frame_h, columns, frames: columns * rows, frame, anchor, flip, pixel, smooth })
            }
            "" => return f.err("kind", "is missing; use group, text, lines, mesh, rect, image, ascii or sprite"),
            other => return f.err("kind", format!("'{other}' isn't a kind; use group, text, lines, mesh, rect, image, ascii or sprite")),
        };
        f.finish()?;
        let element = Element { kind, glow, count, repeats, lets, visible, opacity, x, y, rotate, scale, color, depth: order };
        Ok((element, budget.times(count as usize)))
    }

    fn segments(&self, f: &Fields, raw: &str, scope: &Scope, budget: &mut Budget) -> Result<Vec<Segment>> {
        let mut segments = Vec::new();
        let mut literal = String::new();
        let mut rest = raw;
        while let Some(pos) = rest.find(['{', '}']) {
            literal.push_str(&rest[..pos]);
            let brace = &rest[pos..];
            if brace.starts_with("{{") || brace.starts_with("}}") {
                literal.push(brace.as_bytes()[0] as char);
                rest = &brace[2..];
                continue;
            }
            if brace.starts_with('}') {
                return f.err("text", "has a '}' without a '{'; write }} for a brace");
            }
            let Some(end) = brace.find('}') else {
                return f.err("text", "has a '{' without a '}'; write {{ for a brace");
            };
            let inside = &brace[1..end];
            let (src, spec) = match inside.split_once(';') {
                Some((src, spec)) => (src, Some(spec.trim())),
                None => (inside, None),
            };
            let format = match spec {
                None => Format { width: 0, zero: false, decimals: 0 },
                Some(spec) => parse_format(spec)
                    .ok_or(())
                    .or_else(|_| f.err("text", format!("'{spec}' isn't a format: use a width like 3 or 03 (zero-padded), and/or decimals like .2")))?,
            };
            let program = self.compile(f, "text", src, scope, Ty::Num, budget)?;
            if !literal.is_empty() {
                segments.push(Segment::Literal(std::mem::take(&mut literal)));
            }
            segments.push(Segment::Value(program, format));
            rest = &brace[end + 1..];
        }
        literal.push_str(rest);
        if !literal.is_empty() {
            segments.push(Segment::Literal(literal));
        }
        Ok(segments)
    }

    fn image(&mut self, f: &Fields, file: &str) -> Result<usize> {
        if let Some(i) = self.image_names.iter().position(|n| n == file) {
            return Ok(i);
        }
        let lower = file.to_ascii_lowercase();
        if ![".png", ".jpg", ".jpeg", ".webp"].iter().any(|ext| lower.ends_with(ext)) {
            return f.err("file", "images must be .png, .jpg, .jpeg or .webp");
        }
        let bytes = self.read(file, IMAGE_FILE_LIMIT).or_else(|e| f.err("file", e))?;
        let size = crate::raster::probe_image(&bytes).or_else(|e| f.err("file", format!("{file}: {e}")))?;
        self.images.push(ImageSource::File { name: file.to_string(), bytes });
        self.image_names.push(file.to_string());
        self.image_sizes.push(size);
        Ok(self.images.len() - 1)
    }

    /// The sheet `image`, or someone's badge fitted to its size when `badge` is set and they have one, and in the
    /// theme's colors when `themed` is set. Worked out once per pack load.
    fn prepared(&mut self, f: &Fields, image: usize, badge: bool, themed: bool) -> Result<usize> {
        let ImageSource::File { name, bytes } = &self.images[image] else { return Ok(image) };
        let key = format!("\0{name}{}{}", if badge { ":badge" } else { "" }, if themed { ":theme" } else { "" });
        if let Some(i) = self.image_names.iter().position(|n| *n == key) {
            return Ok(i);
        }
        let (name, bytes) = (name.clone(), bytes.clone());
        let (width, height) = self.image_sizes[image];
        let own = crate::raster::decode_rgba(&bytes, width, height).or_else(|e| f.err("file", format!("{name}: {e}")))?;
        let mut rgba = own;
        if badge {
            if let Some(theirs) = self.badge(width, height) {
                rgba = theirs;
            }
        }
        if themed {
            let palette = self.env.palette.cloned().unwrap_or_else(Palette::default_theme);
            crate::raster::to_palette(&mut rgba, width, &palette);
        }
        self.images.push(ImageSource::Pixels { name: key.clone(), width, height, rgba });
        self.image_names.push(key);
        self.image_sizes.push((width, height));
        Ok(self.images.len() - 1)
    }

    /// Someone's badge, fitted within `width` x `height` and centered on transparency; or none, if they have none or
    /// it can't be read (which is noted, and the pack's own image is shown).
    fn badge(&mut self, width: u32, height: u32) -> Option<Vec<u8>> {
        let base = self.env.badge?;
        for ext in BADGE_EXTENSIONS {
            let path = base.with_extension(ext);
            let bytes = match safefs::read_file(&path, BADGE_FILE_LIMIT) {
                Ok(b) => b,
                Err(_) if !path.exists() => continue,
                Err(e) => {
                    self.warnings.push(format!("badge: {}", e.0));
                    return None;
                }
            };
            return match crate::raster::fit_rgba(&bytes, width, height) {
                Ok(rgba) => Some(rgba),
                Err(e) => {
                    self.warnings.push(format!("badge: {}: {e}", path.display()));
                    None
                }
            };
        }
        None
    }

    fn art(&mut self, f: &Fields, file: &str) -> Result<usize> {
        if let Some(i) = self.art_names.iter().position(|n| n == file) {
            return Ok(i);
        }
        if !file.ends_with(".txt") {
            return f.err("art", "art must be a .txt file");
        }
        let bytes = self.read(file, ART_LIMIT).or_else(|e| f.err("art", e))?;
        let art = parse_art(&bytes).or_else(|e| f.err("art", format!("{file}: {e}")))?;
        self.arts.push(art);
        self.art_names.push(file.to_string());
        Ok(self.arts.len() - 1)
    }

    /// `[fonts]`: name = "file.ttf", for text elements' `font`.
    fn fonts(&mut self, f: &mut Fields<'_>) -> Result<()> {
        let Some(value) = f.get("fonts") else { return Ok(()) };
        let Some(table) = value.as_table() else { return f.err("fonts", "must be a table of name = \"file.ttf\"") };
        if table.len() > MAX_FONTS {
            return f.err("fonts", format!("can name at most {MAX_FONTS} fonts"));
        }
        for (name, file) in table {
            if !expr::valid_name(name) {
                return f.err("fonts", format!("'{name}' isn't a valid name: use lowercase letters, digits and _"));
            }
            let Some(file) = file.as_str() else { return f.err("fonts", format!("{name} must be a file name in quotes")) };
            let lower = file.to_ascii_lowercase();
            if !(lower.ends_with(".ttf") || lower.ends_with(".otf")) {
                return f.err("fonts", format!("{name}: fonts must be .ttf or .otf files"));
            }
            let bytes = self.read(file, FONT_FILE_LIMIT)?;
            if let Err(e) = check_font(&bytes) {
                return f.err("fonts", format!("{name}: {file} {e}"));
            }
            self.font_names.push(name.clone());
            self.fonts.push((file.to_string(), bytes));
        }
        Ok(())
    }

    fn branding(&mut self) -> Result<usize> {
        if let Some(i) = self.art_names.iter().position(|n| n == "\0branding") {
            return Ok(i);
        }
        // The screensaver should always show something: the user's art, else Omarchy's logo, else a placeholder.
        let mut art = None;
        for path in [self.env.branding, self.env.logo].into_iter().flatten() {
            match safefs::read_file(path, ART_LIMIT).map_err(|e| e.0).and_then(|bytes| parse_art(&bytes)) {
                Ok(a) => {
                    art = Some(a);
                    break;
                }
                Err(e) => self.warnings.push(format!("{}: {e}", path.display())),
            }
        }
        let art = match art {
            Some(a) => a,
            None => {
                self.warnings.push("no screensaver art could be read, so a placeholder stands in for it".into());
                parse_art(b"SCREENSAFE").map_err(LoadError)?
            }
        };
        self.arts.push(art);
        self.art_names.push("\0branding".into());
        Ok(self.arts.len() - 1)
    }

    fn scene(&mut self, table: &toml::Table, index: usize, scope: &Scope, defaults: (f64, f64, &str)) -> Result<Scene> {
        let mut f = Fields::new(table, format!("scene {}", index + 1));
        let name = f.text("name", 60)?;
        if !name.is_empty() {
            f.path = format!("scene \"{name}\"");
        }
        let duration = self.fixed(&mut f, "duration", 10.0, 0.1, 3600.0, scope)?;
        let hold = self.fixed(&mut f, "hold", 0.0, 0.0, 3600.0, scope)?;
        let fade = f.number("fade", defaults.1, 0.0, 5.0)?.min((duration + hold) / 2.0);
        let fps = f.number("fps", defaults.0, 1.0, MAX_FPS)?;
        let mut scope = scope.clone();
        let b = self.builtins;
        scope.bind("t", b.t, Ty::Num);
        scope.bind("u", b.u, Ty::Num);
        scope.bind("dur", b.dur, Ty::Num);
        let mut budget = Budget::default();
        let lets = self.lets(&mut f, &mut scope, &mut budget)?;
        let background = self.expr(&mut f, "background", defaults.2, &scope, Ty::Color, &mut budget)?;
        self.elements = 0;
        let mut draw = Vec::new();
        for (i, table) in f.tables("draw")?.into_iter().enumerate() {
            let (element, element_budget) = self.element(table, format!("{} › draw {}", f.path, i + 1), &scope, 0)?;
            budget.add(element_budget);
            draw.push(element);
        }
        if budget.cost > FRAME_COST_LIMIT {
            return f.err("", format!("needs about {} operations a frame; the limit is {FRAME_COST_LIMIT}. Use fewer counts or samples", budget.cost));
        }
        if budget.points > FRAME_POINTS_LIMIT {
            return f.err("", format!("draws about {} points a frame; the limit is {FRAME_POINTS_LIMIT}", budget.points));
        }
        f.finish()?;
        Ok(Scene { name, duration, hold, fade, fps, background, lets, draw, cost: budget.cost, points: budget.points })
    }
}

/// One `[[setting]]`: its name, label, kind and default, and the value given for it, if valid.
fn read_setting(f: &mut Fields, scope: &Scope, earlier: &[Setting], given: &[(String, f64)]) -> Result<Setting> {
    let Some(name) = f.string("name")? else { return f.err("name", "is required: the name expressions use") };
    if !expr::valid_name(&name) || scope.contains(&name) || earlier.iter().any(|s| s.name == name) {
        return f.err("name", format!("'{name}' is not a free name: use lowercase letters, digits and _, not a built-in or one already used"));
    }
    let label = f.text("label", 40)?;
    if label.trim().is_empty() {
        return f.err("label", "is required: what the panel calls it");
    }
    let kind_name = f.string("kind")?.unwrap_or_default();
    let (kind, default) = match kind_name.as_str() {
        "toggle" => (SettingKind::Toggle, f.boolean("default", false)? as u8 as f64),
        "choice" => {
            let labels = f.strings("choices")?;
            if !(2..=6).contains(&labels.len()) || labels.iter().any(|l| l.trim().is_empty() || l.chars().count() > 16 || l.chars().any(char::is_control)) {
                return f.err("choices", "must be 2 to 6 labels of up to 16 characters");
            }
            let values: Vec<f64> = match f.get("values") {
                Some(toml::Value::Array(v)) => v.iter().filter_map(number).collect(),
                _ => return f.err("values", "must be a list of numbers, one for each choice"),
            };
            if values.len() != labels.len() || values.iter().any(|v| !v.is_finite()) {
                return f.err("values", "must be a list of numbers, one for each choice");
            }
            let default = f.number("default", values[0], f64::MIN, f64::MAX)?;
            if !values.iter().any(|v| (v - default).abs() < 1e-9) {
                return f.err("default", "must be one of the values");
            }
            (SettingKind::Choice { labels, values }, default)
        }
        "range" => {
            let (min, max) = (f.number("min", 0.0, -1e6, 1e6)?, f.number("max", 1.0, -1e6, 1e6)?);
            let step = f.number("step", 1.0, 1e-6, 1e6)?;
            if max <= min || (max - min) / step > 1000.0 {
                return f.err("max", "must be more than min, and at most 1000 steps from it");
            }
            let default = f.number("default", min, min, max)?;
            let unit = f.text("unit", 8)?;
            (SettingKind::Range { min, max, step, unit }, default)
        }
        _ => return f.err("kind", "must be \"toggle\", \"choice\" or \"range\""),
    };
    let mut setting = Setting { name, label, kind, default, value: default };
    setting.default = setting.accept(Some(default));
    setting.value = setting.accept(given.iter().find(|(n, _)| *n == setting.name).map(|(_, v)| *v));
    Ok(setting)
}

fn caret(at: Option<usize>) -> String {
    match at {
        Some(at) => format!("\n    {}^", " ".repeat(at)),
        None => String::new(),
    }
}

/// Splits "name = expression" at its first lone '='.
fn split_let(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    for (i, &c) in bytes.iter().enumerate() {
        if c == b'=' {
            let prev = if i > 0 { bytes[i - 1] } else { b' ' };
            let next = bytes.get(i + 1).copied().unwrap_or(b' ');
            if next != b'=' && !matches!(prev, b'<' | b'>' | b'!' | b'=') {
                let name = line[..i].trim();
                let src = line[i + 1..].trim();
                return if name.is_empty() || src.is_empty() { None } else { Some((name, src)) };
            }
        }
    }
    None
}

fn parse_format(spec: &str) -> Option<Format> {
    let (width, decimals) = match spec.split_once('.') {
        Some((w, d)) => (w, Some(d)),
        None => (spec, None),
    };
    let zero = width.starts_with('0') && width.len() > 1;
    let width: u8 = if width.is_empty() { 0 } else { width.parse().ok()? };
    let decimals: u8 = match decimals {
        None => 0,
        Some(d) => d.parse().ok()?,
    };
    (width <= 24 && decimals <= 6).then_some(Format { width, zero, decimals })
}

pub fn format_number(value: f64, format: Format) -> String {
    let text = format!("{:.*}", format.decimals as usize, value.clamp(-1e15, 1e15));
    let text = if text.starts_with('-') && text[1..].chars().all(|c| c == '0' || c == '.') { text[1..].to_string() } else { text };
    let width = format.width as usize;
    if text.len() >= width {
        text
    } else if format.zero {
        match text.strip_prefix('-') {
            Some(rest) => format!("-{}{rest}", "0".repeat(width - text.len())),
            None => format!("{}{text}", "0".repeat(width - text.len())),
        }
    } else {
        format!("{}{text}", " ".repeat(width - text.len()))
    }
}

fn number(v: &toml::Value) -> Option<f64> {
    match v {
        toml::Value::Integer(i) => Some(*i as f64),
        toml::Value::Float(f) if f.is_finite() && f.abs() < 1e6 => Some(*f),
        _ => None,
    }
}

fn points2(v: &toml::Value) -> Option<Vec<[f64; 2]>> {
    let items = v.as_array()?;
    if items.len() < 2 || items.len() > MAX_SAMPLES as usize {
        return None;
    }
    items
        .iter()
        .map(|p| match p.as_array()?.as_slice() {
            [x, y] => Some([number(x)?, number(y)?]),
            _ => None,
        })
        .collect()
}

fn points3(v: &toml::Value) -> Option<Vec<[f64; 3]>> {
    let items = v.as_array()?;
    if items.len() < 2 || items.len() > MAX_VERTICES {
        return None;
    }
    items
        .iter()
        .map(|p| match p.as_array()?.as_slice() {
            [x, y, z] => Some([number(x)?, number(y)?, number(z)?]),
            _ => None,
        })
        .collect()
}

fn edges(v: &toml::Value, vertices: usize) -> Option<Vec<[u32; 2]>> {
    let items = v.as_array()?;
    if items.is_empty() || items.len() > MAX_EDGES {
        return None;
    }
    items
        .iter()
        .map(|p| match p.as_array()?.as_slice() {
            [toml::Value::Integer(a), toml::Value::Integer(b)] if (0..vertices as i64).contains(a) && (0..vertices as i64).contains(b) => {
                Some([*a as u32, *b as u32])
            }
            _ => None,
        })
        .collect()
}

/// Text art: UTF-8, tabs as spaces, no other control characters; blank lines at either end are dropped.
/// Checks that `bytes` is a TrueType or OpenType font with the characters text needs.
pub fn check_font(bytes: &[u8]) -> std::result::Result<(), String> {
    let face = ttf_parser::Face::parse(bytes, 0).map_err(|e| format!("isn't a font ScreenSafe can read ({e})"))?;
    if face.glyph_index('M').is_none() || face.glyph_index('0').is_none() {
        return Err("has no Latin letters or digits".into());
    }
    if face.units_per_em() == 0 {
        return Err("has no size".into());
    }
    Ok(())
}

pub fn parse_art(bytes: &[u8]) -> std::result::Result<Vec<Vec<char>>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "art must be UTF-8 text".to_string())?;
    let mut rows: Vec<Vec<char>> = Vec::new();
    for line in text.lines() {
        let mut row = Vec::new();
        for c in line.chars() {
            match c {
                '\t' => row.extend(std::iter::repeat_n(' ', 4 - row.len() % 4)),
                '\r' => {}
                c if c.is_control() => return Err("art contains a control character".into()),
                c => row.push(c),
            }
        }
        while row.last() == Some(&' ') {
            row.pop();
        }
        if row.len() > ART_COLS {
            return Err(format!("art is wider than {ART_COLS} columns"));
        }
        rows.push(row);
    }
    while rows.first().is_some_and(Vec::is_empty) {
        rows.remove(0);
    }
    while rows.last().is_some_and(Vec::is_empty) {
        rows.pop();
    }
    if rows.is_empty() {
        return Err("art is empty".into());
    }
    if rows.len() > ART_ROWS {
        return Err(format!("art is taller than {ART_ROWS} rows"));
    }
    Ok(rows)
}

/// Loads and checks a pack directory. `env` supplies the palette and the files outside the pack it may use.
pub fn load(path: &Path, env: &Environment) -> Result<Pack> {
    let dir = Dir::open(path).map_err(|e| LoadError(e.0))?;
    let bytes = dir.read(MANIFEST, MANIFEST_LIMIT).map_err(|e| LoadError(e.0))?;
    let text = String::from_utf8(bytes).or_else(|_| fail(format!("{MANIFEST} must be UTF-8 text")))?;
    let mut pack = load_manifest(dir, &text, env)?;
    pack.fingerprint = fingerprint(&text, &pack, env);
    Ok(pack)
}

/// FNV-1a, 64 bits: fast, and good enough to tell that something changed. It isn't a security measure.
struct Fnv(u64);

impl Fnv {
    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ b as u64).wrapping_mul(0x0100_0000_01b3);
        }
        // Each part's length too, so that moving bytes between parts changes the digest.
        for b in (bytes.len() as u64).to_le_bytes() {
            self.0 = (self.0 ^ b as u64).wrapping_mul(0x0100_0000_01b3);
        }
    }
}

fn fingerprint(manifest: &str, pack: &Pack, env: &Environment) -> u64 {
    let mut h = Fnv(0xcbf2_9ce4_8422_2325);
    h.bytes(env!("CARGO_PKG_VERSION").as_bytes());
    h.bytes(manifest.as_bytes());
    for image in &pack.images {
        match image {
            ImageSource::File { name, bytes } => {
                h.bytes(name.as_bytes());
                h.bytes(bytes);
            }
            ImageSource::Pixels { name, width, height, rgba } => {
                h.bytes(name.as_bytes());
                h.bytes(&width.to_le_bytes());
                h.bytes(&height.to_le_bytes());
                h.bytes(rgba);
            }
        }
    }
    for (name, bytes) in pack.fonts.iter().chain(&pack.backgrounds) {
        h.bytes(name.as_bytes());
        h.bytes(bytes);
    }
    for art in &pack.arts {
        for row in art {
            h.bytes(row.iter().collect::<String>().as_bytes());
        }
    }
    match env.palette {
        None => h.bytes(b"default palette"),
        Some(palette) => {
            for (name, rgba) in &palette.colors {
                h.bytes(name.as_bytes());
                for c in rgba {
                    h.bytes(&c.to_bits().to_le_bytes());
                }
            }
            h.bytes(if palette.light { b"light" } else { b"dark" });
        }
    }
    h.0
}

fn load_manifest(dir: Dir, text: &str, env: &Environment) -> Result<Pack> {
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| {
        let line = e.span().map(|s| text[..s.start.min(text.len())].lines().count().max(1)).unwrap_or(0);
        LoadError(format!("{MANIFEST} line {line}: {}", e.message()))
    })?;
    let mut f = Fields::new(&table, "");
    let format = f.integer("format", 0, 0, i64::MAX)?;
    if format != FORMAT {
        return f.err("format", format!("must be {FORMAT} (this ScreenSafe reads pack format {FORMAT})"));
    }
    let preview = match f.get("preview") {
        None => None,
        Some(_) => Some(f.number("preview", 0.0, 0.0, 86400.0)?),
    };
    let mut meta = Meta {
        name: f.text("name", 60)?,
        author: f.text("author", 60)?,
        license: f.text("license", 40)?,
        description: f.text("description", 300)?,
        preview,
        settings: Vec::new(),
    };
    if meta.name.trim().is_empty() {
        return f.err("name", "is required");
    }
    let stage = match f.string("stage")?.as_deref() {
        None | Some("16:9") => Stage::Aspect(16.0 / 9.0),
        Some("screen") => Stage::Screen,
        Some(other) => match other.split_once(':').and_then(|(w, h)| Some((w.trim().parse::<f64>().ok()?, h.trim().parse::<f64>().ok()?))) {
            Some((w, h)) if w > 0.0 && h > 0.0 && (0.25..=4.0).contains(&(w / h)) => Stage::Aspect(w / h),
            _ => return f.err("stage", "must be \"screen\" or a ratio like \"16:9\" or \"4:3\""),
        },
    };
    // Pixel art: the whole screen, as a canvas of chunky pixels.
    let stage = match f.get("pixels") {
        None => stage,
        Some(_) => {
            if table.contains_key("stage") && stage != Stage::Screen {
                return f.err("pixels", "fills the screen, so stage must be \"screen\" or left out");
            }
            Stage::Pixels(f.integer("pixels", 224, MIN_PIXELS, MAX_PIXELS)? as u32)
        }
    };
    let fps = f.number("fps", 30.0, 1.0, MAX_FPS)?;
    let fade = f.number("fade", 0.5, 0.0, 5.0)?;
    let default_background = f.text("background", 200)?;
    let default_background = if default_background.is_empty() { "background".to_string() } else { default_background };

    let palette_owned;
    let palette = match env.palette {
        Some(p) => p,
        None => {
            palette_owned = Palette::default_theme();
            &palette_owned
        }
    };
    let mut loader = Loader {
        dir,
        env,
        slots: 0,
        initial: Vec::new(),
        builtins: Builtins {
            time: 0,
            cycle: 0,
            aspect: 0,
            screen_aspect: 0,
            t: 0,
            u: 0,
            dur: 0,
            i: 0,
            n: 0,
            k: 0,
            s: 0,
            j: 0,
            m: 0,
            e: 0,
            ex: 0,
            ey: 0,
            ez: 0,
        },
        images: Vec::new(),
        image_names: Vec::new(),
        image_sizes: Vec::new(),
        sheets: Vec::new(),
        sheet_pixels: 0,
        stage,
        arts: Vec::new(),
        art_names: Vec::new(),
        fonts: Vec::new(),
        font_names: Vec::new(),
        bytes_read: 0,
        elements: 0,
        uses_backgrounds: false,
        warnings: Vec::new(),
    };
    let mut b = loader.builtins;
    for slot in [
        &mut b.time,
        &mut b.cycle,
        &mut b.aspect,
        &mut b.screen_aspect,
        &mut b.t,
        &mut b.u,
        &mut b.dur,
        &mut b.i,
        &mut b.n,
        &mut b.k,
        &mut b.s,
        &mut b.j,
        &mut b.m,
        &mut b.e,
        &mut b.ex,
        &mut b.ey,
        &mut b.ez,
    ] {
        *slot = loader.alloc(1)?;
    }
    loader.builtins = b;
    let mut scope = Scope::default();
    scope.bind("time", b.time, Ty::Num);
    scope.bind("cycle", b.cycle, Ty::Num);
    scope.bind("aspect", b.aspect, Ty::Num);
    scope.bind("screen_aspect", b.screen_aspect, Ty::Num);
    // Reserved so a pack value can't take a name that a scene, element or curve provides.
    for name in ["t", "u", "dur"] {
        scope.reserve(name, "is only available inside a scene");
    }
    for name in ["i", "n", "k"] {
        scope.reserve(name, "is only available inside a drawing element");
    }
    for name in ["s", "j", "m"] {
        scope.reserve(name, "is only available in px and py");
    }
    for name in ["e", "ex", "ey", "ez"] {
        scope.reserve(name, "is only available in edge_color");
    }
    for (name, color) in &palette.colors {
        let slot = loader.alloc(4)?;
        loader.initial[slot as usize..slot as usize + 4].copy_from_slice(color);
        scope.bind(name, slot, Ty::Color);
    }
    // Whether the theme is light or dark, so a pack can choose what suits it: `dark` is always `!light`.
    for (name, value) in [("light", palette.light), ("dark", !palette.light)] {
        let slot = loader.alloc(1)?;
        loader.initial[slot as usize] = if value { 1.0 } else { 0.0 };
        scope.bind(name, slot, Ty::Bool);
    }

    let mut new_names: Vec<(String, u16, Ty)> = Vec::new();
    if let Some(value) = f.get("params") {
        let Some(params) = value.as_table() else { return f.err("params", "must be a table: [params]") };
        if params.len() > MAX_LETS {
            return f.err("params", format!("has more than {MAX_LETS} entries"));
        }
        for (name, value) in params {
            if !expr::valid_name(name) || scope.contains(name) {
                return f.err("params", format!("'{name}' is not a free name: use lowercase letters, digits and _, not a built-in name"));
            }
            let Some(v) = number(value) else { return f.err("params", format!("{name} must be a number")) };
            let slot = loader.alloc(1)?;
            loader.initial[slot as usize] = v;
            new_names.push((name.clone(), slot, Ty::Num));
        }
    }
    for (name, slot, ty) in new_names.drain(..) {
        scope.bind(&name, slot, ty);
    }
    // `[[setting]]`: what someone may change in the panel. Each is a name like a param's, holding the value chosen.
    let declared = f.tables("setting")?;
    if declared.len() > MAX_SETTINGS {
        return f.err("setting", format!("a pack can offer at most {MAX_SETTINGS} settings"));
    }
    for (n, table) in declared.into_iter().enumerate() {
        let mut sf = Fields::new(table, format!("setting {}", n + 1));
        let setting = read_setting(&mut sf, &scope, &meta.settings, loader.env.settings)?;
        sf.finish()?;
        let slot = loader.alloc(1)?;
        loader.initial[slot as usize] = setting.value;
        let ty = if setting.kind == SettingKind::Toggle { Ty::Bool } else { Ty::Num };
        scope.bind(&setting.name, slot, ty);
        meta.settings.push(setting);
    }
    if let Some(value) = f.get("colors") {
        let Some(colors) = value.as_table() else { return f.err("colors", "must be a table: [colors]") };
        if colors.len() > MAX_LETS {
            return f.err("colors", format!("has more than {MAX_LETS} entries"));
        }
        let mut machine = expr::Machine::new(0);
        for (name, value) in colors {
            if !expr::valid_name(name) || scope.contains(name) {
                return f.err("colors", format!("'{name}' is not a free name: use lowercase letters, digits and _, not a palette or built-in name"));
            }
            let Some(src) = value.as_str() else { return f.err("colors", format!("{name} must be a color expression in quotes")) };
            let program = expr::compile(src, &scope, Ty::Color).or_else(|e| f.err("colors", format!("{name}: {e}\n    {src}{}", caret(e.at))))?;
            let color = machine.color(&program, &loader.initial);
            let slot = loader.alloc(4)?;
            loader.initial[slot as usize..slot as usize + 4].copy_from_slice(&color);
            new_names.push((name.clone(), slot, Ty::Color));
        }
    }
    for (name, slot, ty) in new_names {
        scope.bind(&name, slot, ty);
    }

    loader.fonts(&mut f)?;

    let scene_tables = f.tables("scene")?;
    if scene_tables.is_empty() {
        return f.err("scene", "a pack needs at least one [[scene]]");
    }
    if scene_tables.len() > MAX_SCENES {
        return f.err("scene", format!("a pack can have at most {MAX_SCENES} scenes"));
    }
    let mut scenes = Vec::new();
    for (i, table) in scene_tables.into_iter().enumerate() {
        scenes.push(loader.scene(table, i, &scope, (fps, fade, &default_background))?);
    }
    let length: f64 = scenes.iter().map(Scene::length).sum();
    if let Some(at) = preview {
        if at >= length {
            return f.err("preview", format!("is at or past the pack's end, {length} seconds in"));
        }
    }
    f.finish()?;

    let mut backgrounds = Vec::new();
    if loader.uses_backgrounds {
        match env.theme {
            None => loader.warnings.push("the pack shows the theme's backgrounds, but no theme was given".into()),
            Some(theme) => backgrounds = read_backgrounds(theme, &mut loader.warnings),
        }
    }
    Ok(Pack {
        meta,
        stage,
        scenes,
        builtins: loader.builtins,
        initial: loader.initial,
        images: loader.images,
        backgrounds,
        arts: loader.arts,
        fonts: loader.fonts,
        warnings: loader.warnings,
        fingerprint: 0,
    })
}

/// The theme's background images, sorted by name. Unreadable ones are skipped with a warning.
fn read_backgrounds(theme: &Path, warnings: &mut Vec<String>) -> Vec<(String, Vec<u8>)> {
    let dir = match Dir::open(theme).and_then(|d| d.subdir("backgrounds")) {
        Ok(d) => d,
        Err(e) => {
            warnings.push(format!("the theme's backgrounds can't be read: {e}"));
            return Vec::new();
        }
    };
    let names = match dir.files(256) {
        Ok(n) => n,
        Err(e) => {
            warnings.push(e.0);
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    let mut total = 0u64;
    for name in names {
        let lower = name.to_ascii_lowercase();
        if ![".png", ".jpg", ".jpeg", ".webp"].iter().any(|ext| lower.ends_with(ext)) {
            continue;
        }
        if out.len() == BACKGROUNDS_LIMIT {
            warnings.push(format!("only the first {BACKGROUNDS_LIMIT} theme backgrounds are shown"));
            break;
        }
        match dir.read(&name, IMAGE_FILE_LIMIT) {
            Ok(bytes) if total + bytes.len() as u64 <= PACK_BYTES_LIMIT => {
                total += bytes.len() as u64;
                out.push((name, bytes));
            }
            Ok(_) => {
                warnings.push("the theme's backgrounds are too large to show them all".into());
                break;
            }
            Err(e) => warnings.push(e.0),
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn temp_pack(name: &str, manifest: &str, files: &[(&str, &[u8])]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("screensafe-pack-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MANIFEST), manifest).unwrap();
        for (rel, bytes) in files {
            std::fs::write(dir.join(rel), bytes).unwrap();
        }
        dir
    }

    fn load_str(name: &str, manifest: &str) -> Result<Pack> {
        let dir = temp_pack(name, manifest, &[("art.txt", b"  /\\\n /  \\\n/____\\\n")]);
        let result = load(&dir, &Environment::default());
        std::fs::remove_dir_all(&dir).unwrap();
        result
    }

    fn err(name: &str, manifest: &str) -> String {
        match load_str(name, manifest) {
            Ok(_) => panic!("{name} loaded"),
            Err(e) => e.0,
        }
    }

    const HEADER: &str = "format = 1\nname = \"Test\"\n";

    #[test]
    fn settings_are_declared_checked_and_bound() {
        let decl = "[[setting]]\nname = \"pace\"\nlabel = \"Speed\"\nkind = \"choice\"\nchoices = [\"Slow\", \"Fast\"]\nvalues = [0.5, 2]\ndefault = 0.5\n\
                    [[setting]]\nname = \"rain\"\nlabel = \"Rain\"\nkind = \"toggle\"\ndefault = true\n\
                    [[setting]]\nname = \"count\"\nlabel = \"Stars\"\nkind = \"range\"\nmin = 100\nmax = 400\nstep = 50\ndefault = 200\n";
        let scene = "[[scene]]\nduration = 5\n[[scene.draw]]\nkind = \"rect\"\nvisible = \"rain\"\nx = \"pace + count\"\n";
        let dir = temp_pack("settings", &format!("{HEADER}{decl}{scene}"), &[]);
        let load_with = |given: &[(String, f64)]| load(&dir, &Environment { settings: given, ..Default::default() }).unwrap_or_else(|e| panic!("{e}"));
        let values = |p: &Pack| p.meta.settings.iter().map(|s| s.value).collect::<Vec<_>>();

        let defaults = load_with(&[]);
        assert_eq!(values(&defaults), vec![0.5, 1.0, 200.0]);
        assert_eq!(defaults.meta.settings[0].label, "Speed");
        // Values given are taken when valid: a range's snapped to its steps, a toggle's anything but zero.
        let given = [("pace".to_string(), 2.0), ("rain".to_string(), 0.0), ("count".to_string(), 262.0), ("other".to_string(), 9.0)];
        assert_eq!(values(&load_with(&given)), vec![2.0, 0.0, 250.0]);
        // Otherwise the defaults: a choice that isn't offered, a range out of bounds.
        let wrong = [("pace".to_string(), 3.0), ("count".to_string(), 900.0)];
        assert_eq!(values(&load_with(&wrong)), vec![0.5, 1.0, 200.0]);
        std::fs::remove_dir_all(&dir).unwrap();

        // Declarations are checked like everything else.
        let bad = |d: &str| err("badsetting", &format!("{HEADER}{d}{scene}"));
        assert!(bad("[[setting]]\nname = \"t\"\nlabel = \"T\"\nkind = \"toggle\"\n").contains("not a free name"));
        assert!(bad("[[setting]]\nname = \"a\"\nlabel = \"A\"\nkind = \"slider\"\n").contains("kind"));
        assert!(bad("[[setting]]\nname = \"a\"\nlabel = \"A\"\nkind = \"choice\"\nchoices = [\"One\"]\nvalues = [1]\n").contains("2 to 6"));
        assert!(bad("[[setting]]\nname = \"a\"\nlabel = \"A\"\nkind = \"range\"\nmin = 0\nmax = 1\nstep = 0.0001\n").contains("1000 steps"));
        assert!(bad("[[setting]]\nname = \"a\"\nlabel = \"A\"\nkind = \"toggle\"\ncolour = 1\n").contains("isn't a setting"));
        let many: String = (0..9).map(|i| format!("[[setting]]\nname = \"s{i}\"\nlabel = \"S\"\nkind = \"toggle\"\n")).collect();
        assert!(bad(&many).contains("at most 8"));

        // A setting can choose how long a scene lasts, worked out as the pack loads.
        let timed =
            "[[setting]]\nname = \"each\"\nlabel = \"Each picture\"\nkind = \"choice\"\nchoices = [\"Short\", \"Long\"]\nvalues = [10, 40]\ndefault = 10\n\
                     [[scene]]\nduration = \"each * 1.5\"\nhold = \"each / 10\"\n[[scene.draw]]\nkind = \"rect\"\n";
        let dir = temp_pack("timed", &format!("{HEADER}{timed}"), &[]);
        let long = [("each".to_string(), 40.0)];
        let pack = load(&dir, &Environment { settings: &long, ..Default::default() }).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((pack.scenes[0].duration, pack.scenes[0].hold), (60.0, 4.0));
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(err("timedbad", &format!("{HEADER}[[scene]]\nduration = \"0 - 5\"\n")).contains("comes to -5"));
    }

    #[test]
    fn minimal_and_full_packs_load() {
        let pack = load_str("min", &format!("{HEADER}[[scene]]\nduration = 2\n[[scene.draw]]\nkind = \"rect\"\n")).unwrap();
        assert_eq!(pack.scenes.len(), 1);
        assert_eq!(pack.length(), 2.0);
        let full = format!(
            r##"{HEADER}author = "A"
license = "MIT"
stage = "screen"
fps = 24
[params]
spin = 0.9
[colors]
glow = "mix(accent, foreground, 0.3)"
[[scene]]
name = "all"
duration = 3
hold = 2
let = ["a = t * spin", "c = a > 1 ? glow : red"]
background = "darker_background"
[[scene.draw]]
kind = "group"
count = 3
let = ["gx = i * 0.1"]
clip = ["-1", "-0.5", "1", "0.5 * u"]
  [[scene.draw.draw]]
  kind = "lines"
  samples = 100
  px = "s - 0.5 + gx"
  py = "0.1 * sin(s * tau + t)"
  color = "c"
  [[scene.draw.draw]]
  kind = "mesh"
  vertices = [[-1, -1, -1], [1, -1, -1], [1, 1, -1]]
  edges = [[0, 1], [1, 2]]
  ry = "a"
[[scene.draw]]
kind = "text"
text = "INSTALL TIME: {{{{ {{max(0, 420 - t * 40); 03}} SEC }}}}"
reveal = "t * 130"
cursor = "fract(t) < 0.5"
[[scene.draw]]
kind = "ascii"
art = "art.txt"
effect = "decrypt"
colors = ["accent", "foreground"]
"##
        );
        let pack = load_str("full", &full).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(pack.stage, Stage::Screen);
        assert_eq!(pack.scenes[0].fps, 24.0);
        assert_eq!(pack.arts.len(), 1);
        let Kind::Text(text) = &pack.scenes[0].draw[1].kind else { panic!("text") };
        assert_eq!(text.segments.len(), 3);
        assert!(matches!(&text.segments[0], Segment::Literal(l) if l == "INSTALL TIME: { "));
        assert!(matches!(&text.segments[1], Segment::Value(_, f) if *f == Format { width: 3, zero: true, decimals: 0 }));
    }

    #[test]
    fn helpful_errors() {
        let scene = "[[scene]]\nduration = 2\n";
        assert!(err("fmt", "format = 2\nname = \"x\"\n").contains("must be 1"));
        assert!(err("noname", "format = 1\n").contains("name: is required"));
        assert!(err("noscene", HEADER).contains("at least one [[scene]]"));
        assert!(err("unknown", &format!("{HEADER}colour = 1\n{scene}")).contains("colour: isn't a setting"));
        assert!(err("preview", &format!("{HEADER}preview = 2\n{scene}")).contains("preview: is at or past the pack's end"));
        assert!(err("preview-neg", &format!("{HEADER}preview = -1\n{scene}")).contains("preview"));
        let pack = load_str("preview-ok", &format!("{HEADER}preview = 1.5\n{scene}")).unwrap();
        assert_eq!(pack.meta.preview, Some(1.5));
    }

    #[test]
    fn fingerprints_follow_what_a_pack_is_made_from() {
        let scene = "[[scene]]\nduration = 2\n";
        let a = load_str("fp-a", &format!("{HEADER}{scene}")).unwrap().fingerprint;
        let again = load_str("fp-a2", &format!("{HEADER}{scene}")).unwrap().fingerprint;
        let b = load_str("fp-b", &format!("{HEADER}[[scene]]\nduration = 3\n")).unwrap().fingerprint;
        assert_ne!(a, 0);
        assert_eq!(a, again);
        assert_ne!(a, b);
        let image = |name: &str, bytes: &[u8]| {
            let manifest = format!("{HEADER}[[scene]]\nduration = 2\n[[scene.draw]]\nkind = \"image\"\nfile = \"a.png\"\n");
            load(&temp_pack(name, &manifest, &[("a.png", bytes)]), &Environment::default()).unwrap().fingerprint
        };
        let png = crate::raster::tests::tiny_png();
        let mut changed = png.clone();
        changed.push(0);
        assert_ne!(image("fp-img1", &png), image("fp-img2", &changed));
        let theme = Palette::from_toml("foreground = \"#ff0000\"\n").unwrap();
        let env = Environment { palette: Some(&theme), ..Default::default() };
        let dir = temp_pack("fp-theme", &format!("{HEADER}{scene}"), &[]);
        assert_ne!(load(&dir, &env).unwrap().fingerprint, load(&dir, &Environment::default()).unwrap().fingerprint);
        // A theme's mode alone changes what a pack may draw, so its fingerprint too.
        let light = Palette::from_toml("mode = \"light\"\n").unwrap();
        let dark = Palette::from_toml("mode = \"dark\"\n").unwrap();
        let fp = |p: &Palette| load(&dir, &Environment { palette: Some(p), ..Default::default() }).unwrap().fingerprint;
        assert_ne!(fp(&light), fp(&dark));
        assert!(err("kind", &format!("{HEADER}{scene}[[scene.draw]]\nkind = \"blob\"\n")).contains("'blob' isn't a kind"));
        let e = err("expr", &format!("{HEADER}{scene}[[scene.draw]]\nkind = \"rect\"\nx = \"sin(t\"\n"));
        assert!(e.contains("scene 1 › draw 1 › x") && e.contains("expected ',' or ')' in the call to sin()") && e.contains('^'), "{e}");
        assert!(err("typo", &format!("{HEADER}{scene}[[scene.draw]]\nkind = \"rect\"\ncolor = \"acent\"\n")).contains("did you mean 'accent'"));
        assert!(err("let", &format!("{HEADER}{scene}let = [\"t = 1\"]\n")).contains("already defined"));
        // Per-sample values belong to sampled lines, and are costed for every sample.
        let fixed = "[[scene]]\nduration = 5\n[[scene.draw]]\nkind = \"lines\"\npoints = [[0, 0], [1, 1]]\nsample_let = [\"w = 1\"]\n";
        assert!(err("sample_let", &format!("{HEADER}{fixed}")).contains("sample_let"));
        let heavy = "[[scene]]\nduration = 5\n[[scene.draw]]\nkind = \"lines\"\ncount = 1024\nsamples = 4096\n\
                     sample_let = [\"w = sin(s) * cos(s) * sin(s) * cos(s)\"]\npx = \"w\"\n";
        assert!(err("heavy", &format!("{HEADER}{heavy}")).contains("operations"));
        assert!(err("letname", &format!("{HEADER}{scene}let = [\"Sin = 1\"]\n")).contains("isn't a valid name"));
        assert!(err("letform", &format!("{HEADER}{scene}let = [\"x == 1\"]\n")).contains("name = expression"));
        assert!(err("param", &format!("{HEADER}[params]\npi = 3\n{scene}")).contains("not a free name"));
        assert!(err("param2", &format!("{HEADER}[params]\nu = 3\n{scene}")).contains("not a free name"));
        assert!(err("colort", &format!("{HEADER}[colors]\nc = \"t > 1 ? red : blue\"\n{scene}")).contains("only available inside a scene"));
        assert!(err("count", &format!("{HEADER}{scene}[[scene.draw]]\nkind = \"rect\"\ncount = 5000\n")).contains("between 1 and 1024"));
        assert!(err("brace", &format!("{HEADER}{scene}[[scene.draw]]\nkind = \"text\"\ntext = \"a {{t\"\n")).contains("without a '}'"));
        assert!(err("spec", &format!("{HEADER}{scene}[[scene.draw]]\nkind = \"text\"\ntext = \"{{t;x}}\"\n")).contains("isn't a format"));
        assert!(err("path", &format!("{HEADER}{scene}[[scene.draw]]\nkind = \"ascii\"\nart = \"../x.txt\"\n")).contains("plain relative path"));
        assert!(err("img", &format!("{HEADER}{scene}[[scene.draw]]\nkind = \"image\"\nfile = \"x.gif\"\n")).contains(".png"));
        assert!(err("edges", &format!("{HEADER}{scene}[[scene.draw]]\nkind = \"mesh\"\nvertices = [[0,0,0],[1,1,1]]\nedges = [[0,2]]\n"))
            .contains("vertex numbers"));
        assert!(err("stage", &format!("{HEADER}stage = \"wide\"\n{scene}")).contains("ratio"));
        assert!(err("toml", "format = = 1").contains("line 1"));
    }

    #[test]
    fn sprites_and_pixel_art_are_checked() {
        let png = crate::raster::tests::tiny_png();
        let scene = "[[scene]]\nduration = 5\n";
        let sprite = |extra: &str| format!("{HEADER}pixels = 224\n{scene}[[scene.draw]]\nkind = \"sprite\"\nfile = \"s.png\"\n{extra}");
        let try_load = |name: &str, manifest: &str| {
            let dir = temp_pack(name, manifest, &[("s.png", &png)]);
            let result = load(&dir, &Environment::default());
            std::fs::remove_dir_all(&dir).unwrap();
            result
        };
        let pack = try_load("sprite-ok", &sprite("frame_size = [2, 2]\nanchor = [1, 2]\nframe = \"t * 8\"\nflip = \"t > 1\"\n")).unwrap();
        assert_eq!(pack.stage, Stage::Pixels(224));
        let Kind::Sprite(s) = &pack.scenes[0].draw[0].kind else { panic!("a sprite") };
        assert_eq!((s.columns, s.frames, s.anchor), (2, 2, [1.0, 2.0]));
        let e = |name: &str, extra: &str| try_load(name, &sprite(extra)).err().map(|e| e.0).unwrap_or_default();
        assert!(e("big", "frame_size = [8, 2]\n").contains("larger than the sheet"));
        assert!(e("zero", "frame_size = [0, 2]\n").contains("whole numbers"));
        assert!(e("anchor", "anchor = [\"a\", 1]\n").contains("two numbers"));
        let e = try_load("stage", &format!("{HEADER}stage = \"16:9\"\npixels = 224\n{scene}")).err().unwrap().0;
        assert!(e.contains("stage must be"), "{e}");
        let e = try_load("few", &format!("{HEADER}pixels = 8\n{scene}")).err().unwrap().0;
        assert!(e.contains("between 64"), "{e}");
    }

    #[test]
    fn fonts_are_checked() {
        let with_font = |name: &str, fonts: &str, bytes: &[u8], text: &str| {
            let manifest = format!("{HEADER}[fonts]\n{fonts}\n[[scene]]\nduration = 2\n[[scene.draw]]\nkind = \"text\"\ntext = \"A\"\n{text}");
            let dir = temp_pack(name, &manifest, &[("f.ttf", bytes), ("f.txt", bytes)]);
            let result = load(&dir, &Environment::default());
            std::fs::remove_dir_all(&dir).unwrap();
            result
        };
        let font = crate::raster::FONT;
        let pack = with_font("fontok", "mono = \"f.ttf\"", font, "font = \"mono\"\nstretch = 0.7\n").unwrap();
        assert_eq!(pack.fonts.len(), 1);
        let Kind::Text(text) = &pack.scenes[0].draw[0].kind else { panic!() };
        assert_eq!((text.font, text.stretch), (1, 0.7));
        let e = |r: Result<Pack>| r.err().map(|e| e.0).unwrap_or_default();
        assert!(e(with_font("fontbad", "mono = \"f.ttf\"", b"not a font at all", "")).contains("isn't a font ScreenSafe can read"));
        assert!(e(with_font("fontext", "mono = \"f.txt\"", font, "")).contains("must be .ttf or .otf"));
        assert!(e(with_font("fontname", "mono = \"f.ttf\"", font, "font = \"sans\"\n")).contains("'sans' isn't one of the pack's [fonts]"));
        assert!(e(with_font("fontstretch", "mono = \"f.ttf\"", font, "stretch = 9\n")).contains("stretch"));
        assert!(e(with_font("fontup", "Mono = \"f.ttf\"", font, "")).contains("isn't a valid name"));
    }

    #[test]
    fn glow_settings() {
        let scene = "[[scene]]\nduration = 2\n[[scene.draw]]\nkind = \"rect\"\n";
        let pack = load_str("glowok", &format!("{HEADER}{scene}glow = \"0.5 + 0.5 * sin(t)\"\nglow_radius = 0.01\n")).unwrap();
        let glow = pack.scenes[0].draw[0].glow.as_ref().unwrap();
        assert_eq!(glow.radius, 0.01);
        assert!(err("glowalone", &format!("{HEADER}{scene}glow_radius = 0.01\n")).contains("only applies with glow"));
        assert!(err("glowwide", &format!("{HEADER}{scene}glow = 1\nglow_radius = 1\n")).contains("glow_radius"));
    }

    #[test]
    fn budgets_are_enforced() {
        let heavy = format!("{HEADER}[[scene]]\n[[scene.draw]]\nkind = \"lines\"\ncount = 1024\nsamples = 4096\npx = \"s\"\npy = \"sin(s)\"\n");
        assert!(err("heavy", &heavy).contains("limit"));
        let elements = format!("{HEADER}[[scene]]\n{}", "[[scene.draw]]\nkind = \"rect\"\n".repeat(MAX_ELEMENTS + 1));
        assert!(err("many", &elements).contains("at most"));
        let mut nested = format!("{HEADER}[[scene]]\n");
        let mut prefix = "scene.draw".to_string();
        for _ in 0..6 {
            nested.push_str(&format!("[[{prefix}]]\nkind = \"group\"\n"));
            prefix.push_str(".draw");
        }
        nested.push_str(&format!("[[{prefix}]]\nkind = \"rect\"\n"));
        assert!(err("nest", &nested).contains("nest at most"));
    }

    #[test]
    fn formats_numbers() {
        let f = |w, z, d| Format { width: w, zero: z, decimals: d };
        assert_eq!(format_number(69.4, f(3, true, 0)), "069");
        assert_eq!(format_number(-5.0, f(4, true, 0)), "-005");
        assert_eq!(format_number(2.345, f(0, false, 2)), "2.35");
        assert_eq!(format_number(7.0, f(3, false, 0)), "  7");
        assert_eq!(format_number(-0.2, f(0, false, 0)), "0");
        assert_eq!(format_number(1e9, f(3, true, 0)), "1000000000");
        assert_eq!(parse_format("03"), Some(f(3, true, 0)));
        assert_eq!(parse_format(".2"), Some(f(0, false, 2)));
        assert_eq!(parse_format("5.1"), Some(f(5, false, 1)));
        assert_eq!(parse_format("0"), Some(f(0, false, 0)));
        assert_eq!(parse_format("x"), None);
        assert_eq!(parse_format(".9"), None);
    }

    #[test]
    fn art_parsing() {
        assert_eq!(parse_art(b"\n\nab\t c  \n\n").unwrap(), vec![vec!['a', 'b', ' ', ' ', ' ', 'c']]);
        assert!(parse_art(b"a\x1b[31mb").is_err());
        assert!(parse_art(b"\xff").is_err());
        assert!(parse_art(b"   \n").is_err());
        assert!(parse_art(&vec![b'x'; 500]).is_err());
        assert_eq!(split_let("a = b == c"), Some(("a", "b == c")));
        assert_eq!(split_let("a <= b"), None);
        assert_eq!(split_let("x = y >= 1 ? 1 : 0"), Some(("x", "y >= 1 ? 1 : 0")));
    }

    #[test]
    fn branding_and_backgrounds() {
        let dir = temp_pack("brand", &format!("{HEADER}[[scene]]\n[[scene.draw]]\nkind = \"ascii\"\nsource = \"branding\"\n[[scene.draw]]\nkind = \"image\"\nsource = \"theme-backgrounds\"\n"), &[]);
        let art = dir.join("branding.txt");
        std::fs::write(&art, "OMARCHY").unwrap();
        let theme = dir.join("theme");
        std::fs::create_dir_all(theme.join("backgrounds")).unwrap();
        std::fs::write(theme.join("backgrounds/1.png"), crate::raster::tests::tiny_png()).unwrap();
        std::fs::write(theme.join("backgrounds/notes.md"), "x").unwrap();
        let env = Environment { palette: None, theme: Some(&theme), branding: Some(&art), logo: None, badge: None, settings: &[] };
        let pack = load(&dir, &env).unwrap();
        assert_eq!(pack.arts[0], vec!["OMARCHY".chars().collect::<Vec<_>>()]);
        assert_eq!(pack.backgrounds.len(), 1);
        let pack = load(&dir, &Environment::default()).unwrap();
        assert_eq!(pack.warnings.len(), 2);
        // Missing art falls back to the logo.
        let logo = dir.join("logo.txt");
        std::fs::write(&logo, "LOGO").unwrap();
        let missing = dir.join("missing.txt");
        let env = Environment { palette: None, theme: None, branding: Some(&missing), logo: Some(&logo), badge: None, settings: &[] };
        let pack = load(&dir, &env).unwrap();
        assert_eq!(pack.arts[0], vec!["LOGO".chars().collect::<Vec<_>>()]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
