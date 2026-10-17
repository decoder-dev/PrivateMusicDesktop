#!/usr/bin/env python3
"""Reuse the mobile app's original icons and mood artwork on desktop.
Run: python scripts/sync-ios-assets.py --source /path/to/PrivateMusic2
"""
import argparse
from pathlib import Path
from PIL import Image

parser = argparse.ArgumentParser()
parser.add_argument("--source", type=Path, required=True)
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
assets = args.source / "PrivateMusic/Resources/Assets.xcassets"
icon = Image.open(assets / "AppIcon.appiconset/AppIcon.png").convert("RGBA")
icons = root / "src-tauri/icons"
for name, size in [("icon.png",1024),("32x32.png",32),("128x128.png",128),("128x128@2x.png",256)]:
    icon.resize((size,size),Image.Resampling.LANCZOS).save(icons/name)
icon.save(icons/"icon.ico",format="ICO",sizes=[(16,16),(24,24),(32,32),(48,48),(64,64),(128,128),(256,256)])
icon.resize((1024,1024),Image.Resampling.LANCZOS).save(icons/"icon.icns",format="ICNS")
out = root / "ui/assets"
out.mkdir(exist_ok=True)
icon.resize((128,128),Image.Resampling.LANCZOS).save(out/"app-icon.png")
for name in ["Energy","Calm","Rain","Sun","Love"]:
    image = Image.open(assets/f"Discovery{name}.imageset/cover.png").convert("RGB")
    image.thumbnail((640,640),Image.Resampling.LANCZOS)
    image.save(out/f"discovery-{name.lower()}.webp",quality=88,method=6)
print("Synced app icon and five original mood covers")
