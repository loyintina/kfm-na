#!/bin/bash
# na-autopsy.sh — 一键收尸包(2026-08-27,自观测第三块配套)
#
#   bash scripts/na-autopsy.sh [备注]
#
# 干三件事:
#   ①触发 trace-req / stats-req,让沙箱把最新行踪环和统计快照落盘;
#   ②把闸门目录的档案全量拉回 /root/kfm-na/autopsy/<时间戳>[-备注]/;
#   ③打印摘要:stats 全文 + panic.log 末行 + trace.txt 末五行。
#
# 适用:装机实测出了异常(卡死/闪退/输入失灵),一条命令收齐现场,
# 不用逐个文件 scp。拉回后原档不动(覆写制档案留沙箱里继续转)。
set -euo pipefail

NA_KEY=/root/.ssh/na_probe_key
NA_TMP=/data/data/dev.kfm.na/files/usr/tmp
TS="$(date +%Y%m%d-%H%M%S)"
NOTE="${1:-}"
DEST="/root/kfm-na/autopsy/$TS${NOTE:+-$NOTE}"

gate() {
    ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
        -o StrictHostKeyChecking=no localhost "$1"
}

echo "[autopsy] ① 触发 trace/stats 落盘..."
gate "rm -f $NA_TMP/trace.txt $NA_TMP/stats-res; touch $NA_TMP/trace-req $NA_TMP/stats-req" >/dev/null
ok=""
for _ in $(seq 1 30); do
    sleep 0.3
    if gate "test -f $NA_TMP/stats-res -a -f $NA_TMP/trace.txt"; then
        ok=1; break
    fi
done
if [ -z "$ok" ]; then
    echo "⚠️  9 秒内 stats/trace 没落齐——值守线程可能挂了,存量档案照拉" >&2
fi

echo "[autopsy] ② 拉回档案 → $DEST"
mkdir -p "$DEST"
# 有哪个拉哪个,缺档不致命
for f in panic.log panic-trace.txt loop-stall.log trace.txt stats-res \
         loader-pick flight-rec.bin ping-res; do
    scp -P 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
        -o StrictHostKeyChecking=no -q \
        "localhost:$NA_TMP/$f" "$DEST/" 2>/dev/null || true
done

echo "[autopsy] ③ 摘要"
echo "== stats-res =="
[ -f "$DEST/stats-res" ] && cat "$DEST/stats-res" || echo "(缺)"
if [ -f "$DEST/panic.log" ]; then
    n=$(wc -l < "$DEST/panic.log")
    echo "== panic.log: ${n} 行,末行 =="
    tail -1 "$DEST/panic.log"
else
    echo "== panic.log: 无(清白) =="
fi
if [ -f "$DEST/trace.txt" ]; then
    echo "== trace.txt 末五行 =="
    tail -5 "$DEST/trace.txt"
fi
echo "[autopsy] 收齐: $DEST"
ls -la "$DEST"
