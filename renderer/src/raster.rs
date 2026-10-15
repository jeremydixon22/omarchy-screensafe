//! Drawing a display list into pixels, in software.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use image::{ImageFormat, ImageReader, Limits};
use tiny_skia::{Color, FillRule, FilterQuality, LineCap, LineJoin, Mask, Paint, PathBuilder, Pixmap, PixmapMut, PixmapPaint, Stroke, Transform};

use crate::eval::{FontMetrics, Frame, ImageRef, Item};
use crate::pack::{Fit, ImageSource, Pack, Stage};

pub const FONT: &[u8] = include_bytes!("../fonts/JetBrainsMono-Regular.ttf");

const MAX_IMAGE_SIDE: u32 = 16384;
const MAX_IMAGE_PIXELS: u64 = 64_000_000;
const MAX_DECODE_BYTES: u64 = 512 * 1024 * 1024;
const GLYPH_CACHE: usize = 4096;
/// And at most this many bytes of coverage, since text whose size changes every frame makes new glyphs every frame.
const GLYPH_CACHE_BYTES: usize = 64 << 20;
const IMAGE_CACHE: usize = 3;
/// How many of a frame's glows are kept from one frame to the next.
const GLOW_CACHE: usize = 32;
/// Resampled copies kept, one for each image and screen size in use.
const SCALED_CACHE: usize = 6;
/// Frames an image's shown size must hold before a copy is resampled to it.
const STEADY_FRAMES: u32 = 8;
/// How long an image coming into view is held back while its copy is resampled, before it's drawn the slow way.
const MAX_HOLD: Duration = Duration::from_millis(1500);
/// How long an image that was held back takes to fade in once it's ready, so it doesn't appear all at once.
const ARRIVAL_FADE: f32 = 0.5;
/// An image not drawn for this long is coming into view again when it's next shown.
const SEEN_RECENTLY: Duration = Duration::from_millis(500);

fn reader(bytes: &[u8]) -> Result<ImageReader<Cursor<&[u8]>>, String> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    match reader.format() {
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP) => {}
        _ => return Err("is not a PNG, JPEG or WebP image".into()),
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    Ok(reader)
}

/// Checks that `bytes` is an image ScreenSafe will decode, returning its size.
pub fn probe_image(bytes: &[u8]) -> Result<(u32, u32), String> {
    let (w, h) = reader(bytes)?.into_dimensions().map_err(|e| e.to_string())?;
    if w == 0 || h == 0 || w as u64 * h as u64 > MAX_IMAGE_PIXELS {
        return Err(format!("is {w} x {h}; images may have at most {MAX_IMAGE_PIXELS} pixels"));
    }
    Ok((w, h))
}

/// Decodes an image as straight RGBA, requiring it to be exactly `width` x `height`.
pub fn decode_rgba(bytes: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    probe_image(bytes)?;
    let rgba = reader(bytes)?.decode().map_err(|e| e.to_string())?.to_rgba8();
    if rgba.dimensions() != (width, height) {
        return Err(format!("decoded as {:?}, not {width} x {height}", rgba.dimensions()));
    }
    Ok(rgba.into_raw())
}

/// Decodes an image and fits it within `width` x `height`, keeping its shape, centered on transparency: straight
/// RGBA. Shrinking averages; enlarging blends smoothly.
pub fn fit_rgba(bytes: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    probe_image(bytes)?;
    let src = reader(bytes)?.decode().map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = src.dimensions();
    let scale = (width as f64 / w as f64).min(height as f64 / h as f64);
    let (nw, nh) = (((w as f64 * scale).round() as u32).clamp(1, width), ((h as f64 * scale).round() as u32).clamp(1, height));
    let filter = if scale < 1.0 { image::imageops::FilterType::Triangle } else { image::imageops::FilterType::CatmullRom };
    let fitted = image::imageops::resize(&src, nw, nh, filter);
    let mut out = image::RgbaImage::new(width, height);
    image::imageops::overlay(&mut out, &fitted, ((width - nw) / 2) as i64, ((height - nh) / 2) as i64);
    Ok(out.into_raw())
}

/// The theme's colors that an image is drawn in by `palette = "theme"`.
const THEME_INKS: [&str; 20] = [
    "background",
    "lighter_background",
    "selection",
    "muted",
    "foreground",
    "bright_foreground",
    "accent",
    "red",
    "orange",
    "yellow",
    "green",
    "cyan",
    "blue",
    "magenta",
    "bright_red",
    "bright_yellow",
    "bright_green",
    "bright_cyan",
    "bright_blue",
    "bright_magenta",
];

/// Redraws straight RGBA pixels in the theme's colors: each the nearest of them by eye (a weighted distance), with a
/// 4 x 4 ordered dither so gradients become patterns of nearby colors, as pixel art draws them. Edges are made crisp:
/// each pixel is either fully shown or not at all.
pub fn to_palette(rgba: &mut [u8], width: u32, palette: &crate::palette::Palette) {
    const BAYER: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    let inks: Vec<[i32; 3]> = palette
        .colors
        .iter()
        .filter(|(n, _)| THEME_INKS.contains(n))
        .map(|(_, c)| [(c[0] * 255.0).round() as i32, (c[1] * 255.0).round() as i32, (c[2] * 255.0).round() as i32])
        .collect();
    for (i, px) in rgba.chunks_exact_mut(4).enumerate() {
        if px[3] < 128 {
            px.copy_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let (x, y) = (i % width as usize, i / width as usize);
        let nudge = (BAYER[y % 4][x % 4] * 2 - 15) * 2;
        let c = [px[0] as i32 + nudge, px[1] as i32 + nudge, px[2] as i32 + nudge];
        let mut best = (i64::MAX, [0; 3]);
        for ink in &inks {
            // The "redmean" distance: red and blue count by how red the pair is, as the eye weighs them.
            let rmean = (c[0] + ink[0]) / 2;
            let (dr, dg, db) = ((c[0] - ink[0]) as i64, (c[1] - ink[1]) as i64, (c[2] - ink[2]) as i64);
            let d = (((512 + rmean as i64) * dr * dr) >> 8) + 4 * dg * dg + (((767 - rmean as i64) * db * db) >> 8);
            if d < best.0 {
                best = (d, *ink);
            }
        }
        px.copy_from_slice(&[best.1[0] as u8, best.1[1] as u8, best.1[2] as u8, 255]);
    }
}

/// A pixmap from straight RGBA.
fn pixmap_from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    for (dst, src) in pixmap.data_mut().chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
        let a = src[3] as u16;
        dst[0] = ((src[0] as u16 * a + 127) / 255) as u8;
        dst[1] = ((src[1] as u16 * a + 127) / 255) as u8;
        dst[2] = ((src[2] as u16 * a + 127) / 255) as u8;
        dst[3] = src[3];
    }
    Some(pixmap)
}

/// Decodes an image, shrinking it to fit within `max_w` x `max_h`.
pub fn decode_image(bytes: &[u8], max_w: u32, max_h: u32) -> Result<Pixmap, String> {
    probe_image(bytes)?;
    let mut rgba = reader(bytes)?.decode().map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = rgba.dimensions();
    let shrink = (max_w as f64 / w as f64).min(max_h as f64 / h as f64);
    if shrink < 1.0 {
        let nw = ((w as f64 * shrink).round() as u32).max(1);
        let nh = ((h as f64 * shrink).round() as u32).max(1);
        rgba = image::imageops::resize(&rgba, nw, nh, image::imageops::FilterType::Triangle);
    }
    let (w, h) = rgba.dimensions();
    let mut pixmap = Pixmap::new(w, h).ok_or("image is too large")?;
    for (dst, src) in pixmap.data_mut().chunks_exact_mut(4).zip(rgba.as_raw().chunks_exact(4)) {
        let a = src[3] as u16;
        dst[0] = ((src[0] as u16 * a + 127) / 255) as u8;
        dst[1] = ((src[1] as u16 * a + 127) / 255) as u8;
        dst[2] = ((src[2] as u16 * a + 127) / 255) as u8;
        dst[3] = src[3];
    }
    Ok(pixmap)
}

/// Where the stage sits on a screen of `width` x `height` pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub width: u32,
    pub height: u32,
    /// Pixels per stage unit.
    pub scale: f64,
    /// For pixel art: how many screen pixels a side each canvas pixel takes.
    pub blocks: u32,
}

impl View {
    pub fn new(width: u32, height: u32, stage: Stage) -> View {
        let (w, h) = (width as f64, height as f64);
        let (scale, blocks) = match stage {
            Stage::Aspect(aspect) => (h.min(w / aspect), 0),
            Stage::Screen => (h, 0),
            // The largest whole number of screen pixels to a canvas pixel that still shows a whole stage unit.
            Stage::Pixels(p) => {
                let blocks = (height / p).max(1);
                ((p * blocks) as f64, blocks)
            }
        };
        View { width, height, scale, blocks }
    }

    /// The stage's width over its height, as the pack sees it: for pixel art, the canvas's width in stage units.
    pub fn aspect(&self) -> f64 {
        if self.blocks > 0 {
            self.width as f64 / self.scale.max(1.0)
        } else {
            self.width as f64 / self.height.max(1) as f64
        }
    }

    fn px(&self, x: f32, y: f32) -> (f32, f32) {
        ((self.width as f64 / 2.0 + x as f64 * self.scale) as f32, (self.height as f64 / 2.0 + y as f64 * self.scale) as f32)
    }
}

type GlyphBitmap = (fontdue::Metrics, Vec<u8>);
/// A copy being resampled on another thread: the image, the size, and where the copy and whether it's opaque arrive.
type Resampling = (ImageRef, (u32, u32), mpsc::Receiver<Option<(Pixmap, bool)>>);

pub struct Raster {
    /// When set (by the benchmarks), milliseconds spent drawing each kind of item, added up.
    pub profile: Option<HashMap<&'static str, f64>>,
    /// The built-in font, then each of the pack's.
    fonts: Vec<fontdue::Font>,
    /// Glyphs by font, character, size in quarter pixels and stretch in 256ths.
    glyphs: HashMap<(u8, char, u32, u32), GlyphBitmap>,
    glyph_bytes: usize,
    /// A layer for glowing elements, kept between frames and cleared after each use.
    glow_layer: Option<Pixmap>,
    /// Each glow of the last frame, by its order in the frame, with what it lit and what it added to the screen, so
    /// an unchanged glow next frame is laid down again rather than drawn and blurred afresh.
    glow_cache: Vec<Option<GlowCache>>,
    glow_scratch: Vec<[f32; 4]>,
    /// Coverage of a mesh's edges, reused from frame to frame.
    coverage: Vec<u8>,
    /// Coverage of a thin line, the size of the screen and all zero between uses.
    line_coverage: Vec<u8>,
    images: Vec<(ImageRef, Arc<Pixmap>)>,
    failed: Vec<ImageRef>,
    /// Images resampled to the sizes they're shown at, with whether each is opaque; most recently used last.
    scaled: Vec<(ImageRef, (u32, u32), Pixmap, bool)>,
    /// The size each image was last asked for, and for how many frames running.
    asked: Vec<(ImageRef, (u32, u32), u32)>,
    /// Images being decoded on another thread.
    pending: Vec<(ImageRef, mpsc::Receiver<Result<Pixmap, String>>)>,
    /// Copies being resampled on another thread.
    resampling: Vec<Resampling>,
    /// When each image was last drawn.
    drawn: Vec<(ImageRef, Instant)>,
    /// When each image coming into view was first held back.
    held_back: Vec<(ImageRef, Instant)>,
    /// When each image that was held back was first drawn.
    arrivals: Vec<(ImageRef, Instant)>,
    /// Sprite sheets, by pack image, decoded whole the first time they're drawn and kept; `None` for one that
    /// couldn't be decoded.
    sheets: HashMap<usize, Option<Arc<Pixmap>>>,
    /// The canvas that pixel art is drawn on before it's scaled up to the screen.
    canvas: Option<Pixmap>,
    /// Decode and resample images on other threads, holding an image back until it's ready, so that a large image
    /// never stalls the screen. Off for `check` and `render`, which must draw every image in its frame.
    pub background_decoding: bool,
    /// Set when an image has become ready since it was last cleared, so a frame that would otherwise look
    /// unchanged is drawn again.
    pub images_arrived: bool,
    /// The largest screen, which decoded images are sized for.
    pub largest: (u32, u32),
    pub warnings: Vec<String>,
}

fn color(c: [f32; 4]) -> Color {
    Color::from_rgba(c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0), c[3].clamp(0.0, 1.0)).unwrap_or(Color::BLACK)
}

impl Raster {
    pub fn new() -> Raster {
        let font = fontdue::Font::from_bytes(FONT, fontdue::FontSettings::default()).expect("the bundled font parses");
        Raster {
            profile: None,
            fonts: vec![font],
            glyphs: HashMap::new(),
            glyph_bytes: 0,
            glow_layer: None,
            glow_cache: Vec::new(),
            glow_scratch: Vec::new(),
            coverage: Vec::new(),
            line_coverage: Vec::new(),
            images: Vec::new(),
            failed: Vec::new(),
            scaled: Vec::new(),
            asked: Vec::new(),
            pending: Vec::new(),
            resampling: Vec::new(),
            drawn: Vec::new(),
            held_back: Vec::new(),
            arrivals: Vec::new(),
            sheets: HashMap::new(),
            canvas: None,
            background_decoding: false,
            images_arrived: false,
            largest: (1920, 1080),
            warnings: Vec::new(),
        }
    }

    /// Reads the pack's fonts, after the built-in one. A font that can't be read is replaced by the built-in one,
    /// with a warning.
    pub fn load_fonts(&mut self, pack: &Pack) {
        self.fonts.truncate(1);
        self.glyphs.clear();
        self.glyph_bytes = 0;
        for (name, bytes) in &pack.fonts {
            match fontdue::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default()) {
                Ok(font) => self.fonts.push(font),
                Err(e) => {
                    self.warnings.push(format!("{name}: {e}; using the built-in font"));
                    let builtin = self.fonts[0].clone();
                    self.fonts.push(builtin);
                }
            }
        }
    }

    /// A sprite sheet, decoded whole the first time it's drawn and kept from then on.
    fn sheet(&mut self, pack: &Pack, image: usize) -> Option<Arc<Pixmap>> {
        if let Some(sheet) = self.sheets.get(&image) {
            return sheet.clone();
        }
        let decoded = match pack.images.get(image) {
            Some(ImageSource::File { name, bytes }) => match decode_image(bytes, crate::pack::MAX_SHEET_SIDE, crate::pack::MAX_SHEET_SIDE) {
                Ok(p) => Some(Arc::new(p)),
                Err(e) => {
                    self.warnings.push(format!("{name}: {e}"));
                    None
                }
            },
            Some(ImageSource::Pixels { width, height, rgba, .. }) => pixmap_from_rgba(*width, *height, rgba).map(Arc::new),
            None => None,
        };
        self.sheets.insert(image, decoded.clone());
        decoded
    }

    /// Each font's proportions: the built-in font's, then each of the pack's.
    pub fn metrics(&self) -> Vec<FontMetrics> {
        self.fonts.iter().map(font_metrics).collect()
    }

    fn image(&mut self, pack: &Pack, source: ImageRef) -> Option<usize> {
        if let Some(i) = self.images.iter().position(|(r, _)| *r == source) {
            // Most recently used last, so the one evicted is the one unused longest.
            let entry = self.images.remove(i);
            self.images.push(entry);
            return Some(self.images.len() - 1);
        }
        if self.failed.contains(&source) {
            return None;
        }
        let (name, bytes) = match source {
            ImageRef::Pack(i) => match &pack.images[i] {
                ImageSource::File { name, bytes } => (name.as_str(), bytes.as_slice()),
                // Prepared images are only ever sprite sheets.
                ImageSource::Pixels { .. } => return None,
            },
            ImageRef::Background(i) => (pack.backgrounds[i].0.as_str(), pack.backgrounds[i].1.as_slice()),
        };
        let (w, h) = self.largest;
        // A little larger than the screen, so a modest zoom stays sharp.
        let (max_w, max_h) = (w.saturating_mul(6) / 5, h.saturating_mul(6) / 5);
        let decoded = if self.background_decoding {
            match self.pending.iter().position(|(r, _)| *r == source) {
                Some(p) => match self.pending[p].1.try_recv() {
                    Ok(result) => {
                        self.pending.remove(p);
                        result
                    }
                    Err(mpsc::TryRecvError::Empty) => return None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        self.pending.remove(p);
                        Err("couldn't be decoded".into())
                    }
                },
                None => {
                    let (send, receive) = mpsc::channel();
                    let bytes = bytes.to_vec();
                    std::thread::spawn(move || {
                        let _ = send.send(decode_image(&bytes, max_w, max_h));
                    });
                    self.pending.push((source, receive));
                    return None;
                }
            }
        } else {
            decode_image(bytes, max_w, max_h)
        };
        match decoded {
            Ok(pixmap) => {
                self.keep(source, pixmap);
                Some(self.images.len() - 1)
            }
            Err(e) => {
                self.warnings.push(format!("{name}: {e}"));
                self.failed.push(source);
                None
            }
        }
    }

    fn keep(&mut self, source: ImageRef, pixmap: Pixmap) {
        if self.images.len() >= IMAGE_CACHE {
            let (gone, _) = self.images.remove(0);
            self.scaled.retain(|(r, _, _, _)| *r != gone);
            self.resampling.retain(|(r, _, _)| *r != gone);
        }
        self.images.push((source, Arc::new(pixmap)));
        self.images_arrived = true;
    }

    /// Starts decoding, on another thread, the images a frame shows, and once one is decoded, resampling it to the
    /// size it will be shown at on each of `views`, so that it's ready by the time it's drawn.
    pub fn prefetch(&mut self, pack: &Pack, frame: &Frame, views: &[View]) {
        if !self.background_decoding {
            return;
        }
        for item in &frame.items {
            if let Item::Image { source, fit, zoom, pan_x, pan_y, transform, .. } = item {
                let Some(index) = self.image(pack, *source) else { continue };
                let image = &self.images[index].1;
                let (iw, ih) = (image.width(), image.height());
                for view in views {
                    let p = Placement::new((iw, ih), *fit, *zoom, *pan_x, *pan_y, transform, view);
                    if let Some(size) = p.whole {
                        self.request_scaled(index, size);
                    }
                }
            }
        }
    }

    /// Takes in any images decoded or resampled on other threads, noting in `images_arrived` that some were.
    pub fn poll_images(&mut self) {
        let mut i = 0;
        while i < self.pending.len() {
            match self.pending[i].1.try_recv() {
                Err(mpsc::TryRecvError::Empty) => i += 1,
                result => {
                    let (source, _) = self.pending.remove(i);
                    match result {
                        Ok(Ok(pixmap)) => self.keep(source, pixmap),
                        Ok(Err(e)) => {
                            self.warnings.push(e);
                            self.failed.push(source);
                        }
                        Err(_) => self.failed.push(source),
                    }
                }
            }
        }
        let mut i = 0;
        while i < self.resampling.len() {
            match self.resampling[i].2.try_recv() {
                Err(mpsc::TryRecvError::Empty) => i += 1,
                result => {
                    let (source, size, _) = self.resampling.remove(i);
                    if let Ok(Some((pixmap, opaque))) = result {
                        self.keep_scaled(source, size, pixmap, opaque);
                        self.images_arrived = true;
                    }
                }
            }
        }
    }

    fn keep_scaled(&mut self, source: ImageRef, size: (u32, u32), pixmap: Pixmap, opaque: bool) {
        self.scaled.retain(|(r, s, _, _)| !(*r == source && *s == size));
        if self.scaled.len() >= SCALED_CACHE {
            self.scaled.remove(0);
        }
        self.scaled.push((source, size, pixmap, opaque));
    }

    /// The copy of an image resampled to exactly `size`, if there is one, with whether it's opaque.
    fn scaled_copy(&mut self, source: ImageRef, size: (u32, u32)) -> Option<(&Pixmap, bool)> {
        let i = self.scaled.iter().position(|(r, s, _, _)| *r == source && *s == size)?;
        // Most recently used last, so the one evicted is the one unused longest.
        let entry = self.scaled.remove(i);
        self.scaled.push(entry);
        self.scaled.last().map(|(_, _, p, o)| (p, *o))
    }

    /// Makes a copy of the image resampled to `size`: on another thread when decoding in the background, at once
    /// otherwise.
    fn request_scaled(&mut self, index: usize, size: (u32, u32)) {
        let source = self.images[index].0;
        if self.scaled.iter().any(|(r, s, _, _)| *r == source && *s == size) || self.resampling.iter().any(|(r, s, _)| *r == source && *s == size) {
            return;
        }
        let image = Arc::clone(&self.images[index].1);
        if self.background_decoding {
            let (send, receive) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = send.send(resample(&image, size));
            });
            self.resampling.push((source, size, receive));
        } else if let Some((pixmap, opaque)) = resample(&image, size) {
            self.keep_scaled(source, size, pixmap, opaque);
        }
    }

    fn drawn_recently(&self, source: ImageRef) -> bool {
        self.drawn.iter().any(|(r, at)| *r == source && at.elapsed() < SEEN_RECENTLY)
    }

    /// The opacity an image that was held back is drawn at as it fades in, asking for frames until it has.
    fn arrival(&mut self, source: ImageRef) -> f32 {
        if let Some(i) = self.held_back.iter().position(|(r, _)| *r == source) {
            self.held_back.remove(i);
            self.arrivals.retain(|(r, _)| *r != source);
            self.arrivals.push((source, Instant::now()));
        }
        let Some(i) = self.arrivals.iter().position(|(r, _)| *r == source) else { return 1.0 };
        let k = self.arrivals[i].1.elapsed().as_secs_f32() / ARRIVAL_FADE;
        if k >= 1.0 {
            self.arrivals.remove(i);
            return 1.0;
        }
        self.images_arrived = true;
        k * k * (3.0 - 2.0 * k)
    }

    fn mark_drawn(&mut self, source: ImageRef) {
        self.drawn.retain(|(r, _)| *r != source);
        self.drawn.push((source, Instant::now()));
        self.held_back.retain(|(r, _)| *r != source);
    }

    pub fn draw(&mut self, pack: &Pack, frame: &Frame, target: &mut PixmapMut, view: View) {
        if view.blocks <= 1 {
            return self.draw_view(pack, frame, target, view);
        }
        // Pixel art: drawn on a canvas a block's width the screen's size, then each canvas pixel is laid down as a
        // block of screen pixels, the canvas's middle on the screen's.
        let b = view.blocks;
        let (cw, ch) = (view.width.div_ceil(b), view.height.div_ceil(b));
        let mut canvas = match self.canvas.take() {
            Some(c) if c.width() == cw && c.height() == ch => c,
            _ => match Pixmap::new(cw, ch) {
                Some(c) => c,
                None => return,
            },
        };
        let canvas_view = View { width: cw, height: ch, scale: view.scale / b as f64, blocks: 1 };
        self.draw_view(pack, frame, &mut canvas.as_mut(), canvas_view);
        upscale(&canvas, target, b);
        self.canvas = Some(canvas);
    }

    fn draw_view(&mut self, pack: &Pack, frame: &Frame, target: &mut PixmapMut, view: View) {
        let bg = frame.background;
        target.fill(color([bg[0], bg[1], bg[2], 1.0]));
        // The clip masks in force, innermost last. `None` is a clip that hides nothing on this screen: drawing through
        // a mask costs far more than drawing without one, so a clip that covers the whole view makes no mask.
        let mut masks: Vec<Option<Mask>> = Vec::new();
        // While a glow is open, what's drawn goes into a layer of its own, which is then blurred onto the screen
        // and laid over it. A glow opened inside another adds nothing more.
        let mut glow: Option<OpenGlow> = None;
        let mut inner = 0usize;
        let mut glows = 0usize;
        let mut next = 0usize;
        while next < frame.items.len() {
            let item = &frame.items[next];
            next += 1;
            match item {
                Item::Clip(quad) => {
                    let corners: Vec<(f32, f32)> = quad.iter().map(|&(x, y)| view.px(x, y)).collect();
                    let outer = masks.last().cloned().flatten();
                    if covers(&corners, view.width as f32, view.height as f32) {
                        masks.push(outer);
                        continue;
                    }
                    let Some(path) = polygon(corners.into_iter()) else {
                        masks.push(outer);
                        continue;
                    };
                    let mask = match outer {
                        Some(mut m) => {
                            m.intersect_path(&path, FillRule::Winding, true, Transform::identity());
                            Some(m)
                        }
                        None => Mask::new(view.width, view.height).map(|mut m| {
                            m.fill_path(&path, FillRule::Winding, true, Transform::identity());
                            m
                        }),
                    };
                    masks.push(mask);
                }
                Item::Unclip => {
                    masks.pop();
                }
                Item::Glow { radius, strength } => {
                    if glow.is_some() {
                        inner += 1;
                        continue;
                    }
                    let index = glows;
                    glows += 1;
                    let sigma = (*radius as f64 * view.scale) as f32;
                    // Kept only for the first few glows of a frame, each of which can hold two screens' worth of pixels.
                    let kept = masks.last().and_then(Option::as_ref).is_none() && index < GLOW_CACHE;
                    // The same items lit the same way as last frame: lay down what they made then, and go on past them.
                    let end = glow_end(&frame.items, next);
                    if kept {
                        if let Some(Some(c)) = self.glow_cache.get(index) {
                            if c.sigma == sigma && c.strength == *strength && c.size == (view.width, view.height) && c.items[..] == frame.items[next..end] {
                                let started = self.profile.is_some().then(Instant::now);
                                add_recorded(target, c.area, &c.added);
                                over_recorded(target, c.layer_area, &c.layer);
                                self.note("glow (kept)", started);
                                next = end + 1;
                                continue;
                            }
                        }
                    }
                    let layer = match self.glow_layer.take() {
                        Some(p) if p.width() == view.width && p.height() == view.height => Some(p),
                        _ => Pixmap::new(view.width, view.height),
                    };
                    match layer {
                        Some(layer) => glow = Some(OpenGlow { layer, sigma, strength: *strength, bounds: Bounds::EMPTY, index, keep: kept }),
                        None => inner += 1,
                    }
                    if kept {
                        if self.glow_cache.len() <= index {
                            self.glow_cache.resize_with(index + 1, || None);
                        }
                        // What this glow lights, kept with what it adds once it's drawn (see finish_glow).
                        self.glow_cache[index] = Some(GlowCache {
                            items: frame.items[next..end].to_vec(),
                            sigma,
                            strength: *strength,
                            size: (view.width, view.height),
                            area: Bounds::EMPTY,
                            added: Vec::new(),
                            layer_area: Bounds::EMPTY,
                            layer: Vec::new(),
                        });
                    } else if let Some(slot) = self.glow_cache.get_mut(index) {
                        *slot = None;
                    }
                }
                Item::EndGlow => {
                    if inner > 0 {
                        inner -= 1;
                    } else if let Some(g) = glow.take() {
                        let started = self.profile.is_some().then(Instant::now);
                        self.finish_glow(target, g);
                        self.note("glow", started);
                    }
                }
                other => {
                    let started = self.profile.is_some().then(Instant::now);
                    let kind = match other {
                        Item::Path { .. } => "path",
                        Item::Glyphs { .. } => "text",
                        Item::Image { .. } => "image",
                        Item::Sprite { .. } => "sprite",
                        _ => "other",
                    };
                    let mask = masks.last().and_then(Option::as_ref);
                    match glow.as_mut() {
                        Some(g) => {
                            let drawn = self.draw_item(pack, other, &mut g.layer.as_mut(), mask, view);
                            g.bounds = g.bounds.union(drawn);
                        }
                        None => {
                            self.draw_item(pack, other, target, mask, view);
                        }
                    }
                    self.note(if mask.is_some() { "masked" } else { kind }, started);
                    if mask.is_some() {
                        self.note(kind, None);
                    }
                }
            }
        }
        if let Some(g) = glow.take() {
            self.finish_glow(target, g);
        }
    }

    /// Adds the time since `started` to the profile under `kind`, when profiling.
    fn note(&mut self, kind: &'static str, started: Option<Instant>) {
        if let (Some(p), Some(s)) = (self.profile.as_mut(), started) {
            *p.entry(kind).or_insert(0.0) += s.elapsed().as_secs_f64() * 1e3;
        }
    }

    /// Blurs a glow's layer onto the screen, lays the layer over it, and clears the layer for next time.
    fn finish_glow(&mut self, target: &mut PixmapMut, mut g: OpenGlow) {
        let bounds = g.bounds.clip(target.width() as i64, target.height() as i64);
        let mut recorded = (Bounds::EMPTY, Vec::new());
        if !bounds.is_empty() {
            recorded = add_glow(target, &g.layer, bounds, g.sigma, g.strength, &mut self.glow_scratch, g.keep);
            over(target, &g.layer, bounds);
        }
        if let Some(Some(c)) = self.glow_cache.get_mut(g.index).filter(|_| g.keep) {
            c.area = recorded.0;
            c.added = recorded.1;
            c.layer_area = bounds;
            c.layer = crop(&g.layer, bounds);
        }
        if !bounds.is_empty() {
            clear(&mut g.layer, bounds);
        }
        self.glow_layer = Some(g.layer);
    }

    /// Draws one item, returning the part of the screen it may have touched.
    fn draw_item(&mut self, pack: &Pack, item: &Item, target: &mut PixmapMut, mask: Option<&Mask>, view: View) -> Bounds {
        match item {
            Item::Path { points, breaks, closed, stroke, fill } => {
                let mut pb = PathBuilder::new();
                let mut next_break = breaks.iter().peekable();
                for (i, &(x, y)) in points.iter().enumerate() {
                    let (px, py) = view.px(x, y);
                    let starts = i == 0 || next_break.peek().is_some_and(|&&b| b as usize == i);
                    if next_break.peek().is_some_and(|&&b| b as usize == i) {
                        next_break.next();
                    }
                    if starts {
                        pb.move_to(px, py);
                    } else {
                        pb.line_to(px, py);
                    }
                }
                if *closed {
                    pb.close();
                }
                let Some(path) = pb.finish() else { return Bounds::EMPTY };
                let r = path.bounds();
                let reach = stroke.map_or(0.0, |(w, _)| w as f64 * view.scale / 2.0) + 2.0;
                let bounds = Bounds::around(r.left() as f64, r.top() as f64, r.right() as f64, r.bottom() as f64, reach);
                if let Some(c) = fill {
                    let mut paint = Paint { anti_alias: true, ..Paint::default() };
                    paint.set_color(color(*c));
                    target.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), mask);
                }
                if let Some((width, c)) = stroke {
                    let mut paint = Paint { anti_alias: true, ..Paint::default() };
                    paint.set_color(color(*c));
                    let px_width = (*width as f64 * view.scale).max(0.5) as f32;
                    // Round joins are costly to build at every vertex of a long sampled curve, whose joins are
                    // nearly straight anyway; those get mitred (bevelled where sharp) joins.
                    let line_join = if points.len() > 64 { LineJoin::Miter } else { LineJoin::Round };
                    // A mesh is many separate edges meeting at shared vertices, where round caps would mostly
                    // overlap; flat ends cost far less to build.
                    let line_cap = if breaks.is_empty() { LineCap::Round } else { LineCap::Square };
                    // Separate two-point edges, as a mesh draws, are drawn by a rasteriser for thin straight
                    // segments with square ends, which is what stroking them would give, at far less cost.
                    // So is a lone straight line too thin for the shape of its ends to show.
                    let thin_line = breaks.is_empty() && points.len() == 2 && !*closed && px_width <= 3.0;
                    if thin_line || (!breaks.is_empty() && segments_only(points.len(), breaks)) {
                        draw_segments(target, mask, points, view, px_width, *c, &mut self.coverage);
                    } else if breaks.is_empty() && px_width <= THIN_LINE {
                        // So is any other thin line, with round ends and joins, at a cost that follows the pixels
                        // it covers rather than the area it spans.
                        let px: Vec<(f32, f32)> = points.iter().map(|&(x, y)| view.px(x, y)).collect();
                        draw_round_line(target, mask, &px, *closed, px_width, *c, &mut self.line_coverage);
                    } else {
                        let stroke = Stroke { width: px_width, line_cap, line_join, miter_limit: 2.0, ..Stroke::default() };
                        target.stroke_path(&path, &paint, &stroke, Transform::identity(), mask);
                    }
                }
                bounds
            }
            Item::Glyphs { size, cell_h, font, stretch, glyphs } => {
                let px_size = (*size as f64 * view.scale) as f32;
                if !(1.0..=2000.0).contains(&px_size) {
                    return Bounds::EMPTY;
                }
                let key_size = (px_size * 4.0).round() as u32;
                let font = if (*font as usize) < self.fonts.len() { *font } else { 0 };
                let stretch = stretch.clamp(0.25, 4.0);
                let key_stretch = (stretch * 256.0).round() as u32;
                let metrics = font_metrics(&self.fonts[font as usize]);
                let cell_w = metrics.advance * *size as f64 * view.scale * stretch as f64;
                let cell_h = *cell_h as f64 * view.scale;
                let ascent = metrics.ascent * *size as f64 * view.scale;
                let mut bounds = Bounds::EMPTY;
                for g in glyphs {
                    if g.color[3] <= 0.0 {
                        continue;
                    }
                    let (x, y) = view.px(g.x, g.y);
                    let cell = Cell { left: x as f64, top: y as f64 - ascent, width: cell_w, height: cell_h };
                    bounds = bounds.union(Bounds::around(cell.left, cell.top, cell.left + cell.width, cell.top + cell.height, px_size as f64 * 0.5 + 2.0));
                    if draw_cell_shape(target, mask, g.ch, cell, g.color) {
                        continue;
                    }
                    let key = (font, g.ch, key_size, key_stretch);
                    if !self.glyphs.contains_key(&key) {
                        let (m, coverage) = self.fonts[font as usize].rasterize(g.ch, key_size as f32 / 4.0);
                        let glyph = if key_stretch == 256 { (m, coverage) } else { stretch_glyph(m, &coverage, stretch) };
                        if self.glyphs.len() >= GLYPH_CACHE || self.glyph_bytes + glyph.1.len() > GLYPH_CACHE_BYTES {
                            self.glyphs.clear();
                            self.glyph_bytes = 0;
                        }
                        self.glyph_bytes += glyph.1.len();
                        self.glyphs.insert(key, glyph);
                    }
                    let (metrics, coverage) = &self.glyphs[&key];
                    let left = (x + metrics.xmin as f32).round() as i64;
                    let top = (y - metrics.ymin as f32 - metrics.height as f32).round() as i64;
                    blend_coverage(target, mask, left, top, metrics.width, metrics.height, coverage, g.color);
                }
                bounds
            }
            Item::Sprite { image, src, x, y, pixel, flip, alpha, smooth } => {
                let Some(sheet) = self.sheet(pack, *image) else { return Bounds::EMPTY };
                if *smooth {
                    draw_sprite_smooth(target, mask, &sheet, *src, view.px(*x, *y), *pixel as f64 * view.scale, *flip, *alpha)
                } else {
                    draw_sprite(target, mask, &sheet, *src, view.px(*x, *y), *pixel as f64 * view.scale, *flip, *alpha)
                }
            }
            Item::Image { source, fit, zoom, pan_x, pan_y, alpha, transform } => {
                let Some(index) = self.image(pack, *source) else { return Bounds::EMPTY };
                let image = &self.images[index].1;
                let p = Placement::new((image.width(), image.height()), *fit, *zoom, *pan_x, *pan_y, transform, &view);
                // Unturned and uniformly scaled: show a copy resampled to its size at whole pixels, rather than
                // filtering every pixel of every frame. The copy is made once its size has held for a few
                // frames, or straight away for an image coming into view, which is held back until it's ready.
                if let Some(size) = p.whole {
                    let held = match self.asked.iter().find(|(r, s, _)| *r == *source && *s == size) {
                        Some((_, _, frames)) => frames + 1,
                        None => 1,
                    };
                    self.asked.retain(|(r, _, _)| *r != *source);
                    self.asked.push((*source, size, held));
                    let arriving = self.background_decoding && !self.drawn_recently(*source);
                    let resampling = |r: &Raster| r.resampling.iter().any(|(s, _, _)| *s == *source);
                    if held >= STEADY_FRAMES || (arriving && !resampling(self)) {
                        self.request_scaled(index, size);
                    }
                    if self.scaled.iter().any(|(r, s, _, _)| *r == *source && *s == size) {
                        let fade_in = self.arrival(*source);
                        let Some((scaled, opaque)) = self.scaled_copy(*source, size) else { return Bounds::EMPTY };
                        blit(target, mask, scaled, opaque, p.x.round() as i64, p.y.round() as i64, *alpha * fade_in);
                        self.mark_drawn(*source);
                        return Bounds::screen(view);
                    }
                    if arriving && resampling(self) {
                        let since = match self.held_back.iter().find(|(r, _)| *r == *source) {
                            Some((_, at)) => *at,
                            None => {
                                self.held_back.push((*source, Instant::now()));
                                Instant::now()
                            }
                        };
                        if since.elapsed() < MAX_HOLD {
                            return Bounds::EMPTY;
                        }
                    }
                }
                let fade_in = self.arrival(*source);
                let image: &Pixmap = &self.images[index].1;
                let t = transform;
                let matrix =
                    Transform::from_row((t.a * p.scale) as f32, (t.b * p.scale) as f32, (t.c * p.scale) as f32, (t.d * p.scale) as f32, p.x as f32, p.y as f32);
                let paint = PixmapPaint { opacity: *alpha * fade_in, quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
                target.draw_pixmap(0, 0, image.as_ref(), &paint, matrix, mask);
                self.mark_drawn(*source);
                Bounds::screen(view)
            }
            Item::Clip(_) | Item::Unclip | Item::Glow { .. } | Item::EndGlow => Bounds::EMPTY,
        }
    }
}

/// A font's proportions. Text is laid out as a terminal lays it out: every character takes the width of an M.
fn font_metrics(font: &fontdue::Font) -> FontMetrics {
    let size = 1000.0;
    let advance = font.metrics('M', size).advance_width as f64 / size as f64;
    let lines = font.horizontal_line_metrics(size);
    let ascent = lines.map(|m| m.ascent as f64 / size as f64).unwrap_or(0.8);
    let line = lines.map(|m| m.new_line_size as f64 / size as f64).unwrap_or(1.2);
    FontMetrics { advance, ascent, line }
}

/// A glyph's coverage made `stretch` times as wide, each new column averaging the old columns it spans.
fn stretch_glyph(mut m: fontdue::Metrics, coverage: &[u8], stretch: f32) -> GlyphBitmap {
    let (w, h) = (m.width, m.height);
    if w == 0 || h == 0 {
        return (m, coverage.to_vec());
    }
    let nw = ((w as f32 * stretch).round() as usize).max(1);
    let scale = w as f32 / nw as f32;
    let mut out = vec![0u8; nw * h];
    for row in 0..h {
        let src = &coverage[row * w..(row + 1) * w];
        for (x, dst) in out[row * nw..(row + 1) * nw].iter_mut().enumerate() {
            let (a, b) = (x as f32 * scale, (x + 1) as f32 * scale);
            let mut sum = 0.0;
            let mut c = a.floor() as usize;
            while (c as f32) < b && c < w {
                let overlap = (b.min(c as f32 + 1.0) - a.max(c as f32)).max(0.0);
                sum += src[c] as f32 * overlap;
                c += 1;
            }
            *dst = (sum / (b - a)).round().min(255.0) as u8;
        }
    }
    m.xmin = (m.xmin as f32 * stretch).round() as i32;
    m.width = nw;
    m.advance_width *= stretch;
    (m, out)
}

/// Whether a path of `count` points, starting a subpath at each of `breaks`, is all two-point segments.
fn segments_only(count: usize, breaks: &[u32]) -> bool {
    count == breaks.len() * 2 && breaks.iter().enumerate().all(|(i, &b)| b as usize == i * 2)
}

/// Draws two-point segments `width` pixels wide with square ends, reaching half their width past either end. Each
/// segment's coverage of each pixel is worked out from the pixel's distance across and along it, as the overlap of
/// the pixel with the segment in each direction. The segments' coverage is gathered into one mask, keeping the most
/// any segment gives a pixel, and laid down once, so segments meeting at a point don't darken it twice.
fn draw_segments(target: &mut PixmapMut, mask: Option<&Mask>, points: &[(f32, f32)], view: View, width: f32, c: [f32; 4], cov: &mut Vec<u8>) {
    let (tw, th) = (target.width() as i64, target.height() as i64);
    let half = width / 2.0;
    let reach = half + 1.0;
    let mut px: Vec<(f32, f32, f32, f32)> = Vec::with_capacity(points.len() / 2);
    let mut bounds = Bounds::EMPTY;
    for pair in points.chunks_exact(2) {
        let (ax, ay) = view.px(pair[0].0, pair[0].1);
        let (bx, by) = view.px(pair[1].0, pair[1].1);
        bounds = bounds.union(Bounds::around(ax.min(bx) as f64, ay.min(by) as f64, ax.max(bx) as f64, ay.max(by) as f64, reach as f64));
        px.push((ax, ay, bx, by));
    }
    let b = bounds.clip(tw, th);
    if b.is_empty() || c[3] <= 0.0 {
        return;
    }
    let (bw, bh) = ((b.x1 - b.x0) as usize, (b.y1 - b.y0) as usize);
    cov.clear();
    cov.resize(bw * bh, 0);
    for &(ax, ay, bx, by) in &px {
        let (dx, dy) = (bx - ax, by - ay);
        let len = (dx * dx + dy * dy).sqrt();
        let (ux, uy) = if len > 0.01 { (dx / len, dy / len) } else { (1.0, 0.0) };
        // Square ends: the segment reaches half its width past each end.
        let (sx, sy) = (ax - ux * half, ay - uy * half);
        let long = len + width;
        let (nx, ny) = (-uy, ux);
        if uy == 0.0 {
            // Level: each row's coverage across is the same all along, tapering only over the end pixels.
            let (left, right) = (sx.min(sx + ux * long), sx.max(sx + ux * long));
            let y0 = ((ay - half - 1.0).floor() as i64).max(b.y0);
            let y1 = ((ay + half + 1.0).ceil() as i64).min(b.y1);
            let x0 = ((left - 1.0).floor() as i64).max(b.x0);
            let x1 = ((right + 1.0).ceil() as i64).min(b.x1);
            for y in y0..y1 {
                let d = y as f32 + 0.5 - ay;
                let across = ((d + 0.5).min(half) - (d - 0.5).max(-half)).clamp(0.0, 1.0);
                if across <= 0.0 {
                    continue;
                }
                let row = (y - b.y0) as usize * bw;
                for x in x0..x1 {
                    let cx = x as f32 + 0.5;
                    let along = ((cx + 0.5).min(right) - (cx - 0.5).max(left)).clamp(0.0, 1.0);
                    let v = (across * along * 255.0 + 0.5) as u8;
                    let cell = &mut cov[row + (x - b.x0) as usize];
                    *cell = (*cell).max(v);
                }
            }
            continue;
        }
        let y0 = ((ay.min(by) - reach).floor() as i64).max(b.y0);
        let y1 = ((ay.max(by) + reach).ceil() as i64).min(b.y1);
        for y in y0..y1 {
            let cy = y as f32 + 0.5 - sy;
            // The pixel centers x on this row within reach: across, |(x - sx) nx + cy ny| <= half + 1; along,
            // -1 <= (x - sx) ux + cy uy <= long + 1.
            let mut lo = f32::NEG_INFINITY;
            let mut hi = f32::INFINITY;
            for (k, base, min, max) in [(nx, cy * ny, -half - 1.0, half + 1.0), (ux, cy * uy, -1.0, long + 1.0)] {
                if k.abs() < 1e-6 {
                    if base < min || base > max {
                        lo = f32::INFINITY;
                    }
                } else {
                    let (p, q) = ((min - base) / k, (max - base) / k);
                    lo = lo.max(p.min(q));
                    hi = hi.min(p.max(q));
                }
            }
            if lo > hi {
                continue;
            }
            let x0 = ((lo + sx - 0.5).floor() as i64).max(b.x0);
            let x1 = ((hi + sx + 0.5).ceil() as i64).min(b.x1);
            let row = (y - b.y0) as usize * bw;
            for x in x0..x1 {
                let cx = x as f32 + 0.5 - sx;
                let across = cx * nx + cy * ny;
                let along = cx * ux + cy * uy;
                // How much of the pixel, as a unit square seen edge-on, lies within the band in each direction.
                let a = ((across + 0.5).min(half) - (across - 0.5).max(-half)).clamp(0.0, 1.0);
                let l = ((along + 0.5).min(long) - (along - 0.5).max(0.0)).clamp(0.0, 1.0);
                let v = (a * l * 255.0 + 0.5) as u8;
                let cell = &mut cov[row + (x - b.x0) as usize];
                if v > *cell {
                    *cell = v;
                }
            }
        }
    }
    blend_coverage(target, mask, b.x0, b.y0, bw, bh, cov, c);
}

/// The widest line, in pixels, that `draw_round_line` draws rather than the general stroker.
const THIN_LINE: f32 = 8.0;

/// Draws a line through `points` (in pixels) `width` pixels wide with round ends and joins. A pixel's coverage is
/// how far its center lies inside half the width of the nearest segment, give or take half a pixel. The segments'
/// coverage is gathered into `cov`, a screen-sized buffer that is all zero between uses, keeping the most any
/// segment gives a pixel so that joins aren't laid down twice; then only the runs of pixels the segments touched
/// are laid down and cleared, each pixel once.
fn draw_round_line(target: &mut PixmapMut, mask: Option<&Mask>, points: &[(f32, f32)], closed: bool, width: f32, c: [f32; 4], cov: &mut Vec<u8>) {
    let (tw, th) = (target.width() as i64, target.height() as i64);
    if points.len() < 2 || c[3] <= 0.0 || tw == 0 || th == 0 {
        return;
    }
    if cov.len() != (tw * th) as usize {
        cov.clear();
        cov.resize((tw * th) as usize, 0);
    }
    let half = width / 2.0;
    let reach = half + 1.0;
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let count = if closed { points.len() } else { points.len() - 1 };
    for i in 0..count {
        let (ax, ay) = points[i];
        let (bx, by) = points[(i + 1) % points.len()];
        if !(ax.is_finite() && ay.is_finite() && bx.is_finite() && by.is_finite()) {
            continue;
        }
        let (dx, dy) = (bx - ax, by - ay);
        let len = (dx * dx + dy * dy).sqrt();
        let (ux, uy) = if len > 0.01 { (dx / len, dy / len) } else { (1.0, 0.0) };
        let (nx, ny) = (-uy, ux);
        let y0 = ((ay.min(by) - reach).floor() as i64).max(0);
        let y1 = ((ay.max(by) + reach).ceil() as i64).min(th);
        for y in y0..y1 {
            let cy = y as f32 + 0.5 - ay;
            // The pixel centers x on this row within reach: across, |(x - ax) nx + cy ny| <= reach; along,
            // -reach <= (x - ax) ux + cy uy <= len + reach.
            let mut lo = f32::NEG_INFINITY;
            let mut hi = f32::INFINITY;
            for (k, base, min, max) in [(nx, cy * ny, -reach, reach), (ux, cy * uy, -reach, len + reach)] {
                if k.abs() < 1e-6 {
                    if base < min || base > max {
                        lo = f32::INFINITY;
                    }
                } else {
                    let (p, q) = ((min - base) / k, (max - base) / k);
                    lo = lo.max(p.min(q));
                    hi = hi.min(p.max(q));
                }
            }
            if lo > hi {
                continue;
            }
            let x0 = ((lo + ax - 0.5).floor() as i64).max(0);
            let x1 = ((hi + ax + 0.5).ceil() as i64).min(tw);
            if x0 >= x1 {
                continue;
            }
            let row = (y * tw) as usize;
            for x in x0..x1 {
                let cx = x as f32 + 0.5 - ax;
                let t = (cx * ux + cy * uy).clamp(0.0, len);
                let (ex, ey) = (cx - ux * t, cy - uy * t);
                let d = (ex * ex + ey * ey).sqrt();
                let v = ((half + 0.5 - d).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                let cell = &mut cov[row + x as usize];
                if v > *cell {
                    *cell = v;
                }
            }
            runs.push((row + x0 as usize, (x1 - x0) as usize));
        }
    }
    // Where runs overlap, the first lays the pixels down and clears them, so the next finds nothing there to add.
    let a = c[3].clamp(0.0, 1.0);
    let channel = |v: f32| (v.clamp(0.0, 1.0) * a * 255.0).round() as u32;
    let (cr, cg, cb, ca) = (channel(c[0]), channel(c[1]), channel(c[2]), (a * 255.0).round() as u32);
    let mask_data = mask.map(|m| m.data());
    let data = target.data_mut();
    for (from, n) in runs {
        for i in from..from + n {
            let k = std::mem::take(&mut cov[i]) as u32;
            if k == 0 {
                continue;
            }
            let k = match mask_data {
                Some(m) => div255(k * m[i] as u32),
                None => k,
            };
            let keep = 255 - div255(ca * k);
            let px = &mut data[i * 4..i * 4 + 4];
            px[0] = (div255(cr * k) + div255(px[0] as u32 * keep)).min(255) as u8;
            px[1] = (div255(cg * k) + div255(px[1] as u32 * keep)).min(255) as u8;
            px[2] = (div255(cb * k) + div255(px[2] as u32 * keep)).min(255) as u8;
            px[3] = (div255(ca * k) + div255(px[3] as u32 * keep)).min(255) as u8;
        }
    }
}

/// Lays each pixel of `canvas` down as a `b` x `b` block of `target`, the two centered on each other.
fn upscale(canvas: &Pixmap, target: &mut PixmapMut, b: u32) {
    let (cw, ch) = (canvas.width() as i64, canvas.height() as i64);
    let (tw, th) = (target.width() as i64, target.height() as i64);
    let b = b as i64;
    let (ox, oy) = ((tw - cw * b) / 2, (th - ch * b) / 2);
    let src = canvas.data();
    let data = target.data_mut();
    let mut row = vec![0u8; tw as usize * 4];
    for cy in 0..ch {
        let (y0, y1) = ((oy + cy * b).max(0), (oy + (cy + 1) * b).min(th));
        if y0 >= y1 {
            continue;
        }
        // One screen row from this canvas row, then copied down the block.
        for (x, px) in row.chunks_exact_mut(4).enumerate() {
            let cx = ((x as i64 - ox).div_euclid(b)).clamp(0, cw - 1);
            let at = ((cy * cw + cx) * 4) as usize;
            px.copy_from_slice(&src[at..at + 4]);
        }
        for y in y0..y1 {
            let at = (y * tw * 4) as usize;
            data[at..at + row.len()].copy_from_slice(&row);
        }
    }
}

/// Draws the rectangle `src` (x, y, width, height) of `sheet` with its top left at `at`, each of its pixels `k`
/// screen pixels square with hard edges, mirrored left to right if `flip`, at `alpha`. A sprite's corner falls on
/// a whole pixel, so it stays crisp as it moves.
#[allow(clippy::too_many_arguments)]
fn draw_sprite(target: &mut PixmapMut, mask: Option<&Mask>, sheet: &Pixmap, src: [u32; 4], at: (f32, f32), k: f64, flip: bool, alpha: f32) -> Bounds {
    let [sx, sy, sw, sh] = src;
    if k <= 0.0 || alpha <= 0.0 || sx + sw > sheet.width() || sy + sh > sheet.height() {
        return Bounds::EMPTY;
    }
    // Each edge falls on the whole pixel nearest it, so sprites placed edge to edge (strips of one picture, say) meet
    // without a gap or an overlap, however the scale falls.
    let (left, top) = (at.0.round() as i64, at.1.round() as i64);
    let (right, bottom) = ((at.0 as f64 + sw as f64 * k).round() as i64, (at.1 as f64 + sh as f64 * k).round() as i64);
    let (dw, dh) = ((right - left).max(1), (bottom - top).max(1));
    let (tw, th) = (target.width() as i64, target.height() as i64);
    let (x0, x1) = (left.max(0), (left + dw).min(tw));
    let (y0, y1) = (top.max(0), (top + dh).min(th));
    if x0 >= x1 || y0 >= y1 {
        return Bounds::EMPTY;
    }
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
    let mask_data = mask.map(|m| m.data());
    let sheet_w = sheet.width() as usize;
    let pixels = sheet.data();
    let data = target.data_mut();
    for y in y0..y1 {
        let row = sy as usize + (((y - top) as f64 + 0.5) / k).floor().clamp(0.0, sh as f64 - 1.0) as usize;
        for x in x0..x1 {
            let mut col = (((x - left) as f64 + 0.5) / k).floor().clamp(0.0, sw as f64 - 1.0) as usize;
            if flip {
                col = sw as usize - 1 - col;
            }
            let s = (row * sheet_w + sx as usize + col) * 4;
            if pixels[s + 3] == 0 {
                continue;
            }
            let at = (y * tw + x) as usize;
            let mut cover = a;
            if let Some(m) = mask_data {
                cover = div255(cover * m[at] as u32);
            }
            if cover == 0 {
                continue;
            }
            // The sheet is premultiplied: source-over at `cover`/255 of its strength.
            let sa = div255(pixels[s + 3] as u32 * cover);
            let keep = 255 - sa;
            let px = &mut data[at * 4..at * 4 + 4];
            for c in 0..3 {
                px[c] = (div255(pixels[s + c] as u32 * cover) + div255(px[c] as u32 * keep)).min(255) as u8;
            }
            px[3] = (sa + div255(px[3] as u32 * keep)).min(255) as u8;
        }
    }
    Bounds { x0, y0, x1, y1 }
}

/// Draws a sprite frame sampled smoothly: each screen pixel takes the four nearest pixels of the frame, weighted by
/// how near they are, so the frame can sit at any position and scale without stepping or blocking. The weights are
/// worked out once a column and once a row, in eighths of a pixel's 256ths, and wholly clear stretches are skipped.
#[allow(clippy::too_many_arguments)]
fn draw_sprite_smooth(target: &mut PixmapMut, mask: Option<&Mask>, sheet: &Pixmap, src: [u32; 4], at: (f32, f32), k: f64, flip: bool, alpha: f32) -> Bounds {
    let [sx, sy, sw, sh] = src;
    if k <= 0.0 || alpha <= 0.0 || sx + sw > sheet.width() || sy + sh > sheet.height() {
        return Bounds::EMPTY;
    }
    let (ax, ay) = (at.0 as f64, at.1 as f64);
    let (tw, th) = (target.width() as i64, target.height() as i64);
    let (x0, x1) = ((ax.floor() as i64).max(0), ((ax + sw as f64 * k).ceil() as i64).min(tw));
    let (y0, y1) = ((ay.floor() as i64).max(0), ((ay + sh as f64 * k).ceil() as i64).min(th));
    if x0 >= x1 || y0 >= y1 {
        return Bounds::EMPTY;
    }
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
    let mask_data = mask.map(|m| m.data());
    let sheet_w = sheet.width() as usize;
    let pixels = sheet.data();
    // For each screen column, the byte offsets within a sheet row of the frame's two columns it falls between (or
    // none, off the frame's edge), and the weight of the second, out of 256; the same for rows, once each.
    let col_at = |c: i64| -> Option<usize> {
        (0..sw as i64).contains(&c).then(|| {
            let c = if flip { sw as i64 - 1 - c } else { c };
            (sx as usize + c as usize) * 4
        })
    };
    let row_at = |r: i64| -> Option<usize> { (0..sh as i64).contains(&r).then(|| (sy as usize + r as usize) * sheet_w * 4) };
    let cols: Vec<(Option<usize>, Option<usize>, u32)> = (x0..x1)
        .map(|x| {
            let u = (x as f64 + 0.5 - ax) / k - 0.5;
            let c0 = u.floor() as i64;
            (col_at(c0), col_at(c0 + 1), ((u - u.floor()) * 256.0).round() as u32)
        })
        .collect();
    let texel = |row: Option<usize>, col: Option<usize>| -> [u32; 4] {
        match (row, col) {
            (Some(r), Some(c)) => {
                let s = r + c;
                [pixels[s] as u32, pixels[s + 1] as u32, pixels[s + 2] as u32, pixels[s + 3] as u32]
            }
            _ => [0; 4],
        }
    };
    let data = target.data_mut();
    for y in y0..y1 {
        let v = (y as f64 + 0.5 - ay) / k - 0.5;
        let r0 = v.floor() as i64;
        let (row0, row1) = (row_at(r0), row_at(r0 + 1));
        if row0.is_none() && row1.is_none() {
            continue;
        }
        let wy = ((v - v.floor()) * 256.0).round() as u32;
        let row_base = (y * tw) as usize;
        for (n, &(c0, c1, wx)) in cols.iter().enumerate() {
            // Clear on all four sides: nothing to draw. Checked on alpha alone, before anything else is read.
            let alpha_at = |row: Option<usize>, col: Option<usize>| match (row, col) {
                (Some(r), Some(c)) => pixels[r + c + 3],
                _ => 0,
            };
            if alpha_at(row0, c0) | alpha_at(row0, c1) | alpha_at(row1, c0) | alpha_at(row1, c1) == 0 {
                continue;
            }
            let (p00, p10, p01, p11) = (texel(row0, c0), texel(row0, c1), texel(row1, c0), texel(row1, c1));
            let mut p = [0u32; 4];
            for (c, out) in p.iter_mut().enumerate() {
                let top = p00[c] * (256 - wx) + p10[c] * wx;
                let bottom = p01[c] * (256 - wx) + p11[c] * wx;
                *out = (top * (256 - wy) + bottom * wy + (1 << 15)) >> 16;
            }
            if p[3] == 0 {
                continue;
            }
            let at = row_base + x0 as usize + n;
            let mut cover = a;
            if let Some(m) = mask_data {
                cover = div255(cover * m[at] as u32);
            }
            if cover == 0 {
                continue;
            }
            let sa = div255(p[3] * cover);
            let keep = 255 - sa;
            let px = &mut data[at * 4..at * 4 + 4];
            for c in 0..3 {
                px[c] = (div255(p[c] * cover) + div255(px[c] as u32 * keep)).min(255) as u8;
            }
            px[3] = (sa + div255(px[3] as u32 * keep)).min(255) as u8;
        }
    }
    Bounds { x0, y0, x1, y1 }
}

/// A rectangle of pixels, from (x0, y0) up to but not including (x1, y1).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bounds {
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
}

impl Bounds {
    const EMPTY: Bounds = Bounds { x0: i64::MAX, y0: i64::MAX, x1: i64::MIN, y1: i64::MIN };

    fn screen(view: View) -> Bounds {
        Bounds { x0: 0, y0: 0, x1: view.width as i64, y1: view.height as i64 }
    }

    /// The whole pixels around a box, with `reach` to spare on every side.
    fn around(left: f64, top: f64, right: f64, bottom: f64, reach: f64) -> Bounds {
        let clamp = |v: f64| v.clamp(-1e6, 1e6) as i64;
        Bounds { x0: clamp((left - reach).floor()), y0: clamp((top - reach).floor()), x1: clamp((right + reach).ceil()), y1: clamp((bottom + reach).ceil()) }
    }

    fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }

    fn union(self, o: Bounds) -> Bounds {
        Bounds { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }

    fn grow(self, by: i64) -> Bounds {
        Bounds { x0: self.x0 - by, y0: self.y0 - by, x1: self.x1 + by, y1: self.y1 + by }
    }

    fn clip(self, w: i64, h: i64) -> Bounds {
        Bounds { x0: self.x0.max(0), y0: self.y0.max(0), x1: self.x1.min(w), y1: self.y1.min(h) }
    }
}

/// A glow being drawn: its layer, its blur in pixels, its strength, and where in the layer anything was drawn.
struct OpenGlow {
    layer: Pixmap,
    sigma: f32,
    strength: f32,
    bounds: Bounds,
    /// Which glow of the frame this is, and whether it may be kept for the next (it isn't inside a clip).
    index: usize,
    keep: bool,
}

/// A glow as last drawn: the items it lit, and, over the screen area it touched, the light it added and the layer of
/// the items themselves.
struct GlowCache {
    items: Vec<Item>,
    sigma: f32,
    strength: f32,
    size: (u32, u32),
    area: Bounds,
    added: Vec<u8>,
    layer_area: Bounds,
    layer: Vec<u8>,
}

/// Adds the layer, blurred by about `sigma` pixels and scaled by `strength`, to the target. The blur is worked out
/// at a fraction of the screen's resolution, over only the part of the layer that was drawn on and its reach.
/// When `record` is set, it also returns the screen area it added to and what it added there, row by row.
fn add_glow(target: &mut PixmapMut, layer: &Pixmap, drawn: Bounds, sigma: f32, strength: f32, small: &mut Vec<[f32; 4]>, record: bool) -> (Bounds, Vec<u8>) {
    let (w, h) = (target.width() as i64, target.height() as i64);
    let reach = (sigma * 3.0).ceil() as i64 + 1;
    let area = drawn.grow(reach).clip(w, h);
    if area.is_empty() || strength <= 0.0 || sigma <= 0.0 {
        return (Bounds::EMPTY, Vec::new());
    }
    let mut added = if record { vec![0u8; ((area.x1 - area.x0) * (area.y1 - area.y0) * 4) as usize] } else { Vec::new() };
    // Each cell of the small grid averages f x f pixels: about a sixth of the blur's spread, as finer cells change
    // nothing a glow shows.
    let f = ((sigma / 1.2).round() as i64).clamp(2, 16);
    let (sw, sh) = (((area.x1 - area.x0 + f - 1) / f) as usize, ((area.y1 - area.y0 + f - 1) / f) as usize);
    small.clear();
    small.resize(sw * sh, [0.0; 4]);
    let data = layer.data();
    let first_cell = (drawn.x0 - area.x0) / f;
    let last_cell = (drawn.x1 - 1 - area.x0) / f;
    for y in drawn.y0..drawn.y1 {
        let row = ((y - area.y0) / f) as usize * sw;
        // Each cell's run of pixels in this row is summed in whole numbers, passing over transparent ones four at a
        // time, and added to the cell once.
        for cell in first_cell..=last_cell {
            let x0 = (area.x0 + cell * f).max(drawn.x0);
            let x1 = (area.x0 + (cell + 1) * f).min(drawn.x1);
            let run = &data[((y * w + x0) * 4) as usize..((y * w + x1) * 4) as usize];
            let mut sum = [0u32; 4];
            for quad in run.chunks(16) {
                if blank(quad) {
                    continue;
                }
                for px in quad.chunks_exact(4) {
                    for c in 0..4 {
                        sum[c] += px[c] as u32;
                    }
                }
            }
            if sum[3] == 0 {
                continue;
            }
            let into = &mut small[row + cell as usize];
            for c in 0..4 {
                into[c] += sum[c] as f32;
            }
        }
    }
    // Three box blurs come close to a Gaussian: each of radius r adds r(r + 1) / 3 to the variance.
    let s = sigma / f as f32;
    let r = (((1.0 + 4.0 * s * s).sqrt() - 1.0) / 2.0).round().max(1.0) as usize;
    let mut line = Vec::new();
    for _ in 0..3 {
        box_blur(small, sw, sh, 1, sw, r, &mut line);
        box_blur(small, sh, sw, sw, 1, r, &mut line);
    }
    // Back to full size, interpolating between cells, and added to what's there. Runs of cells too faint to matter
    // are skipped, so a glow costs little where nothing glows.
    let k = strength / (f * f) as f32;
    // A cell adding less than one and a half levels to every channel isn't worth drawing.
    let faint = 1.5 / k;
    let tdata = target.data_mut();
    let mut row = vec![[0.0f32; 4]; sw];
    let mut lit = vec![false; sw];
    let width = (area.x1 - area.x0) as usize;
    let mut add = vec![0u8; width * 4];
    // The span of cells in each row of the grid bright enough to draw, if any.
    let spans: Vec<Option<(usize, usize)>> = (0..sh)
        .map(|y| {
            let cells = &small[y * sw..(y + 1) * sw];
            let first = cells.iter().position(|c| c[3] > faint)?;
            let last = cells.iter().rposition(|c| c[3] > faint)?;
            Some((first, last))
        })
        .collect();
    for y in area.y0..area.y1 {
        let sy = ((y - area.y0) as f32 + 0.5) / f as f32 - 0.5;
        let ya = sy.floor().clamp(0.0, (sh - 1) as f32) as usize;
        let yb = (ya + 1).min(sh - 1);
        let ty = (sy - ya as f32).clamp(0.0, 1.0);
        let (lo, hi) = match (spans[ya], spans[yb]) {
            (None, None) => continue,
            (Some(a), None) | (None, Some(a)) => a,
            (Some(a), Some(b)) => (a.0.min(b.0), a.1.max(b.1)),
        };
        // One cell either side, which the pixels between it and the span blend towards.
        let (lo, hi) = (lo.saturating_sub(1), (hi + 1).min(sw - 1));
        lit.fill(false);
        for x in lo..=hi {
            let (a, b) = (small[ya * sw + x], small[yb * sw + x]);
            lit[x] = a[3].max(b[3]) > faint;
            let cell = &mut row[x];
            for c in 0..4 {
                cell[c] = (a[c] + (b[c] - a[c]) * ty) * k;
            }
        }
        let base = ((y * w + area.x0) * 4) as usize;
        let line = &mut tdata[base..base + width * 4];
        // Pixel i sits at cell position (i + 0.5) / f - 0.5. Between the centers of cells c and c + 1 the value
        // ramps from one to the other; before the first center and after the last it holds level.
        let fl = f as f32;
        for c in lo..=hi {
            if !(lit[c] || (c < hi && lit[c + 1])) {
                continue;
            }
            let (a, b) = (row[c], if c < hi { row[c + 1] } else { row[c] });
            // The pixels from cell c's center to cell c + 1's, plus the half cell before the first.
            let start = if c == 0 { 0 } else { ((c as f32 + 0.5) * fl - 0.5).ceil().max(0.0) as usize };
            let end = if c + 1 >= sw { width } else { (((c + 1) as f32 + 0.5) * fl - 0.5).ceil().max(0.0) as usize };
            let (start, end) = (start.min(width), end.min(width));
            if start >= end {
                continue;
            }
            // In 1/256ths, color only: the screen beneath is always opaque.
            let tx = ((start as f32 + 0.5) / fl - 0.5 - c as f32).clamp(0.0, 1.0);
            let mut v = [0i32; 3];
            let mut d = [0i32; 3];
            for k in 0..3 {
                v[k] = ((a[k] + (b[k] - a[k]) * tx) * 256.0) as i32;
                d[k] = if c + 1 < sw { ((b[k] - a[k]) / fl * 256.0) as i32 } else { 0 };
            }
            for (j, px) in add[start * 4..end * 4].chunks_exact_mut(4).enumerate() {
                let j = j as i32;
                px[0] = ((v[0] + d[0] * j).max(0) >> 8).min(255) as u8;
                px[1] = ((v[1] + d[1] * j).max(0) >> 8).min(255) as u8;
                px[2] = ((v[2] + d[2] * j).max(0) >> 8).min(255) as u8;
            }
            for (dst, a) in line[start * 4..end * 4].iter_mut().zip(&add[start * 4..end * 4]) {
                *dst = dst.saturating_add(*a);
            }
            if record {
                let at = (((y - area.y0) * (area.x1 - area.x0)) as usize + start) * 4;
                added[at..at + (end - start) * 4].copy_from_slice(&add[start * 4..end * 4]);
            }
        }
    }
    (area, added)
}

/// Where the glow opened at `start` (just after its `Glow`) ends: the index of its matching `EndGlow`, or the end.
fn glow_end(items: &[Item], start: usize) -> usize {
    let mut depth = 0usize;
    for (k, item) in items.iter().enumerate().skip(start) {
        match item {
            Item::Glow { .. } => depth += 1,
            Item::EndGlow if depth == 0 => return k,
            Item::EndGlow => depth -= 1,
            _ => {}
        }
    }
    items.len()
}

/// Adds a recorded glow's light back to the screen, over `area`.
fn add_recorded(target: &mut PixmapMut, area: Bounds, added: &[u8]) {
    if area.is_empty() {
        return;
    }
    let w = target.width() as i64;
    let width = ((area.x1 - area.x0) * 4) as usize;
    let tdata = target.data_mut();
    for y in area.y0..area.y1 {
        let base = ((y * w + area.x0) * 4) as usize;
        let src = &added[(y - area.y0) as usize * width..(y - area.y0 + 1) as usize * width];
        for (quad, add) in tdata[base..base + width].chunks_mut(16).zip(src.chunks(16)) {
            if blank_rgb(add) {
                continue;
            }
            for (d, a) in quad.iter_mut().zip(add) {
                *d = d.saturating_add(*a);
            }
        }
    }
}

/// Lays a recorded layer, cropped to `area`, over the screen.
fn over_recorded(target: &mut PixmapMut, area: Bounds, layer: &[u8]) {
    if area.is_empty() {
        return;
    }
    let w = target.width() as i64;
    let width = ((area.x1 - area.x0) * 4) as usize;
    let tdata = target.data_mut();
    for y in area.y0..area.y1 {
        let base = ((y * w + area.x0) * 4) as usize;
        let src = &layer[(y - area.y0) as usize * width..(y - area.y0 + 1) as usize * width];
        for (dq, sq) in tdata[base..base + width].chunks_mut(16).zip(src.chunks(16)) {
            if blank(sq) {
                continue;
            }
            for (d, s) in dq.chunks_exact_mut(4).zip(sq.chunks_exact(4)) {
                if s[3] == 0 {
                    continue;
                }
                let keep = 255 - s[3] as u32;
                for c in 0..4 {
                    d[c] = (s[c] as u32 + div255(d[c] as u32 * keep)).min(255) as u8;
                }
            }
        }
    }
}

/// A copy of `area` of a layer, row by row.
fn crop(layer: &Pixmap, area: Bounds) -> Vec<u8> {
    if area.is_empty() {
        return Vec::new();
    }
    let w = layer.width() as i64;
    let data = layer.data();
    let mut out = Vec::with_capacity(((area.x1 - area.x0) * (area.y1 - area.y0) * 4) as usize);
    for y in area.y0..area.y1 {
        out.extend_from_slice(&data[((y * w + area.x0) * 4) as usize..((y * w + area.x1) * 4) as usize]);
    }
    out
}

/// Whether a run of added light is all zero (its alpha bytes are always zero; only color is added).
fn blank_rgb(bytes: &[u8]) -> bool {
    bytes.iter().all(|&b| b == 0)
}

/// One pass of a box blur of radius `r` along `lines` lines of `len` cells, `step` apart, starting `stride` apart.
fn box_blur(cells: &mut [[f32; 4]], len: usize, lines: usize, step: usize, stride: usize, r: usize, line: &mut Vec<[f32; 4]>) {
    let norm = 1.0 / (2 * r + 1) as f32;
    for l in 0..lines {
        let start = l * stride;
        line.clear();
        line.extend((0..len).map(|i| cells[start + i * step]));
        let mut sum = [0.0f32; 4];
        // Cells beyond either end count as empty.
        for v in line.iter().take(r.min(len)) {
            for c in 0..4 {
                sum[c] += v[c];
            }
        }
        for i in 0..len {
            if i + r < len {
                for c in 0..4 {
                    sum[c] += line[i + r][c];
                }
            }
            if i > r {
                for c in 0..4 {
                    sum[c] -= line[i - r - 1][c];
                }
            }
            let out = &mut cells[start + i * step];
            for c in 0..4 {
                out[c] = sum[c].max(0.0) * norm;
            }
        }
    }
}

/// Whether a run of up to four pixels is all transparent, read sixteen bytes at a time.
#[inline]
fn blank(bytes: &[u8]) -> bool {
    match <[u8; 16]>::try_from(bytes) {
        Ok(b) => u128::from_ne_bytes(b) == 0,
        Err(_) => bytes.iter().all(|&v| v == 0),
    }
}

/// Lays the premultiplied layer over the target, within `area`.
fn over(target: &mut PixmapMut, layer: &Pixmap, area: Bounds) {
    let w = target.width() as i64;
    let tdata = target.data_mut();
    let ldata = layer.data();
    for y in area.y0..area.y1 {
        let (a, b) = (((y * w + area.x0) * 4) as usize, ((y * w + area.x1) * 4) as usize);
        for (dq, sq) in tdata[a..b].chunks_mut(16).zip(ldata[a..b].chunks(16)) {
            if blank(sq) {
                continue;
            }
            for (d, s) in dq.chunks_exact_mut(4).zip(sq.chunks_exact(4)) {
                if s[3] == 0 {
                    continue;
                }
                let keep = 255 - s[3] as u32;
                for c in 0..4 {
                    d[c] = (s[c] as u32 + div255(d[c] as u32 * keep)).min(255) as u8;
                }
            }
        }
    }
}

/// Makes `area` of the layer transparent again.
fn clear(layer: &mut Pixmap, area: Bounds) {
    let w = layer.width() as i64;
    let data = layer.data_mut();
    for y in area.y0..area.y1 {
        data[((y * w + area.x0) * 4) as usize..((y * w + area.x1) * 4) as usize].fill(0);
    }
}

/// Where an image goes on a screen: its scale from image pixels, the screen position of its top-left corner, and,
/// when it's unturned and uniformly scaled, its size in whole pixels.
struct Placement {
    scale: f64,
    x: f64,
    y: f64,
    whole: Option<(u32, u32)>,
}

impl Placement {
    fn new((iw, ih): (u32, u32), fit: Fit, zoom: f32, pan_x: f32, pan_y: f32, t: &crate::eval::Affine, view: &View) -> Placement {
        let (w, h) = (view.width as f64, view.height as f64);
        let (iw, ih) = (iw as f64, ih as f64);
        let base = match fit {
            Fit::Cover => (w / iw).max(h / ih),
            Fit::Contain => (w / iw).min(h / ih),
        };
        let scale = base * zoom as f64;
        let (dw, dh) = (iw * scale, ih * scale);
        let ox = (w - dw) / 2.0 - pan_x as f64 * (dw - w) / 2.0;
        let oy = (h - dh) / 2.0 - pan_y as f64 * (dh - h) / 2.0;
        let x = t.a * (ox - w / 2.0) + t.c * (oy - h / 2.0) + w / 2.0 + t.e * view.scale;
        let y = t.b * (ox - w / 2.0) + t.d * (oy - h / 2.0) + h / 2.0 + t.f * view.scale;
        let upright = t.b == 0.0 && t.c == 0.0 && t.a > 0.0 && (t.a - t.d).abs() < 1e-12;
        let size = (((iw * scale * t.a).round() as u32).max(1), ((ih * scale * t.a).round() as u32).max(1));
        let whole = (upright && size.0 <= MAX_IMAGE_SIDE && size.1 <= MAX_IMAGE_SIDE).then_some(size);
        Placement { scale, x, y, whole }
    }
}

/// The image resampled to exactly `size`, with whether it's fully opaque.
fn resample(image: &Pixmap, size: (u32, u32)) -> Option<(Pixmap, bool)> {
    let rgba = image::RgbaImage::from_raw(image.width(), image.height(), image.data().to_vec())?;
    // The pixels are premultiplied, which filtering preserves.
    let resized = image::imageops::resize(&rgba, size.0, size.1, image::imageops::FilterType::Triangle);
    let mut pixmap = Pixmap::new(size.0, size.1)?;
    pixmap.data_mut().copy_from_slice(resized.as_raw());
    let opaque = pixmap.data().chunks_exact(4).all(|p| p[3] == 255);
    Some((pixmap, opaque))
}

impl Default for Raster {
    fn default() -> Self {
        Raster::new()
    }
}

/// Whether the convex quadrilateral `quad` (in pixels, either winding) contains the whole `w` by `h` view.
fn covers(quad: &[(f32, f32)], w: f32, h: f32) -> bool {
    let n = quad.len();
    if n < 3 {
        return false;
    }
    let side = |p: (f32, f32), a: (f32, f32), b: (f32, f32)| (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
    [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].iter().all(|&p| {
        let signs: Vec<f32> = (0..n).map(|k| side(p, quad[k], quad[(k + 1) % n])).collect();
        signs.iter().all(|&v| v >= 0.0) || signs.iter().all(|&v| v <= 0.0)
    })
}

/// Copies premultiplied pixels to (x, y), clipped to the target, at `opacity` and inside the mask if there is one.
fn blit(target: &mut PixmapMut, mask: Option<&Mask>, src: &Pixmap, opaque: bool, x: i64, y: i64, opacity: f32) {
    let (tw, th) = (target.width() as i64, target.height() as i64);
    let (sw, sh) = (src.width() as i64, src.height() as i64);
    let (x0, x1) = (x.max(0), (x + sw).min(tw));
    let (y0, y1) = (y.max(0), (y + sh).min(th));
    if x0 >= x1 || y0 >= y1 || opacity <= 0.0 {
        return;
    }
    let mask_data = mask.map(|m| m.data());
    let data = target.data_mut();
    let src_data = src.data();
    let row_len = ((x1 - x0) * 4) as usize;
    let a = opacity.min(1.0);
    for ty in y0..y1 {
        let d = ((ty * tw + x0) * 4) as usize;
        let s = (((ty - y) * sw + (x0 - x)) * 4) as usize;
        let (dst, from) = (&mut data[d..d + row_len], &src_data[s..s + row_len]);
        if a >= 1.0 && mask_data.is_none() && opaque {
            dst.copy_from_slice(from);
            continue;
        }
        let level = (a * 255.0).round() as u32;
        if mask_data.is_none() && opaque {
            // An opaque copy fading in: one weight for every byte, which the compiler can vectorize.
            let keep = 255 - level;
            for (dp, &sp) in dst.iter_mut().zip(from) {
                *dp = div255(sp as u32 * level + *dp as u32 * keep) as u8;
            }
            continue;
        }
        for (i, (dp, sp)) in dst.chunks_exact_mut(4).zip(from.chunks_exact(4)).enumerate() {
            // Integer source-over of premultiplied pixels: k is the source's weight out of 255.
            let k = match mask_data {
                Some(m) => div255(level * m[(ty * tw + x0) as usize + i] as u32),
                None => level,
            };
            let keep = 255 - div255(sp[3] as u32 * k);
            for c in 0..4 {
                dp[c] = div255(sp[c] as u32 * k + dp[c] as u32 * keep).min(255) as u8;
            }
        }
    }
}

/// `x / 255` rounded to nearest: exact up to 255 * 255, and at most one too high up to twice that.
#[inline]
fn div255(x: u32) -> u32 {
    let x = x + 128;
    (x + (x >> 8)) >> 8
}

/// A character cell, in pixels.
#[derive(Clone, Copy)]
struct Cell {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

/// Block elements (U+2580 to U+259F) and Braille patterns (U+2800 to U+28FF) are drawn as shapes filling the cell,
/// as terminals draw them, so neighboring cells join without seams whatever the font has. Returns whether `ch` was
/// one of them.
fn draw_cell_shape(target: &mut PixmapMut, mask: Option<&Mask>, ch: char, cell: Cell, color: [f32; 4]) -> bool {
    let code = ch as u32;
    // Fractions of the cell: (left, top, right, bottom, opacity).
    let mut parts: [(f64, f64, f64, f64, f32); 8] = [(0.0, 0.0, 0.0, 0.0, 0.0); 8];
    let mut count = 0;
    let mut add = |l: f64, t: f64, r: f64, b: f64, a: f32| {
        if count < parts.len() {
            parts[count] = (l, t, r, b, a);
            count += 1;
        }
    };
    match code {
        0x2580 => add(0.0, 0.0, 1.0, 0.5, 1.0),
        0x2581..=0x2588 => add(0.0, 1.0 - (code - 0x2580) as f64 / 8.0, 1.0, 1.0, 1.0),
        0x2589..=0x258F => add(0.0, 0.0, (0x2590 - code) as f64 / 8.0, 1.0, 1.0),
        0x2590 => add(0.5, 0.0, 1.0, 1.0, 1.0),
        0x2591 => add(0.0, 0.0, 1.0, 1.0, 0.25),
        0x2592 => add(0.0, 0.0, 1.0, 1.0, 0.5),
        0x2593 => add(0.0, 0.0, 1.0, 1.0, 0.75),
        0x2594 => add(0.0, 0.0, 1.0, 0.125, 1.0),
        0x2595 => add(0.875, 0.0, 1.0, 1.0, 1.0),
        0x2596..=0x259F => {
            // Quadrants: upper left, upper right, lower left, lower right.
            let quads: u8 = match code {
                0x2596 => 0b0100,
                0x2597 => 0b1000,
                0x2598 => 0b0001,
                0x2599 => 0b1101,
                0x259A => 0b1001,
                0x259B => 0b0111,
                0x259C => 0b1011,
                0x259D => 0b0010,
                0x259E => 0b0110,
                _ => 0b1110,
            };
            for (bit, (l, t)) in [(0.0, 0.0), (0.5, 0.0), (0.0, 0.5), (0.5, 0.5)].iter().enumerate() {
                if quads & (1 << bit) != 0 {
                    add(*l, *t, l + 0.5, t + 0.5, 1.0);
                }
            }
        }
        0x2800..=0x28FF => {
            // Dots 1-3 and 7 down the left column, 4-6 and 8 down the right.
            const DOTS: [(u32, f64, f64); 8] =
                [(0, 0.0, 0.0), (1, 0.0, 1.0), (2, 0.0, 2.0), (3, 1.0, 0.0), (4, 1.0, 1.0), (5, 1.0, 2.0), (6, 0.0, 3.0), (7, 1.0, 3.0)];
            let side = (cell.width / 2.0).min(cell.height / 4.0) * 0.62;
            let (dw, dh) = (side / cell.width, side / cell.height);
            for (bit, col, row) in DOTS {
                if (code - 0x2800) & (1 << bit) != 0 {
                    let cx = (col + 0.5) / 2.0;
                    let cy = (row + 0.5) / 4.0;
                    add(cx - dw / 2.0, cy - dh / 2.0, cx + dw / 2.0, cy + dh / 2.0, 1.0);
                }
            }
        }
        _ => return false,
    }
    for &(l, t, r, b, a) in &parts[..count] {
        // Edges snap to whole pixels, so cells side by side share their boundary exactly.
        let x0 = (cell.left + l * cell.width).round() as i64;
        let x1 = (cell.left + r * cell.width).round() as i64;
        let y0 = (cell.top + t * cell.height).round() as i64;
        let y1 = (cell.top + b * cell.height).round() as i64;
        let (w, h) = ((x1 - x0).max(1) as usize, (y1 - y0).max(1) as usize);
        let coverage = vec![255u8; w * h];
        blend_coverage(target, mask, x0, y0, w, h, &coverage, [color[0], color[1], color[2], color[3] * a]);
    }
    true
}

fn polygon(points: impl Iterator<Item = (f32, f32)>) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for (i, (x, y)) in points.enumerate() {
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    pb.close();
    pb.finish()
}

/// Blends a glyph's coverage in `color` over premultiplied RGBA pixels, inside the clip mask if there is one.
#[allow(clippy::too_many_arguments)]
fn blend_coverage(target: &mut PixmapMut, mask: Option<&Mask>, left: i64, top: i64, w: usize, h: usize, coverage: &[u8], c: [f32; 4]) {
    let (tw, th) = (target.width() as i64, target.height() as i64);
    let (x0, x1) = (left.max(0), (left + w as i64).min(tw));
    let (y0, y1) = (top.max(0), (top + h as i64).min(th));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    // The color premultiplied by its opacity, out of 255.
    let a = c[3].clamp(0.0, 1.0);
    let channel = |v: f32| (v.clamp(0.0, 1.0) * a * 255.0).round() as u32;
    let (cr, cg, cb, ca) = (channel(c[0]), channel(c[1]), channel(c[2]), (a * 255.0).round() as u32);
    let mask_data = mask.map(|m| m.data());
    let data = target.data_mut();
    let n = (x1 - x0) as usize;
    for y in y0..y1 {
        let from = (y - top) as usize * w + (x0 - left) as usize;
        let cov = &coverage[from..from + n];
        let at = (y * tw + x0) as usize;
        let dst = &mut data[at * 4..(at + n) * 4];
        let m = mask_data.map(|m| &m[at..at + n]);
        for (i, (&k, px)) in cov.iter().zip(dst.chunks_exact_mut(4)).enumerate() {
            if k == 0 {
                continue;
            }
            let k = match m {
                Some(m) => div255(k as u32 * m[i] as u32),
                None => k as u32,
            };
            // Source-over: the source at k/255 of its strength, and what's there kept where it doesn't cover.
            let keep = 255 - div255(ca * k);
            px[0] = (div255(cr * k) + div255(px[0] as u32 * keep)).min(255) as u8;
            px[1] = (div255(cg * k) + div255(px[1] as u32 * keep)).min(255) as u8;
            px[2] = (div255(cb * k) + div255(px[2] as u32 * keep)).min(255) as u8;
            px[3] = (div255(ca * k) + div255(px[3] as u32 * keep)).min(255) as u8;
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::eval::{Evaluator, Frame};
    use crate::pack::{load, tests::temp_pack, Environment};

    pub fn tiny_png() -> Vec<u8> {
        let mut out = Vec::new();
        let img = image::RgbaImage::from_pixel(4, 2, image::Rgba([255, 0, 0, 255]));
        image::DynamicImage::ImageRgba8(img).write_to(&mut Cursor::new(&mut out), ImageFormat::Png).unwrap();
        out
    }

    fn pixel(p: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * p.width() + x) * 4) as usize;
        p.data()[i..i + 4].try_into().unwrap()
    }

    fn render(body: &str, files: &[(&str, &[u8])], time: f64, w: u32, h: u32) -> Pixmap {
        let dir = temp_pack(&format!("raster{}", body.len()), &format!("format = 1\nname = \"T\"\nfade = 0\n{body}"), files);
        let pack = load(&dir, &Environment::default()).unwrap_or_else(|e| panic!("{e}"));
        std::fs::remove_dir_all(&dir).unwrap();
        let mut raster = Raster::new();
        let mut eval = Evaluator::new(&pack, 1, raster.metrics());
        let view = View::new(w, h, pack.stage);
        let frame: Frame = eval.frame(&pack, time, view.aspect());
        let mut pixmap = Pixmap::new(w, h).unwrap();
        raster.draw(&pack, &frame, &mut pixmap.as_mut(), view);
        assert!(raster.warnings.is_empty(), "{:?}", raster.warnings);
        pixmap
    }

    #[test]
    fn images_are_checked_and_shrunk() {
        assert_eq!(probe_image(&tiny_png()).unwrap(), (4, 2));
        assert!(probe_image(b"GIF89a....").is_err());
        assert!(probe_image(b"not an image").is_err());
        let p = decode_image(&tiny_png(), 2, 2).unwrap();
        assert_eq!((p.width(), p.height()), (2, 1));
        // A PNG header claiming an enormous image is refused before decoding.
        let mut huge = tiny_png();
        huge[16..20].copy_from_slice(&100_000u32.to_be_bytes());
        huge[20..24].copy_from_slice(&100_000u32.to_be_bytes());
        assert!(probe_image(&huge).is_err());
    }

    #[test]
    fn blocks_and_braille_fill_cells_without_seams() {
        let p = render(
            "[[scene]]\nduration = 5\nbackground = \"#000000\"\n[[scene.draw]]\nkind = \"ascii\"\nart = \"blocks.txt\"\neffect = \"fade\"\nprogress = 1\nfit = 1\ncolors = [\"#ffffff\"]\n",
            &[("blocks.txt", "████\n▀▀⣿⣿\n".as_bytes())],
            1.0,
            320,
            180,
        );
        // Find the art's full-block row and check it's solid white from end to end.
        let ys: Vec<u32> = (0..180).filter(|&y| pixel(&p, 160, y) == [255, 255, 255, 255]).collect();
        let y = ys[ys.len() / 4];
        let xs: Vec<u32> = (0..320).filter(|&x| pixel(&p, x, y) == [255, 255, 255, 255]).collect();
        assert!(xs.len() > 100, "{}", xs.len());
        assert_eq!(xs.last().unwrap() - xs.first().unwrap() + 1, xs.len() as u32, "no gaps between block cells");
        // A Braille cell's eight dots are separate: a line through them crosses gaps.
        let p = render(
            "[[scene]]\nduration = 5\nbackground = \"#000000\"\n[[scene.draw]]\nkind = \"ascii\"\nart = \"dots.txt\"\neffect = \"fade\"\nprogress = 1\nfit = 1\ncolors = [\"#ffffff\"]\n",
            &[("dots.txt", "⣿".as_bytes())],
            1.0,
            320,
            180,
        );
        let lit: Vec<(u32, u32)> = (0..180).flat_map(|y| (0..320).map(move |x| (x, y))).filter(|&(x, y)| pixel(&p, x, y)[0] > 128).collect();
        let (x0, x1) = (lit.iter().map(|p| p.0).min().unwrap(), lit.iter().map(|p| p.0).max().unwrap());
        let (y0, y1) = (lit.iter().map(|p| p.1).min().unwrap(), lit.iter().map(|p| p.1).max().unwrap());
        let across = (x0..=x1).filter(|&x| pixel(&p, x, y0 + 1)[0] > 128).count() as u32;
        let down = (y0..=y1).filter(|&y| pixel(&p, x0 + 1, y)[0] > 128).count() as u32;
        assert!(across < x1 - x0 + 1, "two dots across with a gap");
        assert!(down < y1 - y0 + 1, "four dots down with gaps");
        assert!(lit.len() > 100);
    }

    #[test]
    fn blit_copies_clips_and_fades() {
        let mut target = Pixmap::new(4, 3).unwrap();
        target.fill(Color::from_rgba8(0, 0, 255, 255));
        let mut src = Pixmap::new(2, 2).unwrap();
        src.fill(Color::from_rgba8(255, 0, 0, 255));
        blit(&mut target.as_mut(), None, &src, true, 3, -1, 1.0);
        assert_eq!(pixel(&target, 3, 0), [255, 0, 0, 255], "copied where it overlaps");
        assert_eq!(pixel(&target, 3, 1), [0, 0, 255, 255], "clipped below");
        assert_eq!(pixel(&target, 2, 0), [0, 0, 255, 255], "clipped left of its origin");
        blit(&mut target.as_mut(), None, &src, true, 0, 1, 0.5);
        let p = pixel(&target, 0, 1);
        assert!((127..=128).contains(&p[0]) && (127..=128).contains(&p[2]) && p[3] == 255, "{p:?}");
        blit(&mut target.as_mut(), None, &src, true, -5, -5, 1.0);
        blit(&mut target.as_mut(), None, &src, true, 0, 0, 0.0);
    }

    #[test]
    fn steady_images_use_the_resampled_copy() {
        let png = tiny_png();
        let body = "[[scene]]\nduration = 5\nbackground = \"#000000\"\n[[scene.draw]]\nkind = \"image\"\nfile = \"red.png\"\nfit = \"contain\"\n";
        let dir = temp_pack("steady", &format!("format = 1\nname = \"T\"\nfade = 0\n{body}"), &[("red.png", &png)]);
        let pack = load(&dir, &Environment::default()).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let mut raster = Raster::new();
        raster.largest = (320, 180);
        let mut eval = Evaluator::new(&pack, 1, raster.metrics());
        let view = View::new(320, 180, pack.stage);
        let mut pixmap = Pixmap::new(320, 180).unwrap();
        for _ in 0..STEADY_FRAMES + 1 {
            let frame = eval.frame(&pack, 1.0, view.aspect());
            raster.draw(&pack, &frame, &mut pixmap.as_mut(), view);
        }
        assert_eq!(raster.scaled.len(), 1);
        assert_eq!(raster.scaled[0].1, (320, 160));
        assert!(raster.scaled[0].3, "known to be opaque");
        assert_eq!(pixel(&pixmap, 160, 90), [255, 0, 0, 255]);
        assert_eq!(pixel(&pixmap, 160, 5), [0, 0, 0, 255], "letterboxed");
    }

    #[test]
    fn glow_spreads_light_around_what_glows() {
        let line = "[[scene.draw]]\nkind = \"lines\"\npoints = [[-0.5, 0], [0.5, 0]]\nwidth = 0.01\ncolor = \"#ffffff\"\n";
        let body = |glow: &str| format!("[[scene]]\nduration = 5\nbackground = \"#000000\"\n{line}{glow}");
        let plain = render(&body(""), &[], 1.0, 320, 180);
        let glowing = render(&body("glow = 1\nglow_radius = 0.02\n"), &[], 1.0, 320, 180);
        // The line itself is at least as bright; just beside it, light; far from it, none.
        assert!(pixel(&glowing, 160, 90)[0] >= pixel(&plain, 160, 90)[0]);
        assert_eq!(pixel(&plain, 160, 94)[0], 0);
        let (near, nearer) = (pixel(&glowing, 160, 97)[0], pixel(&glowing, 160, 94)[0]);
        assert!(nearer > 20 && near > 5 && nearer > near, "glow beside the line, fading: {nearer}, {near}");
        assert_eq!(pixel(&glowing, 160, 170), [0, 0, 0, 255], "none far away");
        assert_eq!(pixel(&glowing, 5, 90), [0, 0, 0, 255], "none past the ends");
    }

    #[test]
    fn pack_fonts_and_stretch() {
        let text = |extra: &str| {
            format!("[fonts]\nmono = \"mono.ttf\"\n[[scene]]\nduration = 5\nbackground = \"#000000\"\n[[scene.draw]]\nkind = \"text\"\ntext = \"MMMMMMMM\"\nfont = \"mono\"\nsize = 0.2\nx = -0.8\ny = -0.1\ncolor = \"#ffffff\"\n{extra}")
        };
        let width = |p: &Pixmap| {
            let lit: Vec<u32> = (0..p.width()).filter(|&x| (0..p.height()).any(|y| pixel(p, x, y)[0] > 128)).collect();
            lit.last().unwrap() - lit.first().unwrap()
        };
        let full = render(&text(""), &[("mono.ttf", FONT)], 1.0, 320, 180);
        let narrow = render(&text("stretch = 0.5\n"), &[("mono.ttf", FONT)], 1.0, 320, 180);
        let (w, n) = (width(&full), width(&narrow));
        assert!((n as f64 / w as f64 - 0.5).abs() < 0.06, "half as wide: {n} of {w}");
    }

    #[test]
    fn mesh_edges_cover_their_width() {
        // A level edge 100 pixels long and 4 wide, with square ends reaching 2 past each: 104 x 4 of coverage.
        let mut pixmap = Pixmap::new(200, 100).unwrap();
        pixmap.fill(Color::BLACK);
        let view = View::new(200, 100, crate::pack::Stage::Screen);
        let points = [((-50.0 / 100.0) as f32, 0.0f32), (0.5, 0.0)];
        let mut cov = Vec::new();
        draw_segments(&mut pixmap.as_mut(), None, &points, view, 4.0, [1.0, 1.0, 1.0, 1.0], &mut cov);
        let total: u32 = pixmap.data().chunks_exact(4).map(|p| p[0] as u32).sum();
        assert!((total as f64 / 255.0 - 104.0 * 4.0).abs() < 4.0, "{}", total as f64 / 255.0);
        // A diagonal one: its coverage still adds up to about its area.
        let mut pixmap = Pixmap::new(200, 200).unwrap();
        pixmap.fill(Color::BLACK);
        let view = View::new(200, 200, crate::pack::Stage::Screen);
        let points = [(-0.3f32, -0.3f32), (0.3, 0.3)];
        draw_segments(&mut pixmap.as_mut(), None, &points, view, 3.0, [1.0, 1.0, 1.0, 1.0], &mut cov);
        let total: u32 = pixmap.data().chunks_exact(4).map(|p| p[0] as u32).sum();
        let area = (120.0f64 * 2f64.sqrt() + 3.0) * 3.0;
        assert!((total as f64 / 255.0 - area).abs() / area < 0.05, "{} of {area}", total as f64 / 255.0);
    }

    /// A 4 x 2 sheet of two 2 x 2 frames: red with a transparent bottom-right pixel, then blue.
    fn sheet_png() -> Vec<u8> {
        let mut img = image::RgbaImage::new(4, 2);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = if x < 2 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) };
            if x == 1 && y == 1 {
                *p = image::Rgba([0, 0, 0, 0]);
            }
        }
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img).write_to(&mut Cursor::new(&mut out), ImageFormat::Png).unwrap();
        out
    }

    #[test]
    fn pixel_art_is_drawn_in_whole_blocks() {
        let v = View::new(2304, 1440, crate::pack::Stage::Pixels(224));
        assert_eq!((v.blocks, v.scale), (6, 1344.0));
        assert!((v.aspect() - 2304.0 / 1344.0).abs() < 1e-9, "the canvas is wider than the stage unit is tall");
        // A 64-pixel stage on a 128 x 128 screen: two screen pixels to a canvas pixel. The red frame's top left at
        // the middle of the screen, each of its pixels a 2 x 2 block.
        let body = "pixels = 64\nbackground = \"#000000\"\n[[scene]]\nduration = 5\n[[scene.draw]]\nkind = \"sprite\"\nfile = \"s.png\"\nframe_size = [2, 2]\n";
        let p = render(body, &[("s.png", &sheet_png())], 1.0, 128, 128);
        for (x, y) in [(64, 64), (65, 65), (66, 64), (67, 65), (64, 66)] {
            assert_eq!(pixel(&p, x, y), [255, 0, 0, 255], "red at {x}, {y}");
        }
        assert_eq!(pixel(&p, 66, 66), [0, 0, 0, 255], "the transparent pixel shows the background");
        assert_eq!(pixel(&p, 63, 63), [0, 0, 0, 255]);
        assert_eq!(pixel(&p, 68, 64), [0, 0, 0, 255], "two canvas pixels wide, no more");
        // The second frame, mirrored, with its anchor (its right edge, once mirrored, is its left) at the middle.
        let body = format!("{body}frame = 1\nanchor = [2, 0]\nflip = true\n");
        let p = render(&body, &[("s.png", &sheet_png())], 1.0, 128, 128);
        assert_eq!(pixel(&p, 64, 64), [0, 0, 255, 255]);
        assert_eq!(pixel(&p, 67, 67), [0, 0, 255, 255]);
        assert_eq!(pixel(&p, 63, 64), [0, 0, 0, 255]);
        // Frames go round: frame 3 is frame 1.
        let wrapped = body.replace("frame = 1", "frame = 3");
        assert_eq!(pixel(&render(&wrapped, &[("s.png", &sheet_png())], 1.0, 128, 128), 64, 64), [0, 0, 255, 255]);
    }

    #[test]
    fn sorting_groups_draw_by_depth() {
        // Two squares on the same spot: listed red then blue, but red is deeper, so it's drawn last, on top.
        let squares = |sort: bool| {
            format!(
                "stage = \"screen\"\nbackground = \"#000000\"\n[[scene]]\nduration = 5\n[[scene.draw]]\nkind = \"group\"\nsort = {sort}\n\
                 [[scene.draw.draw]]\nkind = \"rect\"\nw = 0.5\nh = 0.5\ncolor = \"#ff0000\"\ndepth = 2\n\
                 [[scene.draw.draw]]\nkind = \"rect\"\ncount = 2\nw = 0.5\nh = 0.5\ncolor = \"#0000ff\"\ndepth = \"i\"\n"
            )
        };
        assert_eq!(pixel(&render(&squares(true), &[], 1.0, 100, 100), 50, 50), [255, 0, 0, 255]);
        assert_eq!(pixel(&render(&squares(false), &[], 1.0, 100, 100), 50, 50), [0, 0, 255, 255]);
    }

    #[test]
    fn thin_lines_have_round_ends_and_single_joins() {
        // A closed square outline 100 pixels a side and 4 wide, half opaque: its corners are no brighter than its
        // sides, and its coverage adds up to its area.
        let mut pixmap = Pixmap::new(200, 200).unwrap();
        pixmap.fill(Color::BLACK);
        let square = [(50.0, 50.0), (150.0, 50.0), (150.0, 150.0), (50.0, 150.0)];
        let mut cov = Vec::new();
        draw_round_line(&mut pixmap.as_mut(), None, &square, true, 4.0, [1.0, 1.0, 1.0, 0.5], &mut cov);
        assert_eq!(pixel(&pixmap, 50, 50), pixel(&pixmap, 100, 50), "a corner is laid down once");
        assert!((126..=129).contains(&pixel(&pixmap, 100, 50)[0]));
        let total: u32 = pixmap.data().chunks_exact(4).map(|p| p[0] as u32).sum();
        let area = 400.0 * 4.0 + 4.0 * (4.0 - std::f64::consts::PI);
        assert!((total as f64 / 127.5 - area).abs() / area < 0.03, "{} of {area}", total as f64 / 127.5);
        assert!(cov.iter().all(|&v| v == 0), "the coverage is left clear");
        // An open line ends in a half circle just past its last point.
        let mut pixmap = Pixmap::new(200, 200).unwrap();
        pixmap.fill(Color::BLACK);
        draw_round_line(&mut pixmap.as_mut(), None, &[(50.0, 100.0), (150.0, 100.0)], false, 6.0, [1.0; 4], &mut cov);
        assert_eq!(pixel(&pixmap, 151, 100)[0], 255);
        assert_eq!(pixel(&pixmap, 152, 97)[0], 0, "round, not square, at the end");
        assert_eq!(pixel(&pixmap, 155, 100)[0], 0);
    }

    #[test]
    fn images_coming_into_view_wait_for_their_copy() {
        let png = tiny_png();
        let body = "[[scene]]\nduration = 5\nbackground = \"#000000\"\n[[scene.draw]]\nkind = \"image\"\nfile = \"red.png\"\nfit = \"contain\"\n";
        let dir = temp_pack("waiting", &format!("format = 1\nname = \"T\"\nfade = 0\n{body}"), &[("red.png", &png)]);
        let pack = load(&dir, &Environment::default()).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let mut raster = Raster::new();
        raster.background_decoding = true;
        raster.largest = (320, 180);
        let mut eval = Evaluator::new(&pack, 1, raster.metrics());
        let view = View::new(320, 180, pack.stage);
        let mut pixmap = Pixmap::new(320, 180).unwrap();
        let frame = eval.frame(&pack, 1.0, view.aspect());
        raster.draw(&pack, &frame, &mut pixmap.as_mut(), view);
        assert_eq!(pixel(&pixmap, 160, 90), [0, 0, 0, 255], "not drawn before it's ready");
        // Once decoded and resampled on other threads, it fades in, and is never drawn the slow way.
        let start = Instant::now();
        while raster.scaled.is_empty() && start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(5));
            raster.poll_images();
            raster.draw(&pack, &frame, &mut pixmap.as_mut(), view);
        }
        assert_eq!(raster.scaled.len(), 1);
        std::thread::sleep(Duration::from_millis((ARRIVAL_FADE * 1000.0) as u64 + 50));
        raster.draw(&pack, &frame, &mut pixmap.as_mut(), view);
        assert_eq!(pixel(&pixmap, 160, 90), [255, 0, 0, 255]);
    }

    #[test]
    fn fading_copies_blend_evenly() {
        let mut src = Pixmap::new(4, 1).unwrap();
        src.fill(Color::from_rgba8(200, 100, 50, 255));
        let mut dst = Pixmap::new(4, 1).unwrap();
        dst.fill(Color::from_rgba8(0, 0, 100, 255));
        blit(&mut dst.as_mut(), None, &src, true, 0, 0, 0.5);
        assert_eq!(pixel(&dst, 2, 0), [100, 50, 75, 255]);
    }

    #[test]
    fn view_fits_stage() {
        let v = View::new(2304, 1440, Stage::Aspect(16.0 / 9.0));
        assert_eq!(v.scale, 1296.0);
        assert_eq!(View::new(1000, 500, Stage::Screen).scale, 500.0);
        assert_eq!(v.px(0.0, 0.0), (1152.0, 720.0));
    }

    #[test]
    fn smooth_sprites_blend_at_their_edges() {
        let mut sheet = Pixmap::new(4, 4).unwrap();
        sheet.fill(tiny_skia::Color::WHITE);
        let mut target = Pixmap::new(32, 32).unwrap();
        // Four times its size, half a pixel off the grid.
        let drawn = draw_sprite_smooth(&mut target.as_mut(), None, &sheet, [0, 0, 4, 4], (8.5, 8.5), 4.0, false, 1.0);
        assert_eq!((drawn.x0, drawn.y0, drawn.x1, drawn.y1), (8, 8, 25, 25));
        let alpha = |x: u32, y: u32| target.pixel(x, y).unwrap().alpha();
        assert_eq!(alpha(16, 16), 255, "solid in the middle");
        assert!(alpha(8, 16) > 0 && alpha(8, 16) < 255, "partly covered at its edge");
        assert_eq!(alpha(4, 16), 0, "nothing outside");
    }

    #[test]
    fn clips_covering_the_view_make_no_mask() {
        let square = [(-10.0, -10.0), (110.0, -10.0), (110.0, 60.0), (-10.0, 60.0)];
        assert!(covers(&square, 100.0, 50.0));
        let reversed: Vec<(f32, f32)> = square.iter().rev().copied().collect();
        assert!(covers(&reversed, 100.0, 50.0));
        assert!(!covers(&[(0.0, 0.0), (50.0, 0.0), (50.0, 50.0), (0.0, 50.0)], 100.0, 50.0));
        // A turned square, its corners beyond the view's on every side.
        let turned = [(50.0, -80.0), (180.0, 25.0), (50.0, 130.0), (-80.0, 25.0)];
        assert!(covers(&turned, 100.0, 50.0));
        // Turned, but cutting across the view.
        let cutting = [(50.0, -20.0), (120.0, 25.0), (50.0, 70.0), (-20.0, 25.0)];
        assert!(!covers(&cutting, 100.0, 50.0));
    }

    #[test]
    fn badges_replace_a_sheet_and_take_the_theme_colors() {
        use crate::palette::Palette;
        // The pack's own sheet: 8 x 8, solid green. Someone's badge: 4 x 2, solid red, to be letterboxed into it.
        let png = |w: u32, h: u32, c: [u8; 4]| {
            let mut out = Vec::new();
            image::RgbaImage::from_pixel(w, h, image::Rgba(c)).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
            out
        };
        let body = "format = 1\nname = \"T\"\nfade = 0\n[[scene]]\nduration = 5\n[[scene.draw]]\nkind = \"sprite\"\nfile = \"own.png\"\nsource = \"badge\"\n";
        let own = png(8, 8, [0, 255, 0, 255]);
        let dir = temp_pack("badge", body, &[("own.png", &own)]);
        let theirs = std::env::temp_dir().join(format!("screensafe-badge-{}", std::process::id()));
        let sheet = |pack: &Pack| match pack.images.last().unwrap() {
            ImageSource::Pixels { width, height, rgba, .. } => (*width, *height, rgba.clone()),
            _ => panic!("not prepared"),
        };
        let px = |rgba: &[u8], x: usize, y: usize| rgba[(y * 8 + x) * 4..(y * 8 + x) * 4 + 4].to_vec();
        let load_with = |badge: Option<&std::path::Path>, palette: Option<&Palette>| {
            load(&dir, &Environment { badge, palette, ..Default::default() }).unwrap_or_else(|e| panic!("{e}"))
        };

        // No badge: the pack's own sheet.
        let pack = load_with(Some(&theirs), None);
        assert_eq!(px(&sheet(&pack).2, 4, 4), vec![0, 255, 0, 255]);
        let without = pack.fingerprint;

        // A badge: fitted to the sheet's size, keeping its shape, on transparency.
        std::fs::write(theirs.with_extension("png"), png(4, 2, [255, 0, 0, 255])).unwrap();
        let pack = load_with(Some(&theirs), None);
        let (w, h, rgba) = sheet(&pack);
        assert_eq!((w, h), (8, 8));
        assert_eq!(px(&rgba, 4, 4), vec![255, 0, 0, 255]);
        assert_eq!(px(&rgba, 4, 0)[3], 0, "letterboxed above");
        assert_ne!(pack.fingerprint, without, "a new badge changes the fingerprint, so thumbnails are drawn again");

        // A badge that isn't an image: noted, and the pack's own sheet shown.
        std::fs::write(theirs.with_extension("png"), b"not an image").unwrap();
        let pack = load_with(Some(&theirs), None);
        assert_eq!(px(&sheet(&pack).2, 4, 4), vec![0, 255, 0, 255]);
        assert!(pack.warnings.iter().any(|w| w.starts_with("badge:")), "{:?}", pack.warnings);
        std::fs::remove_file(theirs.with_extension("png")).unwrap();

        // The theme's colors: every shown pixel becomes one of them.
        let theme = Palette::from_toml("green = \"#10c020\"\n").unwrap();
        let themed = body.replace("source = \"badge\"", "palette = \"theme\"");
        std::fs::write(dir.join("screensafe.toml"), themed).unwrap();
        let pack = load_with(None, Some(&theme));
        let inks: Vec<Vec<u8>> =
            theme.colors.iter().map(|(_, c)| vec![(c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8, 255]).collect();
        for chunk in sheet(&pack).2.chunks_exact(4) {
            assert!(inks.contains(&chunk.to_vec()), "{chunk:?} isn't a theme color");
        }

        // Only these sources and palettes.
        std::fs::write(dir.join("screensafe.toml"), body.replace("\"badge\"", "\"photos\"")).unwrap();
        assert!(load(&dir, &Environment::default()).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn packs_can_tell_light_themes_from_dark() {
        use crate::palette::Palette;
        let body = "format = 1\nname = \"T\"\nfade = 0\n[[scene]]\nduration = 5\nbackground = \"light ? #ffffff : #000000\"\n\
                    [[scene.draw]]\nkind = \"rect\"\nw = 0.5\nh = 0.5\ncolor = \"light ? #ff0000 : #0000ff\"\n";
        let dir = temp_pack("lightdark", body, &[]);
        let draw = |theme: &str| {
            let palette = Palette::from_toml(theme).unwrap();
            let pack = load(&dir, &Environment { palette: Some(&palette), ..Default::default() }).unwrap_or_else(|e| panic!("{e}"));
            let mut raster = Raster::new();
            let mut eval = Evaluator::new(&pack, 1, raster.metrics());
            let view = View::new(160, 90, pack.stage);
            let frame = eval.frame(&pack, 1.0, view.aspect());
            let mut p = Pixmap::new(160, 90).unwrap();
            raster.draw(&pack, &frame, &mut p.as_mut(), view);
            (pixel(&p, 80, 45), pixel(&p, 2, 2))
        };
        assert_eq!(draw("mode = \"light\"\n"), ([255, 0, 0, 255], [255, 255, 255, 255]));
        assert_eq!(draw("mode = \"dark\"\n"), ([0, 0, 255, 255], [0, 0, 0, 255]));
        // Without a mode, the background decides.
        assert_eq!(draw("background = \"#f2f0e5\"\n").0, [255, 0, 0, 255]);
        // `dark` is its opposite, for whichever reads better.
        let flipped = body.replace("light ?", "dark ?");
        std::fs::write(dir.join("screensafe.toml"), flipped).unwrap();
        assert_eq!(draw("mode = \"light\"\n"), ([0, 0, 255, 255], [0, 0, 0, 255]));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unchanged_glows_are_laid_down_again_exactly() {
        let body = "[[scene]]\nduration = 10\n[[scene.draw]]\nkind = \"group\"\nglow = 2\nglow_radius = 0.02\n\
                    [[scene.draw.draw]]\nkind = \"lines\"\nsamples = 64\npx = \"0.3 * sin(s * tau)\"\npy = \"0.3 * cos(s * tau)\"\n\
                    width = 0.01\ncolor = \"#40ff80\"\ndraw = \"min(1, t / 4)\"\n\
                    [[scene.draw]]\nkind = \"rect\"\nx = \"-0.5 + t * 0.01\"\nw = 0.1\nh = 0.1\ncolor = \"#ff0000\"\n";
        let dir = temp_pack("glowcache", &format!("format = 1\nname = \"T\"\nfade = 0\n{body}"), &[]);
        let pack = load(&dir, &Environment::default()).unwrap_or_else(|e| panic!("{e}"));
        std::fs::remove_dir_all(&dir).unwrap();
        let (w, h) = (320, 180);
        let view = View::new(w, h, pack.stage);
        let draw = |raster: &mut Raster, t: f64| {
            let mut eval = Evaluator::new(&pack, 1, raster.metrics());
            let frame = eval.frame(&pack, t, view.aspect());
            let mut p = Pixmap::new(w, h).unwrap();
            raster.draw(&pack, &frame, &mut p.as_mut(), view);
            p
        };
        // The circle is whole from 4 seconds; the square beside it, outside the glow, keeps moving.
        let mut raster = Raster::new();
        raster.profile = Some(HashMap::new());
        let first = draw(&mut raster, 5.0);
        assert!(!raster.profile.as_ref().unwrap().contains_key("glow (kept)"));
        let again = draw(&mut raster, 6.0);
        assert!(raster.profile.as_ref().unwrap().contains_key("glow (kept)"), "an unchanged glow is kept");
        // Exactly what drawing it afresh gives.
        let fresh = draw(&mut Raster::new(), 6.0);
        assert!(again.data() == fresh.data(), "a kept glow draws the same pixels as a fresh one");
        assert!(first.data() != again.data(), "the square outside it still moves");
        // A changed glow is drawn afresh.
        let mut changed = Raster::new();
        draw(&mut changed, 1.0);
        changed.profile = Some(HashMap::new());
        let later = draw(&mut changed, 2.0);
        assert!(!changed.profile.as_ref().unwrap().contains_key("glow (kept)"));
        assert!(later.data() == draw(&mut Raster::new(), 2.0).data());
    }

    #[test]
    fn draws_background_rects_and_clips() {
        let p = render(
            "[[scene]]\nduration = 5\nbackground = \"#0000ff\"\n[[scene.draw]]\nkind = \"group\"\nclip = [\"-1\", \"-0.5\", \"0\", \"0.5\"]\n[[scene.draw.draw]]\nkind = \"rect\"\nw = 1\nh = 0.5\ncolor = \"#ff0000\"\n",
            &[],
            1.0,
            160,
            90,
        );
        assert_eq!(pixel(&p, 2, 2), [0, 0, 255, 255]);
        assert_eq!(pixel(&p, 70, 45), [255, 0, 0, 255]);
        assert_eq!(pixel(&p, 90, 45), [0, 0, 255, 255], "clipped away right of center");
    }

    #[test]
    fn draws_text_lines_and_images() {
        let png = tiny_png();
        let p = render(
            "[[scene]]\nduration = 5\nbackground = \"#000000\"\n[[scene.draw]]\nkind = \"image\"\nfile = \"red.png\"\nfit = \"contain\"\nopacity = 0.5\n[[scene.draw]]\nkind = \"text\"\ntext = \"MMMM\"\nsize = 0.3\nx = -0.8\ny = -0.45\ncolor = \"#ffffff\"\n[[scene.draw]]\nkind = \"lines\"\npoints = [[-0.8, 0.4], [0.8, 0.4]]\nwidth = 0.02\ncolor = \"#00ff00\"\n",
            &[("red.png", &png)],
            1.0,
            320,
            180,
        );
        assert!((127..=128).contains(&pixel(&p, 160, 90)[0]), "half-transparent red image in the middle");
        let white = (0..60).flat_map(|y| (0..160).map(move |x| (x, y))).filter(|&(x, y)| pixel(&p, x, y)[1] > 200 && pixel(&p, x, y)[0] > 200).count();
        assert!(white > 100, "text drawn ({white} white pixels)");
        assert_eq!(pixel(&p, 160, 162), [0, 255, 0, 255], "line drawn");
    }
}

#[cfg(test)]
mod fuzz {
    use super::*;
    use crate::eval::Evaluator;
    use crate::pack::{load, tests::temp_pack, Environment};

    fn next(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    /// Mutated versions of real packs either fail to load with an error or load and draw without panicking.
    #[test]
    fn mutated_packs_never_panic() {
        let sources = [include_str!("../../packs/orbits/screensafe.toml"), include_str!("../../packs/omarchy/screensafe.toml")];
        let tokens = [
            "",
            "0",
            "-1",
            "1e9",
            "1e-9",
            "999999",
            "\"",
            "[",
            "]",
            "{",
            "}",
            "=",
            "(",
            ")",
            "t",
            "/ 0",
            "^ 99",
            "nan",
            "\n",
            "count = 1024",
            "samples = 4096",
        ];
        let mut state = 0x5eed_u64;
        let mut loaded = 0;
        for round in 0..1500 {
            let base = sources[round % sources.len()];
            let mut text = base.as_bytes().to_vec();
            for _ in 0..1 + next(&mut state) % 3 {
                let at = (next(&mut state) as usize) % text.len().max(1);
                match next(&mut state) % 3 {
                    0 => {
                        let token = tokens[(next(&mut state) as usize) % tokens.len()];
                        text.splice(at..at, token.bytes());
                    }
                    1 => {
                        let end = (at + 1 + (next(&mut state) as usize) % 12).min(text.len());
                        text.drain(at..end);
                    }
                    _ => text[at] = b"0123456789.-+*/()[]{}=\"tuisx#\n "[(next(&mut state) as usize) % 30],
                }
            }
            let Ok(manifest) = String::from_utf8(text) else { continue };
            let dir = temp_pack("fuzz", &manifest, &[]);
            let result = load(&dir, &Environment::default());
            std::fs::remove_dir_all(&dir).unwrap();
            let Ok(pack) = result else { continue };
            loaded += 1;
            let mut raster = Raster::new();
            raster.largest = (96, 54);
            let mut eval = Evaluator::new(&pack, round as u64, raster.metrics());
            let view = View::new(96, 54, pack.stage);
            let mut pixmap = Pixmap::new(96, 54).unwrap();
            for time in [0.0, 0.5, 3.0, 17.0, 31.0, 1e6] {
                let frame = eval.frame(&pack, time, view.aspect());
                raster.draw(&pack, &frame, &mut pixmap.as_mut(), view);
            }
        }
        assert!(loaded > 100, "enough mutants load to exercise drawing ({loaded})");
    }
}

#[cfg(test)]
mod bench {
    use super::*;
    use crate::eval::Evaluator;
    use crate::pack::{load, Environment};
    use std::time::{Duration, Instant};

    /// SCREENSAFE_BENCH_PACK=dir SCREENSAFE_BENCH_THEME=dir cargo test --release bench_frames -- --ignored --nocapture
    #[test]
    #[ignore]
    fn bench_frames() {
        let pack_dir = std::env::var("SCREENSAFE_BENCH_PACK").unwrap();
        let theme = std::env::var("SCREENSAFE_BENCH_THEME").ok().map(std::path::PathBuf::from);
        let env = Environment { theme: theme.as_deref(), ..Environment::default() };
        let pack = load(std::path::Path::new(&pack_dir), &env).unwrap_or_else(|e| panic!("{}", e.0));
        let (w, h) = (2304, 1440);
        let mut raster = Raster::new();
        raster.background_decoding = true;
        raster.largest = (w, h);
        let mut eval = Evaluator::new(&pack, 1, raster.metrics());
        let view = View::new(w, h, pack.stage);
        let mut pixmap = Pixmap::new(w, h).unwrap();
        let start = Instant::now();
        let mut looked = 0.0;
        let until: f64 = std::env::var("SCREENSAFE_BENCH_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(4.0);
        let mut frame_no = 0;
        loop {
            let t = start.elapsed().as_secs_f64();
            if t > until {
                break;
            }
            if t >= looked {
                let f = eval.frame(&pack, t + 2.5, view.aspect());
                raster.prefetch(&pack, &f, &[view]);
                looked = t + 1.0;
            }
            raster.poll_images();
            let frame = eval.frame(&pack, t, view.aspect());
            let a = Instant::now();
            raster.draw(&pack, &frame, &mut pixmap.as_mut(), view);
            let b = Instant::now();
            let _ = crate::flash::measure(pixmap.data(), w as usize, h as usize, w as usize * 4, false);
            let c = Instant::now();
            println!("{frame_no:4} t={t:6.3} draw={:6.1}ms measure={:5.1}ms", (b - a).as_secs_f64() * 1e3, (c - b).as_secs_f64() * 1e3);
            frame_no += 1;
            let next = Duration::from_secs_f64((frame_no as f64) / 24.0);
            if let Some(d) = next.checked_sub(start.elapsed()) {
                std::thread::sleep(d);
            }
        }
    }

    /// SCREENSAFE_BENCH_PACK=dir cargo test --release bench_scenes -- --ignored --nocapture
    #[test]
    #[ignore]
    fn bench_window() {
        // Average evaluation and drawing time, in half-second steps, over a window of a pack's timeline:
        // SCREENSAFE_BENCH_PACK, SCREENSAFE_BENCH_FROM and SCREENSAFE_BENCH_TO (seconds), at 30 frames a second.
        let pack_dir = std::env::var("SCREENSAFE_BENCH_PACK").unwrap();
        let from: f64 = std::env::var("SCREENSAFE_BENCH_FROM").ok().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let to: f64 = std::env::var("SCREENSAFE_BENCH_TO").ok().and_then(|s| s.parse().ok()).unwrap_or(from + 5.0);
        let pack = load(std::path::Path::new(&pack_dir), &Environment::default()).unwrap_or_else(|e| panic!("{}", e.0));
        let (w, h) = (2304, 1440);
        let mut raster = Raster::new();
        raster.largest = (w, h);
        raster.load_fonts(&pack);
        let mut eval = Evaluator::new(&pack, 1, raster.metrics());
        let view = View::new(w, h, pack.stage);
        let mut pixmap = Pixmap::new(w, h).unwrap();
        let profiling = std::env::var("SCREENSAFE_BENCH_PROFILE").is_ok();
        // Warm the caches on the first frame.
        let first = eval.frame(&pack, from, view.aspect());
        raster.draw(&pack, &first, &mut pixmap.as_mut(), view);
        let mut t = from;
        while t < to {
            let (mut e, mut d, mut most, mut frames) = (0.0, 0.0, 0.0f64, 0);
            if profiling {
                raster.profile = Some(HashMap::new());
            }
            let end = t + 0.5;
            while t < end {
                let a = Instant::now();
                let frame = eval.frame(&pack, t, view.aspect());
                let b = Instant::now();
                raster.draw(&pack, &frame, &mut pixmap.as_mut(), view);
                let c = Instant::now();
                e += (b - a).as_secs_f64() * 1e3;
                d += (c - b).as_secs_f64() * 1e3;
                most = most.max((c - b).as_secs_f64() * 1e3);
                frames += 1;
                t += 1.0 / 30.0;
            }
            println!("{:6.1}s  eval {:5.2} ms  draw {:6.2} ms  slowest {:6.2} ms", end - 0.5, e / frames as f64, d / frames as f64, most);
            if let Some(p) = raster.profile.take() {
                let mut parts: Vec<_> = p.into_iter().collect();
                parts.sort_by(|a, b| b.1.total_cmp(&a.1));
                let line: Vec<String> = parts.iter().map(|(k, v)| format!("{k} {:.1}", v / frames as f64)).collect();
                println!("         {}", line.join("  "));
            }
        }
    }

    #[test]
    #[ignore]
    fn bench_scenes() {
        let pack_dir = std::env::var("SCREENSAFE_BENCH_PACK").unwrap();
        let theme = std::env::var("SCREENSAFE_BENCH_THEME").ok().map(std::path::PathBuf::from);
        let env = Environment { theme: theme.as_deref(), ..Environment::default() };
        let pack = load(std::path::Path::new(&pack_dir), &env).unwrap_or_else(|e| panic!("{}", e.0));
        let (w, h) = (2304, 1440);
        let mut raster = Raster::new();
        raster.largest = (w, h);
        raster.load_fonts(&pack);
        let mut eval = Evaluator::new(&pack, 1, raster.metrics());
        let view = View::new(w, h, pack.stage);
        let mut pixmap = Pixmap::new(w, h).unwrap();
        let mut t = 0.0;
        let mut stats: Vec<(f64, f64, f64, u32)> = vec![(0.0, 0.0, 0.0, 0); pack.scenes.len()];
        while t < pack.length() {
            let frame = eval.frame(&pack, t, view.aspect());
            let a = Instant::now();
            let frame2 = eval.frame(&pack, t, view.aspect());
            let b = Instant::now();
            raster.draw(&pack, &frame, &mut pixmap.as_mut(), view);
            let c = Instant::now();
            let st = &mut stats[frame2.scene];
            st.0 += (b - a).as_secs_f64() * 1e3;
            st.1 += (c - b).as_secs_f64() * 1e3;
            st.2 = st.2.max((c - b).as_secs_f64() * 1e3);
            st.3 += 1;
            t += 1.0 / 12.0;
        }
        for (i, s) in stats.iter().enumerate() {
            println!("{:28} eval {:6.2} ms  draw {:6.2} ms  slowest {:6.1} ms", pack.scenes[i].name, s.0 / s.3 as f64, s.1 / s.3 as f64, s.2);
        }
    }
}
