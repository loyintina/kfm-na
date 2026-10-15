#!/bin/bash
# test-pre-merge-commit.sh — BAR-251 钉（追踪信 NA0180）：merge 提交必须过
# chain 闸。病灶：git merge 结构性不触发 pre-commit（只触发
# pre-merge-commit / commit-msg / post-merge），五门第①门在 merge 路径
# 缺席（0166 §二；临时仓探针实证见 NA0180 楼2）。修法＝新增
# .githooks/pre-merge-commit 薄壳 exec 委派 pre-commit（单实现防漂移）。
#
# 考法：临时仓装本仓 .githooks 副本，pre-commit 换成可控桩（exit 1 仿
# chain 红 / exit 0 仿绿）——考的是「钩接线」：merge 时闸在不在、拦不拦，
# 不考 chain 本体。桩被调即知委派生效（git 自己不会在 merge 时调它）。
#
# ★ 2026-10-16 自咬事件（本钉初版链上红）：本考题在 pre-commit 上下文里
# 被调时继承 GIT_DIR/GIT_WORK_TREE/GIT_INDEX_FILE 等环境，临时仓的
# git 调用被环境污染真仓（config 被写 bare/user、HEAD 被切换、refs 被
# 写假提交）——故一切临时仓 git 调用走 git_t（env -u 摘净 GIT_*），
# 钉⑤ 以毒环境实放防回退。
set -euo pipefail
cd "$(dirname "$0")/.."

T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

die() { echo "❌ BAR-251 钉：$1" >&2; exit 1; }

# 临时仓专用 git：摘净钩上下文继承的 GIT_* 环境变量，操作永不落真仓
git_t() {
    env -u GIT_DIR -u GIT_WORK_TREE -u GIT_INDEX_FILE \
        -u GIT_CONFIG_COUNT -u GIT_CONFIG_PARAMETERS \
        -u GIT_CONFIG_GLOBAL -u GIT_CONFIG_SYSTEM \
        -u GIT_OBJECT_DIRECTORY -u GIT_ALTERNATE_OBJECT_DIRECTORIES \
        -u GIT_AUTHOR_NAME -u GIT_AUTHOR_EMAIL \
        -u GIT_COMMITTER_NAME -u GIT_COMMITTER_EMAIL \
        git "$@"
}

# ① 源码锚：钩存在、可执行、且是委派 pre-commit 的薄壳（防回退成两份实现）
[ -x .githooks/pre-merge-commit ] \
    || die "①.githooks/pre-merge-commit 缺失或不可执行"
grep -q 'exec sh "$(dirname "$0")/pre-commit"' .githooks/pre-merge-commit \
    || die "①pre-merge-commit 未委派 pre-commit（闸语义须单实现）"
echo "  ①源码锚（钩在＋薄壳委派）✓"

# 造临时仓：先建仓提交（此时无钩不拦），再装本仓钩副本（pre-commit 换桩）
# ——一桩一案例，装闸在后，考的正是「merge 时闸在不在」
mk_repo() { # $1=桩退出码
    local repo="$T/repo$1"
    git_t init -q -b master "$repo"
    git_t -C "$repo" config user.email t@t
    git_t -C "$repo" config user.name t
    echo base > "$repo/a.txt"
    git_t -C "$repo" add a.txt
    git_t -C "$repo" commit -qm base
    git_t -C "$repo" checkout -qb feature
    echo feat > "$repo/b.txt"
    git_t -C "$repo" add b.txt
    git_t -C "$repo" commit -qm feat
    git_t -C "$repo" checkout -q master
    mkdir -p "$repo/hooks"
    cp .githooks/pre-merge-commit "$repo/hooks/"
    printf '#!/bin/sh\ntouch "%s/pre-commit.ran"\nexit %s\n' "$repo" "$1" \
        > "$repo/hooks/pre-commit"
    chmod +x "$repo/hooks/pre-commit"
    git_t -C "$repo" config core.hooksPath hooks
}

# ② 拦：桩 exit 1（仿 chain 红）→ merge 必须失败、无合并提交、桩确被调
mk_repo 1
if git_t -C "$T/repo1" merge --no-ff feature -m m >/dev/null 2>&1; then
    die "②chain 红时 merge 竟放行（pre-merge-commit 未拦）"
fi
git_t -C "$T/repo1" rev-parse -q --verify MERGE_HEAD >/dev/null \
    || die "②merge 被拦后 MERGE_HEAD 不在（拦点不对）"
[ -e "$T/repo1/pre-commit.ran" ] \
    || die "②桩未被调——pre-merge-commit 委派断（git merge 本不会调 pre-commit）"
echo "  ②拦（桩红 ⇒ merge 中止＋桩经委派被调）✓"

# ③ 放：桩 exit 0（仿 chain 绿）→ merge 成功、合并提交落地
mk_repo 0
git_t -C "$T/repo0" merge --no-ff feature -m m >/dev/null 2>&1 \
    || die "③chain 绿时 merge 竟被拦"
[ -e "$T/repo0/pre-commit.ran" ] || die "③桩未被调（委派断）"
git_t -C "$T/repo0" log --oneline -1 | grep -q m \
    || die "③merge 放行但合并提交未落地"
echo "  ③放（桩绿 ⇒ merge 成＋合并提交落地）✓"

# ④ 对照锚：摘掉 pre-merge-commit 后同一流程不再拦（坐实拦功在本钩，
# 不在别的机制——此钉即变异「摘钩」的常态化）
rm "$T/repo1/hooks/pre-merge-commit"
git_t -C "$T/repo1" merge --abort
if ! git_t -C "$T/repo1" merge --no-ff feature -m m >/dev/null 2>&1; then
    die "④摘钩后 merge 仍被拦（拦功不在本钩，定罪翻案）"
fi
echo "  ④对照锚（摘钩 ⇒ 桩红也拦不住）✓"

# ⑤ 钩上下文防泄漏锚：以 pre-commit 现场的毒环境（GIT_DIR/GIT_WORK_TREE/
# GIT_INDEX_FILE 指真仓）跑一条迷你流程，真仓必须毫发无伤——钉住「临时仓
# git 调用一律 git_t」这条命（2026-10-16 自咬事件：漏摘即真仓被写）
REAL_GITDIR=$(git rev-parse --absolute-git-dir)
REAL_MASTER=$(git rev-parse master)
REAL_HEAD=$(git rev-parse HEAD)
(
    export GIT_DIR="$REAL_GITDIR" GIT_WORK_TREE="$PWD" \
           GIT_INDEX_FILE="$REAL_GITDIR/index"
    mk_repo b
    git_t -C "$T/repob" merge --no-ff feature -m probe >/dev/null 2>&1 || true
)
[ "$(git rev-parse master)" = "$REAL_MASTER" ] \
    || die "⑤毒环境下真仓 master 被写动（GIT_* 泄漏回潮）"
[ "$(git rev-parse HEAD)" = "$REAL_HEAD" ] \
    || die "⑤毒环境下真仓 HEAD 被切换（GIT_* 泄漏回潮）"
git branch --list feature | grep . \
    && die "⑤毒环境下真仓被建 feature 分支（GIT_* 泄漏回潮）"
[ "$(git config --local --get user.email || true)" != "t@t" ] \
    || die "⑤毒环境下真仓 config 被写（GIT_* 泄漏回潮）"
echo "  ⑤钩上下文防泄漏锚（毒环境下真仓四样不变）✓"

echo "✅ BAR-251 钉全绿（五钉）"
