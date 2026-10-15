# Omnipotence

The generator for the built-in Omnipotence pack.

```sh
python3 tools/omnipotence/build.py packs/omnipotence
```

Each of its six pages is a scene of 40 seconds:

1. **Code:** it opens on its own code, the lines of `screensafe.toml` that draw its figure, exactly as they're
   written there (long lines are wrapped for display).
2. **Draw:** the figure draws itself over the code, and each block of code lights up while it's the one drawing.
3. **Turn:** the Hand of Omnipotence reaches in, takes the page's bottom-right corner and pulls it across to the
   left, as a book's page is turned. The page rolls up over a cylinder lying along the fold: its rising front still
   shows the figure, carried round the curl point by point (`sample_let`), and past edge-on we see its back, drawn in
   strips lit by the way each faces. Halfway, the page stands up toward us and lifts away to the top left, uncovering
   the next page's code beneath. Before the turn, the figure is drawn by exactly the code shown; the turn draws a copy
   whose points are carried along.

The figures are a fern, a harmonograph, a spirograph and rose, a torus knot, a sunflower's seeds, and the fern
again before panes of folded light.

The hand is AI-generated (Codex image generation), on chroma green, with an image owned by the author as the
reference for its style and color. Of its three poses, the reaching and pinching ones are used: `hands.py` keys out
the green, fades the forearm into a soft glow so the hand emerges from light, and lines the poses up at the pinch.
The sheet (`hand-sheet.png`), its prompt (`hand-prompt.txt`) and its provenance (`hand-provenance.json`) are kept
here. The hand is released with the pack under its MIT license. Needs Python 3 and Pillow.
