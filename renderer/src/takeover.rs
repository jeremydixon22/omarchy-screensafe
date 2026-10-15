//! Switching Omarchy's own screensaver off while ScreenSafe stands in for it, and back on afterwards.
//!
//! Omarchy keeps its screensaver off while `~/.local/state/omarchy/toggles/screensaver-off` exists. ScreenSafe
//! records in its own state directory whether it created that file, and for which running instance, so it only
//! ever removes a switch it set itself, and an instance that has been replaced can't undo its successor's claim.
//! If you switch Omarchy's screensaver back on yourself while ScreenSafe holds it off, ScreenSafe lets go.

use std::io::Write;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use rustix::fs::{flock, openat, renameat, statat, unlinkat, AtFlags, FileType, FlockOperation, Mode, OFlags, CWD};

pub struct Paths {
    pub toggles: PathBuf,
    pub state: PathBuf,
    /// Omarchy's stay-awake switch.
    pub stay_awake: PathBuf,
}

impl Paths {
    /// Omarchy's toggles live under `$HOME/.local/state` whatever XDG_STATE_HOME says; ScreenSafe's state follows it.
    pub fn from_env() -> Result<Paths, String> {
        let home = std::env::var_os("HOME").map(PathBuf::from).filter(|p| p.is_absolute()).ok_or("HOME must be an absolute path")?;
        let state_home = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(".local/state"));
        Ok(Paths {
            toggles: home.join(".local/state/omarchy/toggles"),
            state: state_home.join("screensafe"),
            stay_awake: home.join(".local/state/omarchy/indicators/stay-awake"),
        })
    }
}

const TOGGLE: &str = "screensaver-off";
const MARKER: &str = "takeover";

#[derive(Debug, PartialEq)]
struct Marker {
    token: String,
    created: bool,
}

fn valid_token(token: &str) -> bool {
    !token.is_empty() && token.len() <= 64 && token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn dir(path: &Path, mode: u32) -> Result<OwnedFd, String> {
    std::fs::create_dir_all(path).map_err(|e| format!("can't create {}: {e}", path.display()))?;
    let fd = rustix::fs::open(path, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty())
        .map_err(|e| format!("can't open {}: {e}", path.display()))?;
    if mode != 0 {
        rustix::fs::fchmod(&fd, Mode::from_raw_mode(mode)).map_err(|e| format!("can't secure {}: {e}", path.display()))?;
    }
    Ok(fd)
}

fn is_file(dir: &OwnedFd, name: &str) -> Result<bool, String> {
    match statat(dir, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(st) if FileType::from_raw_mode(st.st_mode) == FileType::RegularFile => Ok(true),
        Ok(_) => Err(format!("{name} is not a regular file")),
        Err(rustix::io::Errno::NOENT) => Ok(false),
        Err(e) => Err(format!("can't check {name}: {e}")),
    }
}

fn read_marker(state: &OwnedFd) -> Result<Option<Marker>, String> {
    if !is_file(state, MARKER)? {
        return Ok(None);
    }
    let fd = openat(state, MARKER, OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty()).map_err(|e| format!("can't read the marker: {e}"))?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut std::io::Read::take(std::fs::File::from(fd), 256), &mut text).map_err(|e| format!("can't read the marker: {e}"))?;
    let mut lines = text.lines();
    let token = lines.next().unwrap_or("").to_string();
    let created = lines.next() == Some("created");
    Ok(valid_token(&token).then_some(Marker { token, created }))
}

fn write_marker(state: &OwnedFd, marker: &Marker) -> Result<(), String> {
    let tmp = format!("{MARKER}.{}", std::process::id());
    let _ = unlinkat(state, tmp.as_str(), AtFlags::empty());
    let fd = openat(state, tmp.as_str(), OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::from_raw_mode(0o600))
        .map_err(|e| format!("can't write the marker: {e}"))?;
    let mut file = std::fs::File::from(fd);
    write!(file, "{}\n{}\n", marker.token, if marker.created { "created" } else { "found" }).map_err(|e| format!("can't write the marker: {e}"))?;
    file.sync_all().map_err(|e| format!("can't write the marker: {e}"))?;
    renameat(state, tmp.as_str(), state, MARKER).map_err(|e| format!("can't write the marker: {e}"))
}

fn remove(dir: &OwnedFd, name: &str) -> Result<(), String> {
    match unlinkat(dir, name, AtFlags::empty()) {
        Ok(()) | Err(rustix::io::Errno::NOENT) => Ok(()),
        Err(e) => Err(format!("can't remove {name}: {e}")),
    }
}

/// Holds an exclusive lock on the state directory while the toggle and marker change.
fn locked(paths: &Paths) -> Result<(OwnedFd, OwnedFd, OwnedFd), String> {
    let state = dir(&paths.state, 0o700)?;
    let lock = openat(&state, "lock", OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::from_raw_mode(0o600))
        .map_err(|e| format!("can't open the lock: {e}"))?;
    flock(&lock, FlockOperation::LockExclusive).map_err(|e| format!("can't take the lock: {e}"))?;
    let toggles = dir(&paths.toggles, 0)?;
    Ok((state, toggles, lock))
}

/// Takes over from Omarchy's screensaver for the instance `token`. Answers "owned" when ScreenSafe switched it off,
/// "user-off" when it was already off, and "user-on" when you switched it back on since ScreenSafe's last claim.
pub fn claim(paths: &Paths, token: &str) -> Result<&'static str, String> {
    if !valid_token(token) {
        return Err("the token must be 1 to 64 letters, digits or dashes".into());
    }
    let (state, toggles, _lock) = locked(paths)?;
    let off = is_file(&toggles, TOGGLE)?;
    match read_marker(&state)? {
        Some(marker) if !off => {
            let _ = marker;
            remove(&state, MARKER)?;
            Ok("user-on")
        }
        Some(marker) => {
            write_marker(&state, &Marker { token: token.into(), created: marker.created })?;
            Ok(if marker.created { "owned" } else { "user-off" })
        }
        None if off => {
            write_marker(&state, &Marker { token: token.into(), created: false })?;
            Ok("user-off")
        }
        None => {
            // Record the claim before creating the switch, so a failure part-way never leaves a switch nobody owns.
            write_marker(&state, &Marker { token: token.into(), created: true })?;
            let fd = openat(&toggles, TOGGLE, OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::from_raw_mode(0o644))
                .map_err(|e| format!("can't switch Omarchy's screensaver off: {e}"))?;
            drop(fd);
            Ok("owned")
        }
    }
}

/// Hands the screensaver back for `token` (or for whichever instance holds it, when `token` is None).
pub fn release(paths: &Paths, token: Option<&str>) -> Result<&'static str, String> {
    let (state, toggles, _lock) = locked(paths)?;
    let Some(marker) = read_marker(&state)? else { return Ok("released") };
    if token.is_some_and(|t| t != marker.token) {
        return Ok("not-ours");
    }
    if marker.created && is_file(&toggles, TOGGLE)? {
        remove(&toggles, TOGGLE)?;
    }
    remove(&state, MARKER)?;
    Ok("released")
}

/// `{"held":…,"created":…,"omarchyOff":…,"stayAwake":…}`
pub fn status(paths: &Paths) -> Result<String, String> {
    let marker = match rustix::fs::open(&paths.state, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty()) {
        Ok(state) => read_marker(&state)?,
        Err(_) => None,
    };
    let off = match statat(CWD, paths.toggles.join(TOGGLE), AtFlags::SYMLINK_NOFOLLOW) {
        Ok(st) => FileType::from_raw_mode(st.st_mode) == FileType::RegularFile,
        Err(_) => false,
    };
    let stay_awake = statat(CWD, &paths.stay_awake, AtFlags::SYMLINK_NOFOLLOW).is_ok();
    Ok(format!(
        "{{\"held\":{},\"created\":{},\"omarchyOff\":{},\"stayAwake\":{}}}",
        marker.is_some(),
        marker.as_ref().is_some_and(|m| m.created),
        off,
        stay_awake
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(name: &str) -> Paths {
        let root = std::env::temp_dir().join(format!("screensafe-takeover-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Paths { toggles: root.join("omarchy/toggles"), state: root.join("screensafe"), stay_awake: root.join("omarchy/indicators/stay-awake") }
    }

    fn toggle_exists(p: &Paths) -> bool {
        p.toggles.join(TOGGLE).exists()
    }

    #[test]
    fn claims_and_releases_its_own_switch() {
        let p = paths("own");
        assert_eq!(claim(&p, "a1").unwrap(), "owned");
        assert!(toggle_exists(&p));
        assert_eq!(claim(&p, "b2").unwrap(), "owned", "a new instance takes over the claim");
        assert_eq!(release(&p, Some("a1")).unwrap(), "not-ours", "the old instance can't undo it");
        assert!(toggle_exists(&p));
        assert_eq!(status(&p).unwrap(), "{\"held\":true,\"created\":true,\"omarchyOff\":true,\"stayAwake\":false}");
        assert_eq!(release(&p, Some("b2")).unwrap(), "released");
        assert!(!toggle_exists(&p));
        assert_eq!(release(&p, None).unwrap(), "released");
        let mode = std::fs::metadata(&p.state).unwrap();
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode.permissions()) & 0o777, 0o700);
    }

    #[test]
    fn leaves_your_own_switch_alone() {
        let p = paths("user");
        std::fs::create_dir_all(&p.toggles).unwrap();
        std::fs::write(p.toggles.join(TOGGLE), "").unwrap();
        assert_eq!(claim(&p, "a1").unwrap(), "user-off");
        assert_eq!(release(&p, Some("a1")).unwrap(), "released");
        assert!(toggle_exists(&p), "a switch you set stays set");
    }

    #[test]
    fn steps_aside_when_you_switch_omarchy_back_on() {
        let p = paths("back");
        assert_eq!(claim(&p, "a1").unwrap(), "owned");
        std::fs::remove_file(p.toggles.join(TOGGLE)).unwrap();
        assert_eq!(claim(&p, "a1").unwrap(), "user-on");
        assert!(!toggle_exists(&p));
        assert_eq!(status(&p).unwrap(), "{\"held\":false,\"created\":false,\"omarchyOff\":false,\"stayAwake\":false}");
    }

    #[test]
    fn refuses_links_and_bad_tokens() {
        let p = paths("links");
        std::fs::create_dir_all(&p.toggles).unwrap();
        std::os::unix::fs::symlink("/etc/hostname", p.toggles.join(TOGGLE)).unwrap();
        assert!(claim(&p, "a1").is_err());
        assert!(claim(&p, "a b").is_err());
        assert!(claim(&p, "").is_err());
    }
}
