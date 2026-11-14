# Clockwork

An original illustrated mechanical movement, drawn entirely with ScreenSafe's
existing lines, polygons, groups and text. Open gear rims reveal the lower train;
the balance spring and its skeleton bridge occupy a nearer plane.

**Camera** chooses a continuous 2.5D Journey, a fixed Overview or a moving Close-up.
The journey eases into the balance assembly and back out, with depth-dependent
parallax. **Finish** offers Brass, Steel and Theme; Theme also exposes the shared
Color picker. **Pace** changes the whole movement and camera together. **Detail**
controls the fine engraving.

The default runs at 20 FPS with a 32-second cycle. Slow and Lively take 64 and
21⅓ seconds respectively. Tooth and spoke symmetries, spring movement and camera
motion meet at the repeat. It is a decorative movement, rather than a time display.
It is eligible for the existing opt-in prepared-loop system; that system records
a bounded segment when a slower setting lasts longer than its recording window.

All artwork is original ScreenSafe vector data under the repository's MIT
license. Reproduce it with `python3 tools/clockwork/build.py packs/clockwork` from
the repository root. No renderer changes, image assets, executable pack content,
new dependencies or additional sandbox permissions are needed.
