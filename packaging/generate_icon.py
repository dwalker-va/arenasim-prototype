#!/usr/bin/env python3
"""Generate the ArenaSim application icon.

This script is the source of truth for the icon. It emits, from one set of
geometry constants:

  * ``icon.svg``              -- the vector form, for reference and hand-editing
  * ``icon/icon_<n>.png``     -- rasters at every size the platforms want
  * ``windows/icon.ico``      -- the Windows container (PNG-compressed entries)
  * ``macos/AppIcon.icns``    -- the macOS container, via ``iconutil``

Why a generator instead of a committed SVG plus a rasteriser: this machine has
no ``rsvg-convert``, ImageMagick, Pillow, or cairosvg, and adding one purely to
redraw an icon is a toolchain dependency the project does not otherwise need.
Everything here is Python standard library, so the icon regenerates anywhere
Python runs. The ``.icns`` step is the one exception -- it shells out to
``iconutil``, which only exists on macOS. Run this on a Mac when the art
changes; the generated ``.icns`` is committed so no other platform needs it.

Anti-aliasing is analytic rather than supersampled: every shape is expressed as
a signed distance field, and pixel coverage comes from the distance to the edge.
That renders each size independently at full crispness rather than downsampling
a large master.

Usage:
    python3 packaging/generate_icon.py
"""

from __future__ import annotations

import math
import shutil
import struct
import subprocess
import sys
import zlib
from pathlib import Path

# --- Design -----------------------------------------------------------------
#
# The Nagrand bowl in three-quarter view: an elliptical arena floor ringed in
# gold, four pillars standing on it, and the two teams facing off in the middle.
# Pillars are painted back-to-front so the near one occludes what stands behind
# it, which is what sells the depth.
#
# The team colours are the game's own, taken from the combat log's per-team text
# colours in src/states/play_match/rendering/combat_log.rs.
#
# Known trade-off: a scene does not survive shrinking the way a flat mark does.
# Below roughly 48px the pillars merge and the combatants nearly disappear. That
# is accepted -- the icon is rich where icons are actually looked at, and reads
# as a warm shape on dark at toolbar sizes.

BACKGROUND = (26, 32, 44, 255)      # slate, matches the client's dark chrome
FLOOR = (44, 53, 71, 255)           # arena floor, a step lighter than the frame
FLOOR_RIM = (196, 160, 100, 255)    # the bowl's edge
GOLD_LIT = (234, 202, 142, 255)     # top faces and left flanks
GOLD_SHADE = (168, 133, 82, 255)    # right flanks; light comes from upper-left
TEAM_ONE = (100, 150, 255, 255)     # blue -- combat_log.rs team 1
TEAM_TWO = (255, 100, 100, 255)     # red  -- combat_log.rs team 2

CORNER_RADIUS = 0.22                # rounded-square background, unit coords

# Isometric foreshortening: a ground circle of radius r projects to an ellipse
# with vertical semi-axis r * SQUASH. Lower means a steeper camera.
SQUASH = 0.46

CENTER_Y = 0.075                    # arena centre, pushed down for headroom
FLOOR_R = 0.395
RIM_THICKNESS = 0.030

PILLAR_R = 0.255                    # ground-plane radius the pillars stand on
PILLAR_HALF_W = 0.052
PILLAR_H = 0.215

# Pillar azimuths. Deliberately NOT 45-degree diagonals or cardinals: under this
# projection a four-fold symmetric ring puts the back and front pillars at the
# same screen x, where they stack into two tall columns and the depth read dies.
# Offsetting the ring gives four distinct screen positions and leaves mid-arena
# clear for the combatants.
PILLAR_AZIMUTHS = (67, 157, 247, 337)

COMBATANT_OFFSET = 0.050            # half the gap between the two teams
COMBATANT_R = 0.070
COMBATANT_Y = CENTER_Y + 0.025

SIZES = (16, 32, 48, 64, 128, 256, 512, 1024)
ICO_SIZES = (16, 32, 48, 64, 128, 256)

ICONSET = (
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
)


# --- Signed distance fields -------------------------------------------------
#
# Each returns the distance from a point to the shape's edge, negative inside.
# Unit coordinates keep the geometry resolution-independent.


def sd_rounded_box(x: float, y: float, cx: float, cy: float,
                   hw: float, hh: float, radius: float) -> float:
    qx = abs(x - cx) - hw + radius
    qy = abs(y - cy) - hh + radius
    return (
        math.hypot(max(qx, 0.0), max(qy, 0.0))
        + min(max(qx, qy), 0.0)
        - radius
    )


def sd_ellipse(x: float, y: float, cx: float, cy: float,
               rx: float, ry: float) -> float:
    """Approximate distance to an ellipse -- accurate enough for coverage AA."""
    dx, dy = (x - cx) / rx, (y - cy) / ry
    return (math.hypot(dx, dy) - 1.0) * min(rx, ry)


def coverage(distance: float, pixels_per_unit: float) -> float:
    """Convert a signed distance to pixel coverage in [0, 1].

    The transition is one pixel wide regardless of raster size, which is what
    keeps small sizes crisp rather than blurry.
    """
    return max(0.0, min(1.0, 0.5 - distance * pixels_per_unit))


def over(src: tuple[int, int, int, int], dst: list[int], alpha: float) -> None:
    """Composite ``src`` onto ``dst`` in place with ``alpha`` coverage."""
    if alpha <= 0.0:
        return
    src_a = (src[3] / 255.0) * alpha
    if src_a <= 0.0:
        return
    dst_a = dst[3] / 255.0
    out_a = src_a + dst_a * (1.0 - src_a)
    if out_a <= 0.0:
        dst[0] = dst[1] = dst[2] = dst[3] = 0
        return
    for i in range(3):
        blended = (src[i] * src_a + dst[i] * dst_a * (1.0 - src_a)) / out_a
        dst[i] = int(round(max(0.0, min(255.0, blended))))
    dst[3] = int(round(out_a * 255.0))


# --- Scene ------------------------------------------------------------------


def scene_items() -> list[tuple[float, str, tuple]]:
    """Everything standing on the floor plane, sorted back to front.

    Sorting by screen y is what makes the near pillar occlude the combatants and
    the combatants occlude the far pillar.
    """
    items: list[tuple[float, str, tuple]] = []
    for deg in PILLAR_AZIMUTHS:
        t = math.radians(deg)
        cx = PILLAR_R * math.cos(t)
        cy = CENTER_Y + PILLAR_R * math.sin(t) * SQUASH
        items.append((cy, "pillar", (cx, cy)))
    items.append((COMBATANT_Y, "team", (-COMBATANT_OFFSET, TEAM_ONE)))
    items.append((COMBATANT_Y, "team", (COMBATANT_OFFSET, TEAM_TWO)))
    return sorted(items, key=lambda item: item[0])


def draw_pillar(px: list[int], x: float, y: float,
                cx: float, cy: float, ppu: float) -> None:
    """One column: a shaft split into lit and shaded flanks, plus a top cap."""
    shaft = sd_rounded_box(
        x, y, cx, cy - PILLAR_H / 2, PILLAR_HALF_W, PILLAR_H / 2, 0.018
    )
    # Split the shaft down its own axis so it reads as a cylinder rather than a
    # flat slab. Intersecting with the half-plane at the pillar's centre is
    # exact, so the seam lands on the axis at every raster size.
    over(GOLD_LIT, px, coverage(max(shaft, x - cx), ppu))
    over(GOLD_SHADE, px, coverage(max(shaft, cx - x), ppu))
    over(GOLD_LIT, px, coverage(
        sd_ellipse(x, y, cx, cy - PILLAR_H, PILLAR_HALF_W,
                   PILLAR_HALF_W * SQUASH), ppu))


def render(size: int) -> bytes:
    """Render the icon at ``size``x``size`` and return raw RGBA bytes."""
    ppu = float(size)  # one unit spans the whole canvas
    items = scene_items()
    out = bytearray(size * size * 4)

    for row in range(size):
        # Sample pixel centres, in unit coordinates centred on the canvas.
        y = (row + 0.5) / size - 0.5
        for col in range(size):
            x = (col + 0.5) / size - 0.5

            px = [0, 0, 0, 0]
            over(BACKGROUND, px,
                 coverage(sd_rounded_box(x, y, 0, 0, 0.5, 0.5, CORNER_RADIUS), ppu))

            floor = sd_ellipse(x, y, 0, CENTER_Y, FLOOR_R, FLOOR_R * SQUASH)
            over(FLOOR_RIM, px, coverage(floor, ppu))
            over(FLOOR, px, coverage(floor + RIM_THICKNESS, ppu))

            for _, kind, payload in items:
                if kind == "pillar":
                    draw_pillar(px, x, y, payload[0], payload[1], ppu)
                else:
                    cx, colour = payload
                    over(colour, px, coverage(
                        sd_ellipse(x, y, cx, COMBATANT_Y,
                                   COMBATANT_R, COMBATANT_R * SQUASH), ppu))

            base = (row * size + col) * 4
            out[base : base + 4] = bytes(px)

    return bytes(out)


# --- PNG --------------------------------------------------------------------


def _chunk(tag: bytes, payload: bytes) -> bytes:
    return (
        struct.pack(">I", len(payload))
        + tag
        + payload
        + struct.pack(">I", zlib.crc32(tag + payload) & 0xFFFFFFFF)
    )


def encode_png(rgba: bytes, size: int) -> bytes:
    """Encode raw RGBA bytes as a PNG (8-bit truecolour with alpha)."""
    stride = size * 4
    # Filter type 0 (None) on every scanline -- the artwork is small and flat,
    # so smarter filters would buy negligible size for real complexity.
    raw = b"".join(
        b"\x00" + rgba[row * stride : (row + 1) * stride] for row in range(size)
    )
    return (
        b"\x89PNG\r\n\x1a\n"
        + _chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + _chunk(b"IDAT", zlib.compress(raw, 9))
        + _chunk(b"IEND", b"")
    )


# --- ICO --------------------------------------------------------------------


def encode_ico(pngs: dict[int, bytes]) -> bytes:
    """Pack PNGs into a Windows .ico.

    Entries are stored PNG-compressed, which Windows has accepted since Vista
    and which keeps the 256px entry from bloating the file.
    """
    sizes = sorted(pngs)
    header = struct.pack("<HHH", 0, 1, len(sizes))  # reserved, type=icon, count
    offset = len(header) + 16 * len(sizes)

    entries = bytearray()
    payloads = bytearray()
    for size in sizes:
        data = pngs[size]
        dim = 0 if size >= 256 else size  # 0 in the size byte means 256
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset)
        payloads += data
        offset += len(data)

    return bytes(header + entries + payloads)


# --- SVG --------------------------------------------------------------------


def encode_svg() -> str:
    """Emit the same scene as SVG, from the same constants.

    Reference and hand-editing form. The PNG rasters are what actually ship, so
    if the two ever disagree, this is the one that is wrong.
    """
    S = 512.0

    def hexc(c: tuple[int, int, int, int]) -> str:
        return f"#{c[0]:02x}{c[1]:02x}{c[2]:02x}"

    def px(v: float) -> float:
        """Unit coordinate to SVG user units."""
        return (v + 0.5) * S

    def size(v: float) -> float:
        return v * S

    body: list[str] = []
    clips: list[str] = []

    for index, (_, kind, payload) in enumerate(scene_items()):
        if kind == "pillar":
            cx, cy = payload
            left = px(cx - PILLAR_HALF_W)
            top = px(cy - PILLAR_H)
            width = size(PILLAR_HALF_W * 2)
            height = size(PILLAR_H)
            clip = f"pillar{index}"
            clips.append(
                f'    <clipPath id="{clip}">'
                f'<rect x="{left:.2f}" y="{top:.2f}" width="{width:.2f}"'
                f' height="{height:.2f}" rx="{size(0.018):.2f}"/></clipPath>'
            )
            body.append(
                f'  <g clip-path="url(#{clip})">\n'
                f'    <rect x="{left:.2f}" y="{top:.2f}" width="{width:.2f}"'
                f' height="{height:.2f}" fill="{hexc(GOLD_SHADE)}"/>\n'
                f'    <rect x="{left:.2f}" y="{top:.2f}" width="{width / 2:.2f}"'
                f' height="{height:.2f}" fill="{hexc(GOLD_LIT)}"/>\n'
                f'  </g>\n'
                f'  <ellipse cx="{px(cx):.2f}" cy="{top:.2f}"'
                f' rx="{size(PILLAR_HALF_W):.2f}"'
                f' ry="{size(PILLAR_HALF_W * SQUASH):.2f}"'
                f' fill="{hexc(GOLD_LIT)}"/>'
            )
        else:
            cx, colour = payload
            body.append(
                f'  <ellipse cx="{px(cx):.2f}" cy="{px(COMBATANT_Y):.2f}"'
                f' rx="{size(COMBATANT_R):.2f}"'
                f' ry="{size(COMBATANT_R * SQUASH):.2f}"'
                f' fill="{hexc(colour)}"/>'
            )

    newline = "\n"
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">
  <title>ArenaSim</title>
  <defs>
{newline.join(clips)}
  </defs>
  <rect width="512" height="512" rx="{size(CORNER_RADIUS):.2f}"
        ry="{size(CORNER_RADIUS):.2f}" fill="{hexc(BACKGROUND)}"/>
  <ellipse cx="256" cy="{px(CENTER_Y):.2f}" rx="{size(FLOOR_R):.2f}"
           ry="{size(FLOOR_R * SQUASH):.2f}" fill="{hexc(FLOOR_RIM)}"/>
  <ellipse cx="256" cy="{px(CENTER_Y):.2f}"
           rx="{size(FLOOR_R - RIM_THICKNESS):.2f}"
           ry="{size((FLOOR_R - RIM_THICKNESS) * SQUASH):.2f}"
           fill="{hexc(FLOOR)}"/>
{newline.join(body)}
</svg>
"""


# --- Driver -----------------------------------------------------------------


def main() -> int:
    root = Path(__file__).resolve().parent
    png_dir = root / "icon"
    iconset = root / "macos" / "AppIcon.iconset"

    png_dir.mkdir(parents=True, exist_ok=True)
    (root / "windows").mkdir(parents=True, exist_ok=True)
    if iconset.exists():
        shutil.rmtree(iconset)
    iconset.mkdir(parents=True)

    pngs: dict[int, bytes] = {}
    for size in SIZES:
        print(f"rendering {size}x{size}...", flush=True)
        pngs[size] = encode_png(render(size), size)
        (png_dir / f"icon_{size}.png").write_bytes(pngs[size])

    (root / "icon.svg").write_text(encode_svg())
    (root / "windows" / "icon.ico").write_bytes(
        encode_ico({s: pngs[s] for s in ICO_SIZES})
    )

    for name, size in ICONSET:
        (iconset / name).write_bytes(pngs[size])

    icns = root / "macos" / "AppIcon.icns"
    if shutil.which("iconutil"):
        subprocess.run(
            ["iconutil", "-c", "icns", str(iconset), "-o", str(icns)], check=True
        )
        shutil.rmtree(iconset)
        print(f"wrote {icns.relative_to(root.parent)}")
    else:
        print(
            "iconutil not found (macOS only) -- left the iconset in place; "
            "the committed .icns is unchanged.",
            file=sys.stderr,
        )

    print("done.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
