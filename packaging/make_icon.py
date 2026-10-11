#!/usr/bin/env python3
"""Generate the Grit icon assets from `grit-icon.svg`.

Produces the Apple Big Sur icon shape inside this repo's `packaging/`:
macOS does not round icon corners itself — the rounded squircle (with
transparent surroundings) must be baked into every tile, per the
template grid: 824×824 artwork on a 1024 canvas, ~185px corner radius.

Outputs:
- grit-icon-1024.png  master render, rounded
- grit.png            256×256 (Linux hicolor entry)
- grit.icns           every canonical macOS tile

Requirements: macOS (`qlmanage` renders the SVG; `iconutil` builds the
.icns) and Pillow (`python3 -m pip install pillow`).

Usage: `python3 packaging/make_icon.py`
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageOps

HERE = Path(__file__).resolve().parent
SVG = HERE / "grit-icon.svg"
MASTER = HERE / "grit-icon-1024.png"

# Apple's icon grid: artwork box 824×824 centered on a 1024 canvas,
# continuous squircle approximated by a 185px corner radius.
CANVAS = 1024
INSET = 100  # (CANVAS - 824) / 2
RADIUS = 185

ICNS_TILES = [
    (16, "icon_16x16.png"),
    (32, "icon_16x16@2x.png"),
    (32, "icon_32x32.png"),
    (64, "icon_32x32@2x.png"),
    (128, "icon_128x128.png"),
    (256, "icon_128x128@2x.png"),
    (256, "icon_256x256.png"),
    (512, "icon_256x256@2x.png"),
    (512, "icon_512x512.png"),
    (1024, "icon_512x512@2x.png"),
]


def render_master() -> Image.Image:
    """Rasterize the SVG at 1024px (QuickLook renders SVGs on macOS)."""
    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run(
            ["qlmanage", "-t", "-s", str(CANVAS), "-o", tmp, str(SVG)],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        rendered = Path(tmp) / f"{SVG.name}.png"
        image = Image.open(rendered)
        if image.size != (CANVAS, CANVAS):
            image = image.resize((CANVAS, CANVAS), Image.LANCZOS)
        return image.convert("RGBA")


def rounded_icon(master: Image.Image) -> Image.Image:
    """Inset the artwork into an 824×824 rounded squircle mask."""
    box = CANVAS - 2 * INSET
    artwork = ImageOps.fit(master, (box, box), Image.LANCZOS)

    mask = Image.new("L", (box, box), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, box, box), RADIUS, fill=255)

    icon = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    icon.paste(artwork, (INSET, INSET), mask)
    return icon


def main() -> None:
    master = rounded_icon(render_master())
    if "--check" in sys.argv:
        return
    master.save(MASTER)

    # Linux hicolor entry.
    master.resize((256, 256), Image.LANCZOS).save(HERE / "grit.png")

    # macOS tiles (iconutil requires the folder to end in `.iconset`).
    with tempfile.TemporaryDirectory(suffix=".iconset", dir=HERE) as tmp:
        tiles = Path(tmp)
        for size, name in ICNS_TILES:
            master.resize((size, size), Image.LANCZOS).save(tiles / name)
        subprocess.run(
            ["iconutil", "-c", "icns", str(tiles), "-o", str(HERE / "grit.icns")],
            check=True,
        )
    print(f"wrote {MASTER.name}, grit.png, grit.icns from {SVG.name}")


if __name__ == "__main__":
    main()
