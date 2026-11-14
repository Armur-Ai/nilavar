#!/usr/bin/env python3
"""Removes Private Use Area (U+E000–U+F8FF) code points from a font's cmap.

The bundled text fonts (Inter, JetBrains Mono) map some PUA code points to
their own alternates, which would shadow the Phosphor icon font that egui
uses as a fallback. Only cmap entries are removed; glyph outlines are
untouched. Both fonts are SIL OFL 1.1 without Reserved Font Names, which
permits modified redistribution. Requires fontTools (dev-time only).

Usage: strip_pua.py IN.ttf OUT.ttf
"""
import sys
from fontTools.ttLib import TTFont

src, dst = sys.argv[1], sys.argv[2]
font = TTFont(src)
removed = 0
for table in font["cmap"].tables:
    if not hasattr(table, "cmap"):
        continue
    for cp in [c for c in table.cmap if 0xE000 <= c <= 0xF8FF]:
        del table.cmap[cp]
        removed += 1
font.save(dst)
print(f"{src}: removed {removed} PUA cmap entries")
