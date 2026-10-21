#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""生成 app.ico —— 纯标准库，4x 超采样抗锯齿"""
import math
import struct
import zlib
import os

BASE = 256
SS = 4
N = BASE * SS

# 渐变：上 蓝紫 -> 下 紫
C1 = (0x5B, 0x8C, 0xFF)
C2 = (0x9B, 0x5C, 0xFF)


def rr_sdf(px, py, cx, cy, hw, hh, r):
    qx = abs(px - cx) - (hw - r)
    qy = abs(py - cy) - (hh - r)
    ax = qx if qx > 0 else 0.0
    ay = qy if qy > 0 else 0.0
    return math.hypot(ax, ay) + min(max(qx, qy), 0.0) - r


def render_hi():
    """返回 N*N 的 RGBA bytearray"""
    buf = bytearray(N * N * 4)
    # 背景圆角方块
    cx = cy = N / 2.0
    hw = hh = N / 2.0
    rad = 0.22 * N
    # 4 个瓦片（坐标按 256 空间设计，再乘 SS）
    margin, gap = 54.0, 20.0
    tile = (BASE - 2 * margin - gap) / 2.0
    xs = [margin, margin + tile + gap]
    ys = [margin, margin + tile + gap]
    tiles = []
    alpha = [[0.20, 0.20], [0.20, 1.0]]
    for r_ in range(2):
        for c in range(2):
            tiles.append((
                (xs[c] + tile / 2) * SS, (ys[r_] + tile / 2) * SS,
                (tile / 2) * SS, (tile / 2) * SS,
                18 * SS, alpha[r_][c],
            ))

    for y in range(N):
        ty = y / (N - 1.0)
        # 对角线渐变
        row = y * N * 4
        for x in range(N):
            d = rr_sdf(x + 0.5, y + 0.5, cx, cy, hw, hh, rad)
            if d > 0:
                continue
            t = (ty * 0.65 + (x / (N - 1.0)) * 0.35)
            r_ = C1[0] + (C2[0] - C1[0]) * t
            g_ = C1[1] + (C2[1] - C1[1]) * t
            b_ = C1[2] + (C2[2] - C1[2]) * t
            # 顶部柔光
            if ty < 0.55:
                k = (1.0 - ty / 0.55) ** 2 * 0.13
                r_ += (255 - r_) * k
                g_ += (255 - g_) * k
                b_ += (255 - b_) * k
            # 瓦片
            for (tcx, tcy, thw, thh, trad, al) in tiles:
                if abs(x - tcx) > thw + 2 or abs(y - tcy) > thh + 2:
                    continue
                dt = rr_sdf(x + 0.5, y + 0.5, tcx, tcy, thw, thh, trad)
                if dt > 0:
                    continue
                a = al if dt < -1.2 else al * (1.0 - (dt + 1.2) / 1.2)
                r_ += (255 - r_) * a
                g_ += (255 - g_) * a
                b_ += (255 - b_) * a
            i = row + x * 4
            buf[i] = int(max(0, min(255, r_)))
            buf[i + 1] = int(max(0, min(255, g_)))
            buf[i + 2] = int(max(0, min(255, b_)))
            buf[i + 3] = 255
    return buf


def downsample(buf, n, size):
    out = bytearray(size * size * 4)
    f = n // size
    area = f * f
    for y in range(size):
        for x in range(size):
            rs = gs = bs = als = 0
            for yy in range(y * f, (y + 1) * f):
                base = yy * n * 4
                for xx in range(x * f, (x + 1) * f):
                    i = base + xx * 4
                    a = buf[i + 3]
                    rs += buf[i] * a
                    gs += buf[i + 1] * a
                    bs += buf[i + 2] * a
                    als += a
            o = (y * size + x) * 4
            if als:
                out[o] = rs // als
                out[o + 1] = gs // als
                out[o + 2] = bs // als
            out[o + 3] = als // area
    return out


def png_bytes(size, rgba):
    raw = bytearray()
    for y in range(size):
        raw.append(0)
        raw += rgba[y * size * 4:(y + 1) * size * 4]

    def chunk(tag, data):
        return (struct.pack(">I", len(data)) + tag + data +
                struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF))

    return (b"\x89PNG\r\n\x1a\n" +
            chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)) +
            chunk(b"IDAT", zlib.compress(bytes(raw), 9)) +
            chunk(b"IEND", b""))


def main():
    print("rendering %dx%d ..." % (N, N))
    hi = render_hi()
    sizes = [16, 24, 32, 48, 64, 128, 256]
    images = []
    for s in sizes:
        if s == BASE:
            small = downsample(hi, N, BASE)
        else:
            small = downsample(hi, N, s)
        images.append((s, png_bytes(s, small)))
        print("  %d ok" % s)

    # 写 ICO
    head = struct.pack("<HHH", 0, 1, len(images))
    off = 6 + 16 * len(images)
    entries = b""
    data = b""
    for s, p in images:
        entries += struct.pack("<BBBBHHII", s if s < 256 else 0, s if s < 256 else 0,
                               0, 0, 1, 32, len(p), off)
        off += len(p)
        data += p
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "app.ico")
    with open(out, "wb") as f:
        f.write(head + entries + data)
    print("ico ->", out, os.path.getsize(out), "bytes")

    # 预览大图
    prev = os.path.join(os.path.dirname(os.path.abspath(__file__)), "icon_preview.png")
    with open(prev, "wb") as f:
        f.write(images[-1][1])
    print("preview ->", prev)


if __name__ == "__main__":
    main()
