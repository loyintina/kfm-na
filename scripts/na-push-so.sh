#!/bin/bash
# na-push-so.sh — 热更新推送(2026-08-26,配套 crates/na-loader)
#
#   bash scripts/na-push-so.sh [--no-restart] [本地.so路径]
#     缺省:从手机仓 ~/kfm-na/target 拿刚编的 release 核心
#     --no-restart:只推不重启(手动划掉重开生效)
#
# 链路:核心 .so → na 沙箱 {files}/hot/libkfm_na.so(先 .new 再 mv 原子
# 防半读,同 keys-in 协议)→ na-restart.sh 自动体面重启 → na-loader
# dlopen 热更核心。判卷:闸门目录 loader-pick 应有 pick=hot 行 +
# boot 报告的构建戳对得上 + na-ping alive。
set -euo pipefail

NA_KEY=/root/.ssh/na_probe_key
NA_HOT=/data/data/dev.kfm.na/files/hot

na() {
    ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
        -o StrictHostKeyChecking=no localhost "$1"
}

NO_RESTART=0
SRC=""
for a in "$@"; do
    case "$a" in
        --no-restart) NO_RESTART=1 ;;
        *) SRC="$a" ;;
    esac
done

if [ -n "$SRC" ]; then
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
if [ "$NO_RESTART" = 1 ]; then
    echo "✅ 热更核心已就位(--no-restart:不重启,手动划掉重开生效)"
else
    echo "✅ 热更核心已就位,自动重启生效中——"
    bash "$(dirname "$0")/na-restart.sh"
    # ⑥ 冒烟回归(调试闸门.md §十四):热更刚重启过,SKIP_RESTART 直接判
    # 当前 boot。挂了不拦热更(核心已就位),但报表必须看——挂 = 这次
    # 热更可能带回了已销案的病
    echo "=== ⑥ 冒烟回归(挂了不拦热更,但要看) ==="
    SKIP_RESTART=1 bash "$(dirname "$0")/na-regress.sh" \
        PIN-boot PIN-signal BAR-040 \
        || echo "⚠️ 冒烟有挂卷——对照上面报表查案卷" >&2
fi
