#!/bin/bash
# na-panend-cap.sh — BAR-104 贴死交接差分机一键入口（2026-09-17）
#
#   bash scripts/na-panend-cap.sh    # 点播后 20s 内触发一次平移
#                                    #（下池点行=Upper 域 / 切标签=Page 域，
#                                    # 两域共用本仪，域名随 dim 落盘并播报），
#                                    # 自动抓平移全序列帧+首帧稳态落盘
#
# 链路：8024 闸门 touch panend-cap-req → present_frame 逐帧消费（点播制，
# 不投零开销）→ 平移各帧 panend-aNN.rgb + 首帧稳态 panend-b.rgb
# +panend.dim 落 DUMP_DIR → scp 拉回 → PIL 转 PNG + 逐帧对 b 差分
# 报告（BAR-105 复判：只留末帧说不清仪器逐帧看见什么，全序列是裁判）。
#
# 判读：末帧 a≈b（差≈0）=交接无缝；差大且平移解释不了=贴死闪变病灶。
set -euo pipefail

source "$(dirname "$0")/lib/gate-lib.sh"
PY=/root/.venvs/video/bin/python  # PIL+numpy（font venv 无 numpy）

gate "rm -f $NA_TMP/panend-a*.rgb $NA_TMP/panend-b.rgb $NA_TMP/panend.dim; touch $NA_TMP/panend-cap-req"
echo "已点播。请触发一次平移（下池点行=Upper / 切标签=Page）…"

ok=""
for _ in $(seq 1 40); do
    sleep 0.5
    if gate "test -f $NA_TMP/panend-b.rgb -a -f $NA_TMP/panend.dim"; then
        ok=1; break
    fi
done
if [ -z "$ok" ]; then
    echo "❌ 20 秒内没等到交接双帧——没触发平移？na 在前台吗？"
    exit 1
fi

dim=$(gate "cat $NA_TMP/panend.dim")
echo "捕获域: $(echo "$dim" | awk '{print $3}')"
rm -f /tmp/panend-a*.rgb
a_list=$(gate "ls $NA_TMP/panend-a*.rgb 2>/dev/null")
for f in $a_list; do
    gate_pull "$f" "/tmp/$(basename "$f")"
done
gate_pull "$NA_TMP/panend-b.rgb" /tmp/panend-b.rgb

"$PY" - $dim <<'EOF'
import sys, glob
from PIL import Image
w, h = int(sys.argv[1]), int(sys.argv[2])
def load(p):
    raw = open(p, 'rb').read()
    assert len(raw) == w*h*4, f"尺寸对不上: {p} {len(raw)} != {w}*{h}*4"
    return Image.frombytes('RGBA', (w, h), raw, 'raw', 'BGRA').convert('RGB')
import numpy as np
ib = load('/tmp/panend-b.rgb'); ib.save('/tmp/panend-b.png')
yb = np.asarray(ib, dtype=np.int16)
a_files = sorted(glob.glob('/tmp/panend-a*.rgb'))
print(f"平移帧序列: {len(a_files)} 帧 + 稳态 b")
for p in a_files:
    ia = load(p)
    out = p.replace('.rgb', '.png')
    ia.save(out)
    d = np.abs(np.asarray(ia, dtype=np.int16) - yb).mean(axis=2)
    print(f"  {out}: 对b差分 均值 {d.mean():.2f} (>12 像素 {(d>12).sum()})")
if a_files:
    ia = load(a_files[-1])
    d = np.abs(np.asarray(ia, dtype=np.int16) - yb).mean(axis=2)
    ys, xs = (d > 12).nonzero()
    if len(ys):
        print(f"末帧热区 y[{ys.min()}-{ys.max()}] x[{xs.min()}-{xs.max()}]")
        dv = np.clip(d*4, 0, 255).astype(np.uint8)
        Image.fromarray(dv).save('/tmp/panend-diff.png')
        print("✅ /tmp/panend-diff.png")
    else:
        print("✅ 交接逐像素无缝")
EOF
