//! Bounded, approximate flash admission inspired by WCAG 2.2 SC 2.3.1.
//!
//! Every pixel contributes to a 384 x 216 linear-light grid. Qualifying changes
//! are spatially combined BEFORE their rate is counted, separately for rising
//! and falling luminance and transitions to/from saturated red. A past flash
//! elsewhere is never added to the current area. A window is approximately a
//! third of the display in each dimension; this and cell averaging remain
//! spatial approximations, not a conformance or medical certification.
//!
//! WCAG leaves transition duration/synchronicity unspecified. The 66 ms and
//! 20 ms working windows follow Jordan & Vanderheiden (2024), section 6.4:
//! https://doi.org/10.1145/3694790. They handle multi-frame ramps and near-sync
//! changes without accumulating unrelated motion for an entire second.
//! The red test follows WCAG 2.2's CIE 1976 UCS working definition.

use std::collections::VecDeque;
use std::sync::OnceLock;

pub const GRID_W: usize = 384;
pub const GRID_H: usize = 216;
const CELLS: usize = GRID_W * GRID_H;
const WINDOW_W: usize = 128;
const WINDOW_H: usize = 72;
const WINDOW_STEP: usize = 8;
const WINDOWS_W: usize = (GRID_W - WINDOW_W) / WINDOW_STEP + 1;
const WINDOWS_H: usize = (GRID_H - WINDOW_H) / WINDOW_STEP + 1;
const WINDOWS: usize = WINDOWS_W * WINDOWS_H;
const AREA_LIMIT: usize = WINDOW_W * WINDOW_H / 4;
const TRANSITIONS_PER_SECOND: usize = 6;
const TRANSITION_SPAN: f64 = 0.066;
const SYNC_SPAN: f64 = 0.020;
const TIME_EPSILON: f64 = 1e-9; // arithmetic noise at an exact time-window boundary
const HISTORY_LIMIT: usize = 32;
const LUMINANCE_STEP: f32 = 0.1;
const DARK_LIMIT: f32 = 0.8;
const RED_DISTANCE_SQUARED: f32 = 0.2 * 0.2;

fn linear_table() -> &'static [f32; 256] {
    static TABLE: OnceLock<[f32; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [0.0; 256];
        for (i, v) in t.iter_mut().enumerate() {
            let c = i as f32 / 255.0;
            *v = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        }
        t
    })
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Cell {
    pub luminance: f32,
    pub u: f32,
    pub v: f32,
    pub red: bool,
}

impl Cell {
    fn linear(r: f32, g: f32, b: f32) -> Self {
        let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let red = r + g + b > 0.0 && r / (r + g + b) >= 0.8;
        let x = 0.4124564 * r + 0.3575761 * g + 0.1804375 * b;
        let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
        let z = 0.0193339 * r + 0.119192 * g + 0.9503041 * b;
        let d = x + 15.0 * y + 3.0 * z;
        let (u, v) = if d > 0.0 { (4.0 * x / d, 9.0 * y / d) } else { (0.0, 0.0) };
        Cell { luminance, u, v, red }
    }
}

/// Box-average EVERY opaque pixel in linear light; no point subsampling.
/// `bgra` selects B,G,R,A rather than R,G,B,A. Padding is never measured.
pub fn measure(data: &[u8], width: usize, height: usize, stride: usize, bgra: bool) -> Vec<Cell> {
    let table = linear_table();
    let (ri, bi) = if bgra { (2, 0) } else { (0, 2) };
    let mut cells = Vec::with_capacity(CELLS);
    for cy in 0..GRID_H {
        let top = cy * height;
        let bottom = (cy + 1) * height;
        for cx in 0..GRID_W {
            let left = cx * width;
            let right = (cx + 1) * width;
            let mut rgb = [0.0f32; 3];
            for y in top / GRID_H..bottom.div_ceil(GRID_H) {
                let wy = ((y + 1) * GRID_H).min(bottom) - (y * GRID_H).max(top);
                for x in left / GRID_W..right.div_ceil(GRID_W) {
                    let wx = ((x + 1) * GRID_W).min(right) - (x * GRID_W).max(left);
                    let weight = (wx * wy) as f32;
                    let p = &data[y * stride + x * 4..y * stride + x * 4 + 4];
                    rgb[0] += table[p[ri] as usize] * weight;
                    rgb[1] += table[p[1] as usize] * weight;
                    rgb[2] += table[p[bi] as usize] * weight;
                }
            }
            let n = (width * height) as f32;
            cells.push(Cell::linear(rgb[0] / n, rgb[1] / n, rgb[2] / n));
        }
    }
    cells
}

#[derive(Clone, Copy)]
struct CellTrack {
    direction: [i8; 2],
    time: [f64; 2],
}

impl Default for CellTrack {
    fn default() -> Self {
        Self { direction: [0; 2], time: [f64::NEG_INFINITY; 2] }
    }
}

#[derive(Clone, Copy, Default)]
struct Rate {
    direction: i8,
    times: [f64; TRANSITIONS_PER_SECOND + 1],
    count: usize,
}

impl Rate {
    fn recent(&self, now: f64) -> usize {
        self.times[..self.count.min(self.times.len())].iter().filter(|&&t| t > now - 1.0 + TIME_EPSILON).count()
    }

    fn direction(&self, up: bool, down: bool) -> i8 {
        if self.direction != 1 && up {
            1
        } else if self.direction != -1 && down {
            -1
        } else {
            0
        }
    }

    fn commit(&mut self, direction: i8, now: f64) {
        if direction != 0 {
            self.times[self.count % self.times.len()] = now;
            self.count += 1;
            self.direction = direction;
        }
    }
}

#[derive(Clone, Default)]
pub struct Guard {
    cells: Vec<CellTrack>,
    rates: Vec<[Rate; 2]>,
    history: VecDeque<(f64, Vec<Cell>)>,
}

pub struct Assessment {
    pub flashing_cells: usize,
    pub over_limit: bool,
    transitions: Vec<[i8; 2]>,
    window_transitions: Vec<[i8; 2]>,
}

impl Guard {
    pub fn new() -> Self {
        Self { cells: vec![CellTrack::default(); CELLS], rates: vec![[Rate::default(); 2]; WINDOWS], history: VecDeque::new() }
    }

    pub fn assess(&self, cells: &[Cell], now: f64) -> Assessment {
        let mut transitions = vec![[0; 2]; CELLS];
        // Oldest comparisons are bounded in duration, but the immediately
        // previous displayed frame is ALWAYS tested, even after a long hold.
        for (age, (time, previous)) in self.history.iter().rev().enumerate() {
            if age != 0 && now - time > TRANSITION_SPAN + TIME_EPSILON {
                break;
            }
            for (i, c) in cells.iter().enumerate() {
                let p = previous[i];
                if transitions[i][0] == 0 {
                    let delta = c.luminance - p.luminance;
                    let direction = if delta > 0.0 { 1 } else { -1 };
                    if delta.abs() >= LUMINANCE_STEP && c.luminance.min(p.luminance) < DARK_LIMIT && direction != self.cells[i].direction[0] {
                        transitions[i][0] = direction;
                    }
                }
                if transitions[i][1] == 0 && (c.red || p.red) {
                    let distance = (c.u - p.u).powi(2) + (c.v - p.v).powi(2);
                    let direction = if c.red { 1 } else { -1 };
                    if distance > RED_DISTANCE_SQUARED && direction != self.cells[i].direction[1] {
                        transitions[i][1] = direction;
                    }
                }
            }
        }
        let mut window_transitions = vec![[0; 2]; WINDOWS];
        // Production presentation is capped at 60 Hz; approval uses 64 Hz.
        // An unexpected excess of samples fails closed rather than allocating.
        let mut over_limit = self.history.len() >= HISTORY_LIMIT && now - self.history[0].0 <= TRANSITION_SPAN + TIME_EPSILON;
        let mut flashing_cells = 0;
        for family in 0..2 {
            let mut areas = Vec::with_capacity(2);
            let mut current = Vec::with_capacity(2);
            for direction in [1, -1] {
                let mut mask = vec![false; CELLS];
                let mut new = vec![false; CELLS];
                for i in 0..CELLS {
                    new[i] = transitions[i][family] == direction;
                    mask[i] = if transitions[i][family] != 0 {
                        new[i]
                    } else {
                        self.cells[i].direction[family] == direction && now - self.cells[i].time[family] <= SYNC_SPAN + TIME_EPSILON
                    };
                }
                areas.push(window_areas(&mask));
                current.push(window_areas(&new));
            }
            for w in 0..WINDOWS {
                let up = areas[0][w] > AREA_LIMIT && current[0][w] > 0;
                let down = areas[1][w] > AREA_LIMIT && current[1][w] > 0;
                let direction = self.rates[w][family].direction(up, down);
                window_transitions[w][family] = direction;
                if direction != 0 && self.rates[w][family].recent(now) + 1 > TRANSITIONS_PER_SECOND {
                    over_limit = true;
                    flashing_cells = flashing_cells.max(areas[usize::from(direction < 0)][w]);
                }
            }
        }
        Assessment { flashing_cells, over_limit, transitions, window_transitions }
    }

    pub fn commit(&mut self, cells: &[Cell], now: f64, assessment: Assessment) {
        for (track, directions) in self.cells.iter_mut().zip(assessment.transitions) {
            for (family, direction) in directions.into_iter().enumerate() {
                if direction != 0 {
                    track.direction[family] = direction;
                    track.time[family] = now;
                }
            }
        }
        for (rates, directions) in self.rates.iter_mut().zip(assessment.window_transitions) {
            for (rate, direction) in rates.iter_mut().zip(directions) {
                rate.commit(direction, now);
            }
        }
        self.history.push_back((now, cells.to_vec()));
        while self.history.len() > HISTORY_LIMIT || (self.history.len() > 1 && now - self.history[0].0 > TRANSITION_SPAN + TIME_EPSILON) {
            self.history.pop_front();
        }
    }

    pub fn admit(&mut self, cells: &[Cell], now: f64) -> bool {
        let assessment = self.assess(cells, now);
        if assessment.over_limit {
            return false;
        }
        self.commit(cells, now, assessment);
        true
    }

    /// Exact relevant state at the approval path's binary-exact 64 Hz clock.
    /// Ages beyond each working window are canonicalized. Historical insertion
    /// slots and expired rate events cannot affect admission and are omitted.
    pub fn periodic_state(&self, tick: u64) -> Vec<i64> {
        let mut out = vec![2]; // policy/state format revision
        let age = |time: f64| tick as i64 - (time * 64.0) as i64;
        for track in &self.cells {
            for family in 0..2 {
                out.push(track.direction[family] as i64);
                out.push(if track.time[family].is_finite() && age(track.time[family]) <= 1 { age(track.time[family]) } else { -1 });
            }
        }
        for rates in &self.rates {
            for rate in rates {
                out.push(rate.direction as i64);
                let mut ages: Vec<_> = rate.times[..rate.count.min(rate.times.len())].iter().map(|&time| age(time)).filter(|a| *a < 64).collect();
                ages.sort_unstable();
                out.push(ages.len() as i64);
                out.extend(ages);
            }
        }
        out.push(self.history.len() as i64);
        for (time, cells) in &self.history {
            out.push(age(*time).min(5));
            for c in cells {
                out.extend([c.luminance.to_bits() as i64, c.u.to_bits() as i64, c.v.to_bits() as i64, i64::from(c.red)]);
            }
        }
        out
    }
}

fn window_areas(mask: &[bool]) -> Vec<usize> {
    let mut sum = vec![0usize; (GRID_W + 1) * (GRID_H + 1)];
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            sum[(y + 1) * (GRID_W + 1) + x + 1] =
                usize::from(mask[y * GRID_W + x]) + sum[y * (GRID_W + 1) + x + 1] + sum[(y + 1) * (GRID_W + 1) + x] - sum[y * (GRID_W + 1) + x];
        }
    }
    let mut areas = Vec::with_capacity(WINDOWS);
    for yy in 0..WINDOWS_H {
        let y = yy * WINDOW_STEP;
        for xx in 0..WINDOWS_W {
            let x = xx * WINDOW_STEP;
            let at = |xx: usize, yy: usize| sum[yy * (GRID_W + 1) + xx];
            areas.push(at(x + WINDOW_W, y + WINDOW_H) + at(x, y) - at(x, y + WINDOW_H) - at(x + WINDOW_W, y));
        }
    }
    areas
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uniform(rgb: [f32; 3]) -> Vec<Cell> {
        vec![Cell::linear(rgb[0], rgb[1], rgb[2]); CELLS]
    }

    fn violations(frames: impl IntoIterator<Item = Vec<Cell>>, fps: f64) -> usize {
        let mut g = Guard::new();
        frames
            .into_iter()
            .enumerate()
            .filter(|(i, cells)| {
                let a = g.assess(cells, *i as f64 / fps);
                let failed = a.over_limit;
                g.commit(cells, *i as f64 / fps, a);
                failed
            })
            .count()
    }

    #[test]
    fn flash_rate_amplitude_and_bright_state_controls() {
        for (period, a, b, fails) in [(15, 0.0, 1.0, false), (6, 0.0, 1.0, true), (3, 0.2, 0.25, false), (3, 0.85, 1.0, false)] {
            let frames = (0..120).map(|i| uniform([if (i / period) % 2 == 0 { a } else { b }; 3]));
            assert_eq!(violations(frames, 60.0) > 0, fails);
        }
    }

    #[test]
    fn three_flashes_per_second_pass_exact_sliding_boundaries() {
        let frames = (0..95).map(|f| uniform([if f == 0 { 0.0 } else { (((f - 1) / 5 + 1) % 2) as f32 }; 3]));
        assert_eq!(violations(frames, 30.0), 0);
    }

    #[test]
    fn trace_center_temporal_reference_cases() {
        let mut guard = Guard::new();
        let mut name = "";
        let mut expected = false;
        let mut failed = false;
        let mut frame = 0;
        let mut count = 0;
        for line in include_str!("../tests/data/flash-temporal.txt").lines().chain(std::iter::once("case END 0")) {
            if let Some(case) = line.strip_prefix("case ") {
                if !name.is_empty() {
                    assert_eq!(failed, expected, "reference case {name}");
                    count += 1;
                }
                let fields: Vec<_> = case.split_whitespace().collect();
                name = fields[0];
                expected = fields[1] == "1";
                failed = false;
                frame = 0;
                guard = Guard::new();
            } else {
                let rgb: Vec<usize> = line.split_whitespace().map(|c| c.parse().unwrap()).collect();
                let table = linear_table();
                let c = Cell::linear(table[rgb[0]], table[rgb[1]], table[rgb[2]]);
                let cells = vec![c; CELLS];
                let now = frame as f64 / 30.0;
                let assessment = guard.assess(&cells, now);
                failed |= assessment.over_limit;
                guard.commit(&cells, now, assessment);
                frame += 1;
            }
        }
        assert_eq!(count, 98);
    }

    #[test]
    fn sequential_small_patches_are_not_combined() {
        let frames = (0..90).map(|f| {
            let mut cells = uniform([0.0; 3]);
            let phase = f / 9;
            if phase < 3 && f % 2 == 1 {
                for i in phase * 14..(phase + 1) * 14 {
                    for dy in 0..8 {
                        for dx in 0..8 {
                            cells[((i / 16) * 8 + dy) * GRID_W + (i % 16) * 8 + dx] = Cell::linear(1.0, 1.0, 1.0);
                        }
                    }
                }
            }
            cells
        });
        assert_eq!(violations(frames, 90.0), 0);
    }

    #[test]
    fn near_sync_areas_combine_but_separated_areas_do_not() {
        for (offset, fails) in [(1, true), (4, false)] {
            let frames = (0usize..240).map(|f| {
                let mut cells = uniform([0.0; 3]);
                for i in 0..48 {
                    let shifted = if i < 24 { f } else { f.saturating_sub(offset) };
                    if (shifted / 12) % 2 == 1 {
                        for dy in 0..8 {
                            for dx in 0..8 {
                                cells[((i / 16) * 8 + dy) * GRID_W + (i % 16) * 8 + dx] = Cell::linear(1.0, 1.0, 1.0);
                            }
                        }
                    }
                }
                cells
            });
            assert_eq!(violations(frames, 120.0) > 0, fails, "offset {offset}");
        }
    }

    #[test]
    fn gradual_multi_frame_flashes_are_detected() {
        let frames = (0..256).map(|f| {
            let phase = f % 12;
            let v = match phase {
                0..=3 => phase as f32 * 0.04,
                4..=5 => 0.12,
                6..=9 => (9 - phase) as f32 * 0.04,
                _ => 0.0,
            };
            uniform([v; 3])
        });
        assert!(violations(frames, 64.0) > 0);
    }

    #[test]
    fn slow_ramps_do_not_accumulate_into_fast_transitions() {
        let frames = (0usize..240).map(|f| {
            let phase = f % 60;
            let v = if phase < 30 { phase as f32 } else { (59 - phase) as f32 } * 0.004;
            uniform([v; 3])
        });
        assert_eq!(violations(frames, 60.0), 0);
    }

    #[test]
    fn dim_red_intensity_changes_are_not_chromatic_flashes() {
        let frames = (0..120).map(|f| uniform([if f % 6 < 3 { 0.05 } else { 0.13 }, 0.0, 0.0]));
        assert_eq!(violations(frames, 60.0), 0);
        let chromatic = (0..120).map(|f| uniform(if f % 6 < 3 { [0.05, 0.0, 0.0] } else { [0.0, 0.01, 0.0] }));
        assert!(violations(chromatic, 60.0) > 0);
    }

    #[test]
    fn admission_after_holds_matches_rechecking_the_displayed_output() {
        let mut guard = Guard::new();
        let mut checker = Guard::new();
        let mut shown = uniform([0.0; 3]);
        let mut rejected = 0;
        for f in 0..240 {
            let wanted = uniform([if f % 6 < 3 { 0.0 } else { 1.0 }; 3]);
            let now = f as f64 / 60.0;
            if guard.admit(&wanted, now) {
                shown = wanted;
            } else {
                rejected += 1;
            }
            assert!(checker.admit(&shown, now), "displayed frame {f}");
        }
        assert!(rejected > 0);
    }

    #[test]
    fn every_pixel_is_measured_without_grid_aliasing() {
        let (w, h) = (1280, 720);
        let mut checker = vec![255u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let v = if (x + y) % 2 == 0 { 255 } else { 0 };
                checker[(y * w + x) * 4..(y * w + x) * 4 + 3].fill(v);
            }
        }
        let a = measure(&checker, w, h, w * 4, false);
        for p in checker.chunks_exact_mut(4) {
            for c in &mut p[..3] {
                *c = 255 - *c;
            }
        }
        let b = measure(&checker, w, h, w * 4, false);
        assert!(a.iter().zip(&b).all(|(a, b)| (a.luminance - b.luminance).abs() < 0.1));
        let mut holes = vec![255u8; w * h * 4];
        for y in (2..h).step_by(4) {
            for x in (2..w).step_by(4) {
                holes[(y * w + x) * 4..(y * w + x) * 4 + 3].fill(0);
            }
        }
        let on = measure(&holes, w, h, w * 4, false);
        let frames = (0..120).map(|f| if f % 6 < 3 { uniform([0.0; 3]) } else { on.clone() });
        assert!(violations(frames, 60.0) > 0);
    }

    #[test]
    fn channel_order_padding_and_tiny_frames() {
        let rgba = vec![255, 0, 0, 255];
        let bgra = vec![0, 0, 255, 255];
        let a = measure(&rgba, 1, 1, 4, false);
        let b = measure(&bgra, 1, 1, 4, true);
        assert_eq!(a, b);
        assert!(a.iter().all(|c| c.red));
        let padded = vec![255, 0, 0, 255, 0, 255, 0, 0];
        assert_eq!(a, measure(&padded, 1, 1, 8, false));
    }
}

/// Timing-independent upper bound for prepared playback. The presenter permits
/// at most 64 distinct frames per second, in order, with arbitrary holds.
/// Any runtime 66 ms comparison is therefore among the preceding four distinct
/// frames, and 20 ms synchrony can involve at most the preceding distinct frame.
/// Keep ALL possible directions (no phase cancellation), count every possibly
/// large frame (no alternation discount), and bound every 64-frame window.
/// This intentionally rejects some sequences that the live measure permits.
#[derive(Default)]
pub struct TimingBound {
    history: VecDeque<Vec<Cell>>,
    previous: Vec<[bool; 4]>,
    events: VecDeque<Vec<bool>>,
    counts: Vec<usize>,
}

pub struct BoundAssessment {
    pub over_limit: bool,
    possible: Vec<[bool; 4]>,
    events: Vec<bool>,
}

impl TimingBound {
    pub fn new() -> Self {
        Self { history: VecDeque::new(), previous: vec![[false; 4]; CELLS], events: VecDeque::new(), counts: vec![0; WINDOWS] }
    }

    pub fn assess(&self, cells: &[Cell]) -> BoundAssessment {
        let mut possible = vec![[false; 4]; CELLS];
        for previous in &self.history {
            for (i, c) in cells.iter().enumerate() {
                let p = previous[i];
                let delta = c.luminance - p.luminance;
                if delta.abs() >= LUMINANCE_STEP && c.luminance.min(p.luminance) < DARK_LIMIT {
                    possible[i][usize::from(delta < 0.0)] = true;
                }
                if (c.red || p.red) && (c.u - p.u).powi(2) + (c.v - p.v).powi(2) > RED_DISTANCE_SQUARED {
                    possible[i][2 + usize::from(!c.red)] = true;
                }
            }
        }
        let mut events = vec![false; WINDOWS];
        for direction in 0..4 {
            let mask: Vec<_> = possible.iter().zip(&self.previous).map(|(a, b)| a[direction] || b[direction]).collect();
            for (event, area) in events.iter_mut().zip(window_areas(&mask)) {
                *event |= area > AREA_LIMIT;
            }
        }
        let over_limit = events.iter().zip(&self.counts).any(|(&event, &count)| count + usize::from(event) > TRANSITIONS_PER_SECOND);
        BoundAssessment { over_limit, possible, events }
    }

    pub fn commit(&mut self, cells: &[Cell], assessment: BoundAssessment) {
        for (count, &event) in self.counts.iter_mut().zip(&assessment.events) {
            *count += usize::from(event);
        }
        self.events.push_back(assessment.events);
        if self.events.len() > 63 {
            for (count, event) in self.counts.iter_mut().zip(self.events.pop_front().unwrap()) {
                *count -= usize::from(event);
            }
        }
        self.previous = assessment.possible;
        self.history.push_back(cells.to_vec());
        if self.history.len() > 4 {
            self.history.pop_front();
        }
    }

    pub fn periodic_state(&self) -> Vec<i64> {
        let mut state = vec![self.history.len() as i64, self.events.len() as i64];
        for cells in &self.history {
            for c in cells {
                state.extend([c.luminance.to_bits() as i64, c.u.to_bits() as i64, c.v.to_bits() as i64, i64::from(c.red)]);
            }
        }
        state.extend(self.previous.iter().flat_map(|directions| directions.map(i64::from)));
        state.extend(self.events.iter().flat_map(|events| events.iter().map(|&event| i64::from(event))));
        state
    }
}
