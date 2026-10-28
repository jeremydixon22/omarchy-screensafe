# SSVv2: local compressed-video experiment

This is an R&D path, separate from the normal 0.5.0 plugin install and from prepared procedural playback. Ordinary invocation continues to use JPEG SSV v1. Compressed files require the explicit decoder wrapper below; without it they are skipped with a warning. Pack authors still provide data, not commands or native code.

The experiment keeps **16-bit frame references**. It permits up to **65,535 frames**, fixed **24 FPS**, even dimensions up to **1280 × 720**, and **512 MiB per file**. That is at most **45 minutes 30.625 seconds**, subject to storage and conversion budgets. JPEG SSV v1 keeps its existing five-minute, 7,200-frame limits. No renderer frame identifiers were widened.

## Use an isolated checkout

Build the renderer normally with `scripts/build-release`. On a standard Omarchy system the remaining tools and libraries are already supplied by the base installation; there is no additional installer or runtime download. This prototype uses the existing bundled resource launcher and sandbox, and system FFmpeg. H.264 encoding needs FFmpeg's libx264; AV1 encoding uses libaom and decoding uses libdav1d. Missing support fails the operation rather than installing anything.

```sh
python3 tools/videos/ssv2.py SOURCE.mp4 /tmp/ssv2-clips/clip.ssv --codec h264
# Alternatively: --codec av1

python3 tools/videos/play_v2.py --renderer "$PWD/bin/screensafe-$(uname -m)" -- \
  run --pack "$PWD/packs/videos" --videos /tmp/ssv2-clips
```

The wrapper also supports `render`, `check`, `thumbnail` and `catalog`. It does not wrap `preview` or other commands that already use stdin for their own protocol. The default delivery streams pixels directly into renderer-owned buffers; `--delivery snapshot` selects the original sealed-snapshot comparison path. No settings or installed plugin files are changed. Do not present this as marketplace-approved functionality or a finalized interchange standard.

## Encode and validate

The explicit importer accepts a retained, no-follow regular MP4 descriptor of at most 2 GiB. It normalizes one video stream to fixed 8-bit YUV420, dimensions and cadence, with no audio, subtitles or variable resolution. HDR transfer functions are refused. The profiles use closed one-second GOPs without frame reordering. H.264 uses CRF 16, x264's `fastdecode` tuning, and disabled B-frames and scene-cut keyframes. The tuning disables CABAC, deblocking and weighted prediction to reduce software decode work; see the [x264 implementation](https://github.com/mirror/x264/blob/master/common/base.c). AV1 uses libaom CRF 24, no lag/alternate-reference frames, and fixed keyframe spacing. These are experiment profiles, not a claim of equivalent quality between codecs.

Encoder jobs cover 30 seconds for H.264 and 4 seconds for AV1. A confined full decode of each job checks actual dimensions, pixel format, frame count and keyframe spacing. Jobs retain the existing 180 CPU-second, 2 GiB address-space, 64-descriptor, 128 MiB regular-file and 64 MiB scratch limits. Encoder/validation wall deadlines are at most 90 seconds each, with a 30-minute overall codec-pipeline budget. Complexity can exhaust a budget before reaching the format ceiling; no partial archive is published.

The parent streams output into private temporary files. Assembly temporarily needs up to twice the compressed file size, plus a bounded index. Publication never replaces an existing destination, including a racing symlink. Cancellation, changed input or failed profile checks remove staging. No video bytes are downloaded.

## File layout (experimental)

All integer fields are little-endian. The 64-byte header is `8s, 14×u32`:

| Field | Value |
|---|---|
| magic | `SSVID002` |
| width, height | Even, 2..1280 and 2..720 |
| fps, frames | 24; 1..65,535 |
| codec | 2: H.264 Annex B; 1: AV1 length-delimited OBU temporal units |
| GOP | 24 |
| flags | 1: fixed profile without reordering |
| config length | 0 |
| six reserved words | All zero |

The header is followed by one 16-byte `(offset:u64, length:u32, flags:u32)` record per frame, then contiguous payload. Bit 0 marks each multiple of 24 as a GOP start; all other bits are zero. Each access unit is 1 byte..2 MiB. The first starts exactly after the index; holes, overlaps, overflow, extra data and out-of-file ranges are refused. The renderer parses this bounded envelope, never codec syntax or general media containers. Import-side splitting recognizes only Annex B/AUD framing or bounded OBU length fields. Declared index flags are not a security credential or proof of arbitrary bitstream behavior.

## Runtime boundary

A trusted Python broker starts the renderer with a private `SOCK_SEQPACKET` stdin channel. The renderer duplicates it before confinement, then retains the same filesystem/network/process restrictions as ordinary playback. A request carries one retained read-only input descriptor, a sequence number and a bounded frame index. It cannot choose a command, path, filter or decoder option.

The broker independently validates the index and uses one active CPU decoder in the existing user/mount/PID/network namespace cage. It receives no home, compositor socket or GPU node. FFmpeg has fixed decoder/demuxer arguments, thread-only process creation, bounded stderr and pipe backpressure. A cold seek decodes at most one GOP; a sequential request normally decodes one new frame. Each request has a five-second decode budget. The decoder is recycled before its normal cumulative CPU usage reaches the hard limit, so long playback does not depend on an unbounded process budget.

The default delivery transfers a private Unix stream descriptor and Rust reads exactly one bounded frame into owned memory under a total five-second deadline. This avoids routing every frame through Python byte buffers and a new memfd. Rust rejects other socket families/types and checks response identity, descriptor count, dimensions and byte length. Twelve raw frames are cached in Rust for neighbor access and crop/tone variants; returned images are separately owned. An internal clip identity prevents cache confusion when descriptors are reused; frame references remain 16-bit.

Skipped neighboring frames can use a bounded sealed-memfd cache in the broker, and `--delivery snapshot` uses that transport for every frame. Rust verifies all write/grow/shrink/seal locks and exact size before reading a snapshot. Both transports finish the owned copy before compositing or guarding. No worker-writable buffer can change the image after admission. Working sets are bounded independently of clip length; codec reference buffers are separate and resource-limited.

The ordinary rasterizer applies crop, tone, curvature, device/Film art and optional overlay. The existing runtime flash guard remains after final composition. This path does **not** use the procedural cache's offline GPU approval or assume that a media hash constrains decoder output. A compromised codec still has its confined syscall/kernel surface; isolation is not a promise that bugs are impossible.

There is no idle broker polling timer: renderer EOF and the parent's pidfd end the work. Codec death, invalid metadata, incomplete frames, expired deadlines or input mutation refuse the video. A well-formed refusal allows subsequent clips; a broken transport closes the prototype session. Shutdown cleans up the decoder and renderer. Hardware decode is intentionally outside this CPU experiment's authority and qualification.

## Evidence and remaining qualification

Tests cover v1 compatibility, the 16-bit ceiling, malformed indexes, byte-wise stream framing, encoder job/GOP boundaries, a clip longer than five minutes, immutable snapshots, direct/snapshot pixel equality, stream ownership/deadlines, full neighbor/loop access, all six device styles, actual composed-frame flash rejection, and parent-death cleanup.

Local laptop trials of a 47-second, grain-heavy 720p clip found a storage/runtime tradeoff. JPEG v1 occupied 125.1 MiB; tuned H.264 occupied 51.8 MiB and AV1 32.0 MiB. On the same renderer binary, short full-screen trials used about 30% of one CPU core for JPEG, 45–46% for tuned H.264 and 63% for AV1. Direct delivery reduced CPU work compared with the sealed-snapshot prototype; the H.264 tuning reduced it further. Neither compressed path beat the current JPEG runtime on this input. Sampled luma quality against the provided MP4 was higher than v1 for both codecs; these are single-clip observations, not codec-wide quality guarantees.

The tested device styles and loop transitions completed without broker failures. Observed client submissions stayed near 24/s without gaps over 100 ms, which does not establish physical presentation timing or guarantee that every submission contains a fresh video frame. These short trials do not establish thermal endurance.

Earlier laptop power tests already guide the baseline: guarded software H.264 delivery used less package power than hardware decode with CPU readback. Later owned-pixel work also reduced JPEG SSV cost, so the old difference is not a fresh savings claim for this implementation. AV1 was not covered by those power trials. File size, decoder CPU, image quality and watts are distinct measurements; no new battery-runtime percentage is claimed here. Keep ordinary JPEG playback as the default. For clips within v1's limits, explicit one-time preparation through the existing MP4 importer remains the available way to trade local disk space for its lower measured runtime CPU cost.

Before production routing or a marketplace update: extend laptop endurance and native ARM playback qualification, compare quality on more representative inputs, review the new trusted broker and codec-at-playback surface, and decide whether these profiles and envelope are ready to freeze. GitHub attestations cover the committed application executables; they do not certify codec correctness, encoded-media quality or battery savings.
