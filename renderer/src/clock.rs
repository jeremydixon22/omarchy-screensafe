//! The time of day, for packs that show a clock: the wall clock in the local time zone, worked out for each frame.
//!
//! The zone is read once, before the renderer confines itself: from `TZ` if it's set, or else from `/etc/localtime`,
//! whose link names the zone. Its rules, daylight saving included, then give the right offset for every moment,
//! so a screensaver left running overnight keeps the right time across a change. `check`, `render` and thumbnails
//! use a fixed moment instead, so their results never depend on when they were made.

use tz::TimeZone;

/// Where clock values come from.
#[derive(Clone)]
pub struct Clock {
    zone: TimeZone,
    /// The zone's name, as "Europe/Berlin", or "" when it has none.
    name: String,
    /// Unix seconds at the pack's time 0.
    start: f64,
}

/// The clock at one moment.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    /// 0 for Monday to 6 for Sunday.
    pub weekday: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// Seconds since local midnight, with their fraction: the smooth value for a clock's hands.
    pub clock: f64,
    /// Seconds since midnight in UTC, for showing other zones at a fixed offset.
    pub utc_clock: f64,
    /// The local offset from UTC, in seconds.
    pub offset: i32,
    /// The zone's abbreviation at this moment, as "CEST" or "+0530".
    pub abbreviation: String,
}

/// The moment `check`, `render` and thumbnails show: 10:08:30 on Thursday 15 January 2026, UTC.
pub const FIXED: f64 = 1_768_471_710.0;

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const WEEKDAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

impl Clock {
    /// The fixed clock: UTC, at `FIXED` when the pack's time is 0.
    pub fn fixed() -> Clock {
        Clock { zone: TimeZone::utc(), name: "UTC".into(), start: FIXED }
    }

    /// The local clock, with the pack's time 0 now. Falls back to UTC, noting why, if the zone can't be read.
    pub fn local(warnings: &mut Vec<String>) -> Clock {
        let start = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(FIXED);
        match local_zone() {
            Ok((zone, name)) => Clock { zone, name, start },
            Err(e) => {
                warnings.push(format!("the time zone can't be read ({e}); the clock shows UTC"));
                Clock { zone: TimeZone::utc(), name: "UTC".into(), start }
            }
        }
    }

    /// The same clock, reading now when the pack's time is `pack_time` rather than 0.
    pub fn starting_at(mut self, pack_time: f64) -> Clock {
        self.start -= pack_time;
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The clock `seconds` after the pack's time 0.
    pub fn at(&self, seconds: f64) -> Reading {
        let unix = self.start + seconds;
        let whole = unix.floor();
        let (offset, abbreviation) = match self.zone.find_local_time_type(whole as i64) {
            Ok(t) if t.time_zone_designation().is_empty() && t.ut_offset() == 0 => (0, "UTC".to_string()),
            Ok(t) => (t.ut_offset(), t.time_zone_designation().to_string()),
            Err(_) => (0, "UTC".to_string()),
        };
        let local = unix + offset as f64;
        let days = (local / 86400.0).floor();
        let clock = local - days * 86400.0;
        let (year, month, day) = civil(days as i64);
        let s = clock.floor() as u32;
        Reading {
            year,
            month,
            day,
            weekday: ((days as i64 + 3).rem_euclid(7)) as u32,
            hour: s / 3600,
            minute: s / 60 % 60,
            second: s % 60,
            clock,
            utc_clock: unix - (unix / 86400.0).floor() * 86400.0,
            offset,
            abbreviation,
        }
    }
}

impl Reading {
    pub fn month_name(&self) -> &'static str {
        MONTHS[(self.month.clamp(1, 12) - 1) as usize]
    }

    pub fn weekday_name(&self) -> &'static str {
        WEEKDAYS[self.weekday.min(6) as usize]
    }

    /// The offset as "+05:30", "-04:00" or "+00:00".
    pub fn offset_text(&self) -> String {
        let sign = if self.offset < 0 { '-' } else { '+' };
        let m = self.offset.unsigned_abs() / 60;
        format!("{sign}{:02}:{:02}", m / 60, m % 60)
    }
}

/// The year, month and day of a count of days since 1970-01-01 (Howard Hinnant's civil-from-days).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// The largest time zone file read.
const ZONE_BYTES: u64 = 1 << 20;

/// The local time zone and its name: `TZ`'s, if it's set, or `/etc/localtime`'s.
fn local_zone() -> Result<(TimeZone, String), String> {
    if let Ok(tz) = std::env::var("TZ") {
        let tz = tz.trim_start_matches(':');
        if valid_zone_name(tz) {
            if let Ok(bytes) = read_zone(&format!("/usr/share/zoneinfo/{tz}")) {
                return TimeZone::from_tz_data(&bytes).map(|z| (z, tz.to_string())).map_err(|e| e.to_string());
            }
        }
        if !tz.is_empty() && tz.len() <= 64 && tz.is_ascii() {
            if let Ok(zone) = TimeZone::from_posix_tz(tz) {
                return Ok((zone, String::new()));
            }
        }
    }
    let bytes = read_zone("/etc/localtime")?;
    let zone = TimeZone::from_tz_data(&bytes).map_err(|e| e.to_string())?;
    let name = std::fs::read_link("/etc/localtime")
        .ok()
        .and_then(|p| p.to_str().and_then(|s| s.split_once("zoneinfo/").map(|(_, n)| n.to_string())))
        .filter(|n| valid_zone_name(n))
        .unwrap_or_default();
    Ok((zone, name))
}

/// A zone name like "Europe/Berlin" or "America/Argentina/Buenos_Aires": plain names only, never a path out.
fn valid_zone_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.split('/').all(|part| !part.is_empty() && !part.starts_with('.') && part.chars().all(|c| c.is_ascii_alphanumeric() || "_+-".contains(c)))
}

fn read_zone(path: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut bytes = Vec::new();
    file.take(ZONE_BYTES).read_to_end(&mut bytes).map_err(|e| format!("{path}: {e}"))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fixed_clock_reads_ten_past_ten() {
        let r = Clock::fixed().at(0.0);
        assert_eq!((r.year, r.month, r.day, r.weekday), (2026, 1, 15, 3), "Thursday 15 January 2026");
        assert_eq!((r.hour, r.minute, r.second), (10, 8, 30));
        assert_eq!(r.offset_text(), "+00:00");
        assert_eq!(r.weekday_name(), "Thursday");
        assert_eq!(r.month_name(), "January");
        let later = Clock::fixed().at(3600.0 * 14.0);
        assert_eq!((later.day, later.hour, later.weekday), (16, 0, 4), "past midnight, it's Friday");
    }

    #[test]
    fn offsets_follow_daylight_saving() {
        // Berlin: CET (+1) in winter, CEST (+2) from the last Sunday of March.
        let zone = TimeZone::from_posix_tz("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
        let clock = Clock { zone, name: "Europe/Berlin".into(), start: FIXED };
        let winter = clock.at(0.0);
        assert_eq!((winter.hour, winter.offset_text(), winter.abbreviation.as_str()), (11, "+01:00".to_string(), "CET"));
        let summer = clock.at(180.0 * 86400.0);
        assert_eq!((summer.offset_text(), summer.abbreviation.as_str()), ("+02:00".to_string(), "CEST"));
        assert!((summer.utc_clock - winter.utc_clock).abs() < 1e-6, "UTC doesn't move with the zone");
    }

    #[test]
    fn half_hour_and_western_offsets() {
        let india = Clock { zone: TimeZone::fixed(19_800).unwrap(), name: String::new(), start: FIXED }.at(0.0);
        assert_eq!((india.hour, india.minute, india.offset_text()), (15, 38, "+05:30".to_string()));
        let west = Clock { zone: TimeZone::fixed(-4 * 3600).unwrap(), name: String::new(), start: FIXED }.at(0.0);
        assert_eq!((west.hour, west.offset_text()), (6, "-04:00".to_string()));
    }

    #[test]
    fn calendar_dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(-1), (1969, 12, 31));
        assert_eq!(civil(11_016), (2000, 2, 29));
        assert_eq!(civil(20_468), (2026, 1, 15));
    }

    #[test]
    fn zone_names_are_plain() {
        assert!(valid_zone_name("Europe/Berlin"));
        assert!(valid_zone_name("Etc/GMT+5"));
        assert!(!valid_zone_name("../etc/passwd"));
        assert!(!valid_zone_name("/etc/passwd"));
        assert!(!valid_zone_name(""));
    }
}
