#!/bin/bash
# na-push-so.sh — 热更新推送(2026-08-26,配套 crates/na-loader)
#
#   bash scripts/na-push-so.sh [本地.so路径]
#     缺省:从手机仓 ~/kfm-na/target 拿刚编的 release 核心
#
# 链路:核心 .so → na 沙箱 {files}/hot/libkfm_na.so(先 .new 再 mv 原子
# 防半读,同 keys-in 协议)→ 重启 App 后 na-loader 自动 dlopen 它。
# 判卷:闸门目录 loader-pick 应有 pick=hot 行 + boot 报告的构建戳对得上。
# 注意:本脚本只换核心,不重启——重启见 na-restart.sh(后做)或手动
# 划掉重开。
set -euo pipefail

NA_KEY=/root/.ssh/na_probe_key
NA_HOT=/data/data/dev.kfm.na/files/hot

na() {
    ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
        -o StrictHostKeyChecking=no localhost "$1"
}

if [ $# -eq 1 ]; then
    SRC="$1"
    [ -f "$SRC" ] || { echo "❌ 找不到 $SRC" >&2; exit 66; }
    LOCAL_TMP="$SRC"
else
    # 从手机仓拉刚编的核(Termux 私有目录,经 8022 读)
    LOCAL_TMP=/tmp/libkfm_na-hot.so
    ssh -p 8022 -o BatchMode=yes -o ConnectTimeout=8 localhost \
        'cat ~/kfm-na/target/aarch64-linux-android/release/libkfm_na.so' > "$LOCAL_TMP"
fi

SIZE=$(stat -c%s "$LOCAL_TMP")
echo "=== 推送核心 ($SIZE 字节) → hot/ ==="
na "mkdir -p $NA_HOT"
# 原子防半写:.new → mv(若推送中断,旧核心不受损)
ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=10 \
    -o StrictHostKeyChecking=no localhost \
    "cat > $NA_HOT/libkfm_na.so.new && mv $NA_HOT/libkfm_na.so.new $NA_HOT/libkfm_na.so" \
    < "$LOCAL_TMP"
na "ls -la $NA_HOT/libkfm_na.so"
echo "✅ 热更核心已就位。重启 App 生效;之后看 loader-pick 应有 pick=hot"
