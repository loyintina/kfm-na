#!/bin/bash
# na-shot-cpu.sh — 强制 CPU 路截屏（GLES 回读本机 180° 翻转，判卷用正立帧）
set -euo pipefail
source "$(dirname "$0")/lib/gate-lib.sh"
PY=/root/.venvs/font/bin/python
gate "rm -f $NA_TMP/shot.rgb $NA_TMP/shot.dim; touch $NA_TMP/shot-req" >/dev/null
ok=""
for _ in $(seq 1 20); do
    sleep 0.5
    if gate "test -f $NA_TMP/shot.rgb -a -f $NA_TMP/shot.dim"; then ok=1; break; fi
done
[ -z "$ok" ] && { echo "❌ 10 秒没等到 CPU 倒帧"; exit 1; }
dim=$(gate "cat $NA_TMP/shot.dim")
gate_pull "$NA_TMP/shot.rgb" /tmp/na-shot.rgb
"$PY" -c "
import sys
from PIL import Image
w,h=map(int,'$dim'.split())
raw=open('/tmp/na-shot.rgb','rb').read()
assert len(raw)==w*h*4
Image.frombytes('RGBA',(w,h),raw,'raw','BGRA').convert('RGB').save('/tmp/na-shot.png')
"
echo "✅ /tmp/na-shot.png($dim) CPU 正立"
