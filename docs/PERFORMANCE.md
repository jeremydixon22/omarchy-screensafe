# ScreenSafe 0.6.0 performance measurements

The release headline is **about 66% less CPU with optional prepared Cascade playback**, and **about 18% less evaluation/drawing time in Laser Code**. These are specific laptop workloads, not a claim that every screensaver is faster or uses less power. [Machine-readable results](performance-data.json) include the individual measurements.

## Final-binary comparisons

Measured on 14 November 2026, on one x86-64 Omarchy 4.5.0-3 laptop at 2304 × 1440 on external power. The release's runtime binaries are the same as the qualified `3e197a7` candidate; release documentation does not change executable bytes. Each comparison used two runs per condition in forward/reverse order.

| Measurement | Control | 0.6.0 / prepared | Result |
|---|---:|---:|---:|
| Laser Code evaluation + drawing | 32.48 ms/frame, 0.4.8 | 26.76 ms/frame, 0.6.0 | 17.6% less drawing work |
| Complete Laser Code check, including flash analysis | 24.26 CPU seconds | 34.44 CPU seconds | 42.0% more total CPU work |
| Cascade playback, complete process tree | 67.58% of one core, live | 23.21% of one core, prepared | 65.7% less CPU |

The drawing comparison runs the same approved Laser Code pack, default settings, theme, resolution and first 30 seconds at 24 FPS: 720 offscreen frames per run. `drawMsPerFrame` measures evaluation and drawing, excluding flash analysis. Complete process CPU was also measured, so the drawing improvement is not presented as an end-to-end CPU saving. The more thorough 0.6.0 flash analysis adds substantial work in this workload; it remains enabled.

The Cascade comparison caps live playback at 24 FPS and uses the prepared player's fixed 24 FPS. Each activation lasts 60 seconds; CPU is sampled over approximately 40 seconds after a 15-second warmup. `/proc` accounting includes every renderer, controller, sandbox monitor and codec-worker process. Ordinary command timing misses the isolated codec worker and was rejected for this comparison. Observed client submissions across runs were approximately 23.7–24.0 per second; these are not physical display-timing measurements. Prepared playback repeats a checked 32-second segment, while live rendering continues generating variations.

## Earlier power measurements

Paired forward/reverse Harmonograph trials on 26 October 2026, using the earlier integrated player at the same native resolution and a 24 FPS cap, measured **7.76–7.81 W live versus 5.48–5.58 W prepared** in the processor-package energy domain: approximately **29% lower package power**. A final hardened-player confirmation measured 5.57 W. This supports a workload-specific estimate of the optional route's potential, not a fresh power result for the complete 0.6.0 release.

Those figures exclude the display and other whole-system loads. They are not a measured battery-runtime improvement and cannot be converted into one without additional measurements. Video hardware decoding with CPU readback is a different pipeline and is not covered by the procedural-cache saving.

## Choosing prepared playback

Prepared playback is off by default. It records and verifies a 16–32 second loop on known external power, then can reuse it on later activations. Preparation has an up-front CPU, energy and storage cost; a short session may never repay that cost. Simple packs may be cheaper live. Clocks, Photos, Videos, overlays and unsupported displays use normal rendering.

The setting does not reduce admission budgets or let a pack skip safety policy. Normal rendering checks each final composed frame; prepared playback needs a local approval of the exact decoded output and execution profile. See [Security](../SECURITY.md) for the separate trust assumption, timing rules and approximation limits. None of these performance trials certifies medical safety, every GPU or every community pack.
