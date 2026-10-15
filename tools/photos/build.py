#!/usr/bin/env python3
"""Builds the Photos pack's example photos.

    python3 build.py ORIGINALS_DIR packs/photos

ORIGINALS_DIR holds `codex/<set>-<n>.png` (generated, each with a `.json` receipt of its prompt) and
`nasa/<nasa_id>.<ext>` (from NASA's image library, each with its `.meta.json` record). Each becomes
`examples/<set>-<n>.webp`: at most 1600 by 1067 pixels, upright, with no metadata. `provenance.json` beside this file
records where every one came from. Needs Python 3 and Pillow.
"""
import hashlib
import json
import sys
from pathlib import Path

from PIL import Image, ImageOps

SETS = ["landscape", "wildlife", "technology", "aerospace", "computing", "astronomy"]
# Astronomy: photographs by NASA, credited to NASA alone, in the public domain.
NASA = [
    ("art002e021278", "Earthset, seen by the Artemis II crew from Orion during the lunar flyby, 6 April 2026", "NASA"),
    ("iss058e005282", "An aurora over Earth's limb, from the International Space Station, 19 January 2019", "NASA"),
    ("PIA03653", "The dusty center of the Milky Way in infrared, from the Spitzer Space Telescope, 2006", "NASA/JPL-Caltech"),
    ("as08-14-2506", "A nearly full Moon, from Apollo 8, December 1968", "NASA"),
    ("AFRC2017-0233-009", "The diamond ring at the total solar eclipse of 21 August 2017", "NASA/Carla Thomas"),
]
SIZE = (1600, 1067)


def save(src: Path, dest: Path):
    image = ImageOps.exif_transpose(Image.open(src)).convert("RGB")
    image.thumbnail(SIZE, Image.LANCZOS)
    dest.parent.mkdir(parents=True, exist_ok=True)
    image.save(dest, "WEBP", quality=80, method=6)


def main(originals: Path, pack: Path):
    records = []
    for name in SETS[:-1]:
        for n in range(1, 6):
            src = originals / "codex" / f"{name}-{n}.png"
            receipt = json.loads((originals / "codex" / f"{name}-{n}.json").read_text())
            out = pack / "examples" / f"{name}-{n}.webp"
            save(src, out)
            records.append({
                "file": f"examples/{out.name}",
                "source": "AI-generated: Codex image generation",
                "prompt": receipt["prompt"],
                "original_sha256": hashlib.sha256(src.read_bytes()).hexdigest(),
                "license": "MIT, with the pack",
            })
    for n, (nasa_id, about, credit) in enumerate(NASA, 1):
        src = next((originals / "nasa").glob(f"{nasa_id}.*[!n]"))
        out = pack / "examples" / f"astronomy-{n}.webp"
        save(src, out)
        records.append({
            "file": f"examples/{out.name}",
            "source": f"NASA Image and Video Library, {nasa_id}: https://images.nasa.gov/details/{nasa_id}",
            "about": about,
            "credit": credit,
            "original_sha256": hashlib.sha256(src.read_bytes()).hexdigest(),
            "license": "Public domain (NASA); see https://www.nasa.gov/nasa-brand-center/images-and-media/",
        })
    Path(__file__).with_name("provenance.json").write_text(json.dumps(records, indent=1, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main(Path(sys.argv[1]), Path(sys.argv[2]))
