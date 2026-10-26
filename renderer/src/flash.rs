//! Keeping frames within the photosensitivity limit of WCAG 2.2 (success criterion 2.3.1): no more than three
//! general flashes or three red flashes in any one second, over any area larger than a quarter of a 10° visual
//! field.
//!
//! A frame is measured on a 48 x 27 grid of cells. A cell's transition is a change of relative luminance of 0.1
//! or more against its last turning point, in the opposite direction to its previous transition, where the darker
//! of the two is below 0.8; a red transition is the same for WCAG's saturated-red measure, with a step of 20.
//! A flash is two transitions, so a cell flashes when it makes more than six in a second. The 10° field is taken as
//! a third of the screen's width and height (WCAG's 341 x 256 pixels on a 1024 x 768 screen), so an area is too
//! large when more than a quarter of the cells in any 16 x 9 window flash together.
//!
//! When drawing, a frame that would cross the limit is not shown; the previous one stays on screen until showing a
//! new one would be within the limit again.

pub const GRID_W: usize = 48;
pub const GRID_H: usize = 27;
const WINDOW_W: usize = 16;
const WINDOW_H: usize = 9;
const AREA_LIMIT: usize = WINDOW_W * WINDOW_H / 4;
const LUMINANCE_STEP: f32 = 0.1;
const DARK_LIMIT: f32 = 0.8;
const RED_STEP: f32 = 20.0;
const TRANSITIONS_PER_SECOND: usize = 6;
const SAMPLE_STEP: usize = 4;

fn linear_table() -> [f32; 256] {
    let mut t = [0.0; 256];
    for (i, v) in t.iter_mut().enumerate() {
        let c = i as f32 / 255.0;
        *v = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    }
    t
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Cell {
    pub luminance: f32,
    pub red: f32,
}

/// Averages a frame of opaque pixels into the grid. `bgra` says the bytes are B, G, R, A rather than R, G, B, A.
pub fn measure(data: &[u8], width: usize, height: usize, stride: usize, bgra: bool) -> Vec<Cell> {
    let table = linear_table();
    let mut sums = vec![[0.0f32; 3]; GRID_W * GRID_H];
    let mut counts = vec![0u32; GRID_W * GRID_H];
    let (ri, bi) = if bgra { (2, 0) } else { (0, 2) };
    for y in (SAMPLE_STEP / 2..height).step_by(SAMPLE_STEP) {
        let row = &data[y * stride..];
        let cy = y * GRID_H / height;
        for x in (SAMPLE_STEP / 2..width).step_by(SAMPLE_STEP) {
            let p = &row[x * 4..x * 4 + 4];
            let cell = cy * GRID_W + x * GRID_W / width;
            sums[cell][0] += table[p[ri] as usize];
            sums[cell][1] += table[p[1] as usize];
            sums[cell][2] += table[p[bi] as usize];
            counts[cell] += 1;
        }
    }
    sums.iter()
        .zip(&counts)
        .map(|(s, &n)| {
            if n == 0 {
                return Cell::default();
            }
            let (r, g, b) = (s[0] / n as f32, s[1] / n as f32, s[2] / n as f32);
            let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let saturated = r + g + b > 0.0 && r / (r + g + b) >= 0.8;
            let red = if saturated { ((r - g - b) * 320.0).max(0.0) } else { 0.0 };
            Cell { luminance, red }
        })
        .collect()
}

#[derive(Clone, Copy, Default)]
struct Track {
    anchor: f32,
    direction: i8,
    started: bool,
    times: [f64; TRANSITIONS_PER_SECOND + 1],
    count: usize,
}

impl Track {
    /// The direction of the transition `value` makes, if it makes one.
    fn transition(&self, value: f32, step: f32, dark_limit: Option<f32>) -> Option<i8> {
        if !self.started {
            return None;
        }
        let delta = value - self.anchor;
        let dark_enough = |a: f32, b: f32| dark_limit.is_none_or(|limit| a.min(b) < limit);
        if delta.abs() >= step && dark_enough(value, self.anchor) {
            let direction = if delta > 0.0 { 1 } else { -1 };
            if direction != self.direction {
                return Some(direction);
            }
        }
        None
    }

    fn recent(&self, now: f64) -> usize {
        self.times[..self.count.min(self.times.len())].iter().filter(|&&t| t > now - 1.0).count()
    }

    fn update(&mut self, value: f32, now: f64, transition: Option<i8>) {
        if !self.started {
            self.started = true;
            self.anchor = value;
            return;
        }
        match transition {
            Some(direction) => {
                self.direction = direction;
                self.anchor = value;
                let slot = self.count % self.times.len();
                self.times[slot] = now;
                self.count += 1;
            }
            // Follow the value further in the current direction, so the next turn is measured from its peak.
            None if (self.direction > 0 && value > self.anchor) || (self.direction < 0 && value < self.anchor) => self.anchor = value,
            None if self.direction == 0 => {}
            None => {}
        }
    }
}

#[derive(Clone, Default)]
pub struct Guard {
    luminance: Vec<Track>,
    red: Vec<Track>,
}

/// What a frame would do to the flash count.
pub struct Assessment {
    pub flashing_cells: usize,
    pub over_limit: bool,
    transitions: Vec<(Option<i8>, Option<i8>)>,
}

impl Guard {
    pub fn new() -> Guard {
        Guard { luminance: vec![Track::default(); GRID_W * GRID_H], red: vec![Track::default(); GRID_W * GRID_H] }
    }

    pub fn assess(&self, cells: &[Cell], now: f64) -> Assessment {
        let mut flashing = vec![false; GRID_W * GRID_H];
        let mut transitions = Vec::with_capacity(cells.len());
        for (i, cell) in cells.iter().enumerate() {
            let l = self.luminance[i].transition(cell.luminance, LUMINANCE_STEP, Some(DARK_LIMIT));
            let r = self.red[i].transition(cell.red, RED_STEP, None);
            let lc = self.luminance[i].recent(now) + usize::from(l.is_some());
            let rc = self.red[i].recent(now) + usize::from(r.is_some());
            flashing[i] = lc > TRANSITIONS_PER_SECOND || rc > TRANSITIONS_PER_SECOND;
            transitions.push((l, r));
        }
        let (over_limit, flashing_cells) = worst_window(&flashing);
        Assessment { flashing_cells, over_limit, transitions }
    }

    pub fn commit(&mut self, cells: &[Cell], now: f64, assessment: Assessment) {
        for (i, cell) in cells.iter().enumerate() {
            let (l, r) = assessment.transitions[i];
            self.luminance[i].update(cell.luminance, now, l);
            self.red[i].update(cell.red, now, r);
        }
    }

    /// Whether to show this frame; if so, it is recorded.
    pub fn admit(&mut self, cells: &[Cell], now: f64) -> bool {
        let assessment = self.assess(cells, now);
        if assessment.over_limit {
            return false;
        }
        self.commit(cells, now, assessment);
        true
    }

    /// State relevant to future admission, relative to a binary-exact 64 Hz
    /// clock. Prepared-loop approval uses this to establish a periodic
    /// invariant. Old events cannot affect a future one-second window; the
    /// ring's insertion slot and total historical count do not affect it either.
    pub fn periodic_state(&self, tick: u64) -> Vec<i64> {
        let mut out = Vec::new();
        for t in self.luminance.iter().chain(&self.red) {
            out.extend([t.anchor.to_bits() as i64, t.direction as i64, i64::from(t.started)]);
            let mut ages: Vec<i64> =
                t.times[..t.count.min(t.times.len())].iter().map(|time| tick as i64 - (time * 64.0) as i64).filter(|age| *age <= 64).collect();
            ages.sort_unstable();
            out.push(ages.len() as i64);
            out.extend(ages);
        }
        out
    }
}

/// Whether any 16 x 9 window has more than a quarter of its cells flashing, and the most flashing in one window.
fn worst_window(flashing: &[bool]) -> (bool, usize) {
    let mut sum = vec![0usize; (GRID_W + 1) * (GRID_H + 1)];
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            sum[(y + 1) * (GRID_W + 1) + x + 1] =
                usize::from(flashing[y * GRID_W + x]) + sum[y * (GRID_W + 1) + x + 1] + sum[(y + 1) * (GRID_W + 1) + x] - sum[y * (GRID_W + 1) + x];
        }
    }
    let mut worst = 0;
    for y in 0..=GRID_H - WINDOW_H {
        for x in 0..=GRID_W - WINDOW_W {
            let at = |xx: usize, yy: usize| sum[yy * (GRID_W + 1) + xx];
            let n = at(x + WINDOW_W, y + WINDOW_H) + at(x, y) - at(x, y + WINDOW_H) - at(x + WINDOW_W, y);
            worst = worst.max(n);
        }
    }
    (worst > AREA_LIMIT, worst)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 3]) -> Vec<u8> {
        let mut data = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let c = f(x, y);
                data[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        data
    }

    /// Drives a guard with a full-screen alternation between two colors at `hz` changes a second for two seconds,
    /// returning how many changes were shown.
    fn strobe(hz: f64, a: [u8; 3], b: [u8; 3], admit: bool) -> (usize, usize) {
        let (w, h) = (192, 108);
        let fa = measure(&frame(w, h, |_, _| a), w, h, w * 4, false);
        let fb = measure(&frame(w, h, |_, _| b), w, h, w * 4, false);
        let mut guard = Guard::new();
        let mut shown_b = false;
        let mut changes = 0;
        let mut violations = 0;
        let fps = 60.0;
        for i in 0..120 {
            let now = i as f64 / fps;
            let want_b = ((now * hz).floor() as i64) % 2 == 1;
            let cells = if want_b { &fb } else { &fa };
            if admit {
                if guard.admit(cells, now) && want_b != shown_b {
                    shown_b = want_b;
                    changes += 1;
                }
            } else {
                let a = guard.assess(cells, now);
                if a.over_limit {
                    violations += 1;
                }
                guard.commit(cells, now, a);
            }
        }
        (changes, violations)
    }

    #[test]
    fn slow_changes_pass_and_fast_ones_are_limited() {
        let black = [0, 0, 0];
        let white = [255, 255, 255];
        assert_eq!(strobe(4.0, black, white, false).1, 0, "two flashes a second is fine");
        assert!(strobe(10.0, black, white, false).1 > 0, "five flashes a second is detected");
        let (changes, _) = strobe(10.0, black, white, true);
        assert!(changes <= 2 * TRANSITIONS_PER_SECOND, "the guard shows at most six changes a second ({changes} in 2 s)");
        assert!(changes >= 6, "but still shows some ({changes})");
        let gray1 = [100, 100, 100];
        let gray2 = [110, 110, 110];
        assert_eq!(strobe(20.0, gray1, gray2, false).1, 0, "a small luminance change is not a flash");
        let bright1 = [240, 240, 240];
        let bright2 = [255, 255, 255];
        assert_eq!(strobe(20.0, bright1, bright2, false).1, 0, "changes among bright colors are not flashes");
    }

    #[test]
    fn red_flashes_are_detected() {
        let (_, violations) = strobe(10.0, [60, 0, 0], [200, 0, 0], false);
        assert!(violations > 0);
    }

    #[test]
    fn small_areas_may_flash() {
        let (w, h) = (192, 108);
        let on = measure(&frame(w, h, |x, y| if x < 20 && y < 10 { [255, 255, 255] } else { [0, 0, 0] }), w, h, w * 4, false);
        let off = measure(&frame(w, h, |_, _| [0, 0, 0]), w, h, w * 4, false);
        let mut guard = Guard::new();
        for i in 0..120 {
            let now = i as f64 / 60.0;
            let cells = if (i / 3) % 2 == 0 { &on } else { &off };
            assert!(guard.admit(cells, now), "a corner smaller than the area limit can flash");
        }
    }

    #[test]
    fn measures_channel_order() {
        let rgba = frame(192, 108, |_, _| [255, 0, 0]);
        let mut bgra = rgba.clone();
        for p in bgra.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        let a = measure(&rgba, 192, 108, 192 * 4, false);
        let b = measure(&bgra, 192, 108, 192 * 4, true);
        assert_eq!(a, b);
        assert!(a[0].red > 300.0);
        assert!((a[0].luminance - 0.2126).abs() < 1e-4);
    }
}
