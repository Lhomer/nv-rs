#!/usr/bin/env python3
"""Renders New Physics lab frames (examples/np_frames.rs's JSON) to an MP4: a perspective view of
FNV's ragdoll's capsules over a floor grid, a caption per frame; the run at real speed, then again
at a quarter speed. Needs Pillow and ffmpeg.

    python3 -I scripts/np_video.py FRAMES.json OUT.mp4
"""
import json
import math
import os
import subprocess
import sys
import tempfile

from PIL import Image, ImageDraw, ImageFont

W, H = 960, 720


def camera(yaw, pitch, dist, target):
    cy, sy = math.cos(yaw), math.sin(yaw)
    cp, sp = math.cos(pitch), math.sin(pitch)
    eye = (target[0] + sy * cp * dist, target[1] - cy * cp * dist, target[2] + sp * dist)
    f = [target[i] - eye[i] for i in range(3)]
    n = math.sqrt(sum(x * x for x in f))
    f = [x / n for x in f]
    r = [f[1], -f[0], 0.0]
    n = math.sqrt(sum(x * x for x in r))
    r = [x / n for x in r]
    u = [r[1] * f[2] - r[2] * f[1], r[2] * f[0] - r[0] * f[2], r[0] * f[1] - r[1] * f[0]]
    return eye, f, r, u


def project(cam, p):
    eye, f, r, u = cam
    d = [p[i] - eye[i] for i in range(3)]
    z = sum(d[i] * f[i] for i in range(3))
    if z < 1.0:
        return None
    k = 0.9 * H / z
    x = W / 2 + sum(d[i] * r[i] for i in range(3)) * k
    y = H / 2 - sum(d[i] * u[i] for i in range(3)) * k
    return (x, y, z, k)


def draw_frame(cam, floor, frame, font, hits=()):
    img = Image.new("RGB", (W, H), (30, 33, 40))
    d = ImageDraw.Draw(img, "RGBA")
    # The floor's grid (game units, 20 apart).
    for i in range(-6, 7):
        for a, b in (((i * 20, -120, floor), (i * 20, 120, floor)), ((-120, i * 20, floor), (120, i * 20, floor))):
            pa, pb = project(cam, a), project(cam, b)
            if pa and pb:
                d.line([(pa[0], pa[1]), (pb[0], pb[1])], fill=(70, 75, 85), width=1)
    caps = []
    for c in frame["caps"]:
        a, b, r = c[0:3], c[3:6], c[6]
        pa, pb = project(cam, a), project(cam, b)
        if pa and pb:
            caps.append(((pa[2] + pb[2]) / 2, pa, pb, r))
    caps.sort(key=lambda x: -x[0])
    for depth, pa, pb, r in caps:
        shade = max(90, min(230, int(255 - depth * 0.35)))
        col = (shade, int(shade * 0.78), int(shade * 0.55))
        ra, rb = r * pa[3], r * pb[3]
        w = max(2, int(ra + rb))
        d.line([(pa[0], pa[1]), (pb[0], pb[1])], fill=col, width=w)
        for p, rr in ((pa, ra), (pb, rb)):
            d.ellipse([p[0] - rr, p[1] - rr, p[0] + rr, p[1] + rr], fill=col)
    # A hit's arrow for 0.25 s from it: 30 units long, ending at the point.
    for h in hits:
        if 0.0 <= frame["t"] - h[0] <= 0.25:
            p, dv = h[1:4], h[4:7]
            a = [p[i] - dv[i] * 30 for i in range(3)]
            pa, pb = project(cam, a), project(cam, p)
            if pa and pb:
                d.line([(pa[0], pa[1]), (pb[0], pb[1])], fill=(230, 60, 50), width=5)
                d.ellipse([pb[0] - 6, pb[1] - 6, pb[0] + 6, pb[1] + 6], fill=(230, 60, 50))
    d.rectangle([0, H - 34, W, H], fill=(0, 0, 0, 160))
    d.text((10, H - 27), frame["caption"], fill=(235, 235, 235), font=font)
    return img


def main():
    src, out = sys.argv[1], sys.argv[2]
    data = json.load(open(src))
    try:
        font = ImageFont.truetype("DejaVuSans.ttf", 15)
        title_font = ImageFont.truetype("DejaVuSans-Bold.ttf", 20)
    except OSError:
        font = title_font = ImageFont.load_default()
    cam = camera(math.radians(62), math.radians(10), 200.0, (0.0, 0.0, 52.0))
    hits = data.get("hits", [])
    frames = data["frames"]
    with tempfile.TemporaryDirectory() as tmp:
        n = 0
        # Real speed, then a quarter speed (each frame four times).
        for speed, label in ((1, "real speed"), (4, "1/4 speed")):
            for fr in frames:
                img = draw_frame(cam, data["floor"], fr, font, hits)
                ImageDraw.Draw(img).text((10, 10), f"{data['name']}   ({label})", fill=(255, 220, 120), font=title_font)
                for _ in range(speed):
                    img.save(os.path.join(tmp, f"f{n:05d}.png"))
                    n += 1
        fps = round(1.0 / data["frame"])
        subprocess.run(
            ["ffmpeg", "-y", "-loglevel", "error", "-framerate", str(fps), "-i", os.path.join(tmp, "f%05d.png"),
             "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20", out],
            check=True,
        )
    print(out)


if __name__ == "__main__":
    main()
