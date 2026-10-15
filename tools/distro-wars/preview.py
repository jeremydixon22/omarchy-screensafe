"""Renders the background so far, for comparing with the reference."""
import sys
from PIL import Image
import art, table, terrain, ui

img = Image.new("RGB", (art.W, art.H), art.PAL["black"])
table.draw_table(img)
table.draw_rim(img)
ui.draw_plates(img)
# Above the tabletop's top rows: the black the book and bar sit against.
ui.rect(img, 0, 0, art.W, 0, art.PAL["black"])
ui.draw_book(img, ui.world_map())
ui.draw_bar(img)
ui.draw_pointer(img, 2)
ui.draw_penguin(img, False)
ui.draw_shield(img, *ui.KING_SHIELD)
ui.draw_penguin(img, True)
ui.draw_population(img, 0.5, 0.5)
h = terrain.heights()
terrain.draw_land(img, h)
table.draw_back_rim(img, h)
out = sys.argv[1]
img.save(out)
img.resize((art.W * 4, art.H * 4), Image.NEAREST).save(out.replace(".png", "-4x.png"))
