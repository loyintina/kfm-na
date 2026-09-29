#!/usr/bin/env bash
# bar-new.sh — BAR 号唯一领取口（2026-09-29 用户拍板：与信箱领号制同构，
# 治病 = 「grep 空号再写账」的 TOCTOU 竞态——三线共享工作树，grep 与落账
# 之间隔一个工单周期，两线同小时各 grep 出同一空号即双号）。
#
# 一次原子完成四件事（flock 包裹，同刻只许一个领号者）：
#   1. 领号：max(bugs.md 现有号, bar-registry.jsonl 现有号) + 1
#   2. 登记：docs/ledger/bar-registry.jsonl 追加一行（登记即占有）
#   3. 开追踪信：mailbox-cli new（致全体/通报/待落地通报）+ 当场填白话结论
#      与立案节（不留占位——chain 第 4 步信箱执法常绿，不挡别线的闸）
#   4. 回写：信号（NA00NN）补入登记行；信箱小仓自动 commit
#
# 用法: scripts/bar-new.sh --func 开发部 --name 观澜 "主题一句话"
#   主题禁含双引号与反斜杠（jsonl 转义从简）。
#
# 夹具测试口（A 档考题用，正常领号别设）：
#   BAR_NEW_BUGS=路径      替代 bugs.md 扫号源
#   BAR_NEW_REGISTRY=路径  替代 bar-registry.jsonl（锁取同目尾加 .lock）
#   BAR_NEW_NO_LETTER=1    跳过开信（只领号+登记）
#   BAR_NEW_MAILBOX=路径   替代信箱目录（new/verify/gen 指向它）
set -euo pipefail

FUNC="" NAME="" TITLE=""
while [ $# -gt 0 ]; do
    case "$1" in
        --func) FUNC="$2"; shift 2 ;;
        --name) NAME="$2"; shift 2 ;;
        --*) echo "未知参数: $1" >&2; exit 2 ;;
        *) TITLE="$1"; shift ;;
    esac
done
[ -n "$FUNC" ] && [ -n "$NAME" ] && [ -n "$TITLE" ] || {
    echo "用法: scripts/bar-new.sh --func <职能> --name <名字> \"主题\"" >&2; exit 2; }
case "$TITLE" in *[\"\\]*) echo "主题禁含双引号/反斜杠" >&2; exit 2 ;; esac

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUGS="${BAR_NEW_BUGS:-$ROOT/docs/ledger/bugs.md}"
REG="${BAR_NEW_REGISTRY:-$ROOT/docs/ledger/bar-registry.jsonl}"
LOCK="$REG.lock"
CLI="$ROOT/target/debug/mailbox-cli"
MAILBOX="${BAR_NEW_MAILBOX:-/root/.kfm/session/信箱}"

exec 9>"$LOCK"
flock 9

MAX=0
for f in "$BUGS" "$REG"; do
    [ -f "$f" ] || continue
    # 两种账面都扫：bugs.md 的 BAR-NNN 与 registry 的 "bar":NNN；
    # 零命中是合法态（首领/纯一方有号），grep 空果不许触发 pipefail
    n=$(grep -oE '"bar":[0-9]+|BAR-[0-9]+' "$f" | grep -oE '[0-9]+' | sort -n | tail -1 || true)
    if [ -n "$n" ] && [ "$n" -gt "$MAX" ]; then MAX=$n; fi
done
NEXT=$((MAX + 1))

LETTER=""
if [ "${BAR_NEW_NO_LETTER:-}" != "1" ]; then
    [ -x "$CLI" ] || { echo "缺 $CLI——先 cargo build -p mailbox-cli" >&2; exit 1; }
    NOW=$(date '+%Y-%m-%d %H:%M %z')
    TODAY=$(date '+%Y-%m-%d')
    out=$("$CLI" new --mailbox "$MAILBOX" --from-func "$FUNC" --from-name "$NAME" \
        --to-all --type 通报 --title "BAR-$NEXT $TITLE" \
        --kind 通报 --expect "承办线按 AGENTS.md「BAR 追踪信」条款同信追加进展" \
        --criteria "BAR-$NEXT 用户终验结案（信头状态翻 已验证）" \
        --status 待落地通报)
    file=$(printf '%s' "$out" | sed -n 's/.*已生成 \([^ ]*\.md\).*/\1/p')
    [ -n "$file" ] || { echo "mailbox-cli new 输出解析失败: $out" >&2; exit 1; }
    # 2026-09-29 评审修订（白露）：不写本册自指码——契约 §二「分拣码…本册信一律不写，
    # 只在跨册引用时写全码」。原行给编号无条件加 NA 前缀，致使 registry 的 letter 字段
    # 与提交信息的「追踪信 NA00NN」指向并不存在的文件名（NA0015 即此误用的成品）。
    # 新写法剥掉可选分拣码，取纯流水号（`0016号…` 与 `NA0016号…` 都归 `0016`）。
    LETTER=$(printf '%s' "$file" | sed -n 's/^\([A-Z]\{0,4\}\)\([0-9]\{4\}\).*/\2/p')
    path="$MAILBOX/$file"
    plain="本信追踪的问题是：$TITLE。当前状态 = 已立案开工，承办 $FUNC$NAME；立案/定罪/修复/判卷/终验都在本信追加（规矩 = na 仓 AGENTS.md「BAR 追踪信」节），隐藏读者看最新一节即可，无需做任何事。"
    body="### §一 立案（$NOW）\n\n- 主题：$TITLE\n- 承办：$FUNC$NAME\n- 定罪/修复/判卷/终验自下一节起按日期追加；信头状态随生命周期翻转\n  （待落地通报 → 已落地 → 已验证），打回翻回 待落地通报。\n\n——$FUNC$NAME · $TODAY"
    sed -i "s|（待填：面向隐藏读者。三句话内说清是什么事、要不要他做事；不写工作术语。）|$plain|" "$path"
    sed -i "s|（待填：结论与推导一起写——精确留给承重的数字和名字，其余白话；每条发现标「明写／推断／补全」。）|$body|" "$path"
    grep -q '（待填' "$path" && { echo "占位未填尽: $path" >&2; exit 1; }
    "$CLI" verify --mailbox "$MAILBOX" "$path" >/dev/null
    "$CLI" gen --mailbox "$MAILBOX" >/dev/null
    if [ "$MAILBOX" = "/root/.kfm/session/信箱" ] && [ -d "$MAILBOX/.git" ]; then
        (cd "$MAILBOX" && git add "$file" README.md letters-index.jsonl letter-tokens.jsonl \
            && git -c user.name=kfm-na -c user.email=na@kfm.local commit -q \
               -m "feat(信箱): $LETTER BAR-$NEXT 追踪信开卷（$FUNC$NAME：$TITLE）" </dev/null)
    fi
fi

TS=$(date '+%Y-%m-%d %H:%M %z')
printf '{"bar":%d,"title":"%s","claimant_func":"%s","claimant":"%s","letter":"%s","ts":"%s"}\n' \
    "$NEXT" "$TITLE" "$FUNC" "$NAME" "$LETTER" "$TS" >> "$REG"

echo "BAR-$NEXT 已领（$FUNC$NAME：$TITLE）${LETTER:+；追踪信 $LETTER}"
