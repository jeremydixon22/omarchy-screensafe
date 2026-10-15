# Tokyo Drip

The generator for the built-in Tokyo Drip pack.

```sh
python3 tools/tokyo-drip/build.py packs/tokyo-drip
```

A rainy Tokyo side street at night, in the theme's colors. Omarchy's default theme is Tokyo Night, so out of the box
that's the palette.

- **The street:** buildings either side with lit windows and shopfronts, a short curtain over each door; wires
  overhead; a vending machine; two people with clear umbrellas crossing far off. Everything is placed in one-point
  perspective.
- **Signs:** vertical neon signs jutting from the walls, in Japanese: 居酒屋 (izakaya), ラーメン (ramen), カラオケ
  (karaoke, its tube failing), 焼鳥 (yakitori), 喫茶 (coffee house), 寿司 (sushi), 営業中 (open now), ゲーム (games),
  酒 (sake), BAR. Red paper lanterns hang at the izakaya's door.
- **The LED screen** at the end of the street plays its program: a wave, a level meter and rings, then an
  advertisement: for ScreenSafe, its name, its shield and a few seconds of each of its other screensavers; or for
  Omarchy.
- **Rain:** falling at many depths, landing in rings; drops swelling under every sign and falling to the street;
  every sign's light broken into ripples on the wet asphalt.
- **Settings:** how hard it rains (none, light or heavy; with none, the signs stop dripping, though the street
  stays wet), and what the big screen advertises between its pictures: nothing, ScreenSafe, or Omarchy (its name,
  its mark lit in the LEDs themselves, from Omarchy's own `icon.txt`, and its name scrolling past).

Effects that run continuously follow the pack's `time` rather than the scene's, so nothing jumps when the scene
comes round again.

Building it needs Python 3 with fontTools and Pillow, the renderer built at `renderer/target/release/screensafe`, and
Noto Sans CJK (`NotoSansCJK-Bold.ttc`, from Debian's and Ubuntu's `fonts-noto-cjk`):

- **The lettering** is a subset of Noto Sans Mono CJK JP Bold (SIL Open Font License 1.1), cut down to the characters
  the signs use and renamed Tokyo Drip Sign, as the license asks of modified versions. It's written to
  `fonts/sign.otf`, with the license in `fonts/OFL.txt`.
- **The screen's previews** (`previews.png`) are drawn by the renderer's `thumbnail` command from the other built-in
  packs, in Omarchy's default colors, so rebuild after changing one. Theme backgrounds is left out: it would show the
  build machine's own wallpaper.
