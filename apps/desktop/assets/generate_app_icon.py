#!/usr/bin/env python3
"""Generate the AgentTidy desktop app source icon (apps/desktop/assets/app-icon.png).

Why a script instead of a hand-drawn asset: the icon must be reproducible and
tweakable across the macOS/Windows matrix, and the repo rule is that binary
artefacts should be derivable from committed, reviewable sources. This script
draws the 1024x1024 source icon; the platform icon set (icns / ico / all PNG
sizes) is then generated from it with the official Tauri CLI:

    python3 apps/desktop/assets/generate_app_icon.py
    apps/desktop/node_modules/.bin/tauri icon apps/desktop/assets/app-icon.png \
        -o apps/desktop/src-tauri/icons

Design rationale (matches the product's "tidy, local-first, safe" posture):
- macOS-style rounded square with a diagonal teal -> deep emerald gradient,
  echoing the emerald accent used by the GUI (low-risk / safe language).
- White glyph: three evenly stacked rounded bars (tidy session/resource rows)
  crowned by a sparkle ("understand before tidy", cleaned result).
- Everything is drawn at 4x and downscaled with LANCZOS so edges stay smooth at
  every bundled size, including the 32x32 taskbar/favicons.
"""

from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw

# --- geometry (in 1024-base units; multiplied by SUPER at draw time) ---------
SUPER = 4  # supersampling factor; why: PIL has no analytic antialiasing
BASE = 1024
SIZE = BASE * SUPER

# Rounded-square plate: a small margin keeps the shape from clipping against
# square Windows icon frames while staying visually full-bleed in macOS Dock.
PLATE_MARGIN = 40
PLATE_RADIUS = 220

# Glyph: three "tidy rows" plus a sparkle resting above the stack.
BAR_W, BAR_H, BAR_R = 452, 92, 46
BAR_X = (BASE - BAR_W) // 2
BAR_TOPS = [436, 586, 736]  # gap 58 keeps the rows readable down to 32px
SPARKLE_CENTER = (512, 304)
SPARKLE_R = 108
SPARKLE_SMALL_R = 42  # companion star, gives the mark a "glimmer" signature
SPARKLE_SMALL_CENTER = (666, 232)  # tucked in the main star's concave NE corner
SPARKLE_EXP = 2.6  # superellipse exponent: higher = pointier 4-point star

# Palette: teal-600 -> emerald-900 diagonal; white glyph stays high-contrast
# on both ends so the mark reads at taskbar sizes.
GRAD_TOP_LEFT = (16, 142, 130)  # #108E82
GRAD_BOTTOM_RIGHT = (6, 78, 59)  # #064E3B
WHITE = (255, 255, 255, 255)


def s(v: float) -> float:
    """Scale a 1024-base coordinate to the supersampled canvas."""
    return v * SUPER


def build_gradient() -> Image.Image:
    """Diagonal top-left -> bottom-right linear gradient at full canvas size.

    Built as a tiny image and resized up: a smooth linear ramp survives
    interpolation, and this avoids per-pixel Python loops on a 4096^2 canvas.
    """
    small = Image.new("RGB", (BASE, BASE))
    px = small.load()
    for y in range(BASE):
        for x in range(BASE):
            t = (x + y) / (2 * (BASE - 1))
            px[x, y] = tuple(
                round(a + (b - a) * t)
                for a, b in zip(GRAD_TOP_LEFT, GRAD_BOTTOM_RIGHT)
            )
    return small.resize((SIZE, SIZE), Image.BILINEAR)


def sparkle_points(cx: float, cy: float, r: float) -> list[tuple[float, float]]:
    """Four-point sparkle outline as a superellipse ("astroid-like" star).

    Parametric signed-power curve: points at N/E/S/W with concave sides, which
    is the classic "cleaned / magic" mark and stays recognisable when small.
    """
    pts = []
    steps = 180
    for i in range(steps):
        theta = 2 * math.pi * i / steps
        ct, st = math.cos(theta), math.sin(theta)
        x = math.copysign(abs(ct) ** SPARKLE_EXP, ct) * r
        y = math.copysign(abs(st) ** SPARKLE_EXP, st) * r
        pts.append((cx + x, cy + y))
    return pts


def main() -> None:
    out = Path(__file__).resolve().parent / "app-icon.png"

    icon = build_gradient()

    # Plate approach: the gradient covers the full square, so carve the rounded
    # plate by pasting the gradient through a rounded mask and leaving the
    # corners transparent (modern Windows 11 accepts rounded icons).
    mask = Image.new("L", (SIZE, SIZE), 0)
    mask_draw = ImageDraw.Draw(mask)
    mask_draw.rounded_rectangle(
        [s(PLATE_MARGIN), s(PLATE_MARGIN), s(BASE - PLATE_MARGIN), s(BASE - PLATE_MARGIN)],
        radius=s(PLATE_RADIUS),
        fill=255,
    )
    plate = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    plate.paste(icon, (0, 0), mask)

    draw = ImageDraw.Draw(plate)
    for top in BAR_TOPS:
        draw.rounded_rectangle(
            [s(BAR_X), s(top), s(BAR_X + BAR_W), s(top + BAR_H)],
            radius=s(BAR_R),
            fill=WHITE,
        )
    draw.polygon(
        [
            (s(x), s(y))
            for x, y in sparkle_points(*SPARKLE_CENTER, SPARKLE_R)
        ],
        fill=WHITE,
    )
    draw.polygon(
        [
            (s(x), s(y))
            for x, y in sparkle_points(*SPARKLE_SMALL_CENTER, SPARKLE_SMALL_R)
        ],
        fill=WHITE,
    )

    plate.resize((BASE, BASE), Image.LANCZOS).save(out)
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
