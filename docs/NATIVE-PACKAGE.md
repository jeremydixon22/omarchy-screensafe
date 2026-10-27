# Native package

ScreenSafe includes all application executables for both x86-64 and ARM64. A standard Omarchy installation needs no compiler, extra package installation, post-install script, download or privileged setup to use them. Prepared playback remains opt-in and checks display/driver eligibility before use.

| Included executable | Purpose | Runtime dependency | Distribution license |
|---|---|---|---|
| `screensafe-$ARCH` | Pack evaluator, rasterizer, flash guard and Wayland presenter | Static Rust/musl | MIT and bundled-library notices |
| `screensafe-cache-$ARCH` | Waylandless codec/GPU worker | System `libmpv.so.2`, EGL, OpenGL, GBM, libseccomp and glibc | GPL-2.0-or-later; own source also MIT |
| `screensafe-sandbox-$ARCH` | Unprivileged namespace isolation | System libcap and glibc | Bubblewrap LGPL-2.1-or-later |
| `screensafe-media-$ARCH` | Resource limits before entering the import sandbox | Static musl | MIT, musl notices |

`$ARCH` is `x86_64` or `aarch64`. The ARM glibc helpers also use the standard `ld-linux-aarch64.so.1` loader. No helper has a private RPATH, setuid permission or file capability. All are ordinary user executables. GPU access is limited to one render node inside the separate worker cage; neither Quickshell nor the Rust renderer loads native media libraries.

## Omarchy supplies the media and graphics stack

The [Omarchy base package list](https://github.com/omacom/omarchy/blob/quattro/install/omarchy-base.packages) includes FFmpeg, MPV, Hyprland and Python dependencies. Their dependency closure supplies the shared graphics libraries, libcap, libseccomp and glibc. FFmpeg/ffprobe are used at fixed `/usr/bin` paths. ScreenSafe bundles Bubblewrap rather than assuming it was separately installed.

These system libraries and hardware drivers are deliberately left with the distribution. Bundling a second Mesa/VAAPI/driver stack would duplicate security updates and risk driver mismatches. The helpers target stable SONAMEs and glibc 2.34 or newer. The build-only Ubuntu ABI libraries listed in the input lock are not copied into the installed plugin.

MPV's client API version is checked by `screensafe-cache-$ARCH --self-test`; CI runs this on native x86-64 and ARM64 runners. Actual GPU output is checked locally during preparation, including software/hardware-copy equality and two repeat loops. A missing/incompatible library, unsupported display or failing GPU profile selects normal guarded rendering. This is conditional platform support, not a claim that every GPU has been tested.

## Sources, licenses and exact build inputs

- ScreenSafe helper source: [`cache/native/`](../cache/native/), MIT under the root license. For distribution, the GPU worker is supplied under [GPL-2.0-or-later](../licenses/native-GPL-2.0.txt), compatible with MPV's default GPL build. System media libraries retain their own licenses and are not redistributed in this repository.
- Bubblewrap 0.13.1: complete upstream source in [`vendor/bubblewrap-0.13.1/`](../vendor/bubblewrap-0.13.1/), including its [LGPL-2.1 license](../licenses/bubblewrap-LGPL-2.1.txt). Its code is unmodified. The two upstream license symlink aliases (`LICENSE`, `COPYING.LIB`) are identical regular copies of `COPYING` so Omarchy's plugin validator accepts the installation. The release archive SHA-256 is pinned; rebuilding compares every source file and both resolved license aliases to that archive before compilation.
- Static media launcher: includes musl under its [copyright/license notices](../licenses/musl-COPYRIGHT.txt). Zig runtime support is covered by the [Zig MIT license](../licenses/zig-MIT.txt).
- [`scripts/native-inputs.json`](../scripts/native-inputs.json): exact Zig 0.15.2 compiler archives and Ubuntu header/link-time ABI package versions, URLs and SHA-256 values for both architectures. Downloads occur only while explicitly rebuilding, and are checked before extraction or execution.
- [`bin/native-build.json`](../bin/native-build.json): source/recipe/input-lock hashes and exact hashes/sizes of the six native executables. Architecture, allowed dependencies, absence of RPATH and glibc floor are tested.

The root MIT license continues to cover ScreenSafe's own source. It does not relabel third-party components or the distributed GPU worker as MIT-only. There is no requirement to install or build these components from source merely to use the plugin.

## Reproduce and verify

On a Linux x86-64 or ARM64 build machine with Python 3.12+, `dpkg-deb`, and space for temporary build inputs:

```sh
python3 scripts/build-native.py /tmp/screensafe-native
scripts/verify-native
```

Use `--cache PATH` to choose the reproducible download cache and `TMPDIR` to choose temporary build storage. The script extracts packages into a temporary sysroot without running package scripts or installing them into the build machine. It cross-builds both architectures with the same pinned toolchain, baseline CPU targets and fixed path mappings. The sandbox build disables optional SELinux integration; Omarchy's unprivileged-user-namespace environment remains required. It does not alter the host's security policy.

CI compares all six binaries and their build manifest, tests the controller/importer/settings, checks worker API/seccomp on both native architectures, and rebuilds both static Rust renderers. Main and `release/**` pushes attest all eight executables plus the native manifest after those checks pass. Release branches provide candidate evidence without moving the marketplace's current upstream branch.

Ubuntu CI runners additionally load a narrow AppArmor permission for the distro `/usr/bin/bwrap` test harness to create user namespaces. Importer tests run the bundled sandbox inside that context; their mount, network, process and resource checks remain enabled. This is test-machine setup only, not an installation step or a host-wide sysctl change.

```sh
gh attestation verify bin/screensafe-cache-$(uname -m) --repo jeremydixon22/omarchy-screensafe
```

Use the same command for each renderer, sandbox/media launcher and the native manifest. Check the attestation's source commit against the exact candidate under review. A local build receipt or byte comparison is not itself a GitHub attestation, and an earlier release's attestation does not cover these new bytes.
