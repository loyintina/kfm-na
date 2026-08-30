#!/usr/bin/env python3
# orb-on-text-measure.py — 光球「文字穿透」三区指标尺（ai-presence D8 加法合成
# 校准配套；压字反馈 2026-08-30：alpha 混合球内笔画 −32%，改加法后须提亮）
#
# 用法：python3 scripts/orb-on-text-measure.py <截图> <球心x> <球心y> <半径R>
#   例：python3 scripts/orb-on-text-measure.py /tmp/na-shot.png 480 700 60
# 输出：球内(r<0.6R)/球晕(1.2-1.8R)/球外(3-6R) 三区的笔画 p90 与底 p10
#   （亮度 = r+g+b 通道和；p90 ≈ 笔画、黑底终端里 p10 ≈ 底）
# 达标判据（对标参考图 orb-on-white-ref.jpg +90% 提亮）：
#   球内笔画 p90 ≥ 球外笔画 p90（提亮而非遮挡）；
#   底 p10 球内适度抬升（+40~+120，别过曝成紫块）
# 依赖：numpy + PIL（服务器 python3 已有）

import sys

import numpy as np
from PIL import Image

path, cx, cy, R = sys.argv[1], float(sys.argv[2]), float(sys.argv[3]), float(sys.argv[4])
img = np.asarray(Image.open(path).convert('RGB')).astype(np.float64)
H, W, _ = img.shape
ys, xs = np.mgrid[0:H, 0:W]
lum = img.sum(axis=2)
r = np.sqrt((xs - cx) ** 2 + (ys - cy) ** 2)
for name, m in [
    ('球内(r<0.6R)', r < 0.6 * R),
    ('球晕(1.2-1.8R)', (r > 1.2 * R) & (r < 1.8 * R)),
    ('球外(3-6R)', (r > 3 * R) & (r < 6 * R)),
]:
    print(f"{name}: 笔画p90={np.percentile(lum[m], 90):.0f} 底p10={np.percentile(lum[m], 10):.0f}")
