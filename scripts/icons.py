"""Renders the app icons in assets/icon from the 16-module mark geometry.

    python scripts/icons.py

Each size is drawn on a supersampled canvas where every module edge is a whole
subpixel, then box-filtered down, so each output pixel gets its exact coverage.
The tray (crates/sukurin-app/src/tray.rs) and the SVG mark (crates/sukurin-ui)
draw the same geometry.
"""

from pathlib import Path
from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent.parent / "assets" / "icon"
TILE = (21, 22, 25, 255)
BODY = (232, 233, 235, 255)
SHARD = (198, 241, 53, 255)
RADIUS = 3.5 / 16


def render(size, inset=0.0):
    """`inset` is the empty margin on each side as a fraction of `size`."""
    n = size * 16 if inset == 0 else 8192
    tile = round(n * (1 - 2 * inset))
    off = (n - tile) // 2
    m = tile / 16
    assert m == int(m) and off * 2 + tile == n, f"{size}px does not land on whole subpixels"
    im = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    d.rounded_rectangle([off, off, off + tile - 1, off + tile - 1], radius=RADIUS * tile, fill=TILE)
    for i in range(4):
        for j in range(4):
            shard = i + j >= 4
            o = 1 if shard else 0
            x0 = off + (2 + 3 * i + o) * m
            y0 = off + (2 + 3 * j + o) * m
            d.rectangle([x0, y0, x0 + 3 * m - 1, y0 + 3 * m - 1], fill=SHARD if shard else BODY)
    return im.resize((size, size), Image.BOX)


def main():
    OUT.mkdir(parents=True, exist_ok=True)

    ico = [16, 20, 24, 32, 40, 48, 64, 256]
    frames = [render(s) for s in ico]
    frames[-1].save(OUT / "sukurin.ico", sizes=[(s, s) for s in ico], append_images=frames[:-1])

    # macOS draws app icons on an 824/1024 body, so the tile is inset to match its neighbours in Finder and the Dock.
    mac = [16, 32, 64, 128, 256, 512, 1024]
    frames = [render(s, inset=100 / 1024) for s in mac]
    frames[-1].save(OUT / "sukurin.icns", append_images=frames[:-1])

    render(512).save(OUT / "sukurin.png")


if __name__ == "__main__":
    main()
