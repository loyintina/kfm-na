#!/bin/bash
# overlay-pack.sh — L2 overlay 打包核(纯变换,零网络,host 可判卷)
# 设计:docs/active/l2-overlay.md §4/§5
#
# 用法: OUTDIR=<目录> scripts/overlay-pack.sh <overlay名> <deb...>
# 产物: <OUTDIR>/na-overlay-<名>.tar.gz(默认 OUTDIR=.)
#
# 对 Termux deb 做三件事:
#   1. 剥前缀:data.tar 里 ./data/data/com.termux/files/usr/ 下的一切上移为
#      payload 根(usr 相对路径)
#   2. 符号链接不落地,记进 SYMLINKS.txt(沿用 bootstrap 格式 target←link,
#      U+2190),由安装侧建链
#   3. 焊死路径改写:裸前缀等长替换(com.termux ↔ dev.kfm.na 同为 10
#      字符),文本与 ELF 通吃——等长不挪偏移,二进制照样安全
#      (设计页 §8,sshd 案实拍;2026-08-23 起构建侧根治,不再靠 shim)
set -euo pipefail

FROM_USR=/data/data/com.termux/files/usr

[ $# -ge 2 ] || { echo "用法: OUTDIR=<目录> $0 <overlay名> <deb...>"; exit 1; }
name=$1; shift
outdir=${OUTDIR:-.}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/payload" "$work/maint"

for deb in "$@"; do
    pkg=$(basename "$deb" | cut -d_ -f1)
    x="$work/x-$pkg"
    rm -rf "$x"; mkdir -p "$x/root" "$x/ctl"
    (cd "$x" && ar x "$deb")
    tar -xf "$x"/data.tar.* -C "$x/root"
    usr="$x/root$FROM_USR"
    [ -d "$usr" ] || { echo "❌ $pkg: deb 里没有 $FROM_USR——不是 Termux 包?"; exit 1; }
    cp -a "$usr/." "$work/payload/"
    tar -xf "$x"/control.tar.* -C "$x/ctl" 2>/dev/null || true
    for s in preinst postinst prerm postrm; do
        [ -f "$x/ctl/$s" ] && cp "$x/ctl/$s" "$work/maint/$pkg.$s"
    done
    echo "$pkg" >> "$work/pkglist"
done

# 符号链接:登记后摘除(打包侧不落地,安装侧按表建——对齐 bootstrap 语义)
( cd "$work/payload" && find . -type l -printf '%l←%P\n' ) > "$work/SYMLINKS.txt"
find "$work/payload" -type l -delete

# 焊死路径改写:裸前缀等长替换(com.termux ↔ dev.kfm.na 同为 10 字符),
# 文本与 ELF 通吃——等长不挪偏移,不毁段表/重定位(§8,sshd 案实拍)。
# grep -rl 不带 -I:二进制里的焊死串也要揪出来;sed 处理二进制安全
# (替换串等长、模式无换行,GNU sed 对 NUL/长行均无碍)。
grep -rl '/data/data/com\.termux' "$work/payload" "$work/maint" 2>/dev/null \
    | while read -r f; do
        sed -i 's|/data/data/com\.termux|/data/data/dev.kfm.na|g' "$f"
    done

{
    echo "name=$name"
    echo "built=$(date -Iseconds)"
    echo "packages=$(sort -u "$work/pkglist" | tr '\n' ' ')"
} > "$work/MANIFEST"

tar -czf "$outdir/na-overlay-$name.tar.gz" -C "$work" payload SYMLINKS.txt maint MANIFEST
echo "✅ $outdir/na-overlay-$name.tar.gz($(du -h "$outdir/na-overlay-$name.tar.gz" | cut -f1))"
