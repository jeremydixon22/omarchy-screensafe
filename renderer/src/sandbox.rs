//! Confining the renderer, in two layers, with Landlock; `no_new_privs` is set first. On a kernel without Landlock the
//! renderer refuses to read any pack, rather than run unconfined.
//!
//! 1. Before it reads anything from a pack, [`confine_reading`]: the renderer can only read files beneath the folders
//!    and files it was given (the packs, the theme, the user's art), and can't write, run anything, reach the network,
//!    use abstract Unix sockets or signal other processes. So even the parsing of a pack is confined.
//! 2. Once the pack is loaded (and, for a screensaver, the Wayland connection is open), [`confine`]: no filesystem
//!    access at all. The renderer then can't open a file, run a program or reach the network, whatever a pack does.
//!    The panel's live preview stays at the first layer, since it reads its pack again when a setting changes.

use std::path::Path;

use landlock::{Access, AccessFs, AccessNet, BitFlags, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus, Scope, ABI};

/// How fully the kernel enforced a layer, or an error if it couldn't at all. An older kernel enforces only the kinds
/// of access it knows (always including reading files, Landlock's first), which is "partly confined".
fn enforced(status: RulesetStatus) -> Result<&'static str, String> {
    match status {
        RulesetStatus::FullyEnforced => Ok("fully confined"),
        RulesetStatus::PartiallyEnforced => Ok("partly confined (older kernel)"),
        RulesetStatus::NotEnforced => Err("can't confine the renderer: this kernel doesn't provide Landlock, so ScreenSafe won't read packs".into()),
    }
}

/// Denies creating Internet and other non-Unix sockets, including UDP, independently of Landlock ABI.
/// After Wayland connects, no new sockets or connections are needed. TSYNC covers the existing watchdog too.
fn confine_network(final_layer: bool) -> Result<(), String> {
    use seccompiler::{BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter, SeccompRule};
    let fail = |e: &dyn std::fmt::Display| format!("can't restrict network syscalls: {e}");
    let mut rules = std::collections::BTreeMap::new();
    let socket_rules = if final_layer {
        Vec::new()
    } else {
        vec![SeccompRule::new(vec![SeccompCondition::new(0, SeccompCmpArgLen::Dword, SeccompCmpOp::Ne, libc::AF_UNIX as u64).map_err(|e| fail(&e))?])
            .map_err(|e| fail(&e))?]
    };
    for nr in [libc::SYS_socket, libc::SYS_socketpair] {
        rules.insert(nr, socket_rules.clone());
    }
    // io_uring can create sockets without the socket syscall. No renderer path needs a ring.
    rules.insert(libc::SYS_io_uring_setup, Vec::new());
    if final_layer {
        rules.insert(libc::SYS_connect, Vec::new());
    }
    // x32 shares the x86-64 audit architecture but sets a syscall-number bit. Cover those entries too;
    // seccompiler's architecture check rejects the distinct i386 ABI.
    #[cfg(target_arch = "x86_64")]
    for (nr, chain) in rules.clone() {
        rules.insert(nr | 0x4000_0000, chain);
    }
    let arch = std::env::consts::ARCH.try_into().map_err(|e| fail(&e))?;
    let filter: BpfProgram = SeccompFilter::new(rules, SeccompAction::Allow, SeccompAction::Errno(libc::EPERM as u32), arch)
        .map_err(|e| fail(&e))?
        .try_into()
        .map_err(|e| fail(&e))?;
    seccompiler::apply_filter_all_threads(&filter).map_err(|e| fail(&e))
}

/// The first layer: reading only, beneath `paths`. A path that doesn't exist is left out.
pub fn confine_reading(paths: &[&Path]) -> Result<&'static str, String> {
    confine_network(false)?;
    let abi = ABI::V6;
    let fail = |e: landlock::RulesetError| format!("can't confine the renderer: {e}");
    let mut ruleset = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))
        .and_then(|r| r.handle_access(AccessNet::from_all(abi)))
        .and_then(|r| r.scope(Scope::from_all(abi)))
        .and_then(|r| r.create())
        .map_err(fail)?;
    for path in paths {
        let Ok(meta) = std::fs::metadata(path) else { continue };
        let access: BitFlags<AccessFs> = if meta.is_dir() { AccessFs::ReadFile | AccessFs::ReadDir } else { AccessFs::ReadFile.into() };
        let Ok(fd) = PathFd::new(path) else { continue };
        ruleset = ruleset.add_rule(PathBeneath::new(fd, access)).map_err(fail)?;
    }
    let status = ruleset.restrict_self().map_err(fail)?;
    enforced(status.ruleset)
}

/// The second layer: no filesystem access at all. Returns how fully the kernel enforced it.
pub fn confine() -> Result<&'static str, String> {
    confine_network(true)?;
    confine_watchdog()
}

/// Watchdogs need only existing stderr, atomics and timers. Restrict their own filesystem access before
/// acknowledging startup: Landlock is per-thread, whereas network seccomp uses TSYNC for all threads.
pub fn confine_watchdog() -> Result<&'static str, String> {
    let abi = ABI::V6;
    let status = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))
        .and_then(|r| r.handle_access(AccessNet::from_all(abi)))
        .and_then(|r| r.scope(Scope::from_all(abi)))
        .and_then(|r| r.create())
        .and_then(|r| r.restrict_self())
        .map_err(|e| format!("can't confine the renderer: {e}"))?;
    let _ = rustix::process::setrlimit(rustix::process::Resource::Core, rustix::process::Rlimit { current: Some(0), maximum: Some(0) });
    enforced(status.ruleset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_policy_blocks_udp_and_preserves_existing_unix_io() {
        if std::env::var_os("SCREENSAFE_NETWORK_CHILD").is_none() {
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "sandbox::tests::network_policy_blocks_udp_and_preserves_existing_unix_io", "--nocapture"])
                .env("SCREENSAFE_NETWORK_CHILD", "1")
                .output()
                .unwrap();
            assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
            return;
        }
        use std::io::{Read, Write};
        use std::net::{TcpListener, UdpSocket};
        use std::os::unix::net::UnixStream;
        let (send, receive) = std::sync::mpsc::channel();
        let (ready, started) = std::sync::mpsc::sync_channel(0);
        let other = std::thread::spawn(move || {
            confine_watchdog().unwrap();
            ready.send(()).unwrap();
            receive.recv().unwrap();
            assert_eq!(UdpSocket::bind("127.0.0.1:0").unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
        });
        started.recv().unwrap();
        confine_network(false).unwrap();
        send.send(()).unwrap();
        other.join().unwrap();
        for address in ["127.0.0.1:0", "[::1]:0"] {
            assert_eq!(UdpSocket::bind(address).unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
            assert_eq!(TcpListener::bind(address).unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
        }
        let (mut a, mut b) = UnixStream::pair().unwrap();
        confine_network(true).unwrap();
        assert_eq!(UnixStream::pair().unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
        a.write_all(b"ok").unwrap();
        let mut data = [0; 2];
        b.read_exact(&mut data).unwrap();
        assert_eq!(&data, b"ok");
    }

    #[test]
    fn watchdog_filesystem_policy_does_not_leave_an_unconfined_thread() {
        let file = std::env::temp_dir().join(format!("screensafe-watchdog-test-{}", std::process::id()));
        std::fs::write(&file, b"private").unwrap();
        let other = file.clone();
        let confined = std::thread::spawn(move || {
            confine_watchdog().unwrap();
            std::fs::read(other).is_err()
        })
        .join()
        .unwrap();
        assert!(confined);
        assert_eq!(std::fs::read(&file).unwrap(), b"private", "only the watchdog thread was confined");
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn refuses_to_run_unconfined() {
        assert_eq!(enforced(RulesetStatus::FullyEnforced), Ok("fully confined"));
        assert!(enforced(RulesetStatus::PartiallyEnforced).is_ok());
        assert!(enforced(RulesetStatus::NotEnforced).unwrap_err().contains("Landlock"));
    }
}
