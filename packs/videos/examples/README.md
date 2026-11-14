# Included Omarchy ad

Original ScreenSafe animation from the fictional O.M.A.R.C.H.Y. sponsor segment
in the ScreenSafe announcement film, with the project owner's approved parody
mascot. No third-party footage or audio. Distributed under this repository's MIT
license. The loop begins with a short fade up to meet the ending iris-out.

`omarchy-ad-loop.ssv` is silent H.264 SSVv2: 1280 × 720, 24 FPS, 1,132 frames
(47.17 seconds), 49,506,764 bytes. The explicit confined importer produced it with
CRF 16.8 and the documented fixed GOP/fastdecode profile. SHA256SUMS identifies the
exact packaged bytes. The source MP4 is a maintainer input and is not packaged.

The application uses this example when the personal videos folder has no visible
regular `.ssv` files. Personal clips take precedence. Decode, device selection and
the final combined-frame flash guard use the existing video path.

The example uses the ordinary isolated SSVv2 importer, with no inserted frame
holds or temporal smoothing. Complete composed-frame checks cover all six video
styles at 1152 × 720 and Full at 1280 × 720. These checks do not certify arbitrary
crops, tones or displays; runtime guarding remains active after final composition.
Some aggressive source-only crops trigger the approximate guard even though the
tested device compositions pass.

To reproduce from the retained source MP4:

```sh
python3 tools/videos/ssv2.py SOURCE.mp4 OUTPUT.ssv --codec h264
```

Recheck the encoded file through `tools/videos/play_v2.py` with the renderer's
`check` command for every style and intended output geometry. The previous
maintainer filter that silently repeated rejected frames was retired; normalization
preserves the source animation's frame sequence. No maintainer helper is invoked
by installation or playback.
