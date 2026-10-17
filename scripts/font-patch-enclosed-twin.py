#!/usr/bin/env python3
"""font-patch-enclosed-twin.py — 戊/辛直补：往在产烘焙品写带圈族孪生

两版都写：窄版 62 枚（U+E000 起，advance 500/墨盒 480，终端 1 格常规路）
＋溢格版 62 枚（U+E03E 起，advance 仍 500/墨盒 824，左右邻空白才消费）。
生成逻辑与 font-bake-cjk.py 的 redraw_enclosed_twins／
redraw_enclosed_wide_twins 同一核（未来重烘自带，本脚本只是把该步直补到
在产 ttf，免一轮全量重烘）。幂等覆盖。

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
    n_wide = bake.redraw_enclosed_wide_twins(font)
    font.save(dst)

    check = TTFont(dst)
    cmap, cglyf, chmtx = check.getBestCmap(), check["glyf"], check["hmtx"]
    for cp in bake.ENCLOSED_CPS:
        t = bake.twin_cp(cp)
        gn = cmap.get(t)
        assert gn, f"U+{t:04X} 窄版孪生 cmap 未命中"
        assert chmtx[gn][0] == bake.HALF, f"U+{t:04X} 孪生步进非半格"
        b = bake.ink_bounds(cglyf, gn)
        assert b and 400 <= b[2] - b[0] <= bake.HALF, f"U+{t:04X} 墨宽 {b} 出界"

        w = bake.wide_twin_cp(cp)
        gw = cmap.get(w)
        assert gw, f"U+{w:04X} 溢格孪生 cmap 未命中"
        assert chmtx[gw][0] == bake.HALF, f"U+{w:04X} 溢格孪生 advance 非半格"
        bw = bake.ink_bounds(cglyf, gw)
        assert bw and 800 <= bw[2] - bw[0] <= bake.WIDE_TWIN_INK, \
            f"U+{w:04X} 溢格墨宽出界 {bw}"
        assert bw[0] < 0 and bw[2] > bake.HALF, f"U+{w:04X} 溢格孪生未两侧出格 {bw}"
    size_mb = os.path.getsize(dst) / 1048576
    print(f"直补完成：窄版 {n} 枚＋溢格版 {n_wide} 枚写入 {dst}（{size_mb:.2f}MB），判卷通过")


if __name__ == "__main__":
    main()
