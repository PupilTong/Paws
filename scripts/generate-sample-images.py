#!/usr/bin/env python3
"""Generates procedural sample PNGs used by the inline-image examples.

Writes four tiny PNGs:
- examples/example-inline-image/assets/paws-logo.png (blue square, 64x64)
- examples/yew/example-yew-photo-cycle/assets/photo-1.png (red, 64x64)
- examples/yew/example-yew-photo-cycle/assets/photo-2.png (green, 64x64)
- examples/yew/example-yew-photo-cycle/assets/photo-3.png (blue, 64x64)

PNG encoding is hand-rolled via zlib so the script has no third-party
dependencies. Regenerate on demand; the outputs are committed as binaries.
"""
import os
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def make_png(width: int, height: int, rgb: tuple[int, int, int]) -> bytes:
    """Encode a solid-color width x height image as a PNG."""
    r, g, b = rgb
    # Each row: 1 filter byte (0 = None) + width * 3 bytes of RGB.
    row = bytes([0]) + bytes([r, g, b] * width)
    raw = row * height

    def chunk(tag: bytes, data: bytes) -> bytes:
        crc = zlib.crc32(tag + data) & 0xFFFFFFFF
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", crc)

    signature = b"\x89PNG\r\n\x1a\n"
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    idat = zlib.compress(raw, 9)
    return signature + chunk(b"IHDR", ihdr) + chunk(b"IDAT", idat) + chunk(b"IEND", b"")


def write(path: Path, png_bytes: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(png_bytes)
    print(f"wrote {path.relative_to(ROOT)} ({len(png_bytes)} bytes)")


def main() -> None:
    write(
        ROOT / "examples" / "example-inline-image" / "assets" / "paws-logo.png",
        make_png(64, 64, (10, 132, 255)),  # Apple-ish accent blue
    )
    photo_dir = ROOT / "examples" / "yew" / "example-yew-photo-cycle" / "assets"
    write(photo_dir / "photo-1.png", make_png(64, 64, (220, 60, 60)))   # red
    write(photo_dir / "photo-2.png", make_png(64, 64, (60, 200, 90)))   # green
    write(photo_dir / "photo-3.png", make_png(64, 64, (60, 120, 220)))  # blue


if __name__ == "__main__":
    main()
