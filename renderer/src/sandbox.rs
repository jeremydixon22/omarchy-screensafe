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

/// The first layer: reading only, beneath `paths`. A path that doesn't exist is left out.
pub fn confine_reading(paths: &[&Path]) -> Result<&'static str, String> {
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
    fn refuses_to_run_unconfined() {
        assert_eq!(enforced(RulesetStatus::FullyEnforced), Ok("fully confined"));
        assert!(enforced(RulesetStatus::PartiallyEnforced).is_ok());
        assert!(enforced(RulesetStatus::NotEnforced).unwrap_err().contains("Landlock"));
    }
}
