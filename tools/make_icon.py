"""Draws the app icon (an original design: a ridge under a low sun) as a PNG."""
import math, struct, sys, zlib

S = 1024
SS = 2  # samples per axis

def ridge(x, base, amp, seed):
    return base - amp * (0.55 * math.sin(x * 2.1 + seed) + 0.3 * math.sin(x * 5.3 + seed * 2.0) + 0.15 * math.sin(x * 11.0 + seed * 3.1))

def colour(u, v):
    # rounded square mask
    r, m = 0.225, 0.06
    dx, dy = max(abs(u - 0.5) - (0.5 - m - r), 0.0), max(abs(v - 0.5) - (0.5 - m - r), 0.0)
    if math.hypot(dx, dy) > r:
        return (0, 0, 0, 0)
    t = v
    sky = (int(18 + 60 * t), int(22 + 52 * t), int(40 + 70 * t))
    c = sky
    if math.hypot(u - 0.66, v - 0.40) < 0.105:
        c = (246, 190, 96)
    if v > ridge(u * math.pi * 2, 0.60, 0.085, 0.7):
        c = (120, 96, 110)
    if v > ridge(u * math.pi * 2, 0.70, 0.075, 2.3):
        c = (232, 162, 74)
    if v > ridge(u * math.pi * 2, 0.82, 0.05, 4.1):
        c = (30, 36, 48)
    return (*c, 255)

rows = []
for y in range(S):
    row = bytearray([0])
    for x in range(S):
        acc = [0, 0, 0, 0]
        for sy in range(SS):
            for sx in range(SS):
                p = colour((x + (sx + 0.5) / SS) / S, (y + (sy + 0.5) / SS) / S)
                a = p[3]
                acc[0] += p[0] * a; acc[1] += p[1] * a; acc[2] += p[2] * a; acc[3] += a
        n = SS * SS
        if acc[3]:
            row += bytes((acc[0] // acc[3], acc[1] // acc[3], acc[2] // acc[3], acc[3] // n))
        else:
            row += b"\0\0\0\0"
    rows.append(bytes(row))

def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", S, S, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
