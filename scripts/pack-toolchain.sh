#!/bin/bash
# pack-toolchain.sh — 编译舱工具链打包（2026-09-23 编译舱立项，spike 3 原料）。
#
# 策略：capture-first——不追 termux 仓库的 deb 依赖闭包（版本漂移、
# 依赖解析脆），直接快照手机里**已在用**的工具链（rust/clang 及其依赖
# 闭包的 dpkg -L 文件全集），保证打包产物 = 实证可编的那套。deb 直取
# 留作陌生设备连快照都没有时的后备（MODE=fetch，未实现，挂账）。
#
# 两阶段（2026-09-23 三跑实测定稿）：手机侧先落盘 tar.zst，服务器 rsync
# 断点续拉回+sha256 对账。教训：8022 的 Termux 会被 vivo 冻结，长 tar 流
# 必被掐（首跑 24MB、二跑 31MB 残桩实录）——传输必须可续传，不能赌长连接。
#
# 用法：
#   bash scripts/pack-toolchain.sh            # 快照手机 Termux 工具链
#   OUTPUT=/path/na-toolchain.tar.zst ...     # 自定义产物路径
#
# 产物：na-toolchain-aarch64-<date>.tar.zst + .manifest（包版本清单）。
# 设备侧安装/冒烟见 docs/active/编译舱.md spike 3。
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

PHONE=${PHONE:-"u0_a376@localhost"}
PORT=${PORT:-8022}
STAMP=$(date +%Y%m%d)
OUT=${OUTPUT:-/root/kfm-na-toolchain/na-toolchain-aarch64-$STAMP.tar.zst}
# 远端产物路径必须绝对写死——$HOME 会在服务器侧展开成 /root（2026-09-23
# 三跑 zstd: /root/...: No such file or directory 实踩）
REMOTE_PACK=/data/data/com.termux/files/home/na-toolchain-aarch64-$STAMP.tar.zst

echo "[pack-toolchain] 目标：$PHONE:$PORT → $OUT"

# ── 阶段一：手机侧落盘（tar|zstd 在手机本地完成，冻结最多杀一次，重跑即续）
ssh -p "$PORT" -o StrictHostKeyChecking=no -o ConnectTimeout=15 "$PHONE" 'bash -s' <<REMOTE || exit 1
set -uo pipefail
PREFIX=/data/data/com.termux/files/usr
OUT=$REMOTE_PACK
# 依赖闭包：rust/clang 递归依赖，只留已装包名
PKGS=\$(apt-cache depends --recurse --no-recommends --no-suggests \
    --no-conflicts --no-breaks --no-replaces --no-enhances rust clang 2>/dev/null \
    | grep -E '^[a-zA-Z0-9]' | sort -u \
    | while read -r p; do dpkg -s "\$p" >/dev/null 2>&1 && echo "\$p"; done)
echo "[pack-toolchain] 手机侧包闭包：\$(echo "\$PKGS" | tr '\n' ' ')" >&2
# files-only 定档：dpkg -L 连目录一起列，tar 见目录就递归（/data 全树
# 9.4GB 事故实录）；剔目录后实证 9416 文件 1.125GB
{ for p in \$PKGS; do dpkg -L "\$p" 2>/dev/null; done; } | grep '^/' | sort -u \
  | while IFS= read -r f; do [ -f "\$f" ] && printf '%s\n' "\$f"; done > \$HOME/pk-files.txt
echo "[pack-toolchain] 文件数 \$(wc -l < \$HOME/pk-files.txt)" >&2
if [ -s "\$OUT" ]; then
    echo "[pack-toolchain] 手机侧已有产物，跳过落盘（续传友好）" >&2
else
    ionice -c3 nice -n 19 tar cf - -T \$HOME/pk-files.txt 2>/dev/null \
        | zstd -19 -o "\$OUT" || exit 1
fi
sha256sum "\$OUT" > "\$OUT.sha256"
cat "\$OUT.sha256" >&2
REMOTE

# ── 阶段二：rsync 断点续拉（冻结自愈后下轮续），sha256 对账才算成
echo "[pack-toolchain] 回拉（rsync --append-verify，sha256 对账）…"
for attempt in $(seq 1 20); do
    rsync --partial --append-verify -e "ssh -p $PORT -o StrictHostKeyChecking=no -o ConnectTimeout=15" \
        "$PHONE:$REMOTE_PACK" "$OUT" || true
    R_SIZE=$(ssh -p "$PORT" -o StrictHostKeyChecking=no -o ConnectTimeout=15 "$PHONE" "stat -c%s $REMOTE_PACK" 2>/dev/null || echo 0)
    L_SIZE=$(stat -c%s "$OUT" 2>/dev/null || echo 0)
    echo "[pack-toolchain] 第${attempt}轮：本地 $L_SIZE / 手机 $R_SIZE"
    if [ "$R_SIZE" != "0" ] && [ "$L_SIZE" = "$R_SIZE" ]; then
        R_SHA=$(ssh -p "$PORT" -o StrictHostKeyChecking=no -o ConnectTimeout=15 "$PHONE" "sha256sum $REMOTE_PACK" 2>/dev/null | awk '{print $1}')
        L_SHA=$(sha256sum "$OUT" | awk '{print $1}')
        if [ -n "$R_SHA" ] && [ "$R_SHA" = "$L_SHA" ]; then
            echo "[pack-toolchain] ✅ sha256 对账一致（$L_SHA）"
            break
        fi
        echo "[pack-toolchain] sha256 不一致，续拉重对…"
    fi
    [ "$attempt" -lt 20 ] && sleep 20
done
if [ ! -s "$OUT" ] || [ "$(stat -c%s "$OUT")" -lt 104857600 ]; then
    echo "❌ 产物缺失或 <100MB（$(stat -c%s "$OUT" 2>/dev/null) 字节）——8022 通吗？Termux 醒着吗？"
    exit 1
fi

# 版本清单落盘（下次增量/排障对照）
MANIFEST="$OUT.manifest"
ssh -p "$PORT" -o StrictHostKeyChecking=no -o ConnectTimeout=15 "$PHONE" \
    'for p in rust clang; do dpkg -s $p 2>/dev/null | grep -E "^(Package|Version):"; done; rustc --version; cargo --version; clang --version | head -1' \
    > "$MANIFEST" 2>/dev/null || echo "（清单抓取失败，不影响产物）" >> "$MANIFEST"

ls -lh "$OUT"
echo "[pack-toolchain] ✅ 产物 $OUT（清单 $MANIFEST）"
