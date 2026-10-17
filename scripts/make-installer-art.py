#!/usr/bin/env python3
"""Draws the installer artwork (NSIS sidebar and header, DMG background) from
the app icon. Run it again after changing the icon; outputs are committed.

    python3 scripts/make-installer-art.py
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
ICON = Image.open(ROOT / "src-tauri/icons/icon.png").convert("RGBA")
OUT = ROOT / "src-tauri/installer"
BOLD = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
REGULAR = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"


def font(path, size):
    if not Path(path).exists():
        import os
        windows = Path(os.environ.get("WINDIR", "C:/Windows")) / "Fonts"
        path = str(windows / ("segoeuib.ttf" if "Bold" in path else "segoeui.ttf"))
    return ImageFont.truetype(path, size)


def gradient(size, top, bottom):
    width, height = size
    image = Image.new("RGB", size)
    draw = ImageDraw.Draw(image)
    for y in range(height):
        t = y / max(1, height - 1)
        draw.line([(0, y), (width, y)], fill=tuple(round(a + (b - a) * t) for a, b in zip(top, bottom)))
    return image


def glow(image, centre, radius, colour, strength):
    layer = Image.new("RGB", image.size, colour)
    mask = Image.new("L", image.size, 0)
    ImageDraw.Draw(mask).ellipse(
        [centre[0] - radius, centre[1] - radius, centre[0] + radius, centre[1] + radius], fill=int(255 * strength)
    )
    from PIL import ImageFilter
    mask = mask.filter(ImageFilter.GaussianBlur(radius / 2))
    image.paste(layer, (0, 0), mask)


def paste_icon(image, size, position):
    icon = ICON.resize((size, size), Image.LANCZOS)
    image.paste(icon, position, icon)


# NSIS welcome/finish page: 164x314, BMP.
sidebar = gradient((164, 314), (14, 16, 24), (10, 40, 90))
glow(sidebar, (82, 120), 90, (51, 133, 250), 0.55)
paste_icon(sidebar, 96, (34, 72))
draw = ImageDraw.Draw(sidebar)
draw.text((82, 214), "Private Music", font=font(BOLD, 15), fill=(255, 255, 255), anchor="mm")
draw.text((82, 238), "VK Music", font=font(REGULAR, 11), fill=(170, 190, 225), anchor="mm")
sidebar.save(OUT / "sidebar.bmp")

# NSIS header: 150x57, BMP.
header = gradient((150, 57), (14, 16, 24), (14, 40, 84))
paste_icon(header, 40, (100, 8))
ImageDraw.Draw(header).text((10, 28), "Private\nMusic", font=font(BOLD, 12), fill=(255, 255, 255), anchor="lm", spacing=1)
header.save(OUT / "header.bmp")

# DMG window background: 660x400.
dmg = gradient((660, 400), (16, 18, 28), (12, 42, 92))
glow(dmg, (330, 40), 260, (51, 133, 250), 0.35)
draw = ImageDraw.Draw(dmg)
draw.text((330, 46), "Private Music", font=font(BOLD, 26), fill=(255, 255, 255), anchor="mm")
draw.text((330, 78), "Перетащите приложение в «Программы»", font=font(REGULAR, 14), fill=(175, 195, 230), anchor="mm")
y = 200
draw.line([(250, y), (392, y)], fill=(255, 255, 255), width=4)
draw.polygon([(392, y - 14), (420, y), (392, y + 14)], fill=(255, 255, 255))
dmg.save(OUT / "dmg-background.png")
print("wrote", *sorted(p.name for p in OUT.iterdir()))
