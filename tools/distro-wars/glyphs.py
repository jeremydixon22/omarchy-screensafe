"""Small pixel glyphs for the mana bar and the command plates, drawn as text: '#' is the glyph's color, '+' its
shade, '.' nothing. Each command of a God-game is drawn as a Linux counterpart: the rally mark is the Omarchy mark,
the final battle a crashed screen's frown, the earthquake a segfault, and so on."""

GLYPHS = {
    # The mana bar's great powers, from least to greatest.
    # Raise and lower land: pacman, which installs and removes.
    "pacman": """
..###.....
.#####....
####......
###...#.#.
####......
.#####....
..###.....
""",
    # Place the rally mark: the Omarchy mark, at nine pixels.
    "mark": """
#########
#...#...#
#.###.#.#
#.#...#.#
###...#.#
#.#...#.#
#.#####.#
#...#...#
#####.###
""",
    # Earthquake: a segfault, a block of memory cracked through.
    "segfault": """
####.####
#..#.#..#
#...#...#
#..#.#..#
#...#...#
#..#.#..#
####.####
""",
    # Swamp: dependency hell, a chain with a broken link.
    "dephell": """
.##.....##.
#..#...#..#
#..##.##..#
.##.....##.
....#.#....
""",
    # Champion: root, the shell's prompt for the one who may do anything.
    "root": """
..#..#..
.######.
..#..#..
..#..#..
.######.
..#..#..
""",
    # Volcano: a fork bomb, its fuse lit.
    "forkbomb": """
......+.+
.......#.
......#..
..####...
.######..
.######..
.######..
..####...
""",
    # Flood: a memory leak, a stick of memory dripping.
    "memleak": """
#########
#+#+#+#+#
#########
.#.#.#.#.
.........
..#...#..
......#..
""",
    # The final battle: the crash's frown.
    "crash": """
.######.
#......#
#.#..#.#
#......#
#..##..#
#.#..#.#
#......#
.######.
""",
    # The info shield's emblems.
    "crown": """
#.#.#.#
#######
#+#+#+#
#######
""",
    "castle": """
#.#.#.#
#######
##+#+##
###.###
##...##
""",
    "strength": """
#.......
#.#.....
#.#.#...
#.#.#.#.
#.#.#.#.
""",
    # The command plates.
    # Settle: install, a package.
    "package": """
..####..
.#....#.
######.#
#....#.#
#....#.#
######..
""",
    # Fight then settle: a flame war.
    "flame": """
....#...
...##...
..#.#.#.
.#..###.
.#.#..#.
.#....#.
..####..
""",
    # Gather then settle: merge, two branches joining.
    "merge": """
#.....#.
#.....#.
#....#..
#..##...
#.#.....
##......
#.......
""",
    # The info shield's own command keeps its shield.
    "shield": """
#######.
#+++#..#
#+++#..#
#####..#
.#..#+#.
..#.#+..
...##...
""",
    # View settlements: the Omarchy flag.
    "flag": """
#.....
####..
#####.
####..
#.....
#.....
#.....
""",
    # The cursor: a shell prompt.
    "prompt": """
#.......
.#......
..#.....
.#......
#..####.
""",
    # Query: the manual, an open book.
    "manual": """
.##..##.
#..##..#
#.+##+.#
#..##..#
#.+##+.#
.##..##.
""",
    # Options for good and evil: the config, a cog.
    "cog": """
...#....
.#.#.#..
..###...
###.###.
..###...
.#.#.#..
...#....
""",
    # Sound effects: the terminal's bell.
    "bell": """
...#....
..###...
.#####..
.#####..
.#####..
#######.
...#....
""",
    # The game's setup: the world, of mirrors.
    "globe": """
..###...
.#+#+#..
#+#+#+#.
#+#+#+#.
.#+#+#..
..###...
""",
    # Music: still music.
    "note": """
.#####.
.#...#.
.#...#.
##..##.
##..##.
""",
    # Pause: suspend, a moon.
    "moon": """
..###...
.##.....
##......
##......
##......
.##.....
..###...
""",
    # The controller: a keyboard, Omarchy's own way of commanding.
    "keyboard": """
#########
#.#.#.#.#
#########
#.#...#.#
#########
""",
    # View battles: tiled windows, Hyprland's.
    "tiles": """
#########
#...#...#
#...#...#
#...#####
#...#...#
#########
""",
}


def stamp(img, name, x, y, color, shade=None):
    """Draws glyph `name` with its top left at (x, y)."""
    px = img.load()
    rows = [r for r in GLYPHS[name].strip("\n").split("\n")]
    for dy, row in enumerate(rows):
        for dx, ch in enumerate(row):
            c = color if ch == "#" else shade if ch == "+" and shade else None
            if c is not None and 0 <= x + dx < img.width and 0 <= y + dy < img.height:
                px[x + dx, y + dy] = c


def size(name):
    rows = GLYPHS[name].strip("\n").split("\n")
    return max(len(r) for r in rows), len(rows)
