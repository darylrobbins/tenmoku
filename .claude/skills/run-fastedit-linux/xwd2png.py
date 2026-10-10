#!/usr/bin/env python3
"""Converts Xvfb's -fbdir framebuffer (an XWD file) to a PNG, with only the
standard library: `xwd2png.py <Xvfb_screen0> <out.png>`. Assumes the 24-bit
TrueColor screen `driver.sh` starts (32 bits per pixel, little-endian BGRX).
"""

import struct
import sys
import zlib


def main(src: str, out: str) -> None:
    data = open(src, "rb").read()
    header = struct.unpack(">25I", data[:100])
    header_size, width, height = header[0], header[4], header[5]
    bits_per_pixel, bytes_per_line, ncolors = header[11], header[12], header[19]
    if bits_per_pixel != 32:
        sys.exit(f"xwd2png: expected 32 bits per pixel, got {bits_per_pixel}")
    pixels = header_size + ncolors * 12
    rows = []
    for y in range(height):
        row = data[pixels + y * bytes_per_line : pixels + y * bytes_per_line + width * 4]
        rgb = bytearray(width * 3)
        rgb[0::3], rgb[1::3], rgb[2::3] = row[2::4], row[1::4], row[0::4]
        rows.append(b"\0" + bytes(rgb))

    def chunk(kind: bytes, body: bytes) -> bytes:
        crc = zlib.crc32(kind + body)
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", crc)

    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(b"".join(rows)))
        + chunk(b"IEND", b"")
    )
    open(out, "wb").write(png)
    # Distinct colours in a sparse sample: 1 means a blank frame.
    sample = {bytes(r[i : i + 3]) for r in rows[::16] for i in range(1, len(r) - 2, 48)}
    print(f"{out} {width}x{height} colours~{len(sample)}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
