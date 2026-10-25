//! Trusted, opt-in information drawn after pack evaluation, before the combined-frame flash guard.
//! Only fixed clock fields and three numeric percentages enter this layer; packs cannot read them.

use crate::clock::Clock;
use crate::eval::{Frame, Glyph, Item};
use crate::raster::View;

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub enabled: bool,
    pub clock: bool,
    pub date: bool,
    pub battery: bool,
    pub cpu: bool,
    pub memory: bool,
    pub position: u8,
    pub size: u8,
    pub opacity: f32,
    pub color: u8,
    pub drift: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { enabled: false, clock: true, date: true, battery: false, cpu: false, memory: false, position: 8, size: 1, opacity: 0.8, color: 0, drift: true }
    }
}

fn pairs(text: &str) -> Result<Vec<(&str, f64)>, String> {
    if text.len() > 512 || !text.is_ascii() {
        return Err("overlay control exceeds its bounds".into());
    }
    let mut result = Vec::new();
    for pair in text.split(',').filter(|p| !p.is_empty()) {
        let (name, value) = pair.split_once('=').ok_or("overlay control needs name=number")?;
        let value = value.parse::<f64>().ok().filter(|n| n.is_finite()).ok_or("overlay value must be finite")?;
        if result.iter().any(|(key, _)| *key == name) || result.len() >= 11 {
            return Err("duplicate or excess overlay values".into());
        }
        result.push((name, value));
    }
    Ok(result)
}

impl Config {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut c = Self::default();
        for (key, value) in pairs(text)? {
            let toggle = || match value {
                0.0 => Ok(false),
                1.0 => Ok(true),
                _ => Err("overlay toggle must be 0 or 1".to_string()),
            };
            let choice =
                |max: u8| if value.fract() == 0.0 && (0.0..=max as f64).contains(&value) { Ok(value as u8) } else { Err("invalid overlay choice".to_string()) };
            match key {
                "enabled" => c.enabled = toggle()?,
                "clock" => c.clock = toggle()?,
                "date" => c.date = toggle()?,
                "battery" => c.battery = toggle()?,
                "cpu" => c.cpu = toggle()?,
                "memory" => c.memory = toggle()?,
                "drift" => c.drift = toggle()?,
                "position" => c.position = choice(8)?,
                "size" => c.size = choice(2)?,
                "color" => c.color = choice(2)?,
                "opacity" if (0.2..=1.0).contains(&value) => c.opacity = value as f32,
                _ => return Err("unknown or invalid overlay setting".into()),
            }
        }
        Ok(c)
    }
}

pub struct Overlay {
    pub config: Config,
    clock: Clock,
    stats: [f64; 3],
    updated: Option<f64>,
}

pub fn drift(time: f64) -> (f64, f64) {
    let time = if time.is_finite() { time } else { 0.0 };
    (10.0 * (time * std::f64::consts::TAU / 1020.0).sin(), 10.0 * (time * std::f64::consts::TAU / 1380.0).sin())
}

impl Overlay {
    pub fn new(config: Config, clock: Clock) -> Self {
        Self { config, clock, stats: [-1.0; 3], updated: None }
    }

    pub fn control(&mut self, line: &str, time: f64) -> bool {
        if let Some(text) = line.strip_prefix("overlay ") {
            if let Ok(config) = Config::parse(text) {
                self.config = config;
                return true;
            }
        } else if let Some(text) = line.strip_prefix("stats ") {
            if let Ok(values) = pairs(text) {
                let mut stats = [-1.0; 3];
                for (key, value) in values {
                    let index = match key {
                        "cpu" => 0,
                        "memory" => 1,
                        "battery" => 2,
                        _ => return false,
                    };
                    if value != -1.0 && !(0.0..=100.0).contains(&value) {
                        return false;
                    }
                    stats[index] = value;
                }
                self.stats = stats;
                self.updated = Some(time);
                return true;
            }
        }
        false
    }

    pub fn append(&self, frame: &mut Frame, time: f64, view: View, physical_scale: f64, foreground: [f32; 4], metrics: crate::eval::FontMetrics) {
        let c = &self.config;
        if !c.enabled {
            return;
        }
        let sy = view.height as f64 / view.scale;
        let sx = view.width as f64 / view.scale;
        let size = sy * 0.038 * [0.75, 1.0, 1.3][c.size as usize];
        let mut lines = Vec::new();
        let r = self.clock.at(time);
        if c.clock {
            lines.push((format!("{:02}:{:02}", r.hour, r.minute), size));
        }
        if c.date {
            lines.push((
                format!(
                    "{} {:02} {} {}",
                    ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][r.weekday as usize],
                    r.day,
                    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][r.month as usize - 1],
                    r.year
                ),
                size * 0.48,
            ));
        }
        let stats = if self.updated.is_some_and(|at| time >= at && time - at <= 6.0) { self.stats } else { [-1.0; 3] };
        let mut labels = Vec::new();
        for (enabled, label, value) in [(c.battery, "Battery", stats[2]), (c.cpu, "CPU", stats[0]), (c.memory, "Memory", stats[1])] {
            if enabled {
                labels.push(if value < 0.0 { format!("{label} --") } else { format!("{label} {:.0}%", value) });
            }
        }
        // Keep narrow portrait outputs readable: each system reading has its own short line.
        for label in labels {
            lines.push((label, size * 0.44));
        }
        if lines.is_empty() {
            return;
        }
        let physical_scale = physical_scale.clamp(1.0, 65536.0);
        let (dx, dy) = if c.drift {
            let (x, y) = drift(time);
            (x.round(), y.round())
        } else {
            (0.0, 0.0)
        };
        let margin = (sy * 0.04 + 12.0 / physical_scale).min(sx * 0.2);
        let total: f64 = lines.iter().map(|(_, size)| size * 1.35).sum();
        let row = c.position / 3;
        let mut y = match row {
            0 => -sy / 2.0 + margin,
            1 => -total / 2.0,
            _ => sy / 2.0 - margin - total,
        } + dy / physical_scale;
        let col = c.position % 3;
        let mut color = match c.color {
            1 => [0.94, 0.94, 0.94, 1.0],
            2 => [0.08, 0.08, 0.08, 1.0],
            _ => foreground,
        };
        color[3] = c.opacity;
        for (text, font_size) in lines {
            let font_size = font_size.min((sx - 2.0 * margin) / (text.chars().count().max(1) as f64 * metrics.advance));
            let width = text.chars().count() as f64 * font_size * metrics.advance;
            let x = match col {
                0 => -sx / 2.0 + margin,
                1 => -width / 2.0,
                _ => sx / 2.0 - margin - width,
            } + dx / physical_scale;
            let baseline = y + font_size * metrics.ascent;
            let glyphs: Vec<_> = text
                .chars()
                .enumerate()
                .map(|(i, ch)| Glyph { ch, x: (x + i as f64 * font_size * metrics.advance) as f32, y: baseline as f32, color })
                .collect();
            let shadow = glyphs
                .iter()
                .map(|g| Glyph {
                    x: g.x + (1.0 / physical_scale) as f32,
                    y: g.y + (1.0 / physical_scale) as f32,
                    color: [0.0, 0.0, 0.0, c.opacity * 0.65],
                    ..g.clone()
                })
                .collect();
            for glyphs in [shadow, glyphs] {
                frame.items.push(Item::Glyphs { size: font_size as f32, cell_h: font_size as f32, font: 0, stretch: 1.0, glyphs });
            }
            y += font_size * 1.35;
        }
    }
}

/// Fixed allow-list collector. Opens only anonymous CPU/memory aggregates, then confines itself
/// completely before reading its retained descriptors. Never runs in the rendering process.
pub fn system_info() -> Result<String, String> {
    use std::io::Read;
    let open = |path| {
        rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::NONBLOCK,
            rustix::fs::Mode::empty(),
        )
        .map(std::fs::File::from)
        .map_err(|e| format!("can't read system aggregates: {e}"))
    };
    let cpu = open("/proc/stat")?;
    let memory = open("/proc/meminfo")?;
    crate::sandbox::confine()?;
    let read = |file: std::fs::File, limit| {
        let mut text = String::new();
        file.take(limit).read_to_string(&mut text).map_err(|e| e.to_string())?;
        Ok::<_, String>(text)
    };
    parse_system_info(&read(cpu, 4096)?, &read(memory, 16384)?)
}

fn parse_system_info(cpu: &str, memory: &str) -> Result<String, String> {
    let mut words = cpu.lines().next().unwrap_or("").split_whitespace();
    if words.next() != Some("cpu") {
        return Err("missing CPU aggregate".into());
    }
    let numbers: Vec<u64> = words.take(8).map(str::parse).collect::<Result<_, _>>().map_err(|_| "invalid CPU counters")?;
    if numbers.len() < 4 {
        return Err("missing CPU counters".into());
    }
    let total = numbers.iter().try_fold(0u64, |a, b| a.checked_add(*b)).ok_or("CPU counter overflow")?;
    let idle = numbers[3].saturating_add(numbers.get(4).copied().unwrap_or(0));
    let field = |name: &str| memory.lines().find_map(|line| line.strip_prefix(name)?.split_whitespace().next()?.parse::<u64>().ok());
    let mem_total = field("MemTotal:").filter(|n| *n > 0).ok_or("missing memory total")?;
    let available = field("MemAvailable:").filter(|n| *n <= mem_total).ok_or("missing memory availability")?;
    Ok(format!("{{\"total\":{total},\"idle\":{idle},\"memory\":{:.3}}}", 100.0 * (1.0 - available as f64 / mem_total as f64)))
}

/// One bounded read per ready turn; EOF disables control without ending a standalone `run`.
#[derive(Default)]
pub struct Input {
    line: Vec<u8>,
    over: bool,
    closed: bool,
}
impl Input {
    pub fn poll(&mut self) -> Vec<String> {
        use std::os::fd::AsFd;
        if self.closed {
            return Vec::new();
        }
        let stdin = std::io::stdin();
        let input = stdin.as_fd();
        let mut fds = [rustix::event::PollFd::new(&input, rustix::event::PollFlags::IN)];
        if !matches!(rustix::event::poll(&mut fds, Some(&rustix::event::Timespec { tv_sec: 0, tv_nsec: 0 })), Ok(n) if n > 0) {
            return Vec::new();
        }
        let mut bytes = [0; 512];
        let count = match rustix::io::read(input, &mut bytes) {
            Ok(0) => {
                self.closed = true;
                return Vec::new();
            }
            Ok(n) => n,
            Err(_) => return Vec::new(),
        };
        let mut lines = Vec::new();
        for &byte in &bytes[..count] {
            if byte == b'\n' {
                if !self.over {
                    if let Ok(text) = String::from_utf8(std::mem::take(&mut self.line)) {
                        lines.push(text);
                    }
                }
                self.line.clear();
                self.over = false;
            } else if self.line.len() < 512 {
                self.line.push(byte);
            } else {
                self.over = true;
            }
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_accepts_only_finite_known_numbers_and_preserves_state_on_rejection() {
        let mut layer = Overlay::new(Config::default(), Clock::fixed());
        assert!(!layer.config.enabled);
        assert!(layer.control("overlay enabled=1,clock=1,cpu=1,position=4,opacity=0.7", 0.0));
        let previous = layer.config.clone();
        for text in ["enabled=NaN", "enabled=2", "position=9", "opacity=-1", "clock=1,clock=0", "notification=123", "hostname=1", "enabled=$(id)"] {
            assert!(!layer.control(&format!("overlay {text}"), 1.0));
            assert_eq!(layer.config, previous);
        }
        assert!(!layer.control(&format!("overlay {}", "a".repeat(513)), 1.0));
        assert!(layer.control("stats cpu=12,memory=37,battery=82", 1.0));
        for text in ["cpu=NaN", "cpu=101", "memory=-2", "notification=1", "cpu=1,cpu=2"] {
            assert!(!layer.control(&format!("stats {text}"), 2.0));
            assert_eq!(layer.stats, [12.0, 37.0, 82.0]);
        }
    }

    #[test]
    fn system_counters_exclude_guest_time_and_reject_bad_or_missing_fields() {
        let memory = "MemTotal: 1000 kB\nMemAvailable: 250 kB\n";
        assert_eq!(parse_system_info("cpu 10 20 30 40 50 60 70 80 999 999\ncpu0 1000\n", memory).unwrap(), "{\"total\":360,\"idle\":90,\"memory\":75.000}");
        for cpu in ["cpu0 1 2 3 4", "cpu 1 2 bad 4", "cpu 1 2", "cpu 18446744073709551615 1 1 1"] {
            assert!(parse_system_info(cpu, memory).is_err());
        }
        for memory in ["MemTotal: 0\nMemAvailable: 0", "MemTotal: 1\nMemAvailable: 2", "MemTotal: 1000"] {
            assert!(parse_system_info("cpu 1 2 3 4", memory).is_err());
        }
    }

    #[test]
    fn drift_stays_bounded_and_slow_on_both_axes() {
        assert_eq!(drift(0.0), (0.0, 0.0));
        for second in 0..86400 {
            let (x, y) = drift(second as f64);
            let (nx, ny) = drift(second as f64 + 1.0);
            assert!(x.abs() <= 10.0 && y.abs() <= 10.0);
            assert!((nx - x).abs() < 0.062 && (ny - y).abs() < 0.046);
        }
    }

    #[test]
    fn overlay_is_opt_in_keeps_text_inside_each_anchor_and_expires_system_values() {
        let metrics = crate::raster::Raster::new().metrics()[0];
        for (w, h) in [(320, 200), (900, 1600), (3840, 2160), (64, 4096), (4096, 64)] {
            let view = View::new(w, h, crate::pack::Stage::Screen);
            let empty = Frame { background: [0.0; 4], items: Vec::new(), scene: 0, fps: 24.0 };
            let mut layer = Overlay::new(Config::default(), Clock::fixed());
            let mut frame = empty.clone();
            layer.append(&mut frame, 0.0, view, view.scale, [1.0; 4], metrics);
            assert_eq!(frame, empty);
            layer.control("overlay enabled=1,cpu=1,memory=1,battery=1,size=2", 0.0);
            layer.control("stats cpu=17,memory=38,battery=83", 0.0);
            for position in 0..9 {
                layer.config.position = position;
                let mut frame = empty.clone();
                layer.append(&mut frame, 3.0, view, view.scale, [1.0; 4], metrics);
                let mut words = String::new();
                for item in &frame.items {
                    if let Item::Glyphs { glyphs, .. } = item {
                        for g in glyphs {
                            assert!(g.x.abs() < w as f32 / view.scale as f32 / 2.0 && g.y.abs() < 0.5);
                            words.push(g.ch);
                        }
                    }
                }
                assert!(words.contains("10:08") && words.contains("17%") && words.contains("38%") && words.contains("83%"));
            }
            let mut stale = empty.clone();
            layer.append(&mut stale, 7.0, view, view.scale, [1.0; 4], metrics);
            let words: String = stale
                .items
                .iter()
                .filter_map(|i| if let Item::Glyphs { glyphs, .. } = i { Some(glyphs.iter().map(|g| g.ch).collect::<String>()) } else { None })
                .collect();
            assert!(words.contains("CPU --") && !words.contains("17%"));
        }
    }
}
