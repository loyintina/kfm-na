#!/usr/bin/env python3
"""anim-strip-png.py — 动画期回读抽帧串拼图（P3 渲染源真相，2026-09-07）。

用法: python3 scripts/anim-strip-png.py [tail行数，默认 20000]
读 /root/kfm-na/field-reports.log 里 [anim-strip] 行（gles_present.rs
capture_report 分块外发），按 run 拼回原始 RGB，落 PNG 到 /tmp/anim-strip/。

行格式: ts [anim-strip] frameIdx|w|h|chunkIdx|total|hexchunk
判定新 run: frameIdx==0 且 chunkIdx==0（轮次奇偶分流，一拍 4~5 帧）。
PNG 为纯 stdlib 写出（zlib+struct，无 PIL 依赖），RGB8。
"""

import re
import struct
import sys
import zlib
from pathlib import Path

LOG = Path("/root/kfm-na/field-reports.log")
OUT = Path("/tmp/anim-strip")
PAT = re.compile(r"^\S+ \[anim-strip\] (\d+)\|(\d+)\|(\d+)\|(\d+)\|(\d+)\|([0-9a-f]*)$")


def write_png(w: int, h: int, rgb: bytes) -> bytes:
    raw = b"".join(b"\x00" + rgb[y * w * 3 : (y + 1) * w * 3] for y in range(h))

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data))
        )

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(raw, 6))
        + chunk(b"IEND", b"")
    )


def main() -> None:
    tail = int(sys.argv[1]) if len(sys.argv) > 1 else 20000
    lines = LOG.read_text(errors="ignore").splitlines()[-tail:]
    runs: list[dict] = []  # [{key: (w,h,size), chunks: {ci: bytes}}]
    cur = None
    for line in lines:
        m = PAT.match(line.strip())
        if not m:
            continue
        fi, w, h, ci, _total, hexs = m.groups()
        if int(fi) == 0 and int(ci) == 0:
            cur = {"w": int(w), "h": int(h), "size": int(w) * int(h) * 3, "chunks": {}}
            runs.append(cur)
        if cur is None:
            continue
        cur["chunks"][(int(fi), int(ci))] = bytes.fromhex(hexs)
    OUT.mkdir(exist_ok=True)
    made = 0
    for r_idx, r in enumerate(runs):
        # 重组：按 (帧, 块) 排序后顺序填（分块等长+尾块短，排序即偏移序）
        ordered = sorted(r["chunks"].items())
        got = bytearray()
        for (_fi, _ci), data in ordered:
            got += data
        if len(got) != r["size"]:
            print(f"run{r_idx}: 缺块 {len(got)}/{r['size']}——跳过")
            continue
        p = OUT / f"run{r_idx}.png"
        p.write_bytes(write_png(r["w"], r["h"], bytes(got)))
        print(f"✅ {p} ({r['w']}x{r['h']})")
        made += 1
    print(f"共 {made}/{len(runs)} run 拼图 → {OUT}")


if __name__ == "__main__":
    main()
