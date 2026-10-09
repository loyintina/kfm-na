#!/usr/bin/env bash
# bar-new.sh — BAR 号唯一领取口（2026-09-29 用户拍板：与信箱领号制同构，
# 治病 = 「grep 空号再写账」的 TOCTOU 竞态——三线共享工作树，grep 与落账
# 之间隔一个工单周期，两线同小时各 grep 出同一空号即双号）。
#
# BAR-189（0055 裁决甲案+三补丁）：领号唯一源扶正**信箱**——树内两账
# （bugs.md/registry）在分支流下树-local，登记到落 master 隔一个工单周期
# （185 双号实案 0053）；信箱开信即 commit 小仓 = 唯一跨线即时可见账。
# BAR-241（MAIN0135 裁决，237 撞号定罪）：号源再收口为**号位登记簿 ∪
# 信箱全量 H1**——登记簿 = 90-信箱/20-契约与向量/bar-号位登记.jsonl
# （设施仓内、跨树即时可见、登记即占有）；「改号/补登不开信」的占号动作
# 在 H1 里不可见（237 实案：只在 master 树内两账登记），登记簿是唯一能
# 兜住这类动作的账。树内两账（bugs.md/registry）降为**投影**——继续维护、
# 继续被 chain 检查，但不再作号源。同族前案：185 双号（0053）/202 双号
# （符号链接扫描得 0）/237-238 撞号（MAIN0135 §一）；另 registry 扫描曾
# 恒空（grep 不容忍冒号后空格，而存量被一次手工合并全量重写为带空格，
# 0135 楼1 实证）——本版扫描口径带空格容忍，写入格式被考题钉死。
# 领号口径再收窄到账位（BAR-192）：登记簿只认 "bar":N 字段（冒号后空格
# 容忍）、信箱 H1 只认每行首行首个 BAR-NNN——prose/正文援引的号不许毒
# max（BAR-999 陷阱号曾毒出实领 1000 跳号）。
# fail-closed（0055 补丁② + MAIN0135 §三.5）：登记簿或信箱任一不可读 =
# **拒领**，不许退回树内旧账。名册预检提到领号前（§六 名字登记前移后
# 开信 fail-closed，预检杜绝「号领了信生不出」）。
#
# 一次原子完成四件事（flock 包裹——锁落登记簿旁，跨树同一把；同刻只许
# 一个领号者）：
#   1. 领号：max(号位登记簿, 信箱全量 H1) + 1
#   2. 登记：登记簿追加一行（登记即占有）+ 树内 registry 投影同步追加
#   3. 开追踪信：mailbox-cli new（致全体/通报/待落地通报）+ 当场填白话结论
#      与立案节（不留占位——chain 第 4 步信箱执法常绿，不挡别线的闸）
#   4. 回写：信号（NNNN）入登记行；信箱小仓自动 commit（信+登记簿+投影）
#
# 用法: scripts/bar-new.sh --func 开发部 --name 观澜 "主题一句话"
#   主题禁含双引号与反斜杠（jsonl 转义从简）。
#
# 夹具测试口（test-bar-new.sh 用，正常领号别设）：
#   BAR_NEW_LEDGER=路径    替代号位登记簿（锁取同目尾加 .lock）
#   BAR_NEW_REGISTRY=路径  替代 bar-registry.jsonl（投影回写目标）
#   BAR_NEW_NO_LETTER=1    跳过开信（只领号+登记；信箱扫描仍执行）
#   BAR_NEW_MAILBOX=路径   替代信箱目录（扫描源/名册/new 指向它）
#   ※ LEDGER/REGISTRY/MAILBOX 三件全设或全不设——部分设置 = 半隔离拒跑
#     （0135 楼9②：漏设 REGISTRY，假号 1001 写进真 registry 实案）
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
# 夹具隔离完整性闸（0135 楼9② 白露实撞：只设 BAR_NEW_LEDGER 漏设
# BAR_NEW_REGISTRY → 假号 1001 写进真 registry）——三个重定向测试口
# 要么全设要么全不设，部分设置 = 半隔离 = 拒跑
_fx=0
for _v in BAR_NEW_LEDGER BAR_NEW_REGISTRY BAR_NEW_MAILBOX; do
    [ -n "${!_v+x}" ] && _fx=$((_fx+1))
done
if [ "$_fx" -ne 0 ] && [ "$_fx" -ne 3 ]; then
    echo "夹具隔离不完整：BAR_NEW_LEDGER/REGISTRY/MAILBOX 须全设或全不设（当前设了 $_fx/3）——拒跑（0135 楼9 假号 1001 实案）" >&2
    exit 2
fi
unset _fx _v
REG="${BAR_NEW_REGISTRY:-$ROOT/docs/ledger/bar-registry.jsonl}"
LEDGER="${BAR_NEW_LEDGER:-$HOME/90-信箱/20-契约与向量/bar-号位登记.jsonl}"
LOCK="$LEDGER.lock"
CLI="$ROOT/target/debug/mailbox-cli"
# 2026-10-06（观澜 0126 9楼）：清主仓 target 会连坐领号器前置件——
# 缺件自愈：cargo build -p mailbox-cli（39s 级，一次性），别让全信箱
# 的领号停在「谁记得补建」上。
[ -x "$CLI" ] || {
    echo "[bar-new] mailbox-cli 缺（清 target 后遗症）——自动重建中"
    (cd "$ROOT" && cargo build -p mailbox-cli) || {
        echo "❌ mailbox-cli 重建失败——领号中止" >&2
        exit 1
    }
}
MAILBOX="${BAR_NEW_MAILBOX:-$HOME/.kfm/session/信箱}"
DEFAULT_MAILBOX="$HOME/.kfm/session/信箱"
# 符号链接解引用（202 双号实案 2026-09-30：生产信箱 = 上面那个路径
# → 90-信箱/10-NA信箱 的 symlink，find 默认 -P 对**起点链接**不下钻——
# 扫描静默得 0 封、max 退回树内旧账 → 与观澜 0092 撞号。readlink -f 对
# 非链接路径原样返回，幂等）
MAILBOX="$(readlink -f "$MAILBOX")"
# BAR-217：DEFAULT_MAILBOX 同归一——上一行把 MAILBOX 解引用成真身
# （/root/90-信箱/10-NA信箱），DEFAULT 不归一 = 下方「= 默认信箱才自动
# commit」的字符串比恒假，追踪信永不自动提交（0138/0143/0144 untracked
# 漂流三封实证）。两边都 readlink -f 再比（幂等，非链接原样返回）
DEFAULT_MAILBOX="$(readlink -f "$DEFAULT_MAILBOX")"

# 名册预检（0055 补丁③，提到领号前）：§六《名字登记前移》后开信对未入册名
# fail-closed——不预检会留下「号领了、信生不出」的坑。名册出处 = 信箱
# roster.json → 主册 roster.json；两处都没有 = 无法校验（双环境手机端），
# 告警放行。查到名册而名不在册 = 拒领。
ROSTER="$MAILBOX/roster.json"
[ -f "$ROSTER" ] || ROSTER="$HOME/90-信箱/00-主册/roster.json"
if [ -f "$ROSTER" ]; then
    grep -q "\"$NAME\"" "$ROSTER" || {
        echo "名字 $NAME 不在名册（$ROSTER）——先入册再领号（信箱契约 §六）" >&2; exit 1; }
else
    echo "警告：名册两处皆无，名字在册预检跳过（双环境告警）" >&2
fi

exec 9>"$LOCK"
flock 9

MAX=0
# 号源第一账 = 号位登记簿（BAR-241，MAIN0135 §三：权威账）。fail-closed：
# 不可读 = 拒领（白露回填到位前本工具全线拒领，宁停不错发）。
# 口径只认 jsonl 的 "bar":N 字段、冒号后空格容忍（0135 楼1 实证：存量曾
# 被手工合并全量重写为带空格格式，不容忍 = 整账恒空）；prose 不毒 max。
[ -r "$LEDGER" ] || {
    echo "号位登记簿不可读：$LEDGER——拒领（MAIN0135 §三.5，存量回填未到位/路径错）" >&2; exit 1; }
n=$(grep -oE '"bar": *[0-9]+' "$LEDGER" | grep -oE '[0-9]+' | sort -n | tail -1 || true)
if [ -n "$n" ] && [ "$n" -gt "$MAX" ]; then MAX=$n; fi

# 号源第二账 = 信箱全量信件 **H1** 的 BAR-NNN（0055 甲案扶正，兜存量——
# 登记簿回填前的老号多只有 H1 在册）。信箱开信即 commit 小仓，跨线即时
# 可见。口径 = H1 首行（闻灯 0054：文件名出生冻结，让号改号只改 H1——
# 扫文件名会复活旧号）。含归档目录（0055 补丁①：老号信归档后 max 不许
# 回退，否则等于把抽过的号再发一次）。
# 补丁②：信箱不可读 = 拒领，不许静默退回树内旧账（那正是用了过期账的病灶）。
# 扫描无条件执行（NO_LETTER 只跳过开信，不跳过领号账——夹具请配 BAR_NEW_MAILBOX）。
[ -d "$MAILBOX" ] || {
    echo "信箱不可读：$MAILBOX——拒领（0055 补丁②，不许退回树内旧账）" >&2; exit 1; }
# 每行 H1 只认**首个** BAR-NNN（BAR-192：H1 标题正文援引他号如「修复
# BAR-123 回归」不许毒 max——领号信标题形制 BAR-$NEXT 在前，首号即本信号）。
# 空扫描面 = 病灶（链接/权限/路径错），不许当「零封信箱」静默领 1 号——
# 与补丁②同族：扫描源异常一律拒领，fail-closed
SCAN_N=$(find "$MAILBOX" -name '*.md' -not -path '*/.git/*' 2>/dev/null | wc -l)
[ "$SCAN_N" -gt 0 ] || {
    echo "信箱扫描面为空：$MAILBOX（0 封 .md）——扫描异常拒领（202 双号实案同族病灶）" >&2; exit 1; }
while IFS= read -r n; do
    [ -n "$n" ] && [ "$n" -gt "$MAX" ] && MAX=$n
done < <(find "$MAILBOX" -name '*.md' -not -path '*/.git/*' -exec head -q -n 1 {} + 2>/dev/null \
    | awk 'match($0, /BAR-[0-9]+/) { print substr($0, RSTART+4, RLENGTH-4) }' || true)
NEXT=$((MAX + 1))

# 关于段（2026-10-08 用户拍板：**题目自己填、不许机械截断**）——调用方用 BAR_ABOUT 传
# 一句**纯汉字短题**（2–12 字；禁 连接字 号/致/复/的/及/等；禁点号；**禁 ASCII——
# 号不进 about**（MAIN0135 楼6 白露撤回楼3：JS gen 关于段守纯汉字旧约，BAR 号必含
# ASCII 必卡全册 gen，清和 0169 实撞）；号只在 H1 与登记簿，文件名结构性不带号 =
# 文件名与 H1 号分叉（0169 文件名 239/H1 240 实案）不可能再发生）。
# 缺则拒绝开工（宁可当场问一句，不要落一个残句名）；含非汉字本工具当场拒，
# 别等 gen 才炸（清和 0135 楼5 洞②的提前闸）。
ABOUT="${BAR_ABOUT:-}"
if [ -z "$ABOUT" ]; then
    echo "❌ 缺关于段：跑 bar-new 时带上 BAR_ABOUT=\"<纯汉字短题>\"（2–12 字、禁 号致的及等、禁点、禁 ASCII——号不进 about）" >&2
    echo "   例：BAR_ABOUT=\"领号器号源收口\" bash scripts/bar-new.sh --func 开发部 --name 闻灯 \"领号器号源\"" >&2
    exit 1
fi
printf '%s' "$ABOUT" | python3 -c 'import sys,re; sys.exit(0 if re.fullmatch(r"[\u4e00-\u9fff]{2,12}", sys.stdin.read()) else 1)' || {
    echo "❌ 关于段只许 2–12 个纯汉字（号不进 about，MAIN0135 楼6）：$ABOUT" >&2; exit 1; }
# 直写 --about "$ABOUT"（0135 楼5 洞①急件：ABOUT_OPT 中间变量展开不做二次
# 引号解析，字面双引号进 about 值 → 必被 mailbox-cli 字符校验拒，fresh
# master 领号全撞；清和两格 diff 收编）

LETTER=""
if [ "${BAR_NEW_NO_LETTER:-}" != "1" ]; then
    [ -x "$CLI" ] || { echo "缺 $CLI——先 cargo build -p mailbox-cli" >&2; exit 1; }
    NOW=$(date '+%Y-%m-%d %H:%M %z')
    TODAY=$(date '+%Y-%m-%d')
    out=$("$CLI" new --mailbox "$MAILBOX" --from-func "$FUNC" --from-name "$NAME" \
        --to-all --type 通报 --title "BAR-$NEXT $TITLE" \
        --kind 通报 --expect "承办线（$FUNC$NAME）以楼层追加进展：$TITLE" \
        --criteria "BAR-$NEXT 用户终验结案（信头状态翻 已验证）" \
        --status 待落地通报 --about "$ABOUT")
    file=$(printf '%s' "$out" | sed -n 's/.*已生成 \([^ ]*\.md\).*/\1/p')
    [ -n "$file" ] || { echo "mailbox-cli new 输出解析失败: $out" >&2; exit 1; }
    # 2026-09-29 评审修订（白露）：不写本册自指码——契约 §二「分拣码…本册信一律不写，
    # 只在跨册引用时写全码」。原行给编号无条件加 NA 前缀，致使 registry 的 letter 字段
    # 与提交信息的「追踪信 NA00NN」指向并不存在的文件名（NA0015 即此误用的成品）。
    # 新写法剥掉可选分拣码，取纯流水号（`0016号…` 与 `NA0016号…` 都归 `0016`）。
    LETTER=$(printf '%s' "$file" | sed -n 's/^\([A-Z]\{0,4\}\)\([0-9]\{4\}\).*/\2/p')
    path="$MAILBOX/$file"
    plain="本信追踪的问题是：$TITLE。当前状态 = 已立案开工，承办 $FUNC$NAME；立案在信纸、后续进展都在本信楼层追加（规矩 = na 仓 AGENTS.md「BAR 追踪信」节：追加一律走楼层），隐藏读者看最新一楼即可，无需做任何事。"
    body="### §一 立案（$NOW）\n\n- 主题：$TITLE\n- 承办：$FUNC$NAME\n- 定罪/修复/判卷/终验自下一楼起按时间追加（letters floor——信纸只此立案节，2026-10-04 修律）；信头状态随生命周期翻转\n  （待落地通报 → 已落地 → 已验证），打回翻回 待落地通报。\n\n——$FUNC$NAME · $TODAY"
    sed -i "s|（待填：面向隐藏读者。三句话内说清是什么事、要不要他做事；不写工作术语。）|$plain|" "$path"
    sed -i "s|（待填：结论与推导一起写——精确留给承重的数字和名字，其余白话；每条发现标「明写／推断／补全」。）|$body|" "$path"
    grep -q '（待填' "$path" && { echo "占位未填尽: $path" >&2; exit 1; }
    "$CLI" verify --mailbox "$MAILBOX" "$path" >/dev/null
    "$CLI" gen --mailbox "$MAILBOX" >/dev/null
    # BAR-226：册探测改用 git rev-parse——na 册并入设施仓（90-信箱）后
    # .git 在设施仓根、册子目录里无字面 .git 目录，旧闸 `[ -d "$MAILBOX/.git" ]`
    # 恒假 → 追踪信开了不落盘、静默漂流（0154/0155 实证）。rev-parse 沿父目录
    # 找仓，独立册仓与设施仓子目录两形态都认；git add 的 pathspec 相对 cwd
    # （册目录）解析、commit 落所属仓，两形态行为一致。
    if [ "$MAILBOX" = "$DEFAULT_MAILBOX" ] && git -C "$MAILBOX" rev-parse --git-dir >/dev/null 2>&1; then
        # 2026-10-05（白露 MAIN0123 §五 残留②）：提交改走 letters commit——
        # 裸 git 会被设施门③④拒（作者=kfm-na ≠ 信封作者；暂存区对表），拒后
        # 滞留暂存区还挡全仓发楼（MAIN0113 习性）。letters commit 自带
        # verify→gen→精确 pathspec→信封署名，失败即退不滞留。
        LETTERS_BIN="${LETTERS_BIN:-$(dirname "$MAILBOX")/30-工具/letters}"
        if [ -x "$LETTERS_BIN" ]; then
            "$LETTERS_BIN" commit "$path" \
                -m "feat(信箱): $LETTER BAR-$NEXT 追踪信开卷（$FUNC$NAME：$TITLE）" \
                || echo "⚠ letters commit 失败——信在盘未提交，请手动 letters commit $file" >&2
        else
            echo "⚠ letters 不在 $LETTERS_BIN——信在盘未提交，请手动 letters commit $file" >&2
        fi
    fi
fi

TS=$(date '+%Y-%m-%d %H:%M %z')
# 登记即占有：登记簿（权威账，跨树可见）与树内 registry 投影同步落。
# 两账写入格式被 test-bar-new.sh 钉死（0135 楼2：读侧修得再对，写侧
# 换格式而考题不红 = 白修）；registry 带空格对齐存量书写惯例（592317b
# 手工合并重写后全库通行），扫描口径两格式都容忍。
# 字段与 registry 同名 + src（MAIN0135 楼4 白露回填簿同款 schema）；
# letter 存全文件名（与回填行一致），NO_LETTER 夹具口 src=领号、letter 空
SRC=开信; [ -n "$LETTER" ] || SRC=领号
printf '{"bar": %d, "title": "%s", "claimant_func": "%s", "claimant": "%s", "letter": "%s", "ts": "%s", "src": "%s", "state": "有效"}\n' \
    "$NEXT" "$TITLE" "$FUNC" "$NAME" "${file:-}" "$TS" "$SRC" >> "$LEDGER"
printf '{"bar": %d, "title": "%s", "claimant_func": "%s", "claimant": "%s", "letter": "%s", "ts": "%s"}\n' \
    "$NEXT" "$TITLE" "$FUNC" "$NAME" "$LETTER" "$TS" >> "$REG"

# 登记簿提交（册探测逻辑同上方 letters commit 块的 BAR-226 rev-parse 注释）：
# 信件本体已由上方 letters commit 落盘，此处只补权威账——权威账不落提交 =
# 跨树可见性空话（237 撞号正因为占号动作没落跨树账）。署名归领号人
# （信箱提交门④拒 kfm-na 代署——0167 实证），邮箱取名册、缺则 <名>@kfm.dev。
if [ "$MAILBOX" = "$DEFAULT_MAILBOX" ] && git -C "$MAILBOX" rev-parse --git-dir >/dev/null 2>&1; then
    EMAIL=$(python3 -c '
import json,sys
try: print(json.load(open(sys.argv[1]))["names"][sys.argv[2]].get("email","") or "")
except Exception: print("")' "$ROSTER" "$NAME" 2>/dev/null)
    [ -n "$EMAIL" ] || EMAIL="$NAME@kfm.dev"
    (cd "$MAILBOX" && git add "$LEDGER" \
        && GIT_AUTHOR_NAME="$NAME" GIT_AUTHOR_EMAIL="$EMAIL" \
           GIT_COMMITTER_NAME="$NAME" GIT_COMMITTER_EMAIL="$EMAIL" \
           git commit -q -m "chore(号位): BAR-$NEXT 登记（$FUNC$NAME：$TITLE）" </dev/null)
fi

echo "BAR-$NEXT 已领（$FUNC$NAME：$TITLE）${LETTER:+；追踪信 $LETTER}"
