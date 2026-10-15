//! Reading pack files without following links out of the pack.
//!
//! Every file is opened beneath an already-open directory with `openat2`, refusing symbolic links, magic links and
//! other mounts anywhere in the path, and is read only up to a size limit.

use std::fs::File;
use std::io::Read;
use std::os::fd::OwnedFd;
use std::path::Path;

use rustix::fs::{openat2, Mode, OFlags, ResolveFlags, CWD};

#[derive(Debug)]
pub struct FsError(pub String);

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub struct Dir {
    fd: OwnedFd,
    pub display: String,
}

const READ_FLAGS: OFlags = OFlags::RDONLY.union(OFlags::CLOEXEC).union(OFlags::NOCTTY).union(OFlags::NONBLOCK);

const BENEATH: ResolveFlags = ResolveFlags::BENEATH.union(ResolveFlags::NO_SYMLINKS).union(ResolveFlags::NO_MAGICLINKS).union(ResolveFlags::NO_XDEV);

impl Dir {
    /// Opens a directory the user named. The path itself may contain links; nothing opened through it may.
    pub fn open(path: &Path) -> Result<Dir, FsError> {
        let fd = rustix::fs::open(path, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC, Mode::empty())
            .map_err(|e| FsError(format!("can't open {}: {e}", path.display())))?;
        Ok(Dir { fd, display: path.display().to_string() })
    }

    /// Opens `rel` beneath this directory without following any link.
    fn open_beneath(&self, rel: &str, flags: OFlags) -> Result<OwnedFd, FsError> {
        check_relative(rel)?;
        match openat2(&self.fd, rel, flags, Mode::empty(), BENEATH) {
            Err(rustix::io::Errno::NOSYS) => self.walk(rel, flags),
            result => result.map_err(|e| FsError(format!("can't open {}/{rel}: {e}", self.display))),
        }
    }

    /// Where `openat2` is unavailable (kernels before 5.6, some seccomp sandboxes, user-mode emulators), the checked
    /// path is walked one plain component at a time, each opened without following links. That keeps the same
    /// guarantee, except that the walk may cross into another mount.
    fn walk(&self, rel: &str, flags: OFlags) -> Result<OwnedFd, FsError> {
        let fail = |e: rustix::io::Errno| FsError(format!("can't open {}/{rel}: {e}", self.display));
        let parts: Vec<&str> = rel.split('/').collect();
        let (last, folders) = parts.split_last().ok_or_else(|| FsError(format!("'{rel}' is empty")))?;
        let mut dir: Option<OwnedFd> = None;
        for part in folders {
            let parent = dir.as_ref().unwrap_or(&self.fd);
            dir =
                Some(rustix::fs::openat(parent, *part, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty()).map_err(fail)?);
        }
        rustix::fs::openat(dir.as_ref().unwrap_or(&self.fd), *last, flags | OFlags::NOFOLLOW, Mode::empty()).map_err(fail)
    }

    pub fn subdir(&self, rel: &str) -> Result<Dir, FsError> {
        let fd = self.open_beneath(rel, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC)?;
        Ok(Dir { fd, display: format!("{}/{rel}", self.display) })
    }

    /// Reads a regular file beneath this directory, failing if it is larger than `limit` bytes.
    pub fn read(&self, rel: &str, limit: u64) -> Result<Vec<u8>, FsError> {
        let fd = self.open_beneath(rel, READ_FLAGS)?;
        read_regular(fd, limit, &format!("{}/{rel}", self.display))
    }

    /// `read` through the fallback walk, so tests check it on kernels that have `openat2`.
    #[cfg(test)]
    fn read_walking(&self, rel: &str, limit: u64) -> Result<Vec<u8>, FsError> {
        check_relative(rel)?;
        read_regular(self.walk(rel, READ_FLAGS)?, limit, rel)
    }

    /// Names of the regular files directly in this directory, sorted, at most `limit` of them.
    pub fn files(&self, limit: usize) -> Result<Vec<String>, FsError> {
        // A fresh descriptor, so each listing starts at the beginning.
        let fresh = rustix::fs::openat(&self.fd, ".", OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC, Mode::empty())
            .map_err(|e| FsError(format!("can't list {}: {e}", self.display)))?;
        let mut dir = rustix::fs::Dir::new(fresh).map_err(|e| FsError(format!("can't list {}: {e}", self.display)))?;
        let mut names = Vec::new();
        for entry in dir.by_ref() {
            let entry = entry.map_err(|e| FsError(format!("can't list {}: {e}", self.display)))?;
            if entry.file_type() != rustix::fs::FileType::RegularFile {
                continue;
            }
            let Ok(name) = entry.file_name().to_str() else { continue };
            if check_relative(name).is_ok() {
                names.push(name.to_string());
                if names.len() > limit {
                    return Err(FsError(format!("{} holds more than {limit} files", self.display)));
                }
            }
        }
        names.sort();
        Ok(names)
    }
}

/// Reads a single file the user named, refusing a link in its last component.
pub fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>, FsError> {
    let fd = rustix::fs::openat(CWD, path, OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NOCTTY | OFlags::NONBLOCK, Mode::empty())
        .map_err(|e| FsError(format!("can't open {}: {e}", path.display())))?;
    read_regular(fd, limit, &path.display().to_string())
}

fn read_regular(fd: OwnedFd, limit: u64, name: &str) -> Result<Vec<u8>, FsError> {
    let stat = rustix::fs::fstat(&fd).map_err(|e| FsError(format!("can't read {name}: {e}")))?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile {
        return Err(FsError(format!("{name} is not a regular file")));
    }
    if stat.st_size as u64 > limit {
        return Err(FsError(format!("{name} is larger than {} KiB", limit / 1024)));
    }
    let mut bytes = Vec::with_capacity(stat.st_size as usize);
    File::from(fd).take(limit + 1).read_to_end(&mut bytes).map_err(|e| FsError(format!("can't read {name}: {e}")))?;
    if bytes.len() as u64 > limit {
        return Err(FsError(format!("{name} is larger than {} KiB", limit / 1024)));
    }
    Ok(bytes)
}

/// A pack path: up to four plain components of letters, digits, '.', '_' and '-', none starting with '.'.
pub fn check_relative(rel: &str) -> Result<(), FsError> {
    let bad = || FsError(format!("'{rel}' is not a plain relative path inside the pack"));
    if rel.is_empty() || rel.len() > 128 {
        return Err(bad());
    }
    let parts: Vec<&str> = rel.split('/').collect();
    if parts.len() > 4 {
        return Err(bad());
    }
    for part in parts {
        if part.is_empty() || part.starts_with('.') || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-') {
            return Err(bad());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("screensafe-safefs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn relative_paths() {
        assert!(check_relative("art.txt").is_ok());
        assert!(check_relative("img/a-b_c.webp").is_ok());
        for bad in ["", "../x", "a/../b", "/etc/passwd", ".hidden", "a//b", "a b", "a/b/c/d/e", "a\\b", "é"] {
            assert!(check_relative(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn refuses_links_and_limits_size() {
        let dir = temp("links");
        std::fs::write(dir.join("ok.txt"), "hello").unwrap();
        std::fs::write(dir.join("big.txt"), vec![b'x'; 2000]).unwrap();
        symlink("/etc/hostname", dir.join("link.txt")).unwrap();
        std::fs::create_dir(dir.join("sub")).unwrap();
        symlink("..", dir.join("sub/up")).unwrap();
        let d = Dir::open(&dir).unwrap();
        assert_eq!(d.read("ok.txt", 100).unwrap(), b"hello");
        assert!(d.read("big.txt", 1000).unwrap_err().0.contains("larger"));
        assert!(d.read("link.txt", 1000).is_err());
        assert!(d.read("sub/up/ok.txt", 1000).is_err());
        // The fallback walk refuses the same links.
        assert_eq!(d.read_walking("ok.txt", 100).unwrap(), b"hello");
        assert!(d.read_walking("link.txt", 1000).is_err());
        assert!(d.read_walking("sub/up/ok.txt", 1000).is_err());
        std::fs::write(dir.join("sub/inner.txt"), "in").unwrap();
        assert_eq!(d.read_walking("sub/inner.txt", 100).unwrap(), b"in");
        symlink(dir.join("sub"), dir.join("sublink")).unwrap();
        assert!(d.read_walking("sublink/inner.txt", 100).is_err(), "a linked folder is refused too");
        assert!(d.read("sublink/inner.txt", 100).is_err());
        assert!(d.read("sub", 1000).is_err());
        assert_eq!(d.files(10).unwrap(), vec!["big.txt", "ok.txt"]);
        assert!(d.files(1).is_err());
        assert!(read_file(&dir.join("link.txt"), 100).is_err());
        assert_eq!(read_file(&dir.join("ok.txt"), 100).unwrap(), b"hello");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn refuses_fifos_without_blocking() {
        let dir = temp("fifo");
        let path = dir.join("pipe");
        rustix::fs::mknodat(CWD, &path, rustix::fs::FileType::Fifo, Mode::from_raw_mode(0o600), 0).unwrap();
        let d = Dir::open(&dir).unwrap();
        assert!(d.read("pipe", 100).unwrap_err().0.contains("not a regular file"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
