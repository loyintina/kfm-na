#!/bin/bash
# build-overlay.sh — L2 overlay 管线编排(在手机真 Termux 里跑)
# 设计:docs/active/l2-overlay.md §2
#
# 用法: scripts/build-overlay.sh <overlay名> <包...>
#   例: scripts/build-overlay.sh base openssh git
#
# 三段:apt 解依赖闭包(空 status 骗全量下载地址) → curl 拉回 deb →
# 调 overlay-pack.sh 剥前缀重打包 → 落交接点 ~/w/kfm-na-overlays/
# (na 侧 /storage/emulated/0/工作台/kfm-na-overlays/)
set -euo pipefail

[ $# -ge 2 ] || { echo "用法: $0 <overlay名> <包...>"; exit 1; }
name=$1; shift

work="${TMPDIR:-/tmp}/kfm-overlay-$name"
rm -rf "$work"; mkdir -p "$work/debs" "$work/cache"
: > "$work/empty-status"

echo "=== [overlay 1/3] apt 解依赖闭包($*) ==="
# 先刷清单:陈旧清单会给出已被官方源撤下的旧版本 URL,下载 404
# (2026-08-23 zsh 单包实拍踩中)
apt-get update -qq
# --print-uris 只算不下载。两道空闸缺一不可:
#   空 status        = 一切依赖按未装算,闭包完整;
#   空 cache/archives = 本机 apt 缓存里躺过的 deb 不算数(2026-08-22 实拍:
#                       只骗 status 没骗缓存,openssh/git 本体被「已在缓存」
#                       吞掉,overlay 只装了依赖没有主包)
apt-get -y -o Dir::State::status="$work/empty-status" \
    -o Dir::Cache::archives="$work/cache" \
    -o APT::Get::Download-Only=true --print-uris install "$@" \
    > "$work/uris.txt"
grep -oE "'https?://[^']+'" "$work/uris.txt" | tr -d "'" > "$work/urls.txt"
[ -s "$work/urls.txt" ] || { echo "❌ 没解出任何下载地址(包名打错了?)"; exit 1; }

echo "=== [overlay 2/3] 下载 $(wc -l < "$work/urls.txt") 个 deb ==="
(cd "$work/debs" && while read -r u; do curl -fsSL -O "$u"; done < "$work/urls.txt")

echo "=== [overlay 3/3] 剥前缀重打包 ==="
OUTDIR="$work" bash "$(dirname "$0")/overlay-pack.sh" "$name" "$work"/debs/*.deb

drop="$HOME/w/kfm-na-overlays"
mkdir -p "$drop"
cp "$work/na-overlay-$name.tar.gz" "$drop/"
echo "✅ 落交接点: $drop/na-overlay-$name.tar.gz"
