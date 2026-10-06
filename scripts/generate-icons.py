#!/usr/bin/env python3
"""Generate Shellfox icons with Python 3 + Pillow 11.1.0.
From the repo in WSL: python3 scripts/generate-icons.py
The source already has transparent corners and antialiased rounded edges.
"""
from pathlib import Path
from io import BytesIO
import json
import struct
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / 'resources/icon'
source = Image.open(OUT / 'shellfox-icon-source.png').convert('RGBA')
alpha = source.getchannel('A')
corners = [source.getpixel(p)[3] for p in [(0, 0), (source.width-1, 0), (0, source.height-1), (source.width-1, source.height-1)]]
if any(corners):
    raise ValueError('Source corners are not transparent; inspect artwork before generation')
# Ignore near-invisible exterior alpha noise when finding the rounded tile bounds.
bounds = alpha.point(lambda a: 255 if a > 127 else 0).getbbox()
tile = source.crop(bounds)
canvas = Image.new('RGBA', (1024, 1024))
width = 922
height = round(width * tile.height / tile.width)
tile = tile.resize((width, height), Image.Resampling.LANCZOS)
# Paste without a mask to retain original RGB and alpha, rather than squaring alpha.
canvas.paste(tile, ((1024-width)//2, (1024-height)//2))
canvas.save(OUT / 'icon.png')
sizes = [16, 24, 32, 48, 64, 128, 256, 512]
for size in sizes:
    canvas.resize((size, size), Image.Resampling.LANCZOS).save(OUT / f'icon-{size}.png')
canvas.save(OUT / 'icon.ico', sizes=[(s, s) for s in sizes if s <= 256])
canvas.save(OUT / 'favicon.ico', sizes=[(16, 16), (32, 32)])
# Modern ICNS PNG representations, including Retina entries. No iconutil required.
chunks = []
for kind, size in [('icp4', 16), ('icp5', 32), ('icp6', 64), ('ic07', 128), ('ic08', 256), ('ic09', 512), ('ic10', 1024), ('ic11', 32), ('ic12', 64), ('ic13', 256), ('ic14', 512)]:
    buffer = BytesIO()
    canvas.resize((size, size), Image.Resampling.LANCZOS).save(buffer, format='PNG')
    data = buffer.getvalue()
    chunks.append(kind.encode('ascii') + struct.pack('>I', len(data)+8) + data)
payload = b''.join(chunks)
(OUT / 'icon.icns').write_bytes(b'icns' + struct.pack('>I', len(payload)+8) + payload)
print(json.dumps({'sourceSize': source.size, 'sourceAlphaRange': alpha.getextrema(), 'cornerAlpha': corners, 'tileBounds': bounds, 'outputTileSize': [width, height], 'sizes': sizes}, indent=2))
