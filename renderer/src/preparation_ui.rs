//! Trusted preparation feedback, composed before the normal combined-frame guard.
//! Progress is advisory and never grants authority to play cached media.
use crate::eval::{FontMetrics, Frame, Glyph, Item};
use crate::raster::View;

pub struct Preparation {
    visible: bool,
    value: u16,
    phase: &'static str,
    finished: Option<f64>,
    replacement: bool,
    decided: bool,
    began: Option<f64>,
    work: f64,
    samples: u32,
    previous: Option<f64>,
    cover_presented: Option<f64>,
    sampling_started: bool,
}

impl Preparation {
    pub fn new() -> Self {
        Self {
            visible: true,
            value: 0,
            phase: "Checking readiness",
            finished: None,
            replacement: true,
            decided: false,
            began: None,
            work: 0.0,
            samples: 0,
            previous: None,
            cover_presented: None,
            sampling_started: false,
        }
    }
    pub fn active(&self, time: f64) -> bool {
        self.visible && self.finished.is_none_or(|at| time - at < 2.5)
    }
    pub fn replacement(&self, time: f64) -> bool {
        self.active(time) && self.replacement
    }
    /// The first submitted picture is feedback only. Samples begin on a later
    /// draw and are overwritten before the guarded surface is submitted.
    pub fn sample_pending(&self, time: f64) -> bool {
        self.active(time) && !self.decided && self.sampling_started
    }
    pub fn probing(&self, time: f64) -> bool {
        self.sample_pending(time) && self.cover_presented.is_some_and(|shown| time > shown)
    }
    pub fn control(&mut self, line: &str, time: f64) -> bool {
        let words: Vec<_> = line.split_whitespace().take(4).collect();
        if words.len() != 3 || words[0] != "preparation" {
            return false;
        }
        let Ok(value) = words[1].parse::<u16>() else { return false };
        if value > 1000 {
            return false;
        }
        if words[2] == "idle" {
            self.visible = false;
            self.finished = Some(time);
            return true;
        }
        if words[2] == "exclusive" {
            self.replacement = true;
            self.decided = true;
            say!("{{\"screensafePreparation\":{{\"mode\":\"replacement\"}}}}");
            self.visible = true;
            return true;
        }
        let phase = match words[2] {
            "planning" => "Checking readiness",
            "recording" => "Recording the loop",
            "software" => "Checking playback",
            "hardware" => "Checking accelerated playback",
            "finishing" => "Finishing",
            "ready" => "Ready for next time",
            "cancelled" => "Preparation cancelled",
            "unavailable" => "Live playback is available",
            _ => return false,
        };
        if self.finished.is_some() {
            return false;
        }
        self.visible = true;
        self.value = self.value.max(if words[2] == "ready" { value } else { value.min(999) });
        self.phase = phase;
        if ["recording", "software", "hardware"].contains(&words[2]) {
            self.sampling_started = true;
        }
        if ["ready", "cancelled", "unavailable"].contains(&words[2]) {
            self.finished = Some(time);
        }
        true
    }
    /// Classify actual rendering cost while background work runs, then
    /// retire the live view if the work no longer fits its frame budget.
    pub fn observe(&mut self, time: f64, work: f64, fps: f64, probed: bool) {
        if self.finished.is_some() || (self.decided && self.replacement) {
            return;
        }
        if !self.decided && !probed {
            if self.cover_presented.is_none() {
                self.cover_presented = Some(time);
                say!("{{\"screensafePreparation\":{{\"mode\":\"checking\"}}}}");
            }
            return;
        }
        let began = *self.began.get_or_insert(time);
        let gap = self.previous.replace(time).map(|previous| time - previous).unwrap_or(0.0);
        self.work += work;
        self.samples += 1;
        let budget = 1.0 / fps.clamp(1.0, 60.0);
        if (!self.decided && time - began >= 1.0 && self.samples >= fps.clamp(1.0, 8.0).ceil() as u32)
            || (self.decided && (work > budget * 0.85 || gap > budget * 2.5))
        {
            let replace = self.work / (self.samples as f64) > budget * 0.5 || (self.decided && (work > budget * 0.85 || gap > budget * 2.5));
            if !self.decided || replace {
                self.replacement = replace;
                self.decided = true;
                let mode = if replace { "replacement" } else { "overlay" };
                say!("{{\"screensafePreparation\":{{\"mode\":\"{mode}\"}}}}");
            }
        }
    }
    pub fn append(&self, frame: &mut Frame, time: f64, view: View, foreground: [f32; 4], metrics: FontMetrics, name: &str) {
        if !self.active(time) {
            return;
        }
        let width = view.width as f64 / view.scale;
        let height = view.height as f64 / view.scale;
        let panel = (width * 0.64).min(height * 1.0);
        let size = (height * 0.025).min(panel / 32.0);
        let x = -panel / 2.0 + (time / 70.0).sin() * 8.0 / view.scale;
        let y = if self.replacement { -size * 4.0 } else { height / 6.0 } + (time / 91.0).sin() * 8.0 / view.scale;
        let background = background(foreground, 0.96);
        rect(frame, x - size, y - size, panel + size * 2.0, size * 7.7, background);
        text(frame, "ScreenSafe", x, y, size * 1.0, foreground, metrics);
        let title = format!("Preparing {}", name.chars().take(48).collect::<String>());
        text(frame, &title, x, y + size * 1.6, size * 0.8, foreground, metrics);
        let phase = if self.decided { self.phase } else { "Checking performance" };
        text(frame, &format!("{}  {}%", phase, self.value / 10), x, y + size * 3.0, size * 0.66, foreground, metrics);
        rect(frame, x, y + size * 4.0, panel, size * 0.22, [foreground[0], foreground[1], foreground[2], 0.2]);
        rect(frame, x, y + size * 4.0, panel * self.value as f64 / 1000.0, size * 0.22, foreground);
        text(frame, "Move the pointer or press a key to cancel", x, y + size * 5.8, size * 0.52, foreground, metrics);
    }
}
pub fn background(foreground: [f32; 4], alpha: f32) -> [f32; 4] {
    let linear = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    let luminance = 0.2126 * linear(foreground[0]) + 0.7152 * linear(foreground[1]) + 0.0722 * linear(foreground[2]);
    if luminance > 0.179 {
        [0.025, 0.03, 0.04, alpha]
    } else {
        [0.95, 0.95, 0.96, alpha]
    }
}
fn rect(frame: &mut Frame, x: f64, y: f64, w: f64, h: f64, color: [f32; 4]) {
    frame.items.push(Item::Path {
        points: vec![(x as f32, y as f32), ((x + w) as f32, y as f32), ((x + w) as f32, (y + h) as f32), (x as f32, (y + h) as f32)],
        breaks: Vec::new(),
        closed: true,
        stroke: None,
        fill: Some(color),
    });
}
fn text(frame: &mut Frame, value: &str, x: f64, y: f64, size: f64, color: [f32; 4], metrics: FontMetrics) {
    let glyphs = value
        .chars()
        .enumerate()
        .map(|(i, ch)| Glyph { ch, x: (x + i as f64 * size * metrics.advance) as f32, y: (y + size * metrics.ascent) as f32, color })
        .collect();
    frame.items.push(Item::Glyphs { size: size as f32, cell_h: size as f32, font: 0, stretch: 1.0, glyphs });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preparation_colors_compensate_for_light_dark_and_saturated_theme_inks() {
        assert!(background([0.0, 1.0, 0.0, 1.0], 1.0)[0] < 0.1);
        assert!(background([0.0, 0.0, 1.0, 1.0], 1.0)[0] > 0.9);
        assert!(background([1.0, 1.0, 1.0, 1.0], 1.0)[0] < 0.1);
        assert!(background([0.0, 0.0, 0.0, 1.0], 1.0)[0] > 0.9);
    }
    #[test]
    fn progress_is_bounded_monotonic_and_terminal_only_by_explicit_control() {
        let mut ui = Preparation::new();
        assert!(!ui.control("preparation 1001 ready", 0.0));
        assert!(!ui.control("preparation NaN ready", 0.0));
        assert!(ui.control("preparation 400 recording", 0.0));
        assert!(ui.control("preparation 200 software", 1.0));
        assert_eq!(ui.value, 400);
        assert!(ui.control("preparation 1000 finishing", 2.0));
        assert_eq!(ui.value, 999);
        assert!(ui.control("preparation 1000 ready", 3.0));
        assert_eq!(ui.value, 1000);
        assert!(!ui.active(6.0));
    }
    #[test]
    fn feedback_precedes_sampling_and_a_low_rate_pack_can_be_revealed() {
        let mut ui = Preparation::new();
        assert!(ui.active(0.0) && ui.replacement(0.0));
        assert!(!ui.probing(0.0));
        ui.observe(0.0, 0.002, 1.0, false);
        assert!(!ui.probing(0.0));
        assert!(!ui.sample_pending(0.1), "wait for the actual background workload");
        assert!(ui.control("preparation 20 recording", 0.1));
        ui.observe(1.0, 0.003, 1.0, true);
        assert!(ui.replacement(1.0));
        ui.observe(2.0, 0.003, 1.0, true);
        assert!(ui.decided && !ui.replacement(2.0));
        assert!(ui.control("preparation 1000 ready", 2.0));
        assert!(!ui.active(5.0));
    }

    #[test]
    fn slow_live_work_selects_replacement_and_light_work_can_continue() {
        let mut light = Preparation::new();
        let mut heavy = Preparation::new();
        assert!(light.replacement(0.0));
        assert!(!light.probing(0.0));
        light.observe(0.0, 0.002, 30.0, false);
        heavy.observe(0.0, 0.002, 30.0, false);
        assert!(!light.probing(0.0));
        assert!(!light.probing(0.01));
        assert!(light.control("preparation 20 recording", 0.01));
        assert!(heavy.control("preparation 20 recording", 0.01));
        assert!(light.probing(0.02));
        for i in 1..=35 {
            let t = i as f64 / 30.0;
            light.observe(t, 0.004, 30.0, true);
            heavy.observe(t, 0.025, 30.0, true);
        }
        assert!(light.decided && !light.replacement);
        assert!(heavy.replacement);
        light.observe(1.3, 0.06, 30.0, false);
        assert!(light.replacement);
    }
}
