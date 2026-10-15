# Photos

The source of the built-in Photos pack's example photos: thirty photos in four sets (Nature, Technology, Aerospace and Astronomy), shown when someone has no photos of their
own in `~/Pictures/Screensaver`, or chooses the examples.

```sh
python3 tools/photos/build.py ORIGINALS_DIR packs/photos
```

- **Landscape, Wildlife, Technology, Aerospace and Computing** are AI-generated (Codex image generation), one image
  for each prompt, with no people, text, logos or real landmarks. Each prompt is in `provenance.json`.
- **Astronomy** is five photographs from [NASA's Image and Video Library](https://images.nasa.gov), with the full NASA
  source credits recorded in `provenance.json`: Earthset from Artemis II, an aurora from the International Space Station, the
  Milky Way's center from Spitzer, the Moon from Apollo 8, and the 2017 eclipse's diamond ring. Their sources and
  credits are in `provenance.json`. Use follows [NASA's media guidelines](https://www.nasa.gov/nasa-brand-center/images-and-media/)
  and, for the Spitzer image, [JPL's image use policy](https://www.jpl.nasa.gov/jpl-image-use-policy/).
  Using them doesn't imply NASA's endorsement.

`build.py` turns the originals into the pack's `examples/*.webp`: upright, at most 1600 by 1067 pixels, with no
metadata. `provenance.json` records each one's source and the digest of its original. The generated photos are
released with the pack under its MIT license. Needs Python 3 and Pillow.
