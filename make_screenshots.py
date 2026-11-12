#!/usr/bin/env python3
"""Render high-fidelity ISOLoader desktop screenshots for README.

The renderer mirrors the Anthropic-inspired layout of cli/src/gui.rs
(warm ivory surface, white cards, terracotta accent) at 2x scale so the
resulting PNGs stay crisp on high-DPI GitHub displays.
"""

import os

from PIL import Image, ImageDraw, ImageFont

S = 2  # supersampling factor
W, H = 860 * S, 720 * S

BG = (250, 248, 245)
CARD = (255, 255, 255)
BORDER = (232, 226, 217)
TEXT = (33, 31, 28)
MUTED = (117, 110, 100)
TERRA = (190, 91, 57)
TERRA_LIGHT = (247, 237, 231)
WARN = (165, 87, 42)
SUCCESS = (60, 116, 75)
DISABLED = (215, 208, 198)

REG = "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf"
BOLD = "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf"
MONO = "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf"
SYMBOLS = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"


def font(size, bold=False, mono=False, symbols=False):
    if symbols:
        path = SYMBOLS
    else:
        path = MONO if mono else (BOLD if bold else REG)
    return ImageFont.truetype(path, size * S)


def text(draw, xy, s, size, color, bold=False, mono=False, anchor="la"):
    f, sym = font(size, bold, mono), font(size, symbols=True)
    if any(ord(c) > 0x2700 for c in s):
        # Glyph fallback: draw leading symbol with DejaVu, rest with the
        # sans face, keeping the baseline aligned.
        if anchor == "ra":
            off = draw.textlength(s, font=f) / S
            x = xy[0] - off
            draw.text((x * S, xy[1] * S), s[0], font=sym, fill=color, anchor="la")
            draw.text(((x + draw.textlength(s[0], font=sym) / S) * S, xy[1] * S), s[1:], font=f, fill=color, anchor="la")
        else:
            head, rest = s[0], s[1:]
            draw.text((xy[0] * S, xy[1] * S), head, font=sym, fill=color, anchor="la")
            off = draw.textlength(head, font=sym) / S
            draw.text(((xy[0] + off) * S, xy[1] * S), rest, font=f, fill=color, anchor="la")
    else:
        draw.text((xy[0] * S, xy[1] * S), s, font=f, fill=color, anchor=anchor)


def round_rect(draw, box, radius, fill=None, outline=None, width=1):
    x0, y0, x1, y1 = [v * S for v in box]
    draw.rounded_rectangle(
        (x0, y0, x1, y1), radius=radius * S, fill=fill, outline=outline, width=width * S
    )


def card(draw, y, height):
    round_rect(draw, (24, y, 836, y + height), 12, CARD, BORDER, 1)


def section_header(draw, x, y, num, title):
    text(draw, (x, y), num, 13, TERRA, mono=True)
    text(draw, (x + 30, y - 2), title, 16, TEXT, bold=True)


def screenshot(path, busy=False):
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)

    # --- Header ---------------------------------------------------------
    text(d, (24, 26), "✦", 20, TERRA)
    text(d, (48, 26), "ISOLoader", 20, TEXT, bold=True)
    text(d, (146, 30), "—  A thoughtful utility for bootable drives", 14, MUTED)
    d.line([(24 * S, 66 * S), (836 * S, 66 * S)], fill=BORDER, width=S)

    # --- Headline -------------------------------------------------------
    text(d, (24, 92), "Prepare your USB drive", 28, TEXT, bold=True)
    text(
        d,
        (24, 130),
        "Write an ISO or disk image directly to removable media with block-level safety.",
        14.5,
        MUTED,
    )

    # --- Step 1: image --------------------------------------------------
    y = 168
    card(d, y, 96)
    section_header(d, 42, y + 18, "01", "Disk Image")
    text(d, (42, y + 50), "ubuntu-24.04.2-desktop-amd64.iso", 15, TEXT, bold=True)
    text(d, (42, y + 72), "File size: 5.8 GiB", 13, MUTED)

    bw, bh = 118, 34
    bx0, by0 = 836 - 18 - bw, y + 30
    round_rect(d, (bx0, by0, bx0 + bw, by0 + bh), 8, TERRA_LIGHT, BORDER, 1)
    text(d, (bx0 + bw / 2, by0 + bh / 2 + 1), "Choose file…", 14, TERRA, anchor="mm")

    # --- Step 2: drives -------------------------------------------------
    y = 280
    card(d, y, 196)
    section_header(d, 42, y + 18, "02", "Destination Device")
    text(d, (836 - 18, y + 21), "⟳  Refresh list", 13, MUTED, anchor="ra")

    rows = [
        ("⬚", "SanDisk Ultra USB 3.0", "/dev/sdc  ·  28.6 GiB", True),
        ("⬚", "Samsung Flash Drive FIT", "/dev/sdd  ·  57.3 GiB", False),
    ]
    for i, (icon, model, sub, selected) in enumerate(rows):
        ry = y + 48 + i * 66
        if selected:
            round_rect(d, (42, ry, 818, ry + 56), 8, TERRA_LIGHT, TERRA, 2)
        else:
            round_rect(d, (42, ry, 818, ry + 56), 8, None, BORDER, 1)
        round_rect(d, (58, ry + 18, 74, ry + 34), 3, None, TERRA if selected else (198, 190, 180), 1)
        round_rect(d, (64, ry + 24, 68, ry + 28), 1, TERRA if selected else (198, 190, 180))
        text(d, (90, ry + 10), model, 15, TEXT, bold=True)
        text(d, (90, ry + 32), sub, 12.5, MUTED)

    # --- Step 3: verification ------------------------------------------
    y = 492
    card(d, y, 184)
    section_header(d, 42, y + 18, "03", "Verification & Progress")

    # checked checkbox
    box = (43, y + 48, 59, y + 64)
    round_rect(d, box, 4, TERRA, TERRA, 1)
    d.line(
        [(47 * S, y + 56 * S), (50 * S, y + 59 * S), (56 * S, y + 51 * S)],
        fill=(255, 255, 255),
        width=2 * S,
    )
    text(d, (68, y + 48), "Verify written data against SHA-256 hash", 14, TEXT)

    if busy:
        status, color = "Flashing image. Keep the drive connected.", TERRA
        fraction, label = 0.68, "4.0 GiB / 5.8 GiB"
    else:
        status, color = "Select a disk image and a destination USB drive to begin.", MUTED
        fraction, label = 0.0, "Ready"

    text(d, (42, y + 78), status, 13.5, color)

    bar = (42, y + 100, 818, y + 116)
    round_rect(d, bar, 6, (240, 235, 228))
    if fraction > 0:
        round_rect(d, (bar[0], bar[1], bar[0] + (bar[2] - bar[0]) * fraction, bar[3]), 6, TERRA)
    label_line = f"{label}  ·  68%" if fraction > 0 else label
    text(d, (818, y + 76), label_line, 12, MUTED, anchor="ra")

    round_rect(d, (42, y + 140, 51, y + 149), 1, None, WARN, 1)
    text(d, (44.5, y + 139.5), "!", 8, WARN, bold=True)
    text(
        d,
        (58, y + 135),
        "Writing will permanently overwrite all files on the selected drive.",
        12.5,
        WARN,
    )

    bw, bh = 140, 36
    bx0, by0 = 836 - 18 - bw, y + 128
    enabled = True
    round_rect(d, (bx0, by0, bx0 + bw, by0 + bh), 8, TERRA if enabled else DISABLED)
    label = "Writing… " if busy else "Write to USB  →"
    text(d, (bx0 + bw / 2, by0 + bh / 2 + 1), label, 15, (255, 255, 255), bold=True, anchor="mm")

    out = path
    img.resize((W // S, H // S), Image.LANCZOS).save(out)
    print("wrote", out)


def main():
    os.makedirs("screenshots", exist_ok=True)
    screenshot("screenshots/ubuntu-ready.png", busy=False)
    screenshot("screenshots/ubuntu-progress.png", busy=True)


if __name__ == "__main__":
    main()
