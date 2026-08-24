#!/bin/bash
# test-kfm-pkg.sh — kfm-pkg 原子性考题(BAR-031,挂 chain 第 8 步)
#
# fixture 假 deb 走 overlay-pack 打包,再用 kfm-pkg(host 模式,
# KFM_PREFIX 指向临时目录)判卷三件事:
#   ①正常安装:文件落位/链接建成/登记去重/无标记残留
#   ②中断自愈:测试缝模拟「文件铺完被杀」→ 标记在+链接缺+list 吼 →
#     重装后全愈(链接通/标记摘/登记不重)
#   ③装后校验:payload 被抽走一件 → 校验非零退出+标记保留
set -euo pipefail
cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fix="$work/fix"

# --- 造 fixture deb(与 test-overlay.sh 同型:焊死路径脚本 + 真文件 + symlink + postinst) ---
mkdir -p "$fix/root/data/data/com.termux/files/usr/bin" \
         "$fix/root/data/data/com.termux/files/usr/lib" \
         "$fix/ctl"
printf '#!/data/data/com.termux/files/usr/bin/sh\necho fixhello\n' \
    > "$fix/root/data/data/com.termux/files/usr/bin/fixhello"
chmod 755 "$fix/root/data/data/com.termux/files/usr/bin/fixhello"
printf 'fake-lib' > "$fix/root/data/data/com.termux/files/usr/lib/libfix.so.1"
ln -s libfix.so.1 "$fix/root/data/data/com.termux/files/usr/lib/libfix.so"
printf '#!/data/data/com.termux/files/usr/bin/sh\nmkdir -p /data/data/com.termux/files/usr/var/fix\n' \
    > "$fix/ctl/postinst"
echo 'Package: fixhello' > "$fix/ctl/control"
(cd "$fix/root" && tar -czf "$fix/data.tar.gz" ./data)
(cd "$fix/ctl" && tar -czf "$fix/control.tar.gz" postinst control)
echo '2.0' > "$fix/debian-binary"
(cd "$fix" && ar rcs fixhello_1.0_aarch64.deb debian-binary control.tar.gz data.tar.gz)

# --- 打包成 overlay,放进交接点 ---
mkdir -p "$work/handoff"
OUTDIR="$work/handoff" bash scripts/overlay-pack.sh fixtest "$fix/fixhello_1.0_aarch64.deb" > /dev/null

PREFIX="$work/prefix"
pkg() {
    KFM_PREFIX="$PREFIX" KFM_OVERLAY_DIR="$work/handoff" \
        bash android/assets/kfm-pkg "$@"
}

fail() { echo "❌ $1"; exit 1; }

# ============ ① 正常安装 ============
out=$(pkg install fixtest 2>&1) || fail "正常安装非零: $out"
[ -f "$PREFIX/bin/fixhello" ] || fail "bin/fixhello 未落位"
[ -L "$PREFIX/lib/libfix.so" ] || fail "符号链接 libfix.so 未建"
[ -e "$PREFIX/lib/libfix.so" ] || fail "libfix.so 断链(target 不存在)"
[ -x "$PREFIX/bin/fixhello" ] || fail "fixhello 丢了可执行位"
[ ! -f "$PREFIX/var/kfm-pkg/fixtest.partial" ] || fail "装完还有 .partial 残留"
grep -qx 'fixhello' "$PREFIX/var/kfm-pkg/installed" || fail "installed 未登记"
# 重装:登记不攒重复行
pkg install fixtest > /dev/null 2>&1
[ "$(grep -c '^fixhello$' "$PREFIX/var/kfm-pkg/installed")" = "1" ] \
    || fail "重装后 installed 出现重复登记"
echo "✅ ① 正常安装+登记去重"

# ============ ② 中断自愈 ============
out=$(KFM_PKG_FAKE_KILL=after-files pkg install fixtest 2>&1) && fail "测试缝应非零退出"
[ -f "$PREFIX/var/kfm-pkg/fixtest.partial" ] || fail "中断后 .partial 标记不在"
rm -f "$PREFIX/lib/libfix.so"   # 模拟被杀时链接还没建(zsh 案现场)
# list 必须吼出中断残留
out=$(pkg list 2>&1) || true
echo "$out" | grep -q 'fixtest 上次安装中断' || fail "list 没吼中断残留: $out"
# 重装 = 自愈
out=$(pkg install fixtest 2>&1) || fail "自愈重装非零: $out"
echo "$out" | grep -q '重装即自愈' || fail "自愈路径没识别中断标记: $out"
[ -e "$PREFIX/lib/libfix.so" ] || fail "自愈后 libfix.so 仍断链"
[ ! -f "$PREFIX/var/kfm-pkg/fixtest.partial" ] || fail "自愈后 .partial 未摘"
out=$(pkg list 2>&1) || true
echo "$out" | grep -q '上次安装中断' && fail "自愈后 list 还在吼: $out"
echo "✅ ② 中断留痕+list 吼出+重装自愈"

# ============ ③ 装后校验抓缺件 ============
# 抽掉 payload 里一个文件(打包内容不变,安装侧发现缺件必须非零+留标记)
mkdir -p "$work/tamper"
tar -xzf "$work/handoff/na-overlay-fixtest.tar.gz" -C "$work/tamper"
rm "$work/tamper/payload/lib/libfix.so.1"
tar -czf "$work/handoff/na-overlay-fixtest.tar.gz" -C "$work/tamper" \
    payload SYMLINKS.txt maint MANIFEST
rm -rf "$PREFIX"
out=$(pkg install fixtest 2>&1) && fail "缺件安装应非零退出"
echo "$out" | grep -q '校验' || fail "缺件没被校验抓住: $out"
[ -f "$PREFIX/var/kfm-pkg/fixtest.partial" ] || fail "校验失败后 .partial 未保留"
echo "✅ ③ 装后校验抓缺件+标记保留"

echo "✅ kfm-pkg 原子性考题全过(BAR-031)"
