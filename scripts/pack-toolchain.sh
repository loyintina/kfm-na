#!/bin/bash
# pack-toolchain.sh — 编译舱工具链打包（2026-09-23 编译舱立项，spike 3 原料）。
#
# 策略：capture-first——不追 termux 仓库的 deb 依赖闭包（版本漂移、
# 依赖解析脆），直接快照手机里**已在用**的工具链（rust/clang 及其依赖
# 闭包的 dpkg -L 文件全集），保证打包产物 = 实证可编的那套。deb 直取
# 留作陌生设备连快照都没有时的后备（MODE=fetch，未实现，挂账）。
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

echo "[pack-toolchain] 目标：$PHONE:$PORT → $OUT"

# 手机侧：包闭包（apt-cache depends 递归，滤已装）→ dpkg -L 文件全集 → tar。
# 走 stdout 二进制流，服务器侧重压缩 zstd。
ssh -p "$PORT" -o StrictHostKeyChecking=no "$PHONE" 'bash -s' <<'REMOTE' | zstd -19 -o "$OUT" || exit 1
set -uo pipefail
PREFIX=/data/data/com.termux/files/usr
# 依赖闭包：rust/clang 递归依赖，只留已装包名
PKGS=$(apt-cache depends --recurse --no-recommends --no-suggests \
    --no-conflicts --no-breaks --no-replaces --no-enhances rust clang 2>/dev/null \
    | grep -E '^[a-zA-Z0-9]' | sort -u \
    | while read -r p; do dpkg -s "$p" >/dev/null 2>&1 && echo "$p"; done)
echo "[pack-toolchain] 手机侧包闭包：$(echo "$PKGS" | tr '\n' ' ')" >&2
{
  for p in $PKGS; do dpkg -L "$p" 2>/dev/null; done
  echo "$PREFIX/bin/rustc"   # 保险：闭包漏网的手动兜底
  echo "$PREFIX/bin/cargo"
# 2026-09-23 首跑实踩：dpkg -L 打头路径与 $PREFIX 不符（前缀过滤把闭包
# 文件全滤光，产物只剩两个手动兜底文件 3.3MB）——不过滤前缀，认绝对路径
# 即可（dpkg -L 输出即包内文件全集，目录 tar 也收）。
} | grep '^/' | sort -u | tar cf - -T - 2>/dev/null
REMOTE
rc=$?
# zstd 管线的 rc 是 zstd 的；ssh 失败时 tar 流为空——用产物大小兜底判
# （实测定档：完整工具链压缩后应 >100MB，首跑 3.3MB = 过滤 bug 残桩）
if [ "$rc" -ne 0 ] || [ ! -s "$OUT" ] || [ "$(stat -c%s "$OUT")" -lt 104857600 ]; then
    echo "❌ 打包失败或产物过小（rc=$rc，$(stat -c%s "$OUT" 2>/dev/null) 字节）——8022 通吗？Termux 醒着吗？"
    exit 1
fi

# 版本清单落盘（下次增量/排障对照）
MANIFEST="$OUT.manifest"
ssh -p "$PORT" -o StrictHostKeyChecking=no "$PHONE" \
    'for p in rust clang; do dpkg -s $p 2>/dev/null | grep -E "^(Package|Version):"; done; rustc --version; cargo --version; clang --version | head -1' \
    > "$MANIFEST" 2>/dev/null || echo "（清单抓取失败，不影响产物）" >> "$MANIFEST"

ls -lh "$OUT"
echo "[pack-toolchain] ✅ 产物 $OUT（清单 $MANIFEST）"
