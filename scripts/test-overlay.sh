#!/bin/bash
# test-overlay.sh — overlay-pack 考题(fixture 假 deb 把变换全走一遍)
# 挂 chain 第 9 步。设计:docs/active/l2-overlay.md §7
set -euo pipefail
cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fix="$work/fix"

# --- 造 fixture deb:焊死路径脚本 + 真文件 + symlink + postinst ---
mkdir -p "$fix/root/data/data/com.termux/files/usr/bin" \
         "$fix/root/data/data/com.termux/files/usr/lib" \
         "$fix/ctl"
printf '#!/data/data/com.termux/files/usr/bin/sh\necho fixhello\n' \
    > "$fix/root/data/data/com.termux/files/usr/bin/fixhello"
chmod 755 "$fix/root/data/data/com.termux/files/usr/bin/fixhello"
printf 'fake-lib' > "$fix/root/data/data/com.termux/files/usr/lib/libfix.so.1"
ln -s libfix.so.1 "$fix/root/data/data/com.termux/files/usr/lib/libfix.so"
# 假 ELF:魔数 + NUL + 焊死路径(模拟 sshd 的编译期焊死——§8 等长改写着靶)
printf '\x7fELF\x02\x01\x01\x00/data/data/com.termux/files/usr/etc/ssh/fix.conf\x00padding' \
    > "$fix/root/data/data/com.termux/files/usr/bin/fixelf"
chmod 755 "$fix/root/data/data/com.termux/files/usr/bin/fixelf"
elf_size_before=$(stat -c%s "$fix/root/data/data/com.termux/files/usr/bin/fixelf")
printf '#!/data/data/com.termux/files/usr/bin/sh\nmkdir -p /data/data/com.termux/files/usr/var/fix\n' \
    > "$fix/ctl/postinst"
echo 'Package: fixhello' > "$fix/ctl/control"
(cd "$fix/root" && tar -czf "$fix/data.tar.gz" ./data)
(cd "$fix/ctl" && tar -czf "$fix/control.tar.gz" postinst control)
echo '2.0' > "$fix/debian-binary"
(cd "$fix" && ar rcs fixhello_1.0_aarch64.deb debian-binary control.tar.gz data.tar.gz)

# --- 走管线核 ---
OUTDIR="$work" bash scripts/overlay-pack.sh fixtest "$fix/fixhello_1.0_aarch64.deb" > /dev/null
out="$work/out"; mkdir -p "$out"
tar -xzf "$work/na-overlay-fixtest.tar.gz" -C "$out"

fail() { echo "❌ $1"; exit 1; }

# 1. 前缀剥净:文件躺在 payload 根(usr 相对)
[ -f "$out/payload/bin/fixhello" ] || fail "payload/bin/fixhello 不在"
[ -f "$out/payload/lib/libfix.so.1" ] || fail "payload/lib/libfix.so.1 不在"
find "$out/payload" -path '*com.termux*' | grep -q . && fail "payload 残留 com.termux 路径"

# 2. 文本改写:脚本里 com.termux 一个不剩,na 前缀落位
grep -q 'com.termux' "$out/payload/bin/fixhello" && fail "payload 脚本未改写"
grep -q '/data/data/dev.kfm.na/files/usr/bin/sh' "$out/payload/bin/fixhello" \
    || fail "shebang 没改写成 na 前缀"
grep -q 'com.termux' "$out/maint/fixhello.postinst" && fail "postinst 未改写"
grep -q '/data/data/dev.kfm.na/files/usr/var/fix' "$out/maint/fixhello.postinst" \
    || fail "postinst 内部路径没改写成 na 前缀"

# 2.5 ELF 等长改写(§8):二进制里的焊死串也一个不剩,且字节数不变
[ -f "$out/payload/bin/fixelf" ] || fail "payload/bin/fixelf 不在"
grep -q 'com.termux' "$out/payload/bin/fixelf" && fail "ELF 残留 com.termux"
grep -q '/data/data/dev.kfm.na/files/usr/etc/ssh/fix.conf' "$out/payload/bin/fixelf" \
    || fail "ELF 改写后路径不对"
[ "$(stat -c%s "$out/payload/bin/fixelf")" = "$elf_size_before" ] \
    || fail "ELF 改写后字节数变了(等长铁律破,偏移挪动=毁 ELF)"
[ "$(head -c4 "$out/payload/bin/fixelf")" = "$(printf '\x7fELF')" ] || fail "ELF 魔数被毁"

# 3. 符号链接:登记且摘除
[ "$(cat "$out/SYMLINKS.txt")" = "libfix.so.1←lib/libfix.so" ] \
    || fail "SYMLINKS.txt 不对: $(cat "$out/SYMLINKS.txt")"
[ -z "$(find "$out/payload" -type l)" ] || fail "payload 里残留符号链接"

# 4. MANIFEST 登记
grep -q 'packages=fixhello' "$out/MANIFEST" || fail "MANIFEST 缺包名"

echo "✅ overlay-pack 考题全过"
