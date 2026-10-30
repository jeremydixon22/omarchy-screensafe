# Live capture experiment

This branch adds an explicit experimental `run-capture` command. Normal playback,
panel Preview, and the installed preparation workflow do not enable it. There is
no automatic setting or CI preparation service in this experiment.

The proposed workflow is to play live on the first eligible activation, capture
an unapproved candidate on external power, qualify it, and use it on a later
activation. It must retain separate preparation for devices or packs without
sufficient live headroom.

## Boundary

The command retains a dedicated inherited stdout pipe before reading a pack and
redirects ordinary stdout to `/dev/null`. The retained writer closes when capture
ends, so the encoder receives EOF while the screensaver remains live. The capture output descriptor
must be a FIFO. The capture thread starts after final renderer
confinement and inherits Landlock and the all-thread network policy. No pack gains
filesystem, process or network authority. No Wayland connection reaches encoding.

Only admitted RGBA pixels actually submitted to Wayland enter capture. Three
reusable frame buffers and a replaceable latest frame bound memory; the display
never waits for encoding. The recorder samples at 24 FPS, holding the latest
admitted frame when appropriate. An initial bounded encoder warmup precedes the
steady write and lateness deadlines. Capture cancels if live frame spacing or a
two-second frame budget indicates insufficient headroom. Composition changes and
output incompatibility also cancel capture. Pipe failure leaves live rendering
available; the controller must reject incomplete candidates.

The bounded header identifies the actual loaded source digest, seed, geometry,
frame count and sampling policy. This is recording metadata, not approval. A
trusted controller must compare it with its plan, recheck source/display/profile,
and apply the existing final GPU-output, full repeat, decoder fidelity and flash
checks, including the loop seam and black entry/exit. Hash identity alone is not
approval. Clock-dependent packs, Photos, Videos and live overlays remain excluded.

## Observations

Trials used a single 2304 × 1440 output, 32-second recordings, unchanged runtime
flash guarding and bounded system H.264 encoding. These observations describe one
test device, not a supported-hardware or energy-saving claim.

- Cascade's baseline delivered 30 FPS, with its largest frame gap about 36 ms.
  Live capture retained 30 FPS without a gap over 100 ms. The revised producer
  and `superfast` encoder completed capture in about 32 seconds, followed by
  successful final GPU qualification. The recording was about 17 MiB.
  A separate qualification run kept live playback at 30 FPS throughout both
  decoder checks, with a largest frame gap about 44 ms. The captured cache
  subsequently played across a complete loop.
- Laser Code's baseline delivered about 24 FPS, without a gap over 100 ms.
  The first encoder trial reduced it to about 17 FPS and introduced seven gaps
  over 100 ms in 40 seconds. The revised live-budget guard cancelled capture
  after roughly two seconds, preventing an incomplete candidate from being
  approved. Encoding is not free even when rasterization is reused.
- Separate prepared-startup measurements found that runtime identity checks
  dominated video loading. Bounded parallel full-file hashing and responsive
  metadata-helper waits reduced first-frame submission from about 2.4 seconds
  to about 1.5 seconds. Every content check remains; no timestamp-based approval
  cache or persistent decoder was introduced.

## Before automatic integration

Keep preparation AC-only. Add per-profile retry backoff and visible state for
collecting, qualifying, ready and insufficient headroom. Retain manual preparation
and clear/rebuild controls. Qualifying alongside live playback needs its own
headroom policy; a successful recording does not demonstrate that every later
validation workload is unobtrusive. Keep next-activation reuse rather than an
unchecked mid-run transition. Short previews may not supply a complete recording.
