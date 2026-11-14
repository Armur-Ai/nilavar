#!/usr/bin/env python3
"""Generates the Nilavar app icon (PNG) with no third-party dependencies.

A near-black rounded square, thin green border, a terminal prompt chevron and
cursor bar with a soft glow. Usage: make_icon.py OUT.png SIZE
"""
import math, struct, sys, zlib

BG = (5, 9, 8)
PANEL = (11, 18, 16)
BORDER = (28, 51, 42)
GREEN = (57, 255, 136)

def rounded_rect_alpha(x, y, s, r):
    # signed distance to rounded square [0,s]^2 with radius r
    cx = min(max(x, r), s - r)
    cy = min(max(y, r), s - r)
    d = math.hypot(x - cx, y - cy) - r
    if r <= x <= s - r or r <= y <= s - r:
        d = max(-min(x, y, s - x, s - y), d) if not (r <= x <= s - r and r <= y <= s - r) else -min(x, y, s - x, s - y)
    return d

def seg_dist(px, py, ax, ay, bx, by):
    vx, vy = bx - ax, by - ay
    wx, wy = px - ax, py - ay
    t = max(0.0, min(1.0, (wx * vx + wy * vy) / (vx * vx + vy * vy)))
    return math.hypot(px - (ax + t * vx), py - (ay + t * vy))

def render(size):
    S = 1024.0
    scale = S / size
    px = []
    for j in range(size):
        row = bytearray()
        for i in range(size):
            acc = [0.0, 0.0, 0.0, 0.0]
            for sj in (0.25, 0.75):
                for si in (0.25, 0.75):
                    x = (i + si) * scale
                    y = (j + sj) * scale
                    m = 80.0
                    d = rounded_rect_alpha(x - m, y - m, S - 2 * m, 190.0)
                    if d > 0:
                        continue
                    # vertical gradient background
                    t = (y - m) / (S - 2 * m)
                    col = [BG[k] * (1 - t) + PANEL[k] * t for k in range(3)]
                    # border
                    if d > -14:
                        col = list(BORDER)
                    # chevron ">" and cursor "_"
                    w = 46.0
                    dc = min(seg_dist(x, y, 330, 360, 520, 512), seg_dist(x, y, 520, 512, 330, 664))
                    du = seg_dist(x, y, 580, 664, 720, 664)
                    dd = min(dc, du)
                    glow = math.exp(-max(dd - w, 0) / 55.0) * 0.35
                    col = [col[k] * (1 - glow) + GREEN[k] * glow for k in range(3)]
                    if dd < w:
                        a = min(1.0, (w - dd) / 3.0)
                        col = [col[k] * (1 - a) + GREEN[k] * a for k in range(3)]
                    acc[0] += col[0]; acc[1] += col[1]; acc[2] += col[2]; acc[3] += 255
            row += bytes([int(acc[0] / 4), int(acc[1] / 4), int(acc[2] / 4), int(acc[3] / 4)])
        px.append(bytes(row))
    return px

def png(path, size):
    rows = render(size)
    raw = b"".join(b"\x00" + r for r in rows)
    def chunk(t, d):
        c = struct.pack(">I", len(d)) + t + d
        return c + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    open(path, "wb").write(data)



def ico(path, sizes=(16, 32, 48, 64, 128, 256)):
    """Writes a Windows .ico containing PNG-compressed images."""
    import io, os, tempfile
    blobs = []
    for s in sizes:
        tmp = os.path.join(tempfile.mkdtemp(), "i.png")
        png(tmp, s)
        blobs.append((s, open(tmp, "rb").read()))
    header = struct.pack("<HHH", 0, 1, len(blobs))
    offset = 6 + 16 * len(blobs)
    entries, data = b"", b""
    for s, b in blobs:
        entries += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(b), offset + len(data))
        data += b
    open(path, "wb").write(header + entries + data)


if __name__ == "__main__":
    if sys.argv[1].endswith(".ico"):
        ico(sys.argv[1])
    else:
        png(sys.argv[1], int(sys.argv[2]))
