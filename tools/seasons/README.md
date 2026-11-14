# Building Seasons

Run `python3 tools/seasons/build.py packs/seasons` from the repository root.
Python's standard library and the shared `tools/pack_art.py` writer generate the
entire manifest deterministically. Only that data is used at runtime.

The four seasonal palettes are fixed artwork in Natural tone. Theme tone uses
the ordinary theme palette, including the existing Color control. All geometry
is shared: winter reveals the tree's branches instead of loading another image.
Water, foliage and seasonal details use bounded copies and periodic formulas.

Validate with the shipped renderer's `check` and `render` commands. Check all four
seasons, Fresh breeze, seasonal detail on, and Natural/Theme tones. Inspect both
light and dark themes and wide/portrait crops. The runtime flash guard remains
unchanged.
