#!/usr/bin/env python3
"""Convert a MODE2/2352 .bin (raw 2352-byte sectors) to a cooked 2048-byte .iso.

Each 2352-byte Mode 2 sector is laid out as:
    0..12   sync
    12..15  address (min, sec, frame)
    15      mode (0x02)
    16..24  subheader (8 bytes, duplicated -> verified identical)
    24..    user data
and the user-data payload is 2048 bytes for Form 1 (subheader bit 5 clear) or
2324 bytes for Form 2 (subheader bit 5 set, used for XA-ADPCM audio).

A cooked ISO has a fixed 2048-byte payload per sector, so Form 1 sectors map
cleanly; Form 2 sectors are truncated to their first 2048 bytes, which keeps
every LBA in place (the ISO9660 file system lives in Form 1) but drops the
extra audio/stream bytes those sectors carry.
"""
import hashlib
import os
import sys

SECTOR = 2352
ISO_SECTOR = 2048
CHUNK_SECTORS = 4096


def subheader_form(sec: bytes) -> int:
    sub = sec[16:24]
    assert sub[:4] == sub[4:], "subheader halves differ"
    return (sub[2] >> 5) & 1


def convert(src: str, dst: str) -> None:
    size = os.path.getsize(src)
    total = size // SECTOR
    assert size % SECTOR == 0, f"{src}: trailing partial sector"

    form_counts = [0, 0]
    digest = hashlib.sha256()
    with open(src, "rb") as fin, open(dst, "wb") as fout:
        for start in range(0, total, CHUNK_SECTORS):
            n = min(CHUNK_SECTORS, total - start)
            buf = fin.read(n * SECTOR)
            assert len(buf) == n * SECTOR
            out = bytearray(n * ISO_SECTOR)
            for j in range(n):
                base = j * SECTOR
                sec = buf[base:base + SECTOR]
                form_counts[subheader_form(sec)] += 1
                out[j * ISO_SECTOR:(j + 1) * ISO_SECTOR] = sec[24:24 + ISO_SECTOR]
            fout.write(out)
            digest.update(out)

    print(f"{os.path.basename(dst)}")
    print(f"  sectors: {total}  (form1={form_counts[0]}, form2={form_counts[1]})")
    print(f"  size:    {os.path.getsize(dst)} bytes")
    print(f"  sha256:  {digest.hexdigest()}")


if __name__ == "__main__":
    for path in sys.argv[1:]:
        convert(path, os.path.splitext(path)[0] + ".iso")
