#!/bin/bash
# test-na-push-so-atomic.sh — BAR-150 半截核不落位契约钉（2026-09-24）
#
# 事故原型：na-push-so.sh 旧版 `na_ssh "cat > .new && { cp .last; mv .new .so; }"`
# ——&& 链是假原子：ssh 中途断流（vivo 冻结/切网）远端 cat 收 EOF 照样
# 退出 0，2MB 半截核直接 mv 落位，na 起不来，靠 .last 人肉回滚。
# 修 = md5 对账过了才许 mv，对账失败 exit 非零且现行核不动。
#
# 契约三言（source 级判卷——真传输要手机在场，形态归源码守卫）：
# ①对账（md5sum 本地≠远端比对）必须出现在 mv 之前；
# ②对账失败分支必须 exit 非零（半截核不许落位）；
# ③mv 不许再挂在 cat 的 && 链上（断流假成功直落位的旧形）。
set -u
cd "$(dirname "$0")/.." || exit 1
SRC=scripts/na-push-so.sh
[ -f "$SRC" ] || { echo "❌ $SRC 不存在"; exit 1; }

fails=0
say() { echo "$1"; }

# ③mv 不许挂 cat && 链（旧形回潮 = 当场红）
if grep -q 'cat > .*\.so\.new && ' "$SRC"; then
    say "❌ 契约③破：mv 仍挂在 cat 的 && 链上（断流假成功 = 半截核直落位）"
    fails=1
fi

# ①md5 对账必须在 mv 之前
acc_line=$(grep -n 'RMD5' "$SRC" | head -1 | cut -d: -f1)
mv_line=$(grep -n 'mv \$NA_HOT/libkfm_na\.so\.new' "$SRC" | head -1 | cut -d: -f1)
if [ -z "$acc_line" ] || [ -z "$mv_line" ] || [ "$acc_line" -ge "$mv_line" ]; then
    say "❌ 契约①破：md5 对账（RMD5）必须出现在 mv .new 之前（对账行=${acc_line:-无} mv 行=${mv_line:-无}）"
    fails=1
fi

# ②对账失败必须 exit 非零（半截核不许落位）
if ! grep -A6 'RMD5" != "\$LMD5' "$SRC" | grep -q 'exit [1-9]'; then
    say "❌ 契约②破：对账失败分支没有 exit 非零——半截核会静默落位"
    fails=1
fi

[ "$fails" = 0 ] && { say "✅ BAR-150 半截核不落位三契约全绿"; exit 0; }
exit 1
