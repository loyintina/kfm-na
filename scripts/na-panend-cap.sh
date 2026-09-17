#!/bin/bash
# na-panend-cap.sh — BAR-104 贴死交接差分机一键入口（2026-09-17）
#
#   bash scripts/na-panend-cap.sh    # 点播后 20s 内触发一次 Upper 平移
#                                    #（设置页点下池其他行），自动抓
#                                    # 平移末帧合成+首帧稳态双帧
#
# 链路：8024 闸门 touch panend-cap-req → present_frame 逐帧消费（点播制，
# 不投零开销）→ 平移收尾时 panend-a.rgb（末帧合成）/panend-b.rgb（首帧
# 稳态）+panend.dim 落 DUMP_DIR → scp 拉回 → PIL 转 PNG + 逐像素差分
# 报告（均值/热区/最佳平移残差——区分「位置没到位」与「内容重烘不一致」）。
#
# 判读：a≈b（差≈0）=交接无缝；b 与 a 差大且平移解释不了=贴死闪变病灶。
set -euo pipefail

source "$(dirname "$0")/lib/gate-lib.sh"
PY=/root/.venvs/video/bin/python  # PIL+numpy（font venv 无 numpy）

gate "rm -f $NA_TMP/panend-a.rgb $NA_TMP/panend-b.rgb $NA_TMP/panend.dim; touch $NA_TMP/panend-cap-req"
echo "已点播。请触发一次 Upper 平移（设置页→点下池其他行）…"

ok=""
for _ in $(seq 1 40); do
    sleep 0.5
    if gate "test -f $NA_TMP/panend-a.rgb -a -f $NA_TMP/panend-b.rgb -a -f $NA_TMP/panend.dim"; then
        ok=1; break
    fi
done
if [ -z "$ok" ]; then
    echo "❌ 20 秒内没等到交接双帧——没触发 Upper 平移？na 在前台吗？"
    exit 1
fi

dim=$(gate "cat $NA_TMP/panend.dim")
gate_pull "$NA_TMP/panend-a.rgb" /tmp/panend-a.rgb
gate_pull "$NA_TMP/panend-b.rgb" /tmp/panend-b.rgb

"$PY" - $dim <<'EOF'
import sys
from PIL import Image, ImageChops
w, h = int(sys.argv[1]), int(sys.argv[2])
ra = open('/tmp/panend-a.rgb', 'rb').read()
rb = open('/tmp/panend-b.rgb', 'rb').read()
assert len(ra) == w*h*4 and len(rb) == w*h*4, f"尺寸对不上: {len(ra)}/{len(rb)} != {w}*{h}*4"
ia = Image.frombytes('RGBA', (w, h), ra, 'raw', 'BGRA').convert('RGB')
ib = Image.frombytes('RGBA', (w, h), rb, 'raw', 'BGRA').convert('RGB')
ia.save('/tmp/panend-a.png'); ib.save('/tmp/panend-b.png')
import numpy as np
x = np.asarray(ia, dtype=np.int16); y = np.asarray(ib, dtype=np.int16)
d = np.abs(x - y).mean(axis=2)
print(f"交接差分: 均值 {d.mean():.2f} 最大 {d.max()} (>12 像素 {(d>12).sum()})")
ys, xs = (d > 12).nonzero()
if len(ys):
    print(f"热区 y[{ys.min()}-{ys.max()}] x[{xs.min()}-{xs.max()}]")
    dv = np.clip(d*4, 0, 255).astype(np.uint8)
    Image.fromarray(dv).save('/tmp/panend-diff.png')
    print("✅ /tmp/panend-a.png /tmp/panend-b.png /tmp/panend-diff.png")
else:
    print("✅ 交接逐像素无缝（/tmp/panend-a.png /tmp/panend-b.png）")
EOF
