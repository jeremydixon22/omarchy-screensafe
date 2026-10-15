# Distro Wars

The generator for the built-in Distro Wars pack, a parody of a popular God-game.

```sh
python3 tools/distro-wars/build.py packs/distro-wars
```

It draws every sprite sheet and the background from code (`art.py`, `terrain.py`, `table.py`, `ui.py`, `glyphs.py`,
`sprites.py`), simulates a 30-minute campaign with a fixed seed (`build.py`), and writes the pack's `screensafe.toml`,
which replays the campaign as a function of time. Nothing is taken from any game: every pixel is drawn here. The
Omarchy mark is Omarchy's own, from its `icon.txt`. Needs Python 3 and Pillow.

The campaign keeps a few rules, and the build stops if they're ever broken: the Omarchy side never loses a
settlement, a rival's settlement only changes hands at the end of a fight there, and no two fights share a site at
once. Once the land is all Omarchy's, the King casts two great powers on rival bands: a segfault (an earthquake) at
12 minutes, and Dependency Hell (a swamp) at 21.

Each of a God-game's commands is drawn as a Linux counterpart:

| Command | Drawn as |
| --- | --- |
| Raise and lower land | pacman |
| Rally point | the Omarchy mark |
| Earthquake | segfault |
| Swamp | Dependency Hell |
| Champion | root (`#`) |
| Volcano | fork bomb |
| Flood | memory leak |
| Final battle | crash (the frown) |
| Settle | install (a package) |
| Fight then settle | flame war |
| Gather then settle | merge |
| Cursor | shell prompt |
| Query | the manual |
| Options | config (a cog) |
| Sound effects | the terminal bell |
| Pause | suspend (a moon) |
| Controller | keyboard |
