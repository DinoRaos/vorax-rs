# Draws the overlay's icon (crates/vorax-overlay/assets/vorax.ico): the overlay's gold diamond on a dark rounded
# square, at every size Windows asks for. Run: python tools/make-icon.py
from pathlib import Path
from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent.parent / 'crates' / 'vorax-overlay' / 'assets'
OUT.mkdir(parents=True, exist_ok=True)

BG = (20, 15, 18, 255)
RIM = (227, 178, 90, 140)
GOLD = (255, 214, 138, 255)
DEEP = (196, 140, 60, 255)


def icon(size: int) -> Image.Image:
    k = 4  # drawn 4x and scaled down, for smooth edges
    s = size * k
    img = Image.new('RGBA', (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    pad = s * 0.04
    d.rounded_rectangle([pad, pad, s - pad, s - pad], radius=s * 0.22, fill=BG, outline=RIM, width=max(k, int(s * 0.03)))
    cx, cy = s / 2, s / 2
    h, w = s * 0.31, s * 0.23
    # the diamond: a light upper-left facet and a deeper lower-right one, as cut gold
    d.polygon([(cx, cy - h), (cx + w, cy), (cx, cy + h), (cx - w, cy)], fill=DEEP)
    d.polygon([(cx, cy - h), (cx + w, cy), (cx, cy), (cx - w, cy)], fill=GOLD)
    d.polygon([(cx - w, cy), (cx, cy), (cx, cy + h)], fill=(236, 190, 110, 255))
    return img.resize((size, size), Image.LANCZOS)


sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
big = icon(256)
big.save(OUT / 'vorax.ico', sizes=[(n, n) for n in sizes], append_images=[icon(n) for n in sizes[:-1]])
big.save(OUT / 'vorax.png')
print(f'wrote {OUT / "vorax.ico"}')
