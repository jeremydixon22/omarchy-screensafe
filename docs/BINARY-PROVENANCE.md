# Where the renderer comes from

`bin/screensafe-x86_64` and `bin/screensafe-aarch64` are the renderer, built from the Rust source in [`renderer/`](../renderer) at the same commit, for x86-64 and 64-bit ARM. They're committed so that the plugin works as soon as Omarchy adds it, with nothing to build or download. Anyone can rebuild them and check that they're byte for byte the committed files.

## What makes the build reproducible

- **Toolchain:** Rust 1.97.0, pinned in [`renderer/rust-toolchain.toml`](../renderer/rust-toolchain.toml) with both targets, using its own musl C library and its own linker (`rust-lld`). Nothing from the build machine's system toolchain goes into the executable.
- **Dependencies:** every crate is pinned by version and checksum in [`renderer/Cargo.lock`](../renderer/Cargo.lock), and the build refuses to change it (`--locked`). The crates and their licenses are listed in [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md).
- **Paths:** the checkout, Cargo and rustup folders are rewritten to fixed names (`--remap-path-prefix`), so where you build doesn't change the bytes.
- **Output:** static executables for `x86_64-unknown-linux-musl` (position-independent) and `aarch64-unknown-linux-musl`, built with one codegen unit and link-time optimization. They are stripped of symbols and have no build ID, so they link nothing from the system they run on.
- **Page sizes:** the ARM build's segments are aligned to 64 KiB, so it loads on kernels with 4, 16 and 64 KiB pages, including Asahi Linux on Apple silicon.

[`scripts/build-release`](../scripts/build-release) holds the exact settings.

## Checking it

With [rustup](https://rustup.rs) available, which fetches the pinned toolchain the first time, run this from a checkout:

```sh
scripts/verify-release
```

It rebuilds both executables in a temporary folder and compares each with the committed one, printing the SHA-256 of each that matches and failing if either doesn't.

The same commit has been built twice on one machine, from different checkout folders with separate Cargo homes and target folders, and gave identical bytes. The two builds also draw identical pixels: frames rendered by the ARM build under emulation match the x86-64 build's exactly. The repository's CI runs `scripts/verify-release` on GitHub's runners for every change, runs the tests natively on GitHub's ARM runners, and attests the committed executables for each commit pushed to `main`. To check an attestation:

```sh
gh attestation verify bin/screensafe-$(uname -m) --repo jeremydixon22/omarchy-screensafe
```

## What the executable contains

ScreenSafe's own code is in `renderer/src`, with no `unsafe` code (`#![forbid(unsafe_code)]`). Each executable embeds the JetBrains Mono Regular font from `renderer/fonts`. The libraries it links are pure Rust:

- `wayland-client` and the Wayland protocol crates, using their Rust backend rather than `libwayland`;
- `tiny-skia` and `fontdue` for drawing, and `ttf-parser` (through `fontdue`, and directly to check a pack's fonts);
- `image` for PNG, JPEG and WebP;
- `toml`;
- `tz-rs` for time zones;
- `rustix` for system calls;
- `landlock` for confinement.

Run `bin/screensafe-$(uname -m) version` to see its version, and the pack format and expression language it reads.
