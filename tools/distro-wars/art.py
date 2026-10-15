"""Pixel art for Distro Wars, drawn from code at 256 x 224, a 16-bit console's screen, in a palette of its own.

Nothing here is taken from any game: every shape, and the screen's layout and colors, are drawn by these functions.
"""
from PIL import Image, ImageDraw

W, H = 256, 224

# The palette, by use.
PAL = {
    "black": (0x00, 0x00, 0x00),
    "white": (0xFF, 0xFF, 0xFF),
    # Tabletop stone, grout and rim.
    "stone": (0x63, 0x5A, 0x6B),
    "stone_dark": (0x52, 0x5A, 0x63),
    "grout": (0x31, 0x31, 0x42),
    "grout_dark": (0x29, 0x29, 0x39),
    "rim_light": (0xBD, 0xBD, 0xBD),
    "rim_mid": (0xA5, 0xA5, 0xB5),
    "rim_dark": (0x8C, 0x8C, 0x84),
    "icon": (0xC6, 0xC6, 0xBD),
    "icon_dark": (0x7B, 0x7B, 0x7B),
    "icon_plate": (0x8C, 0x8C, 0x84),
    # Water.
    "water_deep": (0x21, 0x39, 0x9C),
    "water": (0x4A, 0x5A, 0xCE),
    "water_light": (0x42, 0x84, 0xE7),
    "water_crest": (0x39, 0x94, 0xFF),
    # Land.
    "grass_hi": (0x31, 0xDE, 0x31),
    "grass_light": (0x73, 0xC6, 0x5A),
    "grass": (0x39, 0xA5, 0x10),
    "grass_mid": (0x29, 0xAD, 0x29),
    "grass_dim": (0x29, 0x84, 0x00),
    "grass_shade": (0x21, 0x84, 0x18),
    "grass_dark": (0x21, 0x6B, 0x10),
    "grass_deep": (0x10, 0x6B, 0x10),
    "grass_night": (0x18, 0x4A, 0x18),
    "crop": (0xC6, 0xC6, 0x00),
    "crop_light": (0xBD, 0xD6, 0x00),
    "dirt_top": (0xBD, 0x9C, 0x00),
    "dirt_light": (0xA5, 0x84, 0x00),
    "dirt": (0x8C, 0x6B, 0x00),
    "dirt_mid": (0x7B, 0x52, 0x00),
    "dirt_dark": (0x5A, 0x42, 0x00),
    "dirt_deep": (0x42, 0x29, 0x00),
    # Gold of the mana bar and posts.
    "gold_hi": (0xFF, 0xDE, 0x63),
    "gold": (0xE7, 0xCE, 0x00),
    "gold_mid": (0xBD, 0x9C, 0x00),
    "gold_dark": (0x8C, 0x6B, 0x00),
    "bar": (0x7B, 0x52, 0x00),
    "bar_dark": (0x39, 0x31, 0x00),
    "bar_cell": (0x5A, 0x4A, 0x00),
    "orange": (0xEF, 0x84, 0x29),
    "orange_light": (0xFF, 0x8C, 0x4A),
    "yellow": (0xFF, 0xFF, 0x00),
    # Shield and population bars.
    "red": (0xAD, 0x18, 0x00),
    "red_bright": (0xD6, 0x18, 0x00),
    "red_dark": (0x6B, 0x18, 0x00),
    "shield_grey": (0xA5, 0xA5, 0xB5),
    "bar_green": (0x00, 0x94, 0x00),
    "omarchy_green": (0x9E, 0xCE, 0x6A),
    "omarchy_green_dark": (0x4F, 0x7A, 0x2A),
    # Book.
    "page": (0xFF, 0xFF, 0xFF),
    "page_shade": (0xBD, 0xBD, 0xB5),
    "page_grey": (0x9C, 0x9C, 0x9C),
    "cover": (0xAD, 0x18, 0x00),
    "cover_dark": (0x6B, 0x18, 0x00),
    "map_sea": (0x42, 0x84, 0xE7),
    "map_land": (0x39, 0xA5, 0x10),
    "map_land_dark": (0x21, 0x6B, 0x10),
    "map_frame": (0xEF, 0x84, 0x29),
}

# The close-up map: 8 x 8 tiles of 32 x 16; the top corner of the sea-level diamond, and how far a height step
# lifts the land.
TILES = 8
TOP = (128, 87)
STEP = 8


def corner(i, j, h):
    """Screen position of the map corner (i, j) at height h: i runs down to the right, j down to the left."""
    return (TOP[0] + (i - j) * 16, TOP[1] + (i + j) * 8 - h * STEP)


def noise(seed, x, y):
    """A fixed value from 0 to 1 for a pixel."""
    n = (x * 374761393 + y * 668265263 + seed * 2147483647) & 0xFFFFFFFF
    n = ((n ^ (n >> 13)) * 1274126177) & 0xFFFFFFFF
    return ((n ^ (n >> 16)) & 0xFFFF) / 65536.0


def fill_poly(img, pts, color_at):
    """Fills polygon `pts` pixel by pixel with color_at(x, y), using PIL only for the coverage mask."""
    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]
    x0, y0, x1, y1 = int(min(xs)), int(min(ys)), int(max(xs)) + 1, int(max(ys)) + 1
    mask = Image.new("1", (x1 - x0 + 1, y1 - y0 + 1), 0)
    ImageDraw.Draw(mask).polygon([(x - x0, y - y0) for x, y in pts], fill=1)
    px = img.load()
    for y in range(y0, y1 + 1):
        if not 0 <= y < img.height:
            continue
        for x in range(x0, x1 + 1):
            if 0 <= x < img.width and mask.getpixel((x - x0, y - y0)):
                c = color_at(x, y)
                if c is not None:
                    px[x, y] = c


def speckle(seed, base, dots, density):
    """A texture: `base`, with `dots` scattered at `density`, fixed by position."""
    def at(x, y):
        return dots if noise(seed, x, y) < density else base
    return at


def checker(a, b):
    return lambda x, y: a if (x + y) % 2 == 0 else b
