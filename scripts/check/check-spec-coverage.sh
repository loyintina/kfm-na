#!/bin/bash
# check-spec-coverage.sh — 考卷覆盖矩阵(2026-08-27,自我测试缺口④)
#
#   bash scripts/check/check-spec-coverage.sh        # 出矩阵+棘轮比对
#
# 判什么:「功能×考题」对照表——每个模块的 pub 项(fn/const)有多少被
# tests/ 引用。治的病是新功能补题靠自觉。
# 算法:**棘轮制**——基线文件记录各模块「未覆盖数」,重跑后不许涨;
# 加考题把基线改小(数字只能往下),这是进度台账。新模块默认允许。
# 不追两件事(v1 明确弃权,诚实边界):
#   ①引用匹配是词级近似(grep -w),极端同名会误记已覆盖;
#   ②「判卷成本倒挂不出考题」的豁免(getter/装配/常量表)靠人工,
#     本脚本只在 EXEMPT_MODULES 整模块豁免,不做函数级智能。
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BASELINE="$ROOT/scripts/check/spec-coverage-baseline.txt"
MATRIX="$ROOT/docs/ledger/test-coverage-matrix.md"

# B/C 档整模块豁免(A 纯逻辑才进矩阵;理由逐条注明)
EXEMPT_MODULES=(
    src/android_app.rs      # B 档平台胶水:判卷人=真机考官 na-regress
    src/clipboard.rs        # B 档平台胶水(JNI 系统剪贴板):判卷人=真机考官
    src/lib.rs              # 模块声明表,无 pub 行为
    src/plugins/mod.rs      # 声明表
)
is_exempt() {
    local m
    for m in "${EXEMPT_MODULES[@]}"; do
        [ "$1" = "$m" ] && return 0
    done
    return 1
}

# 词级近似的噪声黑名单(过泛标识符不算覆盖证据)
NOISE='^(new|default|main|run|feed)$'

fail=0
declare -a MATRIX_LINES BASELINE_NEW

mkdir -p "$(dirname "$MATRIX")"
{
    echo "<!-- 机械生成：scripts/check/check-spec-coverage.sh —— 请勿手改 -->"
    echo "# 考卷覆盖矩阵（gen:spec-coverage）"
    echo ""
    echo "> 这是什么：每模块 pub 项(fn/const)被 tests/ 引用的对照表。"
    echo "> 棘轮契约：未覆盖数只许降（加考题后手改 scripts/check/"
    echo "> spec-coverage-baseline.txt 对应行），涨了 chain 红。豁免与"
    echo "> 近似边界见脚本头注释。B/C 档豁免模块不进本表。"
    echo ""
    echo "| 模块 | pub项 | 已引用 | 未覆盖 | 未覆盖清单 |"
    echo "|---|---|---|---|---|"

    files=$(find "$ROOT/src" -name '*.rs' ! -name 'mod.rs' | sort)
    for f in $files; do
        rel=${f#"$ROOT"/}
        is_exempt "${rel}" && continue
        names=$(grep -oE '^\s*pub (fn|const) [a-zA-Z_0-9]+' "$f" | awk '{print $3}')
        total=0; covered=0; missing=""
        for n in $names; do
            total=$((total+1))
            if printf '%s' "$n" | grep -qE "$NOISE"; then
                covered=$((covered+1)); continue
            fi
            if grep -rqw -- "$n" "$ROOT/tests/" 2>/dev/null; then
                covered=$((covered+1))
            else
                missing="${missing:+$missing }${n}"
            fi
        done
        # 全零模块(pub 项为 0)跳过不入表
        [ "$total" -eq 0 ] && continue
        uncov=$((total - covered))
        miss_disp=${missing:-—}
        MATRIX_LINES+=("| \`$rel\` | $total | $covered | $uncov | $miss_disp |")
        BASELINE_NEW+=("$rel $uncov")
    done

    printf '%s\n' "${MATRIX_LINES[@]}"
} > "$MATRIX"

# ---- 棘轮比对 ----
# 首跑:生成本线并把当前数当起跑线(诚实起点,不是承诺)
if [ ! -f "$BASELINE" ]; then
    printf '%s\n' "${BASELINE_NEW[@]}" > "$BASELINE"
    echo "[check-spec-coverage] 首跑:基线生成($(wc -l < "$BASELINE") 模块入账)。此后未覆盖数只许降。"
    exit 0
fi

total_fail=0
while read -r mod base; do
    now=$(printf '%s\n' "${BASELINE_NEW[@]:-}" | awk -v m="$mod" '$1==m{print $2}')
    if [ -z "$now" ]; then
        now=0   # 模块被删/全空按 0 记
    fi
    if [ "$now" -gt "$base" ]; then
        echo "❌ [check-spec-coverage] $mod 未覆盖数上涨:$base → $now——补考题或修基线(说明理由后调低)" >&2
        total_fail=$((total_fail+1))
    elif [ "$now" -lt "$base" ]; then
        echo "✅ [check-spec-coverage] $mod 改善:$base → $now(记得下调基线)"
    fi
done < "$BASELINE"

# 新模块入账提示(基线没有的)
printf '%s\n' "${BASELINE_NEW[@]:-}" | while read -r mod uncov; do
    grep -q "^$mod " "$BASELINE" || echo "[check-spec-coverage] 新模块入账:$mod 未覆盖=$uncov"
done

if [ "$total_fail" -gt 0 ]; then
    exit 1
fi
echo "[check-spec-coverage] OK — 棘轮无恶化,矩阵已刷新 docs/ledger/test-coverage-matrix.md"
