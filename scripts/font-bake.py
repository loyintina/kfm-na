#!/usr/bin/env python3
"""font-bake.py — KFM-NA 字体烘焙管线（2026-08-18，BAR-021）

两个动作可组合：
  --subset    GB2312 子集化：只保留 GB2312 可编码字符（一级+二级汉字 6763 个
              + 全角符号 + 框线 + ASCII）。9MB 商业字体实测裁到 1.86MB，
              解析 21ms。判卷：'中'/'─'/'M' 必须在，CJK 字数 = 6763。
  --monoify   半角等宽化：ASCII+常用标点步进统一 500 单位（全角 1000 的一半），
              墨迹几何居中、lsb 钉为变换后真实 xMin（lsb=0 会让 freetype 系
              渲染器把窄字符贴左——样张实拍抓到的真 bug）；超宽字形（~/a/m 等
              墨迹 >480 单位的）XY 等比缩放进格（只压 X 会把像素压成长方形发虚）。

用法：
  font-bake.py 源.ttf 出.ttf [--subset] [--monoify]

依赖：fonttools（pip install fonttools）。开发期工具，不进 chain——
烘焙产物（assets/fonts/ 下的 .ttf）直接提交进库，链上零 python 依赖。
"""
import sys

from fontTools.misc.transform import Transform
from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont

HALF = 500  # 半角目标步进（这些像素字体 upm=1000、全角=1000）
HALF_INK_CAP = HALF - 20  # 墨迹上限：留 20 单位边距防相邻格渗透

# 等宽化覆盖范围：ASCII 可打印区 + 常用弯引号/省略号
HALFWIDTH_CPS = list(range(0x20, 0x7F)) + [0x2018, 0x2019, 0x201C, 0x201D, 0x2026]


# 终端符号补丁表（BAR-022）：纯 GB2312 子集把终端命根符号裁没了——
# 盲文转动点（kimi code spinner）、方块元素、▽ 等几何符号、powerline。
# 实拍病灶：U+25BD tofu 目击刷屏。子集 = GB2312 + 以下区间
PATCH_RANGES = [
    (0x2190, 0x21FF),  # 箭头
    (0x2500, 0x257F),  # 框线（GB2312 只含一部分，补全）
    (0x2580, 0x259F),  # 方块元素
    (0x25A0, 0x25FF),  # 几何符号（▽▼◆ 等）
    (0x2800, 0x28FF),  # 盲文（TUI 转动点）
    (0xE0B0, 0xE0D4),  # powerline 私有区
]


def gb2312_unicodes():
    """GB2312 可编码字符全集 + 终端符号补丁表"""
    cps = []
    for cp in range(0x20, 0xFFFE + 1):
        if 0xD800 <= cp <= 0xDFFF:
            continue
        try:
            chr(cp).encode("gb2312")
            cps.append(cp)
        except (UnicodeEncodeError, ValueError):
            pass
    for lo, hi in PATCH_RANGES:
        cps.extend(range(lo, hi + 1))
    return sorted(set(cps))


def ink_bounds(glyf, gname):
    g = glyf[gname]
    if g.numberOfContours == 0:
        return None
    pen = BoundsPen(glyf)
    g.draw(pen, glyf)
    return pen.bounds


def monoify(font):
    glyf = font["glyf"]
    hmtx = font["hmtx"]
    cmap = font.getBestCmap()
    stats = {"center": 0, "squeeze": 0, "empty": 0}
    for cp in HALFWIDTH_CPS:
        gname = cmap.get(cp)
        if not gname:
            continue
        b = ink_bounds(glyf, gname)
        if not b:
            hmtx[gname] = (HALF, 0)
            stats["empty"] += 1
            continue
        xMin, yMin, xMax, yMax = b
        iw, ih = xMax - xMin, yMax - yMin
        if iw <= HALF_INK_CAP:
            t = Transform(1, 0, 0, 1, (HALF - iw) / 2 - xMin, 0)
            stats["center"] += 1
        else:
            # 超宽：XY 等比缩放（保像素正方形）以底边为锚，再水平居中
            s = HALF_INK_CAP / iw
            t = Transform(s, 0, 0, s, (HALF - iw * s) / 2 - xMin * s, yMin - yMin * s)
            stats["squeeze"] += 1
        pen = TTGlyphPen(glyf)
        glyf[gname].draw(TransformPen(pen, t), glyf)
        glyf[gname] = pen.glyph()
        # lsb 必须钉成变换后的真实 xMin：lsb=0 时 freetype 系渲染器
        # 会把墨迹贴到格子左缘（窄字符「局左」病灶）
        hmtx[gname] = (HALF, ink_bounds(glyf, gname)[0])
    font["post"].isFixedPitch = 1
    return stats


def subset(font, out_path):
    from fontTools import subset as fts

    opts = fts.Options()
    opts.layout_features = ["*"]
    opts.hinting = False
    opts.desubroutinize = True
    unicodes = gb2312_unicodes()
    subsetter = fts.Subsetter(opts)
    subsetter.populate(unicodes=unicodes)
    subsetter.subset(font)
    font.save(out_path)
    return len(unicodes)


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        sys.exit(1)
    src, dst = sys.argv[1], sys.argv[2]
    do_subset = "--subset" in sys.argv
    do_monoify = "--monoify" in sys.argv
    font = TTFont(src)
    if do_monoify:
        print("monoify:", monoify(font))
    if do_subset:
        n = subset(font, dst)
        print(f"subset: GB2312 字符表 {n} 个")
    else:
        font.save(dst)
    # 判卷：出的字体必须真能用
    check = TTFont(dst)
    cmap = check.getBestCmap()
    for ch, cp in [("中", 0x4E2D), ("─", 0x2500), ("M", 0x4D)]:
        assert cp in cmap, f"烘焙产物缺 {ch}"
    # 终端符号补丁覆盖报告（源字体没有的不强求，但要知道缺什么）
    patch = [("▽", 0x25BD), ("█", 0x2588), ("⠋", 0x280B), ("→", 0x2192), ("", 0xE0B0)]
    missing = [f"{ch}U+{cp:04X}" for ch, cp in patch if cp not in cmap]
    print("补丁表缺口:", ",".join(missing) if missing else "无")
    if do_subset:
        cjk = sum(1 for cp in cmap if 0x4E00 <= cp <= 0x9FFF)
        # 满覆盖 = 6763；开源占位字体允许缺少量二级生僻字（缝合像素实测
        # 6618/6763，缺的 145 个全是罕用字），但跌破 6500 就是选错字体了
        print(f"GB2312 汉字覆盖: {cjk}/6763")
        assert cjk >= 6500, f"汉字覆盖过低（{cjk}）——换字体或查子集参数"
    if do_monoify:
        hmtx = check["hmtx"]
        advs = {hmtx[cmap[cp]][0] for cp in range(0x21, 0x7F) if cp in cmap}
        assert advs == {HALF}, f"半角步进应全为 {HALF}，实得 {advs}"
        # lsb 病灶钉：lsb 必须 = 轮廓真实 xMin——lsb=0 时 freetype 系
        # 渲染器把窄字符贴到格子左缘（样张实拍抓到的真 bug）
        cglyf = check["glyf"]
        for cp in (ord("i"), ord("l"), ord("|"), ord("w"), ord("M")):
            gn = cmap.get(cp)
            if gn:
                b = ink_bounds(cglyf, gn)
                if b:
                    assert hmtx[gn][1] == b[0], f"U+{cp:04X} lsb({hmtx[gn][1]}) != xMin({b[0]})"
    print("判卷通过:", dst)


if __name__ == "__main__":
    main()
