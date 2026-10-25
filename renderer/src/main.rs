//! ScreenSafe: screensavers for Omarchy made of data, not code.
#![forbid(unsafe_code)]

/// Prints a line to standard output. If it can't be written, because whoever was reading has gone (the shell reloaded
/// or exited, say), the renderer exits quietly with 141, as a program killed by a broken pipe does, rather than
/// panicking, which with `panic = "abort"` would be reported as a crash.
macro_rules! say {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let mut out = std::io::stdout().lock();
        if writeln!(out, $($arg)*).and_then(|_| out.flush()).is_err() {
            std::process::exit(141);
        }
    }};
}

/// Prints a line to standard error, if anyone is still reading it; if not, the message is dropped.
macro_rules! tell {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr().lock(), $($arg)*);
    }};
}

mod clock;
mod crt;
mod eval;
mod expr;
mod flash;
mod overlay;
mod pack;
mod palette;
mod raster;
mod safefs;
mod sandbox;
mod takeover;
mod video;
mod wayland;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use pack::{Environment, Pack};
use palette::Palette;
use raster::{Raster, View};

const USAGE: &str = "usage:
  screensafe run --pack DIR [--theme DIR] [--branding FILE] [--logo FILE] [--badge PATH] [--photos DIR] [--videos DIR] [--settings NAME=VALUE,...] [--overlay NAME=VALUE,...] [--seed N] [--max-fps N] [--start SECONDS] [--exit-after SECONDS]
  screensafe check DIR [--theme DIR] [--branding FILE] [--logo FILE] [--badge PATH] [--photos DIR] [--videos DIR] [--settings NAME=VALUE,...] [--overlay NAME=VALUE,...] [--seconds N] [--size WxH]
  screensafe render DIR --time SECONDS --out FILE.png [--size WxH] [--theme DIR] [--branding FILE] [--logo FILE] [--badge PATH] [--photos DIR] [--videos DIR] [--settings NAME=VALUE,...] [--overlay NAME=VALUE,...]
  screensafe catalog DIR... [--theme DIR] [--branding FILE] [--logo FILE] [--badge PATH] [--photos DIR] [--videos DIR]
  screensafe preview --pack DIR --region X,Y,W,H [--output NAME] [--theme DIR] [--branding FILE] [--logo FILE] [--badge PATH] [--photos DIR] [--videos DIR] [--settings NAME=VALUE,...] [--overlay NAME=VALUE,...]
  screensafe thumbnail DIR [--size WxH] [--time SECONDS] [--unless FINGERPRINT] [--theme DIR] [--branding FILE] [--logo FILE] [--badge PATH] [--photos DIR] [--videos DIR] [--settings NAME=VALUE,...]
  screensafe takeover claim TOKEN | release [TOKEN] | status
  screensafe photos-folder
  screensafe videos-folder
  screensafe system-info
  screensafe sandbox-test
  screensafe version";

/// Output for the plugin is bounded, so a listing can never grow without limit.
const OUTPUT_LIMIT: usize = 64 * 1024;
/// A thumbnail is bounded: at most this many pixels a side, sent as plain pixels, three bytes each.
const THUMBNAIL_SIDE: u32 = 480;

struct Args {
    positional: Vec<String>,
    options: Vec<(String, String)>,
}

impl Args {
    fn parse(raw: impl Iterator<Item = String>) -> Result<Args, String> {
        let mut positional = Vec::new();
        let mut options = Vec::new();
        let mut raw = raw.peekable();
        while let Some(arg) = raw.next() {
            if let Some(name) = arg.strip_prefix("--") {
                let value = raw.next().ok_or_else(|| format!("--{name} needs a value"))?;
                options.push((name.to_string(), value));
            } else {
                positional.push(arg);
            }
        }
        Ok(Args { positional, options })
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.options.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        self.get(name).map(PathBuf::from)
    }

    fn number(&self, name: &str) -> Result<Option<f64>, String> {
        match self.get(name) {
            None => Ok(None),
            Some(v) => v.parse::<f64>().ok().filter(|n| n.is_finite()).map(Some).ok_or_else(|| format!("--{name} must be a number")),
        }
    }

    fn only(&self, allowed: &[&str]) -> Result<(), String> {
        match self.options.iter().find(|(n, _)| !allowed.contains(&n.as_str())) {
            Some((n, _)) => Err(format!("--{n} isn't an option here\n{USAGE}")),
            None => Ok(()),
        }
    }
}

/// A pack's settings, for the panel: each one's name, label, kind and default, and its choices or range.
fn settings_json(settings: &[pack::Setting]) -> String {
    let items: Vec<String> = settings
        .iter()
        .map(|s| {
            let kind = match &s.kind {
                pack::SettingKind::Toggle => "\"kind\":\"toggle\"".to_string(),
                pack::SettingKind::Choice { labels, values } => format!(
                    "\"kind\":\"choice\",\"choices\":[{}],\"values\":[{}]",
                    labels.iter().map(|l| json(l)).collect::<Vec<_>>().join(","),
                    values.iter().map(|v| format!("{v}")).collect::<Vec<_>>().join(",")
                ),
                pack::SettingKind::Range { min, max, step, unit } => {
                    format!("\"kind\":\"range\",\"min\":{min},\"max\":{max},\"step\":{step},\"unit\":{}", json(unit))
                }
                pack::SettingKind::Color { when } => {
                    let condition = when.as_ref().map(|(name, value)| format!(",\"when\":{},\"whenValue\":{value}", json(name))).unwrap_or_default();
                    format!("\"kind\":\"color\"{condition}")
                }
            };
            format!("{{\"name\":{},\"label\":{},{kind},\"default\":{}}}", json(&s.name), json(&s.label), s.default)
        })
        .collect();
    format!("[{}]", items.join(","))
}

fn json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn theme_palette(theme: Option<&Path>) -> Result<Option<Palette>, String> {
    let Some(theme) = theme else { return Ok(None) };
    match safefs::Dir::open(theme).and_then(|d| d.read("colors.toml", 64 * 1024)) {
        Ok(bytes) => {
            let text = String::from_utf8(bytes).map_err(|_| "the theme's colors.toml isn't UTF-8".to_string())?;
            Palette::from_toml(&text).map(Some)
        }
        // A theme without colors.toml draws in Omarchy's default palette.
        Err(_) => Ok(None),
    }
}

/// Confines the renderer to reading `dirs` and the theme and art it was given, before it parses anything.
fn video_folder(args: &Args) -> Option<PathBuf> {
    args.path("videos").or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(video::FOLDER)))
}

fn confine_reading(args: &Args, dirs: &[PathBuf]) -> Result<&'static str, String> {
    let mut paths: Vec<PathBuf> = dirs.to_vec();
    paths.extend(["theme", "branding", "logo", "photos", "videos"].iter().filter_map(|k| args.path(k)));
    // Someone's badge: only the few files it may be, not the folder they're in.
    if let Some(badge) = args.path("badge") {
        paths.extend(pack::BADGE_EXTENSIONS.iter().map(|ext| badge.with_extension(ext)));
    }
    paths.extend(video_folder(args));
    let refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
    sandbox::confine_reading(&refs)
}

/// `--settings name=value,...`: values for a pack's settings, as numbers or true/false. Up to 8, in at most 1 KiB;
/// the pack itself decides which it offers and which values it accepts.
fn settings(args: &Args) -> Result<Vec<(String, f64)>, String> {
    match args.get("settings") {
        None => Ok(Vec::new()),
        Some(text) => parse_settings(text),
    }
}

fn parse_settings(text: &str) -> Result<Vec<(String, f64)>, String> {
    if text.len() > 1024 {
        return Err("--settings is longer than 1024 characters".into());
    }
    let mut out = Vec::new();
    for pair in text.split(',').filter(|p| !p.is_empty()) {
        let (name, value) = pair.split_once('=').ok_or_else(|| format!("--settings: '{pair}' must look like name=value"))?;
        let value = match value {
            "true" => 1.0,
            "false" => 0.0,
            v => v.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("--settings: {name}'s value must be a number, true or false"))?,
        };
        out.push((name.to_string(), value));
        if out.len() > pack::MAX_SETTINGS {
            return Err(format!("--settings gives more than {} values", pack::MAX_SETTINGS));
        }
    }
    Ok(out)
}

fn load(dir: &Path, args: &Args) -> Result<Pack, String> {
    load_with(dir, args, &settings(args)?, None)
}

/// Loads a pack with the settings `chosen`. With more photos than are held open, `photo_seed` picks which; without
/// one, it's the first by name, so the result, and the pack's fingerprint, is the same every time.
fn load_with(dir: &Path, args: &Args, chosen: &[(String, f64)], photo_seed: Option<u64>) -> Result<Pack, String> {
    let theme = args.path("theme");
    let branding = args.path("branding");
    let logo = args.path("logo");
    let badge = args.path("badge");
    let photos = args.path("photos");
    let videos = video_folder(args);
    let palette = theme_palette(theme.as_deref())?;
    let env = Environment {
        palette: palette.as_ref(),
        theme: theme.as_deref(),
        branding: branding.as_deref(),
        logo: logo.as_deref(),
        badge: badge.as_deref(),
        settings: chosen,
        photos: photos.as_deref(),
        videos: videos.as_deref(),
        photo_seed,
    };
    pack::load(dir, &env).map_err(|e| e.0)
}

/// `run` exits with this when the pack can't be used, so the plugin can fall back to another.
const PACK_UNUSABLE: u8 = 3;

fn run(args: &Args) -> Result<Result<(), String>, String> {
    args.only(&["overlay", "pack", "theme", "branding", "logo", "badge", "photos", "videos", "settings", "seed", "max-fps", "exit-after", "start", "role"])?;
    let dir = args.path("pack").ok_or(USAGE)?;
    let seed = match args.number("seed")? {
        Some(s) => s as u64,
        None => std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1),
    };
    let start = args.number("start")?.unwrap_or(0.0).clamp(0.0, 86400.0);
    // The time zone is read before the renderer confines itself.
    let mut clock_warnings = Vec::new();
    let clock = clock::Clock::local(&mut clock_warnings).starting_at(start);
    let options = wayland::Options {
        seed,
        exit_after: args.number("exit-after")?.map(|s| Duration::from_secs_f64(s.clamp(0.0, 86400.0))),
        max_fps: args.number("max-fps")?.unwrap_or(pack::MAX_FPS).clamp(1.0, pack::MAX_FPS),
        start,
        clock: Some(clock),
        overlay: overlay::Config::parse(args.get("overlay").unwrap_or(""))?,
    };
    confine_reading(args, std::slice::from_ref(&dir))?;
    let pack = match load_with(&dir, args, &settings(args)?, Some(seed)) {
        Ok(p) => p,
        Err(e) => return Ok(Err(e)),
    };
    for w in &clock_warnings {
        tell!("screensafe: {w}");
    }
    let mut raster = Raster::new();
    raster.background_decoding = true;
    let display = wayland::connect()?;
    let confinement = sandbox::confine()?;
    // The pack's fonts are read only once the renderer is confined.
    raster.load_fonts(&pack);
    tell!("screensafe: showing {}; {confinement}; {}", pack.meta.name, display.format());
    for w in &pack.warnings {
        tell!("screensafe: {w}");
    }
    let why = display.run(&pack, &options, &mut raster)?;
    for w in &raster.warnings {
        tell!("screensafe: {w}");
    }
    say!("{why}");
    Ok(Ok(()))
}

/// The panel's live preview: the pack at a place on one screen, taking no input, changed as it runs by lines on
/// standard input (see `wayland::Display::preview`). It's confined as every command that reads packs is, to reading
/// its pack's folder and the theme and art it was given, but not past that: a change of settings reads the pack again.
/// It still can't write anything, run anything or reach the network, and it ends with the panel that started it.
fn live_preview(args: &Args) -> Result<(), String> {
    args.only(&["overlay", "pack", "theme", "branding", "logo", "badge", "photos", "videos", "settings", "region", "output", "seed"])?;
    let dir = args.path("pack").ok_or(USAGE)?;
    let output = args.get("output").map(|o| o.chars().take(64).collect());
    let region = wayland::Region::parse(args.get("region").ok_or(USAGE)?, output)?;
    let mut clock_warnings = Vec::new();
    let clock = clock::Clock::local(&mut clock_warnings);
    let confinement = confine_reading(args, std::slice::from_ref(&dir))?;
    let pack = load(&dir, args)?;
    let options = wayland::Options {
        seed: args.number("seed")?.map(|s| s as u64).unwrap_or(1),
        exit_after: None,
        max_fps: pack::MAX_FPS,
        start: 0.0,
        clock: Some(clock),
        overlay: overlay::Config::parse(args.get("overlay").unwrap_or(""))?,
    };
    let mut raster = Raster::new();
    raster.background_decoding = true;
    raster.load_fonts(&pack);
    let display = wayland::connect_preview(region)?;
    tell!("screensafe: previewing {}; {confinement}, reading only; {}", pack.meta.name, display.format());
    let mut reload = |given: &str| load_with(&dir, args, &parse_settings(given)?, None);
    let why = display.preview(pack, &options, &mut raster, &mut reload)?;
    say!("{why}");
    Ok(())
}

/// Renders the whole loop small, checking every frame for flashes.
fn flash_report(pack: &Pack, seconds: f64, (w, h): (u32, u32), config: overlay::Config) -> (String, bool) {
    let mut raster = Raster::new();
    raster.largest = (w, h);
    raster.load_fonts(pack);
    let view = View::new(w, h, pack.stage);
    let mut eval = eval::Evaluator::new(pack, 1, raster.metrics());
    let mut guard = flash::Guard::new();
    let mut pixmap = tiny_skia::Pixmap::new(w, h).expect("a small pixmap");
    let overlay = overlay::Overlay::new(config, clock::Clock::fixed());
    let end = pack.length().min(seconds);
    let mut time = 0.0;
    let mut frames = 0u64;
    let mut violations: Vec<(f64, usize)> = Vec::new();
    let mut worst = 0;
    let mut drawing = Duration::ZERO;
    let mut slowest = Duration::ZERO;
    while time < end {
        let started = std::time::Instant::now();
        eval.set_pixel_scale(view.scale);
        let mut frame = eval.frame(pack, time, view.aspect());
        overlay.append(&mut frame, time, view, view.scale, pack.foreground, raster.metrics()[0]);
        raster.draw(pack, &frame, &mut pixmap.as_mut(), view);
        let took = started.elapsed();
        drawing += took;
        slowest = slowest.max(took);
        let cells = flash::measure(pixmap.data(), w as usize, h as usize, w as usize * 4, false);
        let assessment = guard.assess(&cells, time);
        worst = worst.max(assessment.flashing_cells);
        if assessment.over_limit && violations.last().is_none_or(|(t, _)| time - t > 1.0) {
            violations.push((time, frame.scene));
        }
        guard.commit(&cells, time, assessment);
        frames += 1;
        time += 1.0 / pack.scenes[frame.scene].fps;
    }
    let listed: Vec<String> = violations.iter().take(20).map(|(t, s)| format!("{{\"time\":{:.2},\"scene\":{}}}", t, json(&pack.scenes[*s].name))).collect();
    let ok = violations.is_empty();
    (
        format!(
            "{{\"checkedSeconds\":{:.1},\"frames\":{frames},\"size\":\"{w}x{h}\",\"drawMsPerFrame\":{:.2},\"slowestDrawMs\":{:.2},\"mostFlashingCells\":{worst},\"passes\":{ok},\"flashes\":[{}]}}",
            end,
            drawing.as_secs_f64() * 1000.0 / frames.max(1) as f64,
            slowest.as_secs_f64() * 1000.0,
            listed.join(",")
        ),
        ok,
    )
}

fn check(args: &Args) -> Result<bool, String> {
    args.only(&["overlay", "theme", "branding", "logo", "badge", "photos", "videos", "settings", "seconds", "size"])?;
    let dir = args.positional.get(1).map(PathBuf::from).ok_or(USAGE)?;
    let size = parse_size(args.get("size"), (960, 540))?;
    confine_reading(args, std::slice::from_ref(&dir))?;
    let pack = match load(&dir, args) {
        Ok(p) => p,
        Err(e) => {
            say!("{{\"ok\":false,\"error\":{}}}", json(&e));
            return Ok(false);
        }
    };
    let seconds = args.number("seconds")?.unwrap_or(600.0).clamp(0.0, 3600.0);
    let (flash, flash_ok) = flash_report(&pack, seconds, size, overlay::Config::parse(args.get("overlay").unwrap_or(""))?);
    let scenes: Vec<String> = pack
        .scenes
        .iter()
        .map(|s| {
            format!(
                "{{\"name\":{},\"duration\":{},\"hold\":{},\"fps\":{},\"operationsPerFrame\":{},\"pointsPerFrame\":{}}}",
                json(&s.name),
                s.duration,
                s.hold,
                s.fps,
                s.cost,
                s.points
            )
        })
        .collect();
    let warnings: Vec<String> = pack.warnings.iter().map(|w| json(w)).collect();
    say!(
        "{{\"ok\":{flash_ok},\"name\":{},\"author\":{},\"license\":{},\"description\":{},\"length\":{:.2},\"scenes\":[{}],\"warnings\":[{}],\"flash\":{flash}}}",
        json(&pack.meta.name),
        json(&pack.meta.author),
        json(&pack.meta.license),
        json(&pack.meta.description),
        pack.length(),
        scenes.join(","),
        warnings.join(","),
    );
    Ok(flash_ok)
}

fn parse_size(value: Option<&str>, default: (u32, u32)) -> Result<(u32, u32), String> {
    match value {
        None => Ok(default),
        Some(s) => s
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
            .filter(|&(w, h)| (16..=8192).contains(&w) && (16..=8192).contains(&h))
            .ok_or_else(|| "--size must look like 1920x1080".to_string()),
    }
}

fn render(args: &Args) -> Result<(), String> {
    args.only(&["overlay", "theme", "branding", "logo", "badge", "photos", "videos", "settings", "time", "out", "size"])?;
    let dir = args.positional.get(1).map(PathBuf::from).ok_or(USAGE)?;
    let out = args.path("out").ok_or(USAGE)?;
    let time = args.number("time")?.unwrap_or(0.0).max(0.0);
    let (w, h) = parse_size(args.get("size"), (1920, 1080))?;
    // The picture's file is made before the renderer is confined, and written to afterwards.
    let file = std::fs::File::create(&out).map_err(|e| format!("can't write {}: {e}", out.display()))?;
    confine_reading(args, std::slice::from_ref(&dir))?;
    let pack = load(&dir, args)?;
    let mut raster = Raster::new();
    raster.largest = (w, h);
    raster.load_fonts(&pack);
    let view = View::new(w, h, pack.stage);
    let mut eval = eval::Evaluator::new(&pack, 1, raster.metrics());
    eval.set_pixel_scale(view.scale);
    let mut frame = eval.frame(&pack, time, view.aspect());
    overlay::Overlay::new(overlay::Config::parse(args.get("overlay").unwrap_or(""))?, clock::Clock::fixed()).append(
        &mut frame,
        time,
        view,
        view.scale,
        pack.foreground,
        raster.metrics()[0],
    );
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("can't allocate the image")?;
    raster.draw(&pack, &frame, &mut pixmap.as_mut(), view);
    let img = image::RgbaImage::from_raw(w, h, pixmap.data().to_vec()).ok_or("can't encode the image")?;
    img.write_with_encoder(image::codecs::png::PngEncoder::new(std::io::BufWriter::new(file))).map_err(|e| format!("can't write {}: {e}", out.display()))?;
    for w in pack.warnings.iter().chain(&raster.warnings) {
        tell!("screensafe: {w}");
    }
    Ok(())
}

/// Draws one frame of a pack, small, for its row in the panel. Prints two lines: `fingerprint` and the pack's
/// fingerprint, then the frame as `pixels W H` and its plain RGB pixels in base64. With `--unless`,
/// a pack whose fingerprint is that one isn't drawn again: the second line is `unchanged`. Nothing is written to
/// disk, and the renderer is confined before it draws.
fn thumbnail(args: &Args) -> Result<(), String> {
    args.only(&["theme", "branding", "logo", "badge", "photos", "videos", "settings", "time", "size", "unless"])?;
    let dir = args.positional.get(1).map(PathBuf::from).ok_or(USAGE)?;
    let (w, h) = parse_size(args.get("size"), (320, 200))?;
    if w > THUMBNAIL_SIDE || h > THUMBNAIL_SIDE {
        return Err(format!("--size can be at most {THUMBNAIL_SIDE} pixels a side for a thumbnail"));
    }
    confine_reading(args, std::slice::from_ref(&dir))?;
    let pack = load(&dir, args)?;
    let fingerprint = format!("{:016x}", pack.fingerprint);
    say!("fingerprint {fingerprint}");
    if args.get("unless") == Some(fingerprint.as_str()) {
        say!("unchanged");
        return Ok(());
    }
    // The pack's own moment, or a few seconds in, once anything fading in has arrived.
    let time = match args.number("time")? {
        Some(t) => t.max(0.0),
        None => pack.meta.preview.unwrap_or_else(|| (pack.length() / 2.0).min(8.0)),
    };
    sandbox::confine()?;
    let mut raster = Raster::new();
    raster.largest = (w, h);
    raster.load_fonts(&pack);
    let view = View::new(w, h, pack.stage);
    let mut eval = eval::Evaluator::new(&pack, 1, raster.metrics());
    eval.set_pixel_scale(view.scale);
    let frame = eval.frame(&pack, time, view.aspect());
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("can't allocate the image")?;
    raster.draw(&pack, &frame, &mut pixmap.as_mut(), view);
    // Plain pixels, three bytes each, row by row, with no image format of their own: the panel puts its own fixed
    // header before them to show them.
    let rgb: Vec<u8> = pixmap.data().chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    say!("pixels {w} {h} {}", base64(&rgb));
    Ok(())
}

fn base64_len(n: usize) -> usize {
    n.div_ceil(3) * 4
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(base64_len(bytes.len()));
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Lists the packs in each directory, one per subdirectory holding a screensafe.toml, as JSON for the plugin.
fn catalog(args: &Args) -> Result<(), String> {
    args.only(&["theme", "branding", "logo", "badge", "photos", "videos"])?;
    let roots: Vec<PathBuf> = args.positional.iter().skip(1).map(PathBuf::from).collect();
    confine_reading(args, &roots)?;
    let palette = theme_palette(args.path("theme").as_deref())?.unwrap_or_else(Palette::default_theme);
    let swatches = palette
        .colors
        .iter()
        .enumerate()
        .map(|(i, (name, c))| {
            let hex = format!("#{:02x}{:02x}{:02x}", (c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8);
            format!("{{\"value\":{},\"name\":{},\"color\":{}}}", i + 1, json(name), json(&hex))
        })
        .collect::<Vec<_>>()
        .join(",");
    let mut entries = Vec::new();
    let mut size = 0;
    for root in args.positional.iter().skip(1) {
        let Ok(listing) = std::fs::read_dir(root) else { continue };
        let mut names = Vec::new();
        for entry in listing.flatten().take(256) {
            let Ok(kind) = entry.file_type() else { continue };
            if !kind.is_dir() {
                continue;
            }
            if let Some(name) = entry.file_name().to_str() {
                if safefs::check_relative(name).is_ok() {
                    names.push(name.to_string());
                }
            }
        }
        names.sort();
        for name in names {
            let path = Path::new(root).join(&name);
            if !path.join(pack::MANIFEST).is_file() {
                continue;
            }
            let mut entry = match load(&path, args) {
                Ok(p) => format!(
                    "{{\"path\":{},\"ok\":true,\"name\":{},\"author\":{},\"license\":{},\"description\":{},\"fingerprint\":\"{:016x}\",\"warnings\":{},\"settings\":{},\"photos\":{},\"videos\":{}}}",
                    json(&path.display().to_string()),
                    json(&p.meta.name),
                    json(&p.meta.author),
                    json(&p.meta.license),
                    json(&p.meta.description),
                    p.fingerprint,
                    p.warnings.len(),
                    settings_json(&p.meta.settings),
                    // How many photos it has to show, or -1 for a pack that doesn't show any.
                    if p.uses_photos { p.photos.len() as i64 } else { -1 },
                    if p.uses_videos { p.videos.len() as i64 } else { -1 }
                ),
                Err(e) => {
                    let short: String = e.lines().next().unwrap_or("").chars().take(300).collect();
                    format!("{{\"path\":{},\"ok\":false,\"name\":{},\"error\":{}}}", json(&path.display().to_string()), json(&name), json(&short))
                }
            };
            // Theme data once, not once per color setting or pack. Keep the catalog's 64 KiB bound.
            if entries.is_empty() {
                entry.pop();
                entry.push_str(&format!(",\"palette\":[{swatches}]}}"));
            }
            size += entry.len() + 1;
            if size > OUTPUT_LIMIT - 64 {
                break;
            }
            entries.push(entry);
        }
    }
    say!("[{}]", entries.join(","));
    Ok(())
}

/// Makes the photos folder, `~/Pictures/Screensaver`, if it isn't there, and prints its path, so the panel can open it
/// for someone to add photos. Each folder is made, or opened, without following a link.
fn media_folder(args: &Args, relative: &str) -> Result<(), String> {
    use rustix::fs::{Mode, OFlags};
    args.only(&[])?;
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).ok_or("HOME isn't set")?;
    let open = |parent: &std::os::fd::OwnedFd, name: &str| {
        rustix::fs::openat(parent, name, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty())
    };
    let mut dir = rustix::fs::open(Path::new(&home), OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC, Mode::empty())
        .map_err(|e| format!("can't open {}: {e}", Path::new(&home).display()))?;
    for name in relative.split('/') {
        match rustix::fs::mkdirat(&dir, name, Mode::from_raw_mode(0o755)) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(e) => return Err(format!("can't make {name}: {e}")),
        }
        dir = open(&dir, name).map_err(|e| format!("can't open {name}: {e}"))?;
    }
    say!("{}", Path::new(&home).join(relative).display());
    Ok(())
}

fn takeover_command(args: &Args) -> Result<(), String> {
    args.only(&[])?;
    let paths = takeover::Paths::from_env()?;
    let result = match args.positional.get(1).map(String::as_str) {
        Some("claim") => takeover::claim(&paths, args.positional.get(2).ok_or(USAGE)?)?.to_string(),
        Some("release") => takeover::release(&paths, args.positional.get(2).map(String::as_str))?.to_string(),
        Some("status") => takeover::status(&paths)?,
        _ => return Err(USAGE.into()),
    };
    say!("{result}");
    Ok(())
}

/// Confines this process as `run` does, then shows that files, programs and the network are out of reach.
fn sandbox_test() -> Result<bool, String> {
    // The first layer: reading beneath the renderer's own folder only.
    let own = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).ok_or("can't find the renderer's folder")?;
    sandbox::confine_reading(&[own.as_path()])?;
    let reads_own = std::fs::read_dir(&own).is_ok();
    let reads_other = std::fs::read("/etc/hostname").is_err();
    let writes = std::fs::write(own.join(".screensafe-sandbox-test"), b"x").is_err();
    // The second: nothing at all.
    let confinement = sandbox::confine()?;
    let read = std::fs::read("/etc/hostname").is_err();
    let list = std::fs::read_dir("/").is_err();
    let exec = std::process::Command::new("/bin/true").status().is_err();
    let tcp = std::net::TcpStream::connect_timeout(&"127.0.0.1:9".parse().expect("an address"), Duration::from_millis(500))
        .err()
        .is_some_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied);
    let net = tcp
        && ["127.0.0.1:0", "[::1]:0"]
            .iter()
            .all(|address| std::net::UdpSocket::bind(address).err().is_some_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied));
    say!(
        "{{\"confinement\":{},\"firstLayerReadsOwnFolder\":{reads_own},\"firstLayerOtherReadBlocked\":{reads_other},\"firstLayerWriteBlocked\":{writes},\"readBlocked\":{read},\"listBlocked\":{list},\"execBlocked\":{exec},\"networkBlocked\":{net}}}",
        json(confinement)
    );
    Ok(reads_own && reads_other && writes && read && list && exec && net)
}

/// How long the panel's quick commands may take before they give up on their own, whatever they're waiting on.
const QUICK_LIMIT: Duration = Duration::from_secs(20);

/// Guards against a shell that stops answering or goes away: the renderer never outlives the process that started
/// it, and a quick command never waits indefinitely, even on a full pipe its reader has stopped draining.
fn start_watchdog(work: fn(), confined: bool) {
    let (ready, wait) = std::sync::mpsc::sync_channel(0);
    std::thread::spawn(move || {
        let result = if confined { sandbox::confine_watchdog().map(|_| ()) } else { Ok(()) };
        let proceed = result.is_ok();
        if ready.send(result).is_ok() && proceed {
            work();
        }
    });
    match wait.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(())) => {}
        result => {
            tell!("screensafe: can't start confined watchdog: {result:?}");
            std::process::exit(1);
        }
    }
}

fn failsafes(command: Option<&str>) {
    // Linux ends this process when the one that started it exits. Started from a terminal, that's the terminal. Not
    // for takeover, whose release runs detached as the plugin unloads and must finish after the shell has gone.
    if command != Some("takeover") {
        let _ = rustix::process::set_parent_process_death_signal(Some(rustix::process::Signal::TERM));
    }
    if matches!(command, Some("catalog" | "thumbnail" | "takeover" | "photos-folder" | "videos-folder" | "version" | "sandbox-test" | "system-info")) {
        start_watchdog(
            || {
                std::thread::sleep(QUICK_LIMIT);
                tell!("screensafe: gave up after {} seconds", QUICK_LIMIT.as_secs());
                std::process::exit(TIMED_OUT);
            },
            matches!(command, Some("catalog" | "thumbnail" | "system-info")),
        );
    }
    if matches!(command, Some("run" | "preview")) {
        start_watchdog(watch_draws, true);
    }
}

/// The longest one frame may take to draw before the pack is given up on.
const STALLED: u32 = 5;

/// Ends the process if a frame is still being drawn after `STALLED` seconds, so a pack too heavy to draw can't keep
/// the screen covered: the plugin then shows the built-in pack, as it does for any pack that can't be used.
fn watch_draws() {
    let mut last = 0;
    let mut seconds = 0;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let now = wayland::DRAWS.load(std::sync::atomic::Ordering::SeqCst);
        seconds = if now % 2 == 1 && now == last { seconds + 1 } else { 0 };
        last = now;
        if seconds >= STALLED {
            tell!("screensafe: a frame took more than {STALLED} seconds to draw");
            std::process::exit(PACK_UNUSABLE as i32);
        }
    }
}

/// The exit code of a quick command that ran out of time.
const TIMED_OUT: i32 = 124;

fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            tell!("screensafe: {e}");
            return ExitCode::from(2);
        }
    };
    failsafes(args.positional.first().map(String::as_str));
    let result = match args.positional.first().map(String::as_str) {
        Some("run") => match run(&args) {
            Ok(Err(e)) => {
                tell!("screensafe: {e}");
                return ExitCode::from(PACK_UNUSABLE);
            }
            other => other.map(|_| true),
        },
        Some("check") => check(&args),
        Some("render") => render(&args).map(|_| true),
        Some("catalog") => catalog(&args).map(|_| true),
        Some("thumbnail") => thumbnail(&args).map(|_| true),
        Some("preview") => live_preview(&args).map(|_| true),
        Some("takeover") => takeover_command(&args).map(|_| true),
        Some("photos-folder") => media_folder(&args, pack::PHOTOS_FOLDER).map(|_| true),
        Some("sandbox-test") => sandbox_test(),
        Some("system-info") => args.only(&[]).and_then(|_| {
            if args.positional.len() != 1 {
                return Err("system-info takes no arguments".into());
            }
            let data = overlay::system_info()?;
            say!("{data}");
            Ok(true)
        }),
        Some("videos-folder") => media_folder(&args, video::FOLDER).map(|_| true),
        Some("version") => {
            say!("screensafe {} (pack format {}, expressions 1)", env!("CARGO_PKG_VERSION"), pack::FORMAT);
            Ok(true)
        }
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            tell!("screensafe: {e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_encodes() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xFB, 0xFF]), "+/8=");
        assert_eq!(base64_len(4), 8);
    }

    #[test]
    fn json_escapes() {
        assert_eq!(json("a\"b\\c\nd\u{1}"), "\"a\\\"b\\\\c\\nd\\u0001\"");
    }

    #[test]
    fn preview_regions_are_whole_pixels_within_bounds() {
        let r = wayland::Region::parse("10,20,352,220", Some("eDP-1".into())).unwrap();
        assert_eq!((r.x, r.y, r.w, r.h, r.output.as_deref()), (10, 20, 352, 220, Some("eDP-1")));
        assert!(wayland::Region::parse("10 20 352 220", None).is_ok(), "spaces too, as the control lines send");
        for bad in ["10,20,352", "10,20,352,220,1", "-1,0,352,220", "0,0,8,220", "0,0,352,5000", "a,b,c,d", ""] {
            assert!(wayland::Region::parse(bad, None).is_err(), "{bad}");
        }
    }

    #[test]
    fn settings_arguments_are_bounded() {
        let parse = |v: &str| settings(&Args::parse(["run", "--settings", v].iter().map(|s| s.to_string())).unwrap());
        assert_eq!(parse("pace=2,rain=false,glow=true").unwrap(), vec![("pace".into(), 2.0), ("rain".into(), 0.0), ("glow".into(), 1.0)]);
        assert!(parse("pace").is_err());
        assert!(parse("pace=fast").is_err());
        assert!(parse("pace=NaN").is_err());
        assert!(parse(&"a=1,".repeat(9)).is_err());
        assert!(parse(&format!("a={}", "1".repeat(1100))).is_err());
    }

    #[test]
    fn args() {
        let a = Args::parse(["check", "dir", "--seconds", "5"].iter().map(|s| s.to_string())).unwrap();
        assert_eq!(a.positional, vec!["check", "dir"]);
        assert_eq!(a.number("seconds").unwrap(), Some(5.0));
        assert!(a.only(&["seconds"]).is_ok());
        assert!(a.only(&[]).is_err());
        assert!(Args::parse(["--x"].iter().map(|s| s.to_string())).is_err());
    }
}
