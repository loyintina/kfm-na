#!/usr/bin/env python3
"""font-bake-cjk.py — KFM-NA CJK 备用字体烘焙管线（2026-09-29，BAR-176）

为什么存在：旧 CJK 备用（FusionPixelMono12-gb2312）只有 GB2312 子集，
超集汉字（如「槃」）主备双缺 = 终端纯黑不可见。本管线把 Noto Sans CJK SC
（SIL OFL，可嵌可再分发）烘成终端等宽可用的「尽量全」黑体大字库：
像素气质让位于覆盖率（用户拍板），汉字一个不许裁。

四道工序（顺序敏感，不可乱）：
  1. subset   先在 CFF 面上裁（快）：保 BMP 汉字全量（URO U+4E00-9FFF +
              扩展 A U+3400-4DBF + 兼容表意 U+F900-FAFF）+ GB2312 全表
              （含全角符号区）+ ASCII + 终端符号补丁表 + 借字清单码位。
              Ext B+（SIP）从第一刀就不保——要进 git，体积账见判卷。
  2. otf2ttf  Noto 是 CFF 轮廓，借形/归格机械要 glyf 表——cu2qu 转
              TrueType（2.8 万字形转换要几分钟，正常）。
  3. 归格     终端按 unicode-width 摆格：宽字符（EAW W/F）保 1em 全角位；
              窄字符 advance 归 0.5em、墨迹居中（超宽 XY 等比压、lsb 钉
              真实 xMin——monoify 同律；Noto 的 ASCII 'M' 原生 812/1000，
              不归格会占 1.6 格）。例外：框线/方块区（U+2500-259F）只做
              X 向压半格（Y 不动）——XY 等比会把线宽压没（tmux 边框
              变虚线），X 压保笔画保拼接。格式控制符（Cf）不动。
  4. borrow   Noto 没有的码点从捐体补（「源字体已有码点跳过」守卫 =
              黑体气质优先）：PATCH/月亮/FFFD 优先捐体 FusionPixel（旧
              备用，符号补丁最全的捐体，含合成 powerline 三角）；Latin/
              通用标点/货币补漏捐体 DejaVuMono→DejaVuSans；emoji 捐体
              NotoEmoji（OFL）。powerline（E0A0-E0D4）走专用律：墨迹
              X 向钉满半格左右缘（拼接件不许留边距），不用通用居中律。

用法（开发期工具，不进 chain；产物 assets/fonts/*.ttf 直接进库）：
  python3 scripts/font-bake-cjk.py 源.otf 出.ttf

判卷（脚本尾自打印+断言）：槃/中/🌑/⠋/░/─/✅/E0B0/⚡/FFFD 的 cmap
命中 + 墨迹非空；汉字总数（URO+ExtA+兼容）≥ 27900；体积 ≤ 15MB；
ASCII 步进全 500；月亮全角位；E0B0/E0B2 墨迹贴满半格左右缘。
"""
import os
import sys
import unicodedata

from fontTools import subset as fts
from fontTools.misc.transform import Transform
from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.recordingPen import DecomposingRecordingPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont, newTable

UPM = 1000          # Noto 与产物同 upm（捐体缩放按各自 upm 比例）
HALF = UPM // 2     # 半角格（= 终端 1 格；全角 1em = 2 格）
INK_CAP = HALF - 20  # 居中律墨迹上限：留 20 单位边距防相邻格渗透

# ---- 保留码位表（子集决策，先量后定：Ext B+ 从第一刀就不保） ----
HAN_RANGES = [(0x4E00, 0x9FFF), (0x3400, 0x4DBF), (0xF900, 0xFAFF)]

# 终端符号补丁表（与 font-bake.py 同源，BAR-022 起）
PATCH_RANGES = [
    (0x2190, 0x21FF),  # 箭头
    (0x2500, 0x257F),  # 框线
    (0x2580, 0x259F),  # 方块元素
    (0x25A0, 0x25FF),  # 几何符号
    (0x2600, 0x26FF),  # 杂项符号
    (0x2700, 0x27BF),  # 装饰符号
    (0x2800, 0x28FF),  # 盲文（kimi code spinner）
    (0xE0A0, 0xE0D4),  # powerline 私有区
]
BORROW_CPS = [0x26A1, 0x2713, 0x2717, 0x2718, 0x271A, 0x279C, 0x27A6]  # ⚡✓✗✘✚➜➦
# 带圈族（BAR-242 翻案，2026-10-14 用户拍板「要真带圈，不接受降级」）：
# Ⓐ-ⓩ U+24B6-24E9 全族 + ⑪-⑳ U+246A-2473 补齐（①-⑩ 已由 GB2312 全表带到）。
# Noto CJK 本家与 DejaVu 双捐体都没有这一族（fc-list :charset=24d0 全库零
# 供体实证）——捐体 = Noto Sans Symbols（SIL OFL 同族许可，见 README）。
# 宽度口径（同日二修，用户真机实测半格版「太小、几乎看不到」）：半格物理
# 上限 = 墨盒 480 单位（17px 圆、圈内字母 ~7px，数字①能读、小写字母不能读）
# ——显式 cells=2 全角（824 墨盒零缩放，≈「中」90% 高，CJK 惯例全角），UI 文本
# 路径 char_cells 同族判 2 格（BAR-199 箭头块同型先例）；终端格路径格宽锁死
# 放不进 → degrade_cell 落单个 ASCII 字母。①-⑩ 未并轨（存量八千处零报障，
# 并轨另裁）。
ENCLOSED_CPS = list(range(0x24B6, 0x24EA)) + list(range(0x246A, 0x2474))
MOON_CPS = list(range(0x1F311, 0x1F319))  # 🌑-🌘
SYMBOL_NARROW_CPS = (
    list(range(0x00A0, 0x0100))
    + list(range(0x2000, 0x2070))
    + list(range(0x20A0, 0x20C0))
    + [0x26A0]
)
EMOJI_CPS = [
    0x2705, 0x274C, 0x2757, 0x2753, 0x2728, 0x2B50, 0x23F0, 0x267B,
    0x1F4A1, 0x1F525, 0x1F389, 0x1F680, 0x1F6A8, 0x1F514, 0x1F512,
    0x1F513, 0x1F50D, 0x1F4C5, 0x1F4E6, 0x1F41B, 0x1F480, 0x1F44D,
    0x1F44E, 0x1F3AF, 0x1F4CC, 0x1F4CE, 0x1F4BB, 0x1F4F1, 0x1F527,
]
FFFD_CPS = [0xFFFD]

# 旧备用=最全符号捐体。BAR-176 后该文件已退库（git 历史可查）——重烘前先
# 取回：git show <BAR-176 提交^>:assets/fonts/FusionPixelMono12-gb2312.ttf > /tmp/fp.ttf
FP_DONOR = os.environ.get("FP_DONOR", "assets/fonts/FusionPixelMono12-gb2312.ttf")
DEJAVU_MONO = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
DEJAVU_SANS = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
EMOJI_DONOR = os.path.expanduser("~/40-资产/kfm-na-toolchain/fonts/NotoEmoji-Regular.ttf")
# 带圈族捐体（BAR-242）：Noto Sans Symbols，SIL OFL（与 Noto CJK 同族许可）
NSS_DONOR = os.path.expanduser("~/40-资产/kfm-na-toolchain/fonts/NotoSansSymbols-Regular.ttf")

# ---- 戊（BAR-242 重开，2026-10-16 NA0171 楼26 判做）：带圈族窄版孪生 ----
# 终端格宽由 tmux/unicode-width 判 1 格，全角 ⓐ 放不进——degrade_cell
# 条件②原降级单字母（用户翻案「要真带圈」）。戊＝同尺寸重试：PUA 窄版
# 孪生（U+E000 起 62 连位，与 ENCLOSED_CPS 同序一一映射），**重绘非缩绘**
# ——环粗 60 墨盒＋圈内字母墨高 ≈55% 圆径（NSS 缩绘版字母仅 ~40%，一修
# 「太小」真因）。主码位全角字形不动（UI 面二修口径不破）；孪生只被
# termview 条件②改道消费。字母捐体 DejaVuSans-Bold（SIL 自由许可族）。
TWIN_BASE = 0xE000
TWIN_LETTER_DONOR = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"

# ---- 辛（BAR-242 再重开，2026-10-17 NA0171 楼38 白露拍「溢格孪生」）----
# 用户判戊「有圈但太小」：1 格 advance=500，孪生墨盒已占 96%＝顶格，格内
# 无余量（字体实测 adv 500/ink 480）。辛＝**advance 仍半格、墨盒放到 824**
# （±162 单位横向溢格 ≈ 半格宽的 32%，对齐二修全角墨盒），渲染侧只在左右
# 邻皆空白时放行 2 格裁剪——列账/wcwidth/tmux 对齐零改动，只动绘制裁剪。
# 62 枚接在窄版之后（U+E03E 起，与 ENCLOSED_CPS 同序）。
WIDE_TWIN_BASE = TWIN_BASE + 62
WIDE_TWIN_INK = 824   # 与二修全角 ⓐ 同墨盒（≈「中」92% 高）
WIDE_RING = 103       # 环粗：窄版 60/480 的同比例（12.5% 圆径）


def keep_unicodes():
    cps = set(range(0x20, 0x7F))
    for cp in range(0x20, 0xFFFE + 1):
        if 0xD800 <= cp <= 0xDFFF:
            continue
        try:
            chr(cp).encode("gb2312")
            cps.add(cp)
        except (UnicodeEncodeError, ValueError):
            pass
    for lo, hi in HAN_RANGES + PATCH_RANGES:
        cps.update(range(lo, hi + 1))
    cps.update(BORROW_CPS + MOON_CPS + SYMBOL_NARROW_CPS + EMOJI_CPS + FFFD_CPS + ENCLOSED_CPS)
    return sorted(cps)


def subset_cff(font):
    opts = fts.Options()
    opts.layout_features = []  # 终端等宽备用件不要 shaping 表（省体积）
    opts.name_IDs = ["*"]      # name 表全保（许可记录在里面）
    unicodes = keep_unicodes()
    s = fts.Subsetter(opts)
    s.populate(unicodes=unicodes)
    s.subset(font)
    return len(unicodes)


def otf_to_ttf(font, max_err=1.0):
    """CFF → TrueType（fontTools Snippets/otf2ttf.py 同律；2.8 万字形
    cu2qu 转换要几分钟，正常）"""
    assert font.sfntVersion == "OTTO" and "CFF " in font
    glyph_order = font.getGlyphOrder()
    glyph_set = font.getGlyphSet()
    font["loca"] = newTable("loca")
    font["glyf"] = glyf = newTable("glyf")
    glyf.glyphOrder = glyph_order
    glyphs = {}
    for i, name in enumerate(glyph_order):
        pen = TTGlyphPen(glyph_set)
        glyph_set[name].draw(Cu2QuPen(pen, max_err))
        glyphs[name] = pen.glyph()
        if (i + 1) % 5000 == 0:
            print(f"  otf2ttf: {i + 1}/{len(glyph_order)}", flush=True)
    glyf.glyphs = glyphs
    del font["CFF "]
    for t in ("VORG", "DSIG"):  # CFF 专用/签名表，TTF 化后无效
        if t in font:
            del font[t]
    font["maxp"] = maxp = newTable("maxp")
    maxp.tableVersion = 0x00010000
    maxp.maxZones = 1
    maxp.maxTwilightPoints = 0
    maxp.maxFunctionDefs = 0
    maxp.maxInstructionDefs = 0
    maxp.maxStorage = 0
    maxp.maxStackElements = 0
    maxp.maxSizeOfInstructions = 0
    maxp.maxComponentElements = 0  # 全简单轮廓（cu2qu 直接展开）
    font.sfntVersion = "\x00\x01\x00\x00"


def ink_bounds(glyf, gname):
    g = glyf[gname]
    if g.numberOfContours == 0:
        return None
    pen = BoundsPen(glyf)
    g.draw(pen, glyf)
    return pen.bounds


def rewrite_glyph(glyf, gname, t):
    pen = TTGlyphPen(glyf)
    glyf[gname].draw(TransformPen(pen, t), glyf)
    glyf[gname] = pen.glyph()


def normalize_narrow(font):
    """窄字符归半格（任务工序 3）。EAW W/F 与格式控制符不动；
    框线/方块区 X 向压半格保笔画保拼接，其余走 monoify 居中律。"""
    glyf, hmtx, cmap = font["glyf"], font["hmtx"], font.getBestCmap()
    stats = {"box_x": 0, "center": 0, "squeeze": 0, "empty": 0, "skip_wide": 0, "skip_cf": 0}
    for cp, gname in sorted(cmap.items()):
        if cp > 0xFFFF:
            continue  # SMP（月亮/emoji）由 borrow 自己归格
        eaw = unicodedata.east_asian_width(chr(cp))
        if eaw in ("W", "F"):
            stats["skip_wide"] += 1
            continue
        if unicodedata.category(chr(cp)) == "Cf":
            stats["skip_cf"] += 1
            continue
        b = ink_bounds(glyf, gname)
        if 0x2500 <= cp <= 0x259F:
            # 框线/方块：X 向压半格（Y 不动）——XY 等比会把线宽压没
            if b:
                t = Transform(HALF / UPM, 0, 0, 1, 0, 0)
                rewrite_glyph(glyf, gname, t)
                stats["box_x"] += 1
            hmtx[gname] = (HALF, ink_bounds(glyf, gname)[0] if b else 0)
            continue
        if not b:
            hmtx[gname] = (HALF, 0)
            stats["empty"] += 1
            continue
        xMin, yMin, xMax, _ = b
        iw = xMax - xMin
        if iw <= INK_CAP:
            t = Transform(1, 0, 0, 1, (HALF - iw) / 2 - xMin, 0)
            stats["center"] += 1
        else:
            s = INK_CAP / iw
            t = Transform(s, 0, 0, s, (HALF - iw * s) / 2 - xMin * s, yMin - yMin * s)
            stats["squeeze"] += 1
        rewrite_glyph(glyf, gname, t)
        hmtx[gname] = (HALF, ink_bounds(glyf, gname)[0])
    font["post"].isFixedPitch = 1
    return stats


def ensure_cmap12(font):
    """取/建 format 12 unicode 子表（SMP 码点唯一容身所；一旦存在就是
    各引擎最优子表——必须全量镜像 BMP 映射，缺 = 汉字全灭实踩）"""
    from fontTools.ttLib.tables._c_m_a_p import CmapSubtable

    for t in font["cmap"].tables:
        if t.isUnicode() and t.format == 12:
            return t
    t = CmapSubtable.newSubtable(12)
    t.platformID, t.platEncID, t.language = 3, 10, 0
    t.cmap = {}
    for table in font["cmap"].tables:
        if table.isUnicode() and table.format == 4:
            t.cmap.update(table.cmap)
    font["cmap"].tables.append(t)
    return t


def register_cmap(font, cp, gname):
    for table in font["cmap"].tables:
        if not table.isUnicode():
            continue
        if cp > 0xFFFF and table.format == 4:
            continue
        table.cmap[cp] = gname
    if cp > 0xFFFF:
        ensure_cmap12(font).cmap[cp] = gname


def twin_cp(cp):
    """带圈族主码位 → PUA 窄版孪生码位（与 ENCLOSED_CPS 同序；族外 None）"""
    if 0x24B6 <= cp <= 0x24E9:
        return TWIN_BASE + (cp - 0x24B6)
    if 0x246A <= cp <= 0x2473:
        return TWIN_BASE + 52 + (cp - 0x246A)
    return None


def wide_twin_cp(cp):
    """带圈族主码位 → PUA **溢格**孪生码位（辛，U+E03E 起，同序；族外 None）"""
    if 0x24B6 <= cp <= 0x24E9:
        return WIDE_TWIN_BASE + (cp - 0x24B6)
    if 0x246A <= cp <= 0x2473:
        return WIDE_TWIN_BASE + 52 + (cp - 0x246A)
    return None


def _ring(pen, cx, cy, r_out, r_in):
    """圆环两轮廓（外 CCW 内 CW，TrueType 非零环绕得环）。三次贝塞尔
    四段近似圆：控制点沿端点切向伸出 kappa*r，kappa=0.5522847498。"""
    k = 0.5522847498307936
    # 东/北/西/南四点，CCW 序；各点行进切向
    for r, ccw in ((r_out, True), (r_in, False)):
        pts = [(cx + r, cy), (cx, cy + r), (cx - r, cy), (cx, cy - r)]
        tang = [(0, 1), (-1, 0), (0, -1), (1, 0)]
        if not ccw:
            pts = [pts[0]] + pts[1:][::-1]
            tang = [tang[0]] + tang[1:][::-1]
            tang = [(-tx, -ty) for tx, ty in tang]
        pen.moveTo(pts[0])
        for i in range(4):
            p0, p1 = pts[i], pts[(i + 1) % 4]
            t0, t1 = tang[i], tang[(i + 1) % 4]
            c1 = (p0[0] + k * r * t0[0], p0[1] + k * r * t0[1])
            c2 = (p1[0] - k * r * t1[0], p1[1] - k * r * t1[1])
            pen.curveTo(c1, c2, p1)
        pen.closePath()


def redraw_enclosed_twins(font):
    """戊：重绘 62 枚窄版孪生进 PUA（幂等覆盖，供设计迭代）。
    画布 500（半格）：圆心 (250,357)（纵对齐在产全角 ⓐ 圆心）、圆径 480、
    环粗 60；字母墨高：小写按 x 高 264、大写按 cap 高 290、数字 240，
    双数字另限总宽 ≤330。"""
    return _redraw_enclosed(
        font,
        base=TWIN_BASE,
        r_out=240,
        r_in=180,
        scale=1.0,
        tag="pua",
    )


def redraw_enclosed_wide_twins(font):
    """辛：重绘 62 枚**溢格**孪生进 PUA（U+E03E 起，幂等覆盖）。
    圆心仍在 advance 中心 (250,357)（纵向与窄版同基，混排不跳行）；
    圆径 824＝二修全角墨盒 → 墨盒横跨 [-162, 662]（**advance 仍 500**，
    左右各溢 162 单位）；环粗按窄版同比例 12.5%＝103；字母墨高按
    824/480 同比例放大（530/482/438，双数字总宽 ≤566）。"""
    return _redraw_enclosed(
        font,
        base=WIDE_TWIN_BASE,
        r_out=WIDE_TWIN_INK // 2,
        r_in=WIDE_TWIN_INK // 2 - WIDE_RING,
        scale=WIDE_TWIN_INK / 480.0,
        tag="wua",
    )


def _redraw_enclosed(font, *, base, r_out, r_in, scale, tag):
    """窄/溢格孪生共用重绘核（同一张 ENCLOSED_CPS 表、同一套载荷排法，
    只有画布与比例不同——两版字形结构一致，只差尺寸）。"""
    donor = TTFont(TWIN_LETTER_DONOR)
    d_cmap, d_gs = donor.getBestCmap(), donor.getGlyphSet()
    glyf, hmtx = font["glyf"], font["hmtx"]
    vmtx = font["vmtx"] if "vmtx" in font else None
    CX, CY, R_OUT, R_IN = HALF // 2, 357, r_out, r_in

    def donor_bounds(gname):
        pen = BoundsPen(d_gs)
        d_gs[gname].draw(pen)
        return pen.bounds

    x_h = donor_bounds(d_cmap[ord("x")])[3] - donor_bounds(d_cmap[ord("x")])[1]
    cap_h = donor_bounds(d_cmap[ord("H")])[3] - donor_bounds(d_cmap[ord("H")])[1]
    dig_h = donor_bounds(d_cmap[ord("8")])[3] - donor_bounds(d_cmap[ord("8")])[1]
    cap_w = 330 * scale

    made = 0
    for i, cp in enumerate(ENCLOSED_CPS):
        if 0x24B6 <= cp <= 0x24CF:
            payload, base_h, target = chr(ord("A") + cp - 0x24B6), cap_h, 290 * scale
        elif 0x24D0 <= cp <= 0x24E9:
            payload, base_h, target = chr(ord("a") + cp - 0x24D0), x_h, 264 * scale
        else:
            payload, base_h, target = str(11 + cp - 0x246A), dig_h, 240 * scale
        pen = TTGlyphPen(glyf)
        _ring(pen, CX, CY, R_OUT, R_IN)
        # 载荷墨盒（donor 空间，逐字排）
        cursor = 0.0
        pieces = []
        for ch in payload:
            dg = d_cmap[ord(ch)]
            b = donor_bounds(dg)
            adv = donor["hmtx"][dg][0]
            pieces.append((dg, cursor, b))
            cursor += adv * 0.88  # 双数字收紧字距
        x0 = min(b[0] + off for _, off, b in pieces)
        x1 = max(b[2] + off for _, off, b in pieces)
        s = min(target / base_h, cap_w / (x1 - x0))
        cx_d, cy_d = (x0 + x1) / 2, (min(b[1] for _, _, b in pieces) + max(b[3] for _, _, b in pieces)) / 2
        for dg, off, _ in pieces:
            rec = DecomposingRecordingPen(d_gs)
            d_gs[dg].draw(rec)
            t = Transform(s, 0, 0, s, CX - s * (cx_d - off), CY - s * cy_d)
            rec.replay(TransformPen(pen, t))
        gname = f"{tag}{base + i:04X}"
        glyf[gname] = pen.glyph()
        b = ink_bounds(glyf, gname)
        hmtx[gname] = (HALF, b[0] if b else 0)
        if vmtx is not None:
            vmtx[gname] = (font["head"].unitsPerEm, 0)
        register_cmap(font, base + i, gname)
        made += 1
    return made


def borrow(font, donor_path, cps, cells="auto", powerline=False):
    """借形补缺（font-bake.py borrow 同律 + powerline 专用律）。
    源字体已有码点跳过（黑体气质优先）；powerline 墨迹 X 向钉满半格
    左右缘（拼接件不许留边距），不走通用居中律。"""
    donor = TTFont(donor_path)
    d_cmap, d_gs = donor.getBestCmap(), donor.getGlyphSet()
    f_cmap = font.getBestCmap()
    scale = font["head"].unitsPerEm / donor["head"].unitsPerEm
    glyf, hmtx = font["glyf"], font["hmtx"]
    vmtx = font["vmtx"] if "vmtx" in font else None
    got, missing = [], []
    for cp in cps:
        if cp in f_cmap:
            continue
        dg = d_cmap.get(cp)
        if not dg:
            missing.append(cp)
            continue
        if cells == "auto":
            cp_cells = 2 if unicodedata.east_asian_width(chr(cp)) in ("W", "F") else 1
        else:
            cp_cells = cells
        unit = HALF * cp_cells
        gname = f"uni{cp:04X}"
        pen = TTGlyphPen(glyf)
        rec = DecomposingRecordingPen(d_gs)
        d_gs[dg].draw(rec)
        rec.replay(TransformPen(pen, Transform(scale, 0, 0, scale, 0, 0)))
        glyf[gname] = pen.glyph()
        b = ink_bounds(glyf, gname)
        if b:
            xMin, yMin, xMax, _ = b
            iw = xMax - xMin
            if powerline:
                # X 向钉满半格左右缘（powerline 拼接件，边距 = 断缝）
                t = Transform(unit / iw, 0, 0, 1, -xMin * (unit / iw), 0)
            elif iw <= unit - 20:
                t = Transform(1, 0, 0, 1, (unit - iw) / 2 - xMin, 0)
            else:
                s = (unit - 20) / iw
                t = Transform(s, 0, 0, s, (unit - iw * s) / 2 - xMin * s, yMin - yMin * s)
            rewrite_glyph(glyf, gname, t)
            hmtx[gname] = (unit, ink_bounds(glyf, gname)[0])
        else:
            hmtx[gname] = (unit, 0)
        if vmtx is not None:
            vmtx[gname] = (font["head"].unitsPerEm, 0)
        register_cmap(font, cp, gname)
        got.append(cp)
    return got, missing


def main():
    if len(sys.argv) != 3:
        print(__doc__)
        sys.exit(1)
    src, dst = sys.argv[1], sys.argv[2]
    font = TTFont(src)
    n = subset_cff(font)
    print(f"1.subset: 保留码位表 {n} 个（BMP 汉字全量+GB2312+补丁表+借字清单；Ext B+ 不保）")
    otf_to_ttf(font)
    print("2.otf2ttf: CFF → glyf 转换完成")
    print(f"3.归格: {normalize_narrow(font)}")

    patch_cps = [cp for lo, hi in PATCH_RANGES for cp in range(lo, hi + 1)]
    pl_cps = list(range(0xE0A0, 0xE0D4 + 1))
    got, miss = borrow(font, FP_DONOR, [c for c in patch_cps if c not in pl_cps])
    print(f"4.borrow(FP): 补丁表借入 {len(got)} 个"
          + (f"，捐体缺 {len(miss)}" if miss else ""))
    got, miss = borrow(font, FP_DONOR, pl_cps, cells=1, powerline=True)
    print(f"4.borrow(FP): powerline 借入 {len(got)} 个（X 向钉满半格）"
          + (f"，捐体缺 {[hex(c) for c in miss]}" if miss else ""))
    got, miss = borrow(font, FP_DONOR, MOON_CPS, cells=2)
    print(f"4.borrow(FP): 月亮借入 {len(got)} 个（全角位）"
          + (f"，捐体缺 {[hex(c) for c in miss]}" if miss else ""))
    got, miss = borrow(font, FP_DONOR, BORROW_CPS)
    print(f"4.borrow(FP): BORROW 借入 {len(got)} 个"
          + (f"，捐体缺 {[hex(c) for c in miss]}" if miss else ""))
    got, miss = borrow(font, DEJAVU_MONO, SYMBOL_NARROW_CPS)
    got2, miss2 = borrow(font, DEJAVU_SANS, miss)
    print(f"4.borrow(DejaVu): 窄符号借入 {len(got)}+{len(got2)} 个"
          + (f"，双捐体仍缺 {[hex(c) for c in miss2]}" if miss2 else ""))
    got, miss = borrow(font, EMOJI_DONOR, EMOJI_CPS)
    print(f"4.borrow(NotoEmoji): emoji 借入 {len(got)} 个"
          + (f"，捐体缺 {[hex(c) for c in miss]}" if miss else ""))
    got, miss = borrow(font, DEJAVU_MONO, FFFD_CPS)
    print(f"4.borrow(DejaVu): FFFD 借入 {len(got)} 个"
          + (f"，捐体缺 {[hex(c) for c in miss]}" if miss else ""))
    got, miss = borrow(font, NSS_DONOR, ENCLOSED_CPS, cells=2)
    print(f"4.borrow(NSS): 带圈族借入 {len(got)} 个（全角，824 墨盒零缩放）"
          + (f"，捐体缺 {[hex(c) for c in miss]}" if miss else ""))
    n_twin = redraw_enclosed_twins(font)
    print(f"4b.redraw(戊): 带圈族窄版孪生 {n_twin} 枚进 PUA U+{TWIN_BASE:04X} 起"
          "（重绘非缩绘：环粗 60＋字母 ≈55% 圆径；终端 1 格面专用）")
    n_wide = redraw_enclosed_wide_twins(font)
    print(f"4c.redraw(辛): 带圈族溢格孪生 {n_wide} 枚进 PUA U+{WIDE_TWIN_BASE:04X} 起"
          f"（advance 仍 {HALF}、墨盒 {WIDE_TWIN_INK} 横向溢格；左右邻空白才消费）")

    font.save(dst)

    # ---------- 判卷 ----------
    import os
    check = TTFont(dst)
    cmap, cglyf, chmtx = check.getBestCmap(), check["glyf"], check["hmtx"]
    probes = [("槃", 0x69C3), ("中", 0x4E2D), ("🌑", 0x1F311), ("⠋", 0x280B),
              ("░", 0x2591), ("─", 0x2500), ("✅", 0x2705), ("", 0xE0B0),
              ("⚡", 0x26A1), ("�", 0xFFFD),
              ("ⓐ", 0x24D0), ("Ⓩ", 0x24CF),
              ("Ⓐ", 0x24B6), ("ⓩ", 0x24E9),
              ("⑪", 0x246A), ("⑳", 0x2473)]
    for ch, cp in probes:
        gn = cmap.get(cp)
        assert gn, f"探针 {ch}U+{cp:04X} cmap 未命中"
        b = ink_bounds(cglyf, gn)
        assert b and b[2] > b[0] and b[3] > b[1], f"探针 {ch}U+{cp:04X} 墨迹为空"
        print(f"  探针 {ch}U+{cp:04X} ✓ cmap+墨迹")
    han = sum(1 for cp in cmap
              if (0x4E00 <= cp <= 0x9FFF) or (0x3400 <= cp <= 0x4DBF) or (0xF900 <= cp <= 0xFAFF))
    print(f"汉字覆盖: {han}（URO+扩展A+兼容表意）")
    assert han >= 27900, f"汉字覆盖 {han} 不达标——这单的意义就是覆盖率"
    advs = {chmtx[cmap[cp]][0] for cp in range(0x21, 0x7F) if cp in cmap}
    assert advs == {HALF}, f"ASCII 步进应全 {HALF}，实得 {advs}"
    for cp in ENCLOSED_CPS:
        gn = cmap.get(cp)
        assert gn, f"U+{cp:04X} 带圈族 cmap 未命中"
        assert chmtx[gn][0] == UPM, f"U+{cp:04X} 带圈族步进非全角（用户判半格太小）"
    for cp in ENCLOSED_CPS:
        t = twin_cp(cp)
        gn = cmap.get(t)
        assert gn, f"U+{t:04X} 窄版孪生 cmap 未命中（戊，主码位 U+{cp:04X}）"
        assert chmtx[gn][0] == HALF, f"U+{t:04X} 孪生步进必须半格（终端 1 格面）"
        b = ink_bounds(cglyf, gn)
        assert b and 400 <= b[2] - b[0] <= HALF, \
            f"U+{t:04X} 孪生墨宽须 ∈[400,500]（粗满但不溢格），实得 {b}"
    for cp in ENCLOSED_CPS:
        t = wide_twin_cp(cp)
        gn = cmap.get(t)
        assert gn, f"U+{t:04X} 溢格孪生 cmap 未命中（辛，主码位 U+{cp:04X}）"
        assert chmtx[gn][0] == HALF, f"U+{t:04X} 溢格孪生 advance 必须仍半格（列账不动）"
        b = ink_bounds(cglyf, gn)
        assert b, f"U+{t:04X} 溢格孪生墨迹为空"
        w = b[2] - b[0]
        assert 800 <= w <= WIDE_TWIN_INK, \
            f"U+{t:04X} 溢格孪生墨宽须 ∈[800,{WIDE_TWIN_INK}]，实得 {w}"
        assert b[0] < 0 and b[2] > HALF, \
            f"U+{t:04X} 溢格孪生必须两侧出格（左右邻空白才放行的前提），实得 {b}"
    for cp in MOON_CPS:
        assert chmtx[cmap[cp]][0] == UPM, f"U+{cp:04X} 月亮非全角位"
    for cp in (0xE0B0, 0xE0B2):
        gn = cmap.get(cp)
        assert gn and chmtx[gn][0] == HALF, f"U+{cp:04X} powerline 步进非半格"
        b = ink_bounds(cglyf, gn)
        assert b and b[0] == 0 and b[2] == HALF, f"U+{cp:04X} 墨迹未贴满半格左右缘: {b}"
    size_mb = os.path.getsize(dst) / 1048576
    print(f"产物体积: {size_mb:.2f}MB")
    assert size_mb <= 15, f"产物 {size_mb:.2f}MB 超 15MB 红线（要进 git）"
    # 没借到的符号清单（缺口边界，进交付报告）
    for lo, hi in PATCH_RANGES:
        lack = [cp for cp in range(lo, hi + 1) if cp not in cmap]
        if lack:
            print(f"缺口 {hex(lo)}-{hex(hi)}: {len(lack)} 个（双源皆无，不补）")
    print("判卷通过:", dst)


if __name__ == "__main__":
    main()
