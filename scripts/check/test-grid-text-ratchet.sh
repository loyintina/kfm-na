#!/usr/bin/env bash
# test-grid-text-ratchet.sh — 棘轮闸考题（BAR-188，挂 chain 第 5 步同段）
# 五夹具：现状绿 / 新点红 / 幽灵行红 / 行号平移绿（翻案钉）/ 多重集计数红。
# 夹具全走 GRID_TEXT_SRC/GRID_TEXT_BASELINE 测试口，不碰真 src 与真基线。
set -uo pipefail
cd "$(dirname "$0")/../.."

GATE=scripts/check/grid-text-ratchet.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/src"
cat > "$T/src/a.rs" <<'RS'
fn f() {
    let w = text_width("名", 30.0);
    draw_text_left("名", 0.0, 0.0);
    draw_text_left("姓", 0.0, 9.0);
}
RS

mk_baseline() {
    GRID_TEXT_SRC="$T/src" GRID_TEXT_BASELINE="$T/baseline.txt" bash "$GATE" --bless >/dev/null
}

pass=0; fail=0
chk() { # chk <名> <期望exit> <实际exit>
    if [ "$2" = "$3" ]; then echo "  ✓ $1"; pass=$((pass+1));
    else echo "  ✗ $1（期望 exit=$2 实得 $3）"; fail=$((fail+1)); fi
}

# 夹具1 现状绿：基线=现状
mk_baseline
GRID_TEXT_SRC="$T/src" GRID_TEXT_BASELINE="$T/baseline.txt" bash "$GATE" >/dev/null 2>&1
chk "现状绿" 0 $?

# 夹具2 新点红：基线外多一个自然步进调用点
cp "$T/src/a.rs" "$T/src/a.rs.bak"
cat >> "$T/src/a.rs" <<'RS'
fn g() { draw_label("新", 0.0, 0.0); }
RS
GRID_TEXT_SRC="$T/src" GRID_TEXT_BASELINE="$T/baseline.txt" bash "$GATE" >/dev/null 2>&1
chk "新点红" 1 $?
mv "$T/src/a.rs.bak" "$T/src/a.rs"

# 夹具3 幽灵行红：基线登记了现存没有的调用点
echo "src/a.rs:measure_items" >> "$T/baseline.txt"
sort -o "$T/baseline.txt" "$T/baseline.txt"
GRID_TEXT_SRC="$T/src" GRID_TEXT_BASELINE="$T/baseline.txt" bash "$GATE" >/dev/null 2>&1
chk "幽灵行红" 1 $?
mk_baseline

# 夹具4 行号平移绿（BAR-188 翻案钉）：调用点上方插 10 行，行号全移、符号多重集不变
{ for i in $(seq 1 10); do echo "// 插行 $i"; done; cat "$T/src/a.rs"; } > "$T/src/a.rs.new"
mv "$T/src/a.rs.new" "$T/src/a.rs"
GRID_TEXT_SRC="$T/src" GRID_TEXT_BASELINE="$T/baseline.txt" bash "$GATE" >/dev/null 2>&1
chk "行号平移绿" 0 $?

# 夹具5 多重集计数红：同文件同符号两次调用删一次，计数减必须咬
sed -i '/draw_text_left("姓"/d' "$T/src/a.rs"
GRID_TEXT_SRC="$T/src" GRID_TEXT_BASELINE="$T/baseline.txt" bash "$GATE" >/dev/null 2>&1
chk "多重集计数红" 1 $?

echo "[test-grid-text-ratchet] $pass 过 / $fail 红"
[ "$fail" = 0 ]
