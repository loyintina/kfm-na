#!/usr/bin/env python3
"""font-patch-enclosed-twin.py — 戊直补：往在产烘焙品写带圈族窄版孪生

生成逻辑与 font-bake-cjk.py 的 redraw_enclosed_twins 同一函数（未来重烘
自带，本脚本只是把该步直补到在产 ttf，免一轮全量重烘）。幂等覆盖。

用法：python3 scripts/font-patch-enclosed-twin.py [目标 ttf]
（缺省 = assets/fonts/NotoSansCJKsc-kfm.ttf，原地改）
"""
import importlib.util
import os
import sys

from fontTools.ttLib import TTFont

spec = importlib.util.spec_from_file_location(
    "bake", os.path.join(os.path.dirname(__file__), "font-bake-cjk.py")
)
bake = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bake)


def main():
    dst = sys.argv[1] if len(sys.argv) > 1 else "assets/fonts/NotoSansCJKsc-kfm.ttf"
    font = TTFont(dst)
    n = bake.redraw_enclosed_twins(font)
    font.save(dst)

    check = TTFont(dst)
    cmap, cglyf, chmtx = check.getBestCmap(), check["glyf"], check["hmtx"]
    for cp in bake.ENCLOSED_CPS:
        t = bake.twin_cp(cp)
        gn = cmap.get(t)
        assert gn, f"U+{t:04X} 孪生 cmap 未命中"
        assert chmtx[gn][0] == bake.HALF, f"U+{t:04X} 孪生步进非半格"
        b = bake.ink_bounds(cglyf, gn)
        assert b and 400 <= b[2] - b[0] <= bake.HALF, f"U+{t:04X} 墨宽 {b} 出界"
    size_mb = os.path.getsize(dst) / 1048576
    print(f"戊直补完成：{n} 枚孪生写入 {dst}（{size_mb:.2f}MB），判卷通过")


if __name__ == "__main__":
    main()
