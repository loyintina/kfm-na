#!/bin/bash
# test-na-regress-meta.sh — 回归套件的套件(2026-08-27,零编译秒级冒烟)
#
# na-regress 判卷依赖一串基础设施(ssh 通道/gate-lib 判卷函数/awk 解析/
# 重启类分拣)。这些前提自己坏了 = 六卷考官集体假挂或假绿。本脚本用
# 可编程假 ssh 桩离线驱动它们,钉住四个「元契约」:
#   ①手机不可达 → 全卷跳过(77)而非全挂,runner exit 0;
#   ②PIN-boot 的 awk 在标准 trace 样本上出正确毫秒数(健康/越线/环空);
#   ③PIN-pump 的差分速率算法在标准水位环样本上出正确速率;
#   ④runner 把「重启类」考官排到普通考官之后。
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail() { echo "❌ $1"; exit 1; }

# ---- 可编程假 ssh:按 FAKE_MODE 返回预设产物 ----
cat > "$tmp/ssh" <<'EOF'
#!/bin/bash
case "${FAKE_MODE:-}" in
    unreachable) exit 255 ;;
    boot-healthy)
        for a in "$@"; do case "$a" in *trace.txt*) cat "$FAKE_TRACE";; esac; done
        ;;
    *) exit 0 ;;
esac
EOF
chmod +x "$tmp/ssh"

run_regress() {  # $1..=考官名;stdout=报表,echo rc
    local rc=0
    PATH="$tmp:$PATH" bash "$here/na-regress.sh" "$@" >/dev/null 2>&1 || rc=$?
    echo "$rc"
}

# ---- 元契约①:手机不可达 = 跳过不挂 ----
export FAKE_MODE=unreachable
rc=$(run_regress PIN-boot)
[ "$rc" = 0 ] || fail "元契约①:通道不通时 runner 应 exit 0(全跳过),实得 $rc"
out=$(FAKE_MODE=unreachable PATH="$tmp:$PATH" bash "$here/na-regress.sh" PIN-boot)
case "$out" in *"跳过 1"*) : ;; *) fail "元契约①:报表应记跳过 1\n$out";; esac

# ---- 元契约②:PIN-boot awk 判卷(样本直接喂给 awk 内核) ----
boot_core() {  # 复刻 PIN-boot 的解析内核:$1=trace 文本 → 最大 ms 或空
    printf '%s' "$1" | awk '
        /android_main 进入/ { boot=1; next }
        boot && /^\[\+[0-9]+ms boot\]/ {
            ms=$1; gsub(/^\[\+0*|ms.*$/, "", ms);
            if (ms+0 > m) m = ms+0
        }
        END { if (m > 0) print m }'
}
T='[+00000000ms boot] android_main 进入 (构建 x)
[+00000021ms boot] softbuffer 上下文建成
[+00000073ms boot] L3: 环境已装——跳过'
[ "$(boot_core "$T")" = 73 ] || fail "元契约②:健康样本应出 73,实得 $(boot_core "$T")"
T2="[+00053000ms boot] android_main 进入 (x)
[+00460000ms boot] 族病样本:460 秒级塞启动路径"
[ "$(boot_core "$T2")" = 460000 ] || fail "元契约②:族病样本应出 460000"
[ -z "$(boot_core 'android_main 进入 无段行')" ] || fail "元契约②:无段行应出空(触发跳过路径)"

# ---- 元契约③:PIN-pump 差分速率(样本喂算法) ----
pump_rate() {  # 复刻 PIN-pump:末两行 t/pump → 速率/s
    printf '%s\n' "$1" | grep '^t=' | tail -2 | awk '
    { for (i=1; i<=NF; i++) {
        if ($i ~ /^t=/) { v=$i; sub(/^t=/, "", v); t = v }
        if ($i ~ /^pump=/) { v=$i; sub(/^pump=/, "", v); p = v }
      }
      printf "%s %s ", t, p }' | {
        read -r t1 p1 t2 p2
        echo $(( (p2 - p1) * 1000 / (t2 - t1) ))
    }
}
H='t=1000 fg=1 pump=188 other
t=3000 fg=1 pump=5689 other'
[ "$(pump_rate "$H")" = 2750 ] || fail "元契约③:(5689-188)*1000/2000 应出 2750,实得 $(pump_rate "$H")"

# ---- 元契约④:重启类排尾 ----
# 假考官 A(普通)/B(重启类):各自在自己名字里留指纹,看执行序
mkfake() {  # $1=名字 $2=是否重启类
    mkdir -p "$tmp/cases"
    {   echo '#!/bin/bash'
        [ "$2" = restart ] && echo "# 重启类"
        echo "echo RAN-$1 >> \"\$ORDER_FILE\"; exit 0"
    } > "$tmp/cases/$1-accept.sh"
    chmod +x "$tmp/cases/$1-accept.sh"
}
mkfake AAA normal
mkfake ZZZ restart
mkdir -p "$tmp/lib"
cp "$here/lib/gate-lib.sh" "$tmp/lib/"
sed_reg="$tmp/na-regress.sh"
sed 's|CASES="$ROOT/scripts/cases"|CASES="$REGRESS_CASES"|; s|NA_ROOT="$(cd "$(dirname "${BASH_SOURCE\[0\]}")/../.." && pwd)"|NA_ROOT="$REGRESS_ROOT"|' \
    "$here/na-regress.sh" > "$sed_reg"
export ORDER_FILE="$tmp/order"
export REGRESS_CASES="$tmp/cases"
export REGRESS_ROOT="$tmp"
: > "$ORDER_FILE"
PATH="$tmp:$PATH" bash "$sed_reg" >/dev/null 2>&1
seq=$(cat "$ORDER_FILE")
[ "$seq" = "RAN-AAA RAN-ZZZ" ] || [ "$(echo $seq)" = "RAN-AAA RAN-ZZZ" ] \
    || fail "元契约④:重启类应排尾(A 普通,Z 重启),实得序:[$(echo $seq)]"

echo "✅ na-regress 元契约四条:跳过语义/boot 解析/泵速率/重启排尾"
