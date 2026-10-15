//! Evaluating a pack at a moment in time into a display list, in stage units.
//!
//! The stage is one unit tall with its origin at the center; x grows to the right and y downwards. Everything a
//! frame shows is a function of the time alone, so any frame can be drawn without drawing the ones before it.

use crate::expr::{hash, Machine, Program};
use crate::pack::{self, Align, Effect, Element, Fit, Kind, Pack, Points, Segment, VAlign};

pub type Rgba = [f32; 4];

/// A 2-D affine transform: (x, y) -> (a x + c y + e, b x + d y + f).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine {
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }

    /// self, then translate by (x, y), rotate by `r` radians and scale by `s`, in local coordinates.
    pub fn then(&self, x: f64, y: f64, r: f64, s: f64) -> Affine {
        let (sin, cos) = r.sin_cos();
        let (la, lb, lc, ld) = (cos * s, sin * s, -sin * s, cos * s);
        Affine {
            a: self.a * la + self.c * lb,
            b: self.b * la + self.d * lb,
            c: self.a * lc + self.c * ld,
            d: self.b * lc + self.d * ld,
            e: self.a * x + self.c * y + self.e,
            f: self.b * x + self.d * y + self.f,
        }
    }

    /// How much the transform scales lengths.
    pub fn scale(&self) -> f64 {
        (self.a * self.d - self.b * self.c).abs().sqrt()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ImageSrc {
    Pack(usize),
    Background(usize),
    Photo(usize),
}

/// An image as it's shown: where its pixels come from, and how they're prepared as it's decoded. Each distinct one
/// is decoded and kept on its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageRef {
    pub src: ImageSrc,
    pub filter: pack::Filter,
    /// The width over the height, in ten-thousandths, of the box the image is cut to fill; 0 for none.
    pub crop: u32,
    /// The longest side, in pixels, it's decoded to; 0 for a little more than the screen.
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    pub ch: char,
    pub x: f32,
    /// The baseline.
    pub y: f32,
    pub color: Rgba,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// Points in stage units; `breaks` mark where a new subpath starts.
    Path {
        points: Vec<(f32, f32)>,
        breaks: Vec<u32>,
        closed: bool,
        stroke: Option<(f32, Rgba)>,
        fill: Option<Rgba>,
    },
    /// Characters at one font size; block and Braille characters fill cells `cell_h` tall. `font` is 0 for the
    /// built-in font, or one more than an index into the pack's fonts; `stretch` widens or narrows each character.
    Glyphs {
        size: f32,
        cell_h: f32,
        font: u8,
        stretch: f32,
        glyphs: Vec<Glyph>,
    },
    Image {
        source: ImageRef,
        fit: Fit,
        /// The box it's fitted to, `w` by `h` stage units centered on its origin; or none, for the whole screen.
        frame: Option<(f32, f32)>,
        zoom: f32,
        pan_x: f32,
        pan_y: f32,
        alpha: f32,
        transform: Affine,
    },
    /// Clip what follows, until the matching `Unclip`, to a quadrilateral.
    Clip([(f32, f32); 4]),
    Unclip,
    /// A frame of a sprite sheet: the rectangle `src` of pack image `image` (x, y, width, height in its pixels),
    /// its top left at (`x`, `y`), each of its pixels `pixel` stage units square, mirrored left to right if `flip`.
    Sprite {
        image: usize,
        src: [u32; 4],
        x: f32,
        y: f32,
        pixel: f32,
        flip: bool,
        alpha: f32,
        /// Sampled smoothly rather than in crisp blocks.
        smooth: bool,
    },
    /// Give what's drawn until the matching `EndGlow` a glow `radius` stage units wide, at `strength`.
    Glow {
        radius: f32,
        strength: f32,
    },
    EndGlow,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub background: Rgba,
    pub items: Vec<Item>,
    pub scene: usize,
    pub fps: f64,
}

/// Font proportions, as fractions of the font size.
#[derive(Clone, Copy, Debug)]
pub struct FontMetrics {
    pub advance: f64,
    pub ascent: f64,
    /// The distance between lines, as a terminal sets them.
    pub line: f64,
}

pub struct Evaluator {
    slots: Vec<f64>,
    /// Where the clock's values come from: fixed unless set otherwise, and its reading for the frame being drawn.
    clock: crate::clock::Clock,
    reading: crate::clock::Reading,
    machine: Machine,
    /// The built-in font's, then each of the pack's fonts'.
    metrics: Vec<FontMetrics>,
    text: String,
}

const DECRYPT_CHARS: &[char] = &['#', '%', '&', '*', '+', '=', '?', '@', '$', '0', '1', '7', 'X', 'Z', '/', '\\', '<', '>'];

fn rgba(c: [f64; 4], alpha: f64) -> Rgba {
    [c[0] as f32, c[1] as f32, c[2] as f32, (c[3] * alpha).clamp(0.0, 1.0) as f32]
}

fn mix(a: [f64; 4], b: [f64; 4], t: f64) -> [f64; 4] {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]
}

/// Keeps drawn coordinates within a range the rasteriser handles exactly.
fn bound(v: f64) -> f32 {
    v.clamp(-1e4, 1e4) as f32
}

impl Evaluator {
    pub fn new(pack: &Pack, seed: u64, metrics: Vec<FontMetrics>) -> Evaluator {
        let clock = crate::clock::Clock::fixed();
        let reading = clock.at(0.0);
        Evaluator { slots: pack.initial.clone(), clock, reading, machine: Machine::new(seed), metrics, text: String::new() }
    }

    /// Shows the clock from `clock` rather than the fixed moment.
    pub fn set_clock(&mut self, clock: crate::clock::Clock) {
        self.clock = clock;
    }

    fn num(&mut self, p: &Program) -> f64 {
        self.machine.num(p, &self.slots)
    }

    fn set(&mut self, slot: u16, v: f64) {
        self.slots[slot as usize] = v;
    }

    fn run_lets(&mut self, lets: &[pack::Let]) {
        for l in lets {
            let base = l.slot as usize;
            if l.program.ty == crate::expr::Ty::Color {
                let c = self.machine.color(&l.program, &self.slots);
                self.slots[base..base + 4].copy_from_slice(&c);
            } else {
                let v = self.machine.num(&l.program, &self.slots);
                self.slots[base] = v;
            }
        }
    }

    fn enter_scene(&mut self, pack: &Pack, scene: usize, t: f64) {
        let s = &pack.scenes[scene];
        let b = pack.builtins;
        self.set(b.t, t);
        self.set(b.u, (t / s.duration).clamp(0.0, 1.0));
        self.set(b.dur, s.duration);
        self.run_lets(&s.lets);
    }

    /// The scene showing at `time` seconds since the screensaver started, and the time within it.
    pub fn locate(pack: &Pack, time: f64) -> (usize, f64, f64) {
        let length = pack.length().max(1e-6);
        let cycle = (time / length).floor().max(0.0);
        let mut t = (time - cycle * length).max(0.0);
        for (i, scene) in pack.scenes.iter().enumerate() {
            if t < scene.length() || i == pack.scenes.len() - 1 {
                return (i, t.min(scene.length()), cycle);
            }
            t -= scene.length();
        }
        (0, 0.0, cycle)
    }

    pub fn frame(&mut self, pack: &Pack, time: f64, screen_aspect: f64) -> Frame {
        let b = pack.builtins;
        let aspect = match pack.stage {
            pack::Stage::Aspect(a) => a,
            pack::Stage::Screen | pack::Stage::Pixels(_) => screen_aspect,
        };
        let (index, t, cycle) = Evaluator::locate(pack, time);
        self.set(b.time, time);
        self.set(b.cycle, cycle);
        self.set(b.aspect, aspect);
        self.set(b.screen_aspect, screen_aspect);
        self.reading = self.clock.at(time);
        let r = &self.reading;
        for (slot, v) in [
            (b.hour, r.hour as f64),
            (b.minute, r.minute as f64),
            (b.second, r.second as f64),
            (b.clock, r.clock),
            (b.day, r.day as f64),
            (b.month, r.month as f64),
            (b.year, r.year as f64),
            (b.weekday, r.weekday as f64),
            (b.utc_offset, r.offset as f64 / 3600.0),
            (b.utc_clock, r.utc_clock),
        ] {
            self.slots[slot as usize] = v;
        }
        let scene = &pack.scenes[index];

        // Towards the end of a scene its background turns into the next one's, so the change is never sudden.
        let mut next_background = None;
        let remaining = scene.length() - t;
        if scene.fade > 0.0 && remaining < scene.fade {
            let next = (index + 1) % pack.scenes.len();
            self.enter_scene(pack, next, 0.0);
            next_background = Some((self.machine.color(&pack.scenes[next].background, &self.slots), 1.0 - remaining / scene.fade));
        }
        self.enter_scene(pack, index, t);
        let mut background = self.machine.color(&scene.background, &self.slots);
        if let Some((next, amount)) = next_background {
            background = mix(background, next, amount);
        }
        let fade = if scene.fade > 0.0 { (t / scene.fade).min(remaining / scene.fade).clamp(0.0, 1.0) } else { 1.0 };

        let mut items = Vec::new();
        if fade > 0.0 {
            for element in &scene.draw {
                self.set_index(pack, 0, 1);
                self.element(pack, element, Affine::IDENTITY, fade, &mut items, None);
            }
        }
        Frame { background: rgba(background, 1.0), items, scene: index, fps: scene.fps }
    }

    fn set_index(&mut self, pack: &Pack, i: u32, count: u32) {
        let b = pack.builtins;
        self.set(b.i, i as f64);
        self.set(b.n, count as f64);
        self.set(b.k, if count > 1 { i as f64 / (count as f64 - 1.0) } else { 0.0 });
    }

    /// Draws each copy of `el` into `out`; or, for a group that sorts, into `sorted`, each copy's items apart with
    /// its depth and its place in the order, to be sorted.
    fn element(
        &mut self,
        pack: &Pack,
        el: &Element,
        parent: Affine,
        parent_alpha: f64,
        out: &mut Vec<Item>,
        mut sorted: Option<&mut Vec<(f64, usize, Vec<Item>)>>,
    ) {
        let mut chunk = Vec::new();
        for i in 0..el.count {
            if el.repeats {
                self.set_index(pack, i, el.count);
            }
            self.run_lets(&el.lets);
            if let Some(visible) = &el.visible {
                if !self.machine.truth(visible, &self.slots) {
                    continue;
                }
            }
            let alpha = self.num(&el.opacity).clamp(0.0, 1.0) * parent_alpha;
            if alpha <= 0.0 {
                continue;
            }
            let depth = el.depth.as_ref().map_or(0.0, |d| self.num(d));
            let out: &mut Vec<Item> = if sorted.is_some() { &mut chunk } else { &mut *out };
            let x = self.num(&el.x);
            let y = self.num(&el.y);
            let rotate = self.num(&el.rotate);
            let scale = self.num(&el.scale).clamp(-100.0, 100.0);
            let transform = parent.then(x, y, rotate, scale);
            let color = self.machine.color(&el.color, &self.slots);
            let glowing = match &el.glow {
                Some(glow) => {
                    let strength = self.num(&glow.strength).clamp(0.0, 4.0);
                    let radius = glow.radius * transform.scale();
                    let on = strength > 0.0 && radius > 0.0;
                    if on {
                        out.push(Item::Glow { radius: bound(radius.min(0.05)), strength: strength as f32 });
                    }
                    on
                }
                None => false,
            };
            match &el.kind {
                Kind::Group { children, clip, sort } => {
                    let clipped = if let Some([l, t, r, bottom]) = clip {
                        let (l, t, r, bottom) = (self.num(l), self.num(t), self.num(r), self.num(bottom));
                        let corner = |x, y| {
                            let (px, py) = transform.apply(x, y);
                            (bound(px), bound(py))
                        };
                        out.push(Item::Clip([corner(l, t), corner(r, t), corner(r, bottom), corner(l, bottom)]));
                        true
                    } else {
                        false
                    };
                    let mut copies = Vec::new();
                    for child in children {
                        self.element(pack, child, transform, alpha, out, sort.then_some(&mut copies));
                        // A child with its own count numbers its copies; give the group's numbers back to the next.
                        if el.repeats && child.repeats {
                            self.set_index(pack, i, el.count);
                        }
                    }
                    // Least depth first, and in order where depths are equal.
                    copies.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                    for (_, _, items) in copies {
                        out.extend(items);
                    }
                    if clipped {
                        out.push(Item::Unclip);
                    }
                }
                Kind::Rect { w, h } => {
                    let (w, h) = (self.num(w) / 2.0, self.num(h) / 2.0);
                    let points = [(-w, -h), (w, -h), (w, h), (-w, h)]
                        .iter()
                        .map(|&(px, py)| {
                            let (sx, sy) = transform.apply(px, py);
                            (bound(sx), bound(sy))
                        })
                        .collect();
                    out.push(Item::Path { points, breaks: Vec::new(), closed: true, stroke: None, fill: Some(rgba(color, alpha)) });
                }
                Kind::Lines(lines) => self.lines(pack, lines, transform, color, alpha, out),
                Kind::Mesh(mesh) => self.mesh(pack, mesh, transform, color, alpha, out),
                Kind::Text(text) => self.text(text, transform, color, alpha, out),
                Kind::Ascii(ascii) => self.ascii(pack, ascii, transform, alpha, out),
                Kind::Sprite(sprite) => {
                    let frames = sprite.frames as f64;
                    let f = self.num(&sprite.frame).floor();
                    let f = ((f - frames * (f / frames).floor()) as u32).min(sprite.frames - 1);
                    let flip = sprite.flip.as_ref().is_some_and(|p| self.machine.truth(p, &self.slots));
                    let pixel = (self.num(&sprite.pixel) * transform.scale()).clamp(1e-5, 1.0);
                    let (ox, oy) = transform.apply(0.0, 0.0);
                    // The anchor stays on the same pixel of the picture when it's mirrored.
                    let ax = if flip { sprite.frame_w as f64 - sprite.anchor[0] } else { sprite.anchor[0] };
                    out.push(Item::Sprite {
                        image: sprite.image,
                        src: [(f % sprite.columns) * sprite.frame_w, (f / sprite.columns) * sprite.frame_h, sprite.frame_w, sprite.frame_h],
                        x: bound(ox - ax * pixel),
                        y: bound(oy - sprite.anchor[1] * pixel),
                        pixel: pixel as f32,
                        flip,
                        alpha: alpha as f32,
                        smooth: sprite.smooth,
                    });
                }
                Kind::Image(image) => {
                    // `index` chooses among several, counting from 0 and going round.
                    let mut pick = |count: usize| {
                        let count_f = count as f64;
                        let index = self.num(&image.index).floor();
                        ((index - count_f * (index / count_f).floor()) as usize).min(count - 1)
                    };
                    let src = match &image.source {
                        pack::ImageFrom::Files(files) => Some(ImageSrc::Pack(files[pick(files.len())])),
                        pack::ImageFrom::Backgrounds if !pack.backgrounds.is_empty() => Some(ImageSrc::Background(pick(pack.backgrounds.len()))),
                        pack::ImageFrom::Photos if !pack.photos.is_empty() => Some(ImageSrc::Photo(pick(pack.photos.len()))),
                        _ => None,
                    };
                    if let Some(src) = src {
                        let crop = match (image.frame, image.fit) {
                            (Some((w, h)), Fit::Cover) => (w / h * 10_000.0).round() as u32,
                            _ => 0,
                        };
                        out.push(Item::Image {
                            source: ImageRef { src, filter: image.filter, crop, limit: 0 },
                            fit: image.fit,
                            frame: image.frame.map(|(w, h)| (w as f32, h as f32)),
                            zoom: self.num(&image.zoom).clamp(0.1, 10.0) as f32,
                            pan_x: self.num(&image.pan_x).clamp(-1.0, 1.0) as f32,
                            pan_y: self.num(&image.pan_y).clamp(-1.0, 1.0) as f32,
                            alpha: alpha as f32,
                            transform,
                        });
                    }
                }
            }
            if glowing {
                out.push(Item::EndGlow);
            }
            if let Some(copies) = sorted.as_deref_mut() {
                let place = copies.len();
                copies.push((depth, place, std::mem::take(&mut chunk)));
            }
        }
    }

    fn lines(&mut self, pack: &Pack, lines: &pack::Lines, transform: Affine, color: [f64; 4], alpha: f64, out: &mut Vec<Item>) {
        let b = pack.builtins;
        let mut local: Vec<(f64, f64)> = Vec::new();
        match &lines.points {
            Points::Fixed(points) => local.extend(points.iter().map(|p| (p[0], p[1]))),
            Points::Sampled { samples, lets, px, py } => {
                let m = *samples as f64;
                self.set(b.m, m);
                for j in 0..*samples {
                    self.set(b.j, j as f64);
                    self.set(b.s, j as f64 / (m - 1.0));
                    self.run_lets(lets);
                    let x = self.num(px);
                    let y = self.num(py);
                    local.push((x, y));
                }
            }
        }
        let draw = self.num(&lines.draw).clamp(0.0, 1.0);
        let width = (self.num(&lines.width) * transform.scale()).clamp(0.0, 0.05);
        let fill = match &lines.fill {
            Some(p) => {
                let c = self.machine.color(p, &self.slots);
                Some(rgba(c, alpha))
            }
            None => None,
        };
        let complete = draw >= 1.0;
        let mut points: Vec<(f64, f64)> = local.iter().map(|&(x, y)| transform.apply(x, y)).collect();
        if lines.closed && complete && points.len() > 2 {
            // A closed outline is drawn as such; a partly drawn one stays open.
        } else if !complete {
            truncate_path(&mut points, draw, lines.closed);
        }
        if points.len() < 2 {
            return;
        }
        let stroke = (width > 0.0).then(|| (width as f32, rgba(color, alpha)));
        let points = points.into_iter().map(|(x, y)| (bound(x), bound(y))).collect();
        out.push(Item::Path { points, breaks: Vec::new(), closed: lines.closed && complete, stroke, fill: if complete { fill } else { None } });
    }

    fn mesh(&mut self, pack: &Pack, mesh: &pack::Mesh, transform: Affine, color: [f64; 4], alpha: f64, out: &mut Vec<Item>) {
        let be = pack.builtins;
        let (rx, ry, rz) = (self.num(&mesh.rx), self.num(&mesh.ry), self.num(&mesh.rz));
        let (ox, oy, oz) = (self.num(&mesh.offset[0]), self.num(&mesh.offset[1]), self.num(&mesh.offset[2]));
        let size = self.num(&mesh.size);
        let distance = self.num(&mesh.distance);
        let width = (self.num(&mesh.width) * transform.scale()).clamp(0.0, 0.05);
        let draw = self.num(&mesh.draw).clamp(0.0, 1.0);
        if width <= 0.0 || draw <= 0.0 {
            return;
        }
        let (sx, cx) = rx.sin_cos();
        let (sy, cy) = ry.sin_cos();
        let (sz, cz) = rz.sin_cos();
        // Turn about the vertical axis, then tilt, then roll, then move.
        let projected: Vec<Option<(f64, f64)>> = mesh
            .vertices
            .iter()
            .map(|v| {
                let (x, y, z) = (v[0], v[1], v[2]);
                let (x, z) = (x * cy + z * sy, -x * sy + z * cy);
                let (y, z) = (y * cx - z * sx, y * sx + z * cx);
                let (x, y) = (x * cz - y * sz, x * sz + y * cz);
                let (x, y, z) = (x + ox, y + oy, z + oz);
                let factor = if distance > 0.0 {
                    let depth = distance + z;
                    if depth < 0.05 {
                        return None;
                    }
                    distance / depth
                } else {
                    1.0
                };
                Some(transform.apply(x * size * factor, y * size * factor))
            })
            .collect();
        let total = mesh.edges.len() as f64 * draw;
        let full = total.floor() as usize;
        let stroke = rgba(color, alpha);
        // Edges grouped by color, in the order each color first appears.
        let mut groups: Vec<EdgeGroup> = Vec::new();
        for (index, edge) in mesh.edges.iter().enumerate() {
            let part = if index < full {
                1.0
            } else if index == full {
                total - full as f64
            } else {
                break;
            };
            if part <= 0.0 {
                break;
            }
            let (Some(a), Some(b)) = (projected[edge[0] as usize], projected[edge[1] as usize]) else { continue };
            let edge_stroke = match &mesh.edge_color {
                Some(program) => {
                    let (va, vb) = (mesh.vertices[edge[0] as usize], mesh.vertices[edge[1] as usize]);
                    self.set(be.e, index as f64);
                    self.set(be.ex, (va[0] + vb[0]) / 2.0);
                    self.set(be.ey, (va[1] + vb[1]) / 2.0);
                    self.set(be.ez, (va[2] + vb[2]) / 2.0);
                    rgba(self.machine.color(program, &self.slots), alpha)
                }
                None => stroke,
            };
            // An edge given no color at all isn't drawn.
            if edge_stroke[3] <= 0.0 {
                continue;
            }
            let group = match groups.iter().position(|g| g.0 == edge_stroke) {
                Some(g) => g,
                None => {
                    groups.push((edge_stroke, Vec::new(), Vec::new()));
                    groups.len() - 1
                }
            };
            let end = (a.0 + (b.0 - a.0) * part, a.1 + (b.1 - a.1) * part);
            let (_, points, breaks) = &mut groups[group];
            breaks.push(points.len() as u32);
            points.push((bound(a.0), bound(a.1)));
            points.push((bound(end.0), bound(end.1)));
        }
        for (c, points, breaks) in groups {
            out.push(Item::Path { points, breaks, closed: false, stroke: Some((width as f32, c)), fill: None });
        }
    }

    fn text(&mut self, text: &pack::Text, transform: Affine, color: [f64; 4], alpha: f64, out: &mut Vec<Item>) {
        let size = (self.num(&text.size) * transform.scale()).clamp(0.002, 1.0);
        let metrics = self.metrics.get(text.font).copied().unwrap_or(self.metrics[0]);
        let mut s = std::mem::take(&mut self.text);
        s.clear();
        for segment in &text.segments {
            match segment {
                Segment::Literal(l) => s.push_str(l),
                Segment::Value(p, format) => {
                    let v = self.machine.num(p, &self.slots);
                    s.push_str(&pack::format_number(v, *format));
                }
                Segment::Clock(word) => match word {
                    pack::ClockText::Zone => s.push_str(&self.reading.abbreviation),
                    pack::ClockText::ZoneName => s.push_str(self.clock.name()),
                    pack::ClockText::Offset => s.push_str(&self.reading.offset_text()),
                    pack::ClockText::Weekday => s.push_str(self.reading.weekday_name()),
                    pack::ClockText::Month => s.push_str(self.reading.month_name()),
                },
            }
        }
        let shown = match &text.reveal {
            Some(p) => self.num(p).clamp(0.0, 1e6) as usize,
            None => usize::MAX,
        };
        let advance = metrics.advance * size * text.stretch;
        let line_step = text.line_height * size;
        let lines: Vec<&str> = s.split('\n').collect();
        let height = (lines.len() as f64 - 1.0) * line_step + size;
        let top = match text.valign {
            VAlign::Top => 0.0,
            VAlign::Middle => -height / 2.0,
            VAlign::Bottom => -height,
        };
        let (ox, oy) = transform.apply(0.0, 0.0);
        let c = rgba(color, alpha);
        let mut glyphs = Vec::new();
        let mut remaining = shown;
        let mut cursor_at = None;
        for (row, line) in lines.iter().enumerate() {
            let width = line.chars().count() as f64 * advance;
            let left = match text.align {
                Align::Left => 0.0,
                Align::Center => -width / 2.0,
                Align::Right => -width,
            };
            let baseline = top + row as f64 * line_step + metrics.ascent * size;
            let mut col = 0usize;
            for ch in line.chars() {
                if remaining == 0 {
                    break;
                }
                remaining -= 1;
                if ch != ' ' {
                    glyphs.push(Glyph { ch, x: bound(ox + left + col as f64 * advance), y: bound(oy + baseline), color: c });
                }
                col += 1;
            }
            cursor_at = Some((ox + left + col as f64 * advance, oy + top + row as f64 * line_step));
            if remaining == 0 || row + 1 == lines.len() {
                break;
            }
            // The line break takes a character's time, as typing does.
            remaining -= 1;
        }
        self.text = s;
        let cursor = match &text.cursor {
            Some(p) => self.machine.truth(p, &self.slots),
            None => false,
        };
        if cursor {
            if let Some((x, y)) = cursor_at {
                let (w, h) = (advance, size);
                let points = vec![(bound(x), bound(y)), (bound(x + w), bound(y)), (bound(x + w), bound(y + h)), (bound(x), bound(y + h))];
                out.push(Item::Path { points, breaks: Vec::new(), closed: true, stroke: None, fill: Some(c) });
            }
        }
        if !glyphs.is_empty() {
            let font = if text.font < self.metrics.len() { text.font as u8 } else { 0 };
            out.push(Item::Glyphs { size: size as f32, cell_h: (size * metrics.line) as f32, font, stretch: text.stretch as f32, glyphs });
        }
    }

    fn ascii(&mut self, pack: &Pack, ascii: &pack::Ascii, transform: Affine, alpha: f64, out: &mut Vec<Item>) {
        let art = &pack.arts[ascii.art];
        let rows = art.len();
        let cols = art.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let aspect = self.slots[pack.builtins.aspect as usize];
        let metrics = self.metrics[0];
        let advance = metrics.advance;
        let line = metrics.line;
        // Cells are shaped as a terminal's: one advance wide and one line tall.
        let size = (ascii.fit * aspect / (cols as f64 * advance)).min(ascii.fit / (rows as f64 * line)) * transform.scale();
        let cell_w = advance * size;
        let cell_h = line * size;
        let (ox, oy) = transform.apply(0.0, 0.0);
        let left = ox - cols as f64 * cell_w / 2.0;
        let top = oy - rows as f64 * cell_h / 2.0;
        let progress = self.num(&ascii.progress).clamp(0.0, 1.0);
        let time = self.slots[pack.builtins.time as usize];
        let cycle = self.slots[pack.builtins.cycle as usize];
        let effect = match ascii.effect {
            Effect::Random => {
                let pick = (hash(cycle * 31.0 + ascii.art as f64, self.machine.seed) * 5.0) as usize;
                [Effect::Print, Effect::Decrypt, Effect::Rain, Effect::Wipe, Effect::Fade][pick.min(4)]
            }
            e => e,
        };
        let colors: Vec<[f64; 4]> = ascii.colors.iter().map(|p| self.machine.color(p, &self.slots)).collect();
        let total: usize = art.iter().map(|r| r.iter().filter(|c| **c != ' ').count()).sum::<usize>().max(1);
        let mut order = 0usize;
        let mut glyphs = Vec::new();
        for (r, row) in art.iter().enumerate() {
            let along = if rows > 1 { r as f64 / (rows - 1) as f64 } else { 0.0 };
            let base = gradient(&colors, along);
            for (c, &ch) in row.iter().enumerate() {
                if ch == ' ' {
                    continue;
                }
                let key = (r * ART_KEY + c) as f64;
                let mut shown = ch;
                let mut cell_alpha = 1.0;
                let mut dy = 0.0;
                let mut color = base;
                match effect {
                    Effect::Print => {
                        if (order as f64) >= progress * 1.05 * total as f64 {
                            cell_alpha = 0.0;
                        }
                    }
                    Effect::Decrypt => {
                        let settle = hash(key, 0x5ec) * 0.8;
                        if progress < settle {
                            let tick = (time * 12.0).floor();
                            shown = DECRYPT_CHARS[(hash(key + tick * 977.0, 0xdec) * DECRYPT_CHARS.len() as f64) as usize % DECRYPT_CHARS.len()];
                            cell_alpha = 0.55;
                            color = mix(base, colors[colors.len() - 1], 0.5);
                        }
                    }
                    Effect::Rain => {
                        let start = hash(c as f64, 0x7a1) * 0.45 + along * 0.25;
                        let fall = ((progress - start) / 0.3).clamp(0.0, 1.0);
                        if fall <= 0.0 {
                            cell_alpha = 0.0;
                        }
                        let eased = 1.0 - (1.0 - fall).powi(3);
                        // Each character drops in from just above the stage.
                        dy = -(1.0 - eased) * (top + r as f64 * cell_h + 0.6);
                    }
                    Effect::Wipe => {
                        let x = c as f64 / cols as f64;
                        cell_alpha = ((progress * 1.25 - x) * 8.0).clamp(0.0, 1.0);
                    }
                    Effect::Fade | Effect::Random => {
                        let u = progress.clamp(0.0, 1.0);
                        cell_alpha = u * u * (3.0 - 2.0 * u);
                    }
                }
                order += 1;
                if cell_alpha <= 0.0 {
                    continue;
                }
                glyphs.push(Glyph {
                    ch: shown,
                    x: bound(left + c as f64 * cell_w),
                    y: bound(top + r as f64 * cell_h + metrics.ascent * size + dy),
                    color: rgba(color, alpha * cell_alpha),
                });
            }
        }
        if !glyphs.is_empty() {
            out.push(Item::Glyphs { size: size as f32, cell_h: cell_h as f32, font: 0, stretch: 1.0, glyphs });
        }
    }
}

const ART_KEY: usize = 1000;

/// A color, and the points and subpath starts of the mesh edges drawn in it.
type EdgeGroup = (Rgba, Vec<(f32, f32)>, Vec<u32>);

fn gradient(colors: &[[f64; 4]], at: f64) -> [f64; 4] {
    if colors.len() == 1 {
        return colors[0];
    }
    let scaled = at.clamp(0.0, 1.0) * (colors.len() - 1) as f64;
    let i = (scaled.floor() as usize).min(colors.len() - 2);
    mix(colors[i], colors[i + 1], scaled - i as f64)
}

/// Shortens a polyline to `fraction` of its length (including the closing edge when `closed`).
fn truncate_path(points: &mut Vec<(f64, f64)>, fraction: f64, closed: bool) {
    if closed && points.len() > 2 {
        points.push(points[0]);
    }
    let lengths: Vec<f64> = points.windows(2).map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt()).collect();
    let total: f64 = lengths.iter().sum();
    let mut left = total * fraction;
    if left <= 0.0 || total <= 0.0 {
        points.truncate(if fraction > 0.0 { 1 } else { 0 });
        return;
    }
    for (i, len) in lengths.iter().enumerate() {
        if left <= *len {
            let t = if *len > 0.0 { left / len } else { 0.0 };
            let (a, b) = (points[i], points[i + 1]);
            points.truncate(i + 1);
            points.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            return;
        }
        left -= len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::{load, tests::temp_pack, Environment};

    const METRICS: FontMetrics = FontMetrics { advance: 0.6, ascent: 0.8, line: 1.2 };

    fn pack(name: &str, body: &str) -> Pack {
        let dir = temp_pack(name, &format!("format = 1\nname = \"T\"\n{body}"), &[("art.txt", b"AB\n C\n")]);
        let pack = load(&dir, &Environment::default()).unwrap_or_else(|e| panic!("{e}"));
        std::fs::remove_dir_all(&dir).unwrap();
        pack
    }

    #[test]
    fn affine() {
        let t = Affine::IDENTITY.then(1.0, 2.0, std::f64::consts::FRAC_PI_2, 2.0);
        let (x, y) = t.apply(1.0, 0.0);
        assert!((x - 1.0).abs() < 1e-12 && (y - 4.0).abs() < 1e-12);
        assert!((t.scale() - 2.0).abs() < 1e-12);
    }

    #[test]
    fn timeline() {
        let p = pack(
            "timeline",
            "fade = 0\n[[scene]]\nduration = 2\nhold = 1\n[[scene.draw]]\nkind = \"rect\"\n[[scene]]\nduration = 4\n[[scene.draw]]\nkind = \"rect\"\n",
        );
        assert_eq!(Evaluator::locate(&p, 0.5), (0, 0.5, 0.0));
        assert_eq!(Evaluator::locate(&p, 3.5), (1, 0.5, 0.0));
        assert_eq!(Evaluator::locate(&p, 7.5), (0, 0.5, 1.0));
    }

    #[test]
    fn fades_and_background_blend() {
        let p = pack(
            "fade",
            "fade = 1\n[[scene]]\nduration = 4\nbackground = \"#000000\"\n[[scene.draw]]\nkind = \"rect\"\ncolor = \"#ffffff\"\n[[scene]]\nduration = 4\nbackground = \"#ffffff\"\n[[scene.draw]]\nkind = \"rect\"\n",
        );
        let mut e = Evaluator::new(&p, 1, vec![METRICS]);
        let start = e.frame(&p, 0.0, 16.0 / 9.0);
        assert!(start.items.is_empty(), "nothing shows at the very start of a fade-in");
        let mid = e.frame(&p, 2.0, 16.0 / 9.0);
        assert_eq!(mid.background, [0.0, 0.0, 0.0, 1.0]);
        let Item::Path { fill: Some(c), .. } = &mid.items[0] else { panic!() };
        assert_eq!(c[3], 1.0);
        let late = e.frame(&p, 3.5, 16.0 / 9.0);
        assert!((late.background[0] - 0.5).abs() < 1e-6);
        let Item::Path { fill: Some(c), .. } = &late.items[0] else { panic!() };
        assert!((c[3] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn text_reveal_format_and_cursor() {
        let p = pack("text", "fade = 0\n[[scene]]\nduration = 10\n[[scene.draw]]\nkind = \"text\"\ntext = \"AB {420 - t * 10; 03}\\nCD\"\nreveal = \"t * 2\"\ncursor = \"t > 0\"\nsize = 0.1\n");
        let mut e = Evaluator::new(&p, 1, vec![METRICS]);
        let f = e.frame(&p, 1.0, 16.0 / 9.0);
        let Item::Glyphs { glyphs, size, .. } = &f.items[1] else { panic!("{:?}", f.items) };
        assert_eq!(*size, 0.1);
        assert_eq!(glyphs.iter().map(|g| g.ch).collect::<String>(), "AB");
        let f = e.frame(&p, 4.0, 16.0 / 9.0);
        let Item::Glyphs { glyphs, .. } = &f.items[1] else { panic!() };
        assert_eq!(glyphs.iter().map(|g| g.ch).collect::<String>(), "AB380C");
        assert!(glyphs[5].y > glyphs[0].y, "the second line is lower");
        assert!(matches!(&f.items[0], Item::Path { fill: Some(_), .. }), "cursor block");
    }

    #[test]
    fn counts_samples_and_draw_fraction() {
        let p = pack(
            "lines",
            "fade = 0\n[[scene]]\nduration = 10\n[[scene.draw]]\nkind = \"lines\"\ncount = 3\nsamples = 5\npx = \"s + i\"\npy = \"k\"\ndraw = \"0.5\"\n",
        );
        let mut e = Evaluator::new(&p, 1, vec![METRICS]);
        let f = e.frame(&p, 1.0, 16.0 / 9.0);
        assert_eq!(f.items.len(), 3);
        let Item::Path { points, .. } = &f.items[2] else { panic!() };
        assert_eq!(points.first(), Some(&(2.0, 1.0)));
        assert_eq!(points.last(), Some(&(2.5, 1.0)));
    }

    #[test]
    fn sample_lets_are_worked_out_for_each_sample() {
        let p = pack(
            "samplelet",
            "fade = 0\n[[scene]]\nduration = 10\n[[scene.draw]]\nkind = \"lines\"\nlet = [\"lift = 2\"]\nsamples = 3\n\
             sample_let = [\"w = s * 4\", \"v = w * lift + j\"]\npx = \"w\"\npy = \"v\"\n",
        );
        let mut e = Evaluator::new(&p, 1, vec![METRICS]);
        let f = e.frame(&p, 1.0, 16.0 / 9.0);
        let Item::Path { points, .. } = &f.items[0] else { panic!() };
        assert_eq!(points, &vec![(0.0, 0.0), (2.0, 5.0), (4.0, 10.0)]);
    }

    #[test]
    fn mesh_projection_and_partial_draw() {
        let p = pack(
            "mesh",
            "fade = 0\n[[scene]]\nduration = 10\n[[scene.draw]]\nkind = \"mesh\"\nvertices = [[-1,0,0],[1,0,0],[1,1,0]]\nedges = [[0,1],[1,2]]\nsize = 1\ndistance = 0\ndraw = 0.75\n",
        );
        let mut e = Evaluator::new(&p, 1, vec![METRICS]);
        let f = e.frame(&p, 1.0, 16.0 / 9.0);
        let Item::Path { points, breaks, .. } = &f.items[0] else { panic!() };
        assert_eq!(breaks, &vec![0, 2]);
        assert_eq!(points[3], (1.0, 0.5));
    }

    #[test]
    fn mesh_offset_moves_before_perspective() {
        let p = pack(
            "offset",
            "fade = 0\n[[scene]]\nduration = 10\n[[scene.draw]]\nkind = \"mesh\"\nvertices = [[0,0,0],[1,0,0]]\nedges = [[0,1]]\nsize = 1\ndistance = 2\noffset_y = 1\noffset_z = 2\n",
        );
        let mut e = Evaluator::new(&p, 1, vec![METRICS]);
        let f = e.frame(&p, 1.0, 16.0 / 9.0);
        let Item::Path { points, .. } = &f.items[0] else { panic!() };
        // Moved down 1 and away 2, so seen at half size: (0, 0.5) and (0.5, 0.5).
        assert_eq!(points[..2], [(0.0, 0.5), (0.5, 0.5)]);
    }

    #[test]
    fn glows_wrap_their_element_and_transparent_edges_go() {
        let p = pack(
            "glows",
            "fade = 0\n[[scene]]\nduration = 10\n[[scene.draw]]\nkind = \"mesh\"\nvertices = [[0,0,0],[1,0,0],[1,1,0]]\nedges = [[0,1],[1,2]]\nedge_color = \"e == 0 ? alpha(white, 0) : white\"\nglow = \"t > 2 ? 1 : 0\"\nglow_radius = 0.01\n",
        );
        let mut e = Evaluator::new(&p, 1, vec![METRICS]);
        let f = e.frame(&p, 3.0, 16.0 / 9.0);
        assert!(matches!(f.items[0], Item::Glow { strength, .. } if strength == 1.0));
        let Item::Path { points, .. } = &f.items[1] else { panic!("{:?}", f.items) };
        assert_eq!(points.len(), 2, "only the colored edge");
        assert_eq!(f.items[2], Item::EndGlow);
        let f = e.frame(&p, 1.0, 16.0 / 9.0);
        assert!(matches!(f.items[0], Item::Path { .. }), "no glow while its strength is 0");
        assert_eq!(f.items.len(), 1);
    }

    #[test]
    fn mesh_edge_colors_group_edges() {
        let p = pack(
            "edgecolor",
            "fade = 0\n[[scene]]\nduration = 10\n[[scene.draw]]\nkind = \"mesh\"\nvertices = [[0,0,0],[1,0,0],[0,1,0],[1,1,0]]\nedges = [[0,1],[2,3],[0,2]]\nedge_color = \"ey < 0.5 ? red : blue\"\n",
        );
        let mut e = Evaluator::new(&p, 1, vec![METRICS]);
        let f = e.frame(&p, 1.0, 16.0 / 9.0);
        assert_eq!(f.items.len(), 2, "one path per color");
        let Item::Path { points, .. } = &f.items[0] else { panic!() };
        assert_eq!(points.len(), 2, "only the bottom edge has its midpoint below 0.5");
    }

    #[test]
    fn mesh_turns_then_tilts() {
        let p = pack(
            "turn",
            "fade = 0\n[[scene]]\nduration = 10\n[[scene.draw]]\nkind = \"mesh\"\nvertices = [[0,0,0],[1,0,0]]\nedges = [[0,1]]\nsize = 1\ndistance = 0\nry = \"pi / 2\"\nrx = \"pi / 2\"\n",
        );
        let mut e = Evaluator::new(&p, 1, vec![METRICS]);
        let f = e.frame(&p, 1.0, 16.0 / 9.0);
        let Item::Path { points, .. } = &f.items[0] else { panic!() };
        // (1, 0, 0) turned a quarter about y goes to (0, 0, -1); tilting that a quarter about x gives (0, 1, 0).
        assert!(points[1].0.abs() < 1e-6 && (points[1].1 - 1.0).abs() < 1e-6, "{:?}", points);
    }

    #[test]
    fn groups_clip_and_restore_index() {
        let p = pack(
            "group",
            "fade = 0\n[[scene]]\nduration = 10\n[[scene.draw]]\nkind = \"group\"\ncount = 2\nx = \"i\"\nclip = [\"-1\", \"-1\", \"1\", \"1\"]\n[[scene.draw.draw]]\nkind = \"rect\"\ncount = 3\n[[scene.draw.draw]]\nkind = \"rect\"\ny = \"i\"\n",
        );
        let mut e = Evaluator::new(&p, 1, vec![METRICS]);
        let f = e.frame(&p, 1.0, 16.0 / 9.0);
        assert_eq!(f.items.len(), 2 * (1 + 3 + 1 + 1));
        assert!(matches!(f.items[0], Item::Clip(_)));
        let Item::Path { points, .. } = &f.items[10] else { panic!("{:?}", f.items[10]) };
        // The second group copy sits at x = 1. Its last child has no count of its own, so it shares the group's
        // i (1), even after the counted child before it numbered its own copies.
        let cx = points.iter().map(|p| p.0).sum::<f32>() / 4.0;
        let cy = points.iter().map(|p| p.1).sum::<f32>() / 4.0;
        assert!((cx - 1.0).abs() < 1e-5 && (cy - 1.0).abs() < 1e-5, "({cx}, {cy})");
    }

    #[test]
    fn ascii_effects_reveal_everything_at_the_end() {
        for effect in ["print", "decrypt", "rain", "wipe", "fade"] {
            let p = pack(
                &format!("ascii-{effect}"),
                &format!("fade = 0\n[[scene]]\nduration = 10\nhold = 2\n[[scene.draw]]\nkind = \"ascii\"\nart = \"art.txt\"\neffect = \"{effect}\"\n"),
            );
            let mut e = Evaluator::new(&p, 1, vec![METRICS]);
            let f = e.frame(&p, 11.0, 16.0 / 9.0);
            let Item::Glyphs { glyphs, .. } = &f.items[0] else { panic!() };
            assert_eq!(glyphs.iter().map(|g| g.ch).collect::<String>(), "ABC", "{effect}");
            assert!(glyphs.iter().all(|g| (g.color[3] - 1.0).abs() < 1e-6), "{effect}");
            let f = e.frame(&p, 0.0, 16.0 / 9.0);
            let shown = match f.items.first() {
                Some(Item::Glyphs { glyphs, .. }) => glyphs.iter().filter(|g| g.color[3] > 0.9 && "ABC".contains(g.ch)).count(),
                _ => 0,
            };
            assert!(shown < 3, "{effect} starts hidden or scrambled");
        }
    }

    #[test]
    fn truncation() {
        let mut p = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)];
        truncate_path(&mut p, 0.25, false);
        assert_eq!(p, vec![(0.0, 0.0), (0.5, 0.0)]);
        let mut square = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
        truncate_path(&mut square, 0.875, true);
        assert_eq!(square.last(), Some(&(0.0, 0.5)));
    }

    #[test]
    fn text_reads_the_clock() {
        let p = pack(
            "clock",
            "fade = 0\n[[scene]]\nduration = 10\n[[scene.draw]]\nkind = \"text\"\ntext = \"{hour;02}:{minute;02} {zone} {offset} {weekday_name} {day} {month_name}\"\n",
        );
        let mut e = Evaluator::new(&p, 1, vec![METRICS]);
        let f = e.frame(&p, 30.0, 16.0 / 9.0);
        let Item::Glyphs { glyphs, .. } = &f.items[0] else { panic!("{:?}", f.items) };
        assert_eq!(glyphs.iter().map(|g| g.ch).filter(|c| *c != ' ').collect::<String>(), "10:09UTC+00:00Thursday15January");
    }

    #[test]
    fn an_image_chooses_among_its_files() {
        let png = crate::raster::tests::tiny_png();
        let dir = temp_pack(
            "files",
            "format = 1\nname = \"T\"\n[[scene]]\nduration = 10\n[[scene.draw]]\nkind = \"image\"\nfiles = [\"a.png\", \"b.png\", \"c.png\"]\nindex = \"t\"\nw = 0.4\nh = 0.2\nfilter = \"sepia\"\n",
            &[("a.png", &png), ("b.png", &png), ("c.png", &png)],
        );
        let p = load(&dir, &Environment::default()).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let mut e = Evaluator::new(&p, 1, vec![METRICS]);
        let chosen = |e: &mut Evaluator, t: f64| match &e.frame(&p, t, 16.0 / 9.0).items[0] {
            Item::Image { source, frame, .. } => (*source, *frame),
            other => panic!("{other:?}"),
        };
        let (a, frame) = chosen(&mut e, 1.5);
        assert_eq!(a.src, ImageSrc::Pack(1));
        assert_eq!(frame, Some((0.4, 0.2)));
        assert_eq!(a.filter, pack::Filter::Sepia);
        assert_eq!(a.crop, 20_000, "cut to the box's 2:1 shape");
        assert_eq!(chosen(&mut e, 4.0).0.src, ImageSrc::Pack(1), "going round");
    }
}
