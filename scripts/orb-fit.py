#!/usr/bin/env python3
# orb-fit.py — 光球 sprite 配方拟合器（ai-presence D8 校准工具）
#
# 用途：给定样式参考图（黑底紫球），坐标下降拟合「Lambert 球体 + 径向光晕 +
# 高光点」三层参数化模型的全部常量，输出 RMSE 与左右对比图。
# 产物是 ai_presence.rs sprite 生成器的常量来源——Rust 侧按同一公式实现，
# 两边的逐像素一致性可用本脚本回归验证（换参考图/调参后重跑即可）。
#
# 用法：python3 scripts/orb-fit.py [参考图路径] [输出前缀]
#   默认：docs/assets/orb-style-ref-20260830.jpg  /tmp/orb-fit
# 依赖：numpy + PIL（服务器 python3 已有）
#
# 模型（全部长度量以球半径 Rs 归一，任意尺寸球可缩放）：
#   光晕层（底）：a(r) = clip((1-r/Rg)^p + tamp*exp(-r/tsig), 0,1)，色 = C_lit
#   球体层：Lambert 明暗 I = max(0, -lx*nx - ly*ny + lz*nz)^k
#           色 = mix(DARK, C_lit, I)，alpha = As（整盘，暗面遮挡光晕=「暗盘」效果）
#   高光点：沿光源方向 0.55*Rs 处小高斯，amp=spa，sigma=sps，过曝部分略往白
# 2026-08-30 拟合结果（参考图 660x660）：见 docs/active/ai-presence.md D8 参数表

import sys
import numpy as np
from PIL import Image

REF_PATH = sys.argv[1] if len(sys.argv) > 1 else 'docs/assets/orb-style-ref-20260830.jpg'
OUT = sys.argv[2] if len(sys.argv) > 2 else '/tmp/orb-fit'

ref = np.asarray(Image.open(REF_PATH).convert('RGB')).astype(np.float64)
H, W, _ = ref.shape
ys, xs = np.mgrid[0:H, 0:W].astype(np.float64)
BG = np.array([11.0, 10.0, 15.0])    # 参考图底（实机截图的深色底，非纯黑）
DARK = np.array([9.0, 8.0, 13.0])    # 球体暗面色

def render(pr):
    sx, sy, Rs, lx, ly, k, bri, Rg, p, tamp, tsig, As, spa, sps = pr
    C_lit = np.array([100.0, 50.0, 200.0]) * bri
    r = np.sqrt((xs - sx) ** 2 + (ys - sy) ** 2)
    a = np.clip(np.clip(1 - r / Rg, 0, 1) ** p + tamp * np.exp(-r / tsig), 0, 1)
    img = BG[None, None, :] * (1 - a[..., None]) + C_lit[None, None, :] * a[..., None]
    dx, dy = (xs - sx) / Rs, (ys - sy) / Rs
    rr = dx ** 2 + dy ** 2
    inside = rr <= 1.0
    z = np.sqrt(np.clip(1 - rr, 0, None))
    lz = np.sqrt(max(0.0, 1 - lx ** 2 - ly ** 2))
    I = np.clip(-lx * dx - ly * dy + lz * z, 0, None) ** k
    hx, hy = sx - lx * Rs * 0.55, sy - ly * Rs * 0.55
    spec = spa * np.exp(-((xs - hx) ** 2 + (ys - hy) ** 2) / (2 * sps ** 2))
    I2 = np.clip(I + spec, 0, 1.6)
    sph = DARK[None, None, :] * (1 - np.clip(I2, 0, 1)[..., None]) \
        + C_lit[None, None, :] * np.clip(I2, 0, 1)[..., None]
    sph = sph + np.clip(I2 - 1, 0, None)[..., None] * np.array([60.0, 40.0, 80.0])[None, None, :]
    a_s = np.where(inside, As, 0.0)
    return img * (1 - a_s[..., None]) + sph * a_s[..., None]

pad = max(H, W) // 6
c = slice(pad, H - pad), slice(pad, W - pad)
def rmse(pr):
    return float(np.sqrt(((render(pr)[c] - ref[c]) ** 2).mean()))

# 初值（660x660 参考图的经验起点；换图先目测调 sx/sy/Rs 到球心/半径附近）
pr = [330.0, 328.0, 72.0, 0.45, 0.50, 2.1, 1.05, 185.0, 2.1, 0.06, 72.0, 0.87, 0.5, 12.0]
names = ['sx', 'sy', 'Rs', 'lx', 'ly', 'k', 'bri', 'Rg', 'p', 'tamp', 'tsig', 'As', 'spa', 'sps']
steps = [4, 4, 4, 0.06, 0.06, 0.25, 0.12, 10, 0.25, 0.03, 15, 0.05, 0.15, 4]
best = rmse(pr)
for _ in range(5):
    for i in range(len(pr)):
        for sgn in (1, -1):
            t = pr[:]; t[i] += sgn * steps[i]
            e = rmse(t)
            if e < best:
                best, pr = e, t
    steps = [s / 2 for s in steps]

print(f"RMSE = {best:.2f}")
for n, v in zip(names, pr):
    print(f"  {n} = {v:.3f}")
im = render(pr)
Image.fromarray(im.astype(np.uint8)).save(f'{OUT}-generated.png')
comp = np.zeros((H, W * 2 + 20, 3), np.uint8)
comp[:, :W] = ref.astype(np.uint8)
comp[:, W + 20:] = im.astype(np.uint8)
Image.fromarray(comp).save(f'{OUT}-compare.png')
print(f"产物: {OUT}-generated.png / {OUT}-compare.png")
