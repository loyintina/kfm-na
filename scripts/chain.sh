#!/bin/bash
# chain.sh — KFM-NA 唯一检查入口（纪律第一档，2026-08-13 出生即有）
#
# 防泄漏闸 → fmt → clippy → android-check → java 编译 → test → build，任一红
# 即中断。pre-commit 钩子挂本脚本，保证每个提交都是绿的。kfmv4 的 chain:auto
# 有 51 步是复杂度长出来的结果，本项目从 4 步开始长（2026-08-13 第 5 步：
# android 目标 check——cfg 盲区防烂；同日第 6 步：Java 皮编译——javac 盲区
# 防烂；2026-08-18 第 1 步前置：字体防泄漏闸——商业字体永不进库，BAR-021）——
# 新检查一律加在这里，禁止另起入口。
cd "$(dirname "$0")/.." || exit 1

# 2026-08-21 降压：整条链 nice +10 / ionice best-effort 最低档——CPU/IO
# 争用时让交互进程（SSH、各会话收发）先行；空闲时编译速度不变
# （nice 只在抢时生效）。顶部自重启一次，全步骤继承，不逐条包。
# 起因：多 agent 同链撞车时交互会话被编译拖卡（2026-08-21 实踩）
if [ -z "$KFM_CHAIN_NICED" ]; then
    # 2026-08-21 增：整链同时进独立 cgroup「kfm-builds」（内存隔离，评审代接）——
    # 编译尖峰只在自己桶里互杀，不再与三线 agent 共享内存账（OOM 连坐可防）。
    # helper 在 kfmv4 侧（共享本机构建基础设施）；缺失/不可写则回退纯 nice。
    # 位置 env 可覆盖（KFM_V4_CGROUP_HELPER），缺省 $HOME 下同级项目。
    V4_CGROUP_HELPER="${KFM_V4_CGROUP_HELPER:-$HOME/10-项目/kfmv4/scripts/build-enter-cgroup.sh}"
    if [ -x "$V4_CGROUP_HELPER" ] && [ -w /sys/fs/cgroup/agent.slice ]; then
        KFM_CHAIN_NICED=1 exec bash "$V4_CGROUP_HELPER" nice -n 10 ionice -c2 -n7 bash "$0" "$@"
    elif command -v ionice >/dev/null 2>&1; then
        KFM_CHAIN_NICED=1 exec nice -n 10 ionice -c2 -n7 bash "$0" "$@"
    else
        KFM_CHAIN_NICED=1 exec nice -n 10 bash "$0" "$@"
    fi
fi

echo "=== [chain 1/13] 字体防泄漏闸（BAR-021） ==="
# 商业字体（assets/fonts/local/）永不进库：gitignore 是第一道，这道闸是
# 第二道机械执法——误 git add -A 也漏不出去。同时卡住超大字体资产
# （占位字体子集化后应 <4MB，超了就是忘了烘焙）
if git ls-files assets/fonts | grep -qi 'local/\|AaHMKJXST'; then
    echo "❌ 商业字体混入暂存区：git rm --cached 后再试"; exit 1
fi
big=$(git ls-files assets/fonts | while read -r f; do
    [ -f "$f" ] || continue
    size=$(stat -c%s "$f")
    # 唯一豁免 = NotoSansCJKsc-kfm.ttf（BAR-176：CJK 备用换黑体大字库，
    # 汉字 27924 全量正是这单的意义，裁体积 = 裁覆盖率——单独帽 16MB，
    # 烘焙脚本判卷自带 ≤15MB 断言）
    if [ "$f" = "assets/fonts/NotoSansCJKsc-kfm.ttf" ]; then
        [ "$size" -gt 16777216 ] && echo "$f"
    else
        [ "$size" -gt 4194304 ] && echo "$f"
    fi
done)
[ -z "$big" ] || { echo "❌ 字体资产超体积帽（未子集化？）: $big"; exit 1; }
# 防泄漏闸之二（2026-09-24，BAR-143 形态升级 纯段落→代码守卫）：
# 密钥/证书类文件永不进库——quic.key.der 被 git add -A 推进双远端事故
bash scripts/check/check-no-secrets.sh || { echo "❌ 密钥防泄漏闸不过"; exit 1; }

echo "=== [chain 2/13] 核心层零依赖闸（多端分层纪律 1，评审裁决 5） ==="
# cordis-na = 多端核心层基座：零依赖是公开承诺（crates/cordis-na/Cargo.toml
# 注释钉死）。多一行依赖 = 核心/壳边界破洞——先讨论改闸，不许偷渡
core_deps=$(cargo tree -p cordis-na --depth 1 --prefix none | tail -n +2 | wc -l)
[ "$core_deps" = "0" ] || { echo "❌ cordis-na 染指依赖（$core_deps 个）：核心层必须零依赖"; exit 1; }

echo "=== [chain 3/13] stats 字段咬合闸(2026-08-27,评审裁决建议) ==="
# StatsSnap 加字段忘进 format! = 观测静默瞎一格(BAR-040 复核揪出的
# 半成品态 E0063)。字段 ↔ 输出键咬合,别名必须在检查脚本里登记
bash scripts/check/check-stats-format.sh || { echo "❌ stats 字段咬合不过"; exit 1; }

# 2026-09-17 自守卫（nz 形态效力阶梯：守卫本身也要被守卫）——仪器证据门
# 被摘出 commit-msg / 脚本丢失或不可执行 = chain 红，代码守卫不许静默退化
grep -q 'check-fix-instrument' .githooks/commit-msg || { echo "❌ commit-msg 钩子缺仪器证据门（check-fix-instrument）"; exit 1; }
[ -x scripts/check/check-fix-instrument.sh ] || { echo "❌ check-fix-instrument.sh 缺失或不可执行"; exit 1; }
[ -x scripts/check/grid-text-ratchet.sh ] || { echo "❌ grid-text-ratchet.sh 缺失或不可执行（BAR-178 棘轮闸不许静默退化）"; exit 1; }
[ -f scripts/check/grid-text-baseline.txt ] || { echo "❌ grid-text-baseline.txt 缺失（BAR-178 棘轮基线）"; exit 1; }
[ -x scripts/check/test-grid-text-ratchet.sh ] || { echo "❌ test-grid-text-ratchet.sh 缺失或不可执行（BAR-188 棘轮考题不许静默退化）"; exit 1; }
[ -x scripts/check/test-bar-new.sh ] || { echo "❌ test-bar-new.sh 缺失或不可执行（BAR-189 领号考题不许静默退化）"; exit 1; }

echo "=== [chain 4/13] na 信箱执法（mailbox.sh） ==="
# 2026-09-29 第 4 步：na 信箱（$HOME/.kfm/session/信箱）全册 verify +
# gen --check-only（信箱目录不存在的双环境自动跳过）。工具链 =
# mailbox-core/mailbox-cli（kfmv4 JS 三件套 Rust 移植，提案 0004）
bash scripts/check/mailbox.sh || { echo "❌ na 信箱执法不过"; exit 1; }

echo "=== [chain 5/13] 网格文字棘轮闸（BAR-178，布局唯一源） ==="
# 2026-09-29 用户拍板：新功能文字/布局不走网格引擎（ui::grid_text）= 红；
# 存量登记 scripts/check/grid-text-baseline.txt，迁移一面收一面（只缩不涨）。
# BAR-188 改约：比对键 = file:symbol 多重集（行号不入键——插行平移不再误红）
bash scripts/check/grid-text-ratchet.sh || { echo "❌ 网格文字棘轮闸不过"; exit 1; }
bash scripts/check/test-grid-text-ratchet.sh || { echo "❌ 棘轮闸考题不过（BAR-188）"; exit 1; }

echo "=== [chain 6/13] cargo fmt --check ==="
# 2026-08-17 workspace 化（crates/cordis-na)：带根包的 workspace 里裸 cargo
# fmt/clippy/test 只覆盖根包——不加 --all/--workspace 会让 crate 考题静默脱链
cargo fmt --all --check || { echo "❌ fmt 不过：跑 cargo fmt --all 后重试"; exit 1; }

echo "=== [chain 7/13] cargo clippy ==="
cargo clippy --workspace --all-targets -- -D warnings || { echo "❌ clippy 不过"; exit 1; }

echo "=== [chain 8/13] cargo check --target aarch64-linux-android ==="
# Android 代码 cfg 在宿主不可见（fmt/clippy/test 都跳过它）——不查就会烂在盲区
# ring（rustls 后端）是第一个要编 C 的依赖：build.rs 找 aarch64-linux-android-clang，
# 服务器得指 NDK；手机 Termux 的 cc 原生就是目标三元组，无需指（2026-08-31）
if [ ! -d /data/data/com.termux ]; then
    NDK_BIN="${KFM_NA_TOOLCHAIN:-$HOME/40-资产/kfm-na-toolchain}/sdk/ndk/27.2.12479018/toolchains/llvm/prebuilt/linux-x86_64/bin"
    export CC_aarch64_linux_android="$NDK_BIN/aarch64-linux-android24-clang"
    export AR_aarch64_linux_android="$NDK_BIN/llvm-ar"
    export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$NDK_BIN/aarch64-linux-android24-clang"
fi
cargo check --target aarch64-linux-android || { echo "❌ Android 目标编译不过"; exit 1; }

echo "=== [chain 9/13] javac（Java 皮编译检查） ==="
# Java 皮（android/java/）是中文输入的命脉，又不在 cargo 视野内——编译检查
# 防「改了 Java 没打过包」的烂尾。APK 全量打包走 scripts/package-apk.sh
# 双环境：服务器用本地 JDK+SDK；手机 Termux 用 openjdk-21 + 拷来的 android.jar
if [ -d /data/data/com.termux ]; then
    JAVAC=javac
    AJAR="$HOME/kfm-na-toolchain/android.jar"
else
    TOOLCHAIN="${KFM_NA_TOOLCHAIN:-$HOME/40-资产/kfm-na-toolchain}"
    JAVAC="$TOOLCHAIN/jdk/bin/javac"
    AJAR="$TOOLCHAIN/sdk/platforms/android-35/android.jar"
fi
rm -rf build/java-check && mkdir -p build/java-check
"$JAVAC" -source 8 -target 8 \
    -cp "$AJAR" \
    -d build/java-check android/java/dev/kfm/na/*.java 2>&1 \
    | grep -v 'bootstrap class path' || true
# javac 的告警（-source 8 过时）不挡路，编译失败才挡
[ "${PIPESTATUS[0]}" -eq 0 ] || { echo "❌ Java 皮编译不过"; exit 1; }
rm -rf build/java-check

echo "=== [chain 10/13] cargo test ==="
# ---- NA0190（2026-10-18 清和裁）两件：失败落日志 + 计时窗类局部串行 ----
# 背景：na-server 的 live_spec／bar171_spec 在并发抢时下会误红（实测两例：
# 链内 5s 级红、单跑 0.2s 全绿，慢 20~24 倍）。处置：
#   ①**只让计时窗类进串行档**：整包 na-server 用 --test-threads=1 跑（就这两个
#     target 在这个 crate 里），其余照旧并行——全链串行要 +7min，局部约 +十几秒；
#   ②**失败落完整输出**：落 logs/chain-fail-<时刻>.log，只取该测试阶段的
#     「失败用例名＋栈＋前后 20 行」，终端打一行指路（不刷屏）。
# 判据（清和写死）：三条落地后同样负载连跑三轮零红；若局部串行后同类仍红，
# 那不是并发问题=真缺陷，立刻回头按真缺陷查，不许拿「又抖了」搪塞。
CHAIN_FAIL_DIR="logs"
mkdir -p "$CHAIN_FAIL_DIR"
chain_test_log="/tmp/kfm-chain-test-$$.log"
_tail_fail() {
    # $1=阶段名；其余=命令。失败时落片段日志并返回 1。
    local label="$1"; shift
    if "$@" >"$chain_test_log" 2>&1; then
        return 0
    fi
    local ts out
    ts=$(date +%Y%m%d-%H%M%S)
    out="$CHAIN_FAIL_DIR/chain-fail-$ts.log"
    {
        echo "# chain 测试阶段失败｜$label｜$ts"
        echo "# 命令：$*"
        echo "# 只落失败用例名＋栈＋命中行前后 20 行（NA0190 清和批）"
        grep -n -E '^---- .* stdout ----|^failures:$|^thread .* panicked' "$chain_test_log" \
            | head -20 | while IFS=: read -r ln rest; do
                local st en
                st=$(( ln > 20 ? ln - 20 : 1 )); en=$(( ln + 20 ))
                echo "----- 命中第 $ln 行：$rest"
                sed -n "${st},${en}p" "$chain_test_log"
            done
        echo "----- 末尾 20 行"
        tail -n 20 "$chain_test_log"
    } >"$out"
    echo "❌ 测试不过（$label）——失败片段：$out"
    return 1
}
_tail_fail "na-server 串行档（计时窗类：live_spec／bar171_spec，--test-threads=1）" \
    cargo test -p na-server -- --test-threads=1 || { exit 1; }
_tail_fail "全工作区并行（--exclude na-server）" \
    cargo test --workspace --exclude na-server || { exit 1; }

echo "=== [chain 11/13] overlay 打包核考题（L2,fixture 假 deb) ==="
# 2026-08-22 第 8 步：overlay-pack 是纯 shell 变换，cargo 看不见——
# fixture 考题钉死剥前缀/改写/建链三规则（设计 docs/active/l2-overlay.md)
bash scripts/test-overlay.sh || { echo "❌ overlay 考题不过"; exit 1; }
# 2026-08-24 同步挂入：kfm-pkg 原子性考题（BAR-031——中断标记/重装自愈/
# 装后校验，zsh 卡死案病根）
bash scripts/test-kfm-pkg.sh || { echo "❌ kfm-pkg 考题不过"; exit 1; }
# 2026-09-17 同步挂入：仪器证据门八言考题（BAR-104/105/106 三部曲——
# 假设驱动修复拦截门的自钉，门被改坏即红）
bash scripts/test-check-fix-instrument.sh || { echo "❌ 仪器证据门考题不过"; exit 1; }
# 2026-09-29 BAR-189：bar-new.sh 领号唯一源考题（0055 甲案三补丁——信箱 H1
# 全量扫描含归档/不可读拒领/名册预检；领号闸被改坏 = 双号竞态回潮）
bash scripts/check/test-bar-new.sh || { echo "❌ bar-new 领号考题不过"; exit 1; }
# 2026-09-19 同步挂入：BAR-109 relay 静默掐连行为钉（4s 死亡线命案——
# 修复被回退 = redroid 一切静画面长连接复死，行为级判卷不 grep 源码）
bash scripts/test-relay-timeout.sh || { echo "❌ BAR-109 relay 考题不过"; exit 1; }
# 2026-09-21 同步挂入：BAR-123 快捷键行三处同尺守卫（抬手 hit 被摘
# chrome_inset 命案——再有人摘 inset 链误伤键行 = 当场红）
bash scripts/test-bar123-keybar-ruler.sh || { echo "❌ BAR-123 同尺守卫考题不过"; exit 1; }
# 2026-09-21 晨班会形态审计挂入：BAR-114 跳框模态压顶守卫（光标层合成
# 位不问模态 = 下池三级框压盖跳框命案——拆层件合成位摘模态闸 = 当场红）
bash scripts/test-bar114-modal-cursor.sh || { echo "❌ BAR-114 模态压顶守卫考题不过"; exit 1; }
# 2026-09-23 同步挂入：BAR-136 pack-toolchain 冻结韧性契约钉（8022 长 tar
# 流被 vivo 冻结掐死三跑命案——续传/对账/绝对路径/files-only 四契约）
# 2026-10-19 挂入：BAR-273 推核前闸（MAIN0192 楼418／420 清和裁——核内嵌 vc 必须与
# 机上 APK versionCode 一致；不一致即拒推。11 机实证：核比壳新 ⇒ 一点终端即崩）
# 探针报行文本层校验（MAIN0192 楼739／740：缺字段用例落在文本层，喂手工造的行；
# 模块内单测只算自述，进证据面必须给第三方可复跑命令）
# 探针报行文本层校验：**暂不在链上**（楼764／766）——dims 词表归契约，工具不内置词表，
# 词表文件未落时它会拒跑（rc=2）。挂着要么红、要么逼我造词表，两者都不许。
# 词表落到 scripts/check/lockprobe-dims-vocab.txt 后，把下面两行取消注释即挂回：
# python3 scripts/check/check-lockprobe-line.py --fixture scripts/check/lockprobe-samples-real.txt || { echo "❌ 探针真样本对表不过"; exit 1; }
# python3 scripts/check/check-lockprobe-line.py --self-test || { echo "❌ 探针报行文本层校验不过"; exit 1; }
# 同接收者重入属性检查（MAIN0192 楼697／700：判据按属性不按行号清单）——命中即红，
# 豁免须在 scripts/check/mutex-reentry-allow.txt 写理由
python3 scripts/check/check-mutex-reentry.py || { echo "❌ 同一 Arc 持锁期内二次取锁（自锁类）"; exit 1; }
# 缺件闸先行（楼517：查「脚本在不在位」必须落在调用方——写在被检脚本自己里，
# 脚本被删时那一条根本不执行，等于没有）。缺件的红与脚本自报错的红分开。
[ -f scripts/check/check-hot-core-vc.sh ] && [ -f scripts/test-hot-core-vc.sh ] \
  || { echo "❌ 推核前闸缺件（check-hot-core-vc.sh 或 test-hot-core-vc.sh 不在位）——闸等于不存在"; exit 1; }
bash scripts/test-hot-core-vc.sh || { echo "❌ 推核前闸考题不过"; exit 1; }
bash scripts/test-pack-toolchain.sh || { echo "❌ BAR-136 pack 契约考题不过"; exit 1; }
# 2026-09-24 同步挂入：BAR-150 半截核不落位契约钉（cat&&mv 假原子——
# ssh 断流 cat 退出 0，2MB 半截核直落位命案；对账先于 mv/失败 exit/
# mv 不挂 cat 链三言）
bash scripts/test-na-push-so-atomic.sh || { echo "❌ BAR-150 半截核契约考题不过"; exit 1; }
# 2026-10-14 同步挂入：BAR-248 拉起腿身份核验钉（8022 三易其主误拉 9 机
# 旧核互踢命案——核验被摘 = 谁占 8022 谁挨 am start；异机拒/同机放/
# 覆盖放/取不到拒/adb 臂直发五行为＋源码锚）
bash scripts/test-gate-am-start.sh || { echo "❌ BAR-248 身份核验考题不过"; exit 1; }
# 2026-10-16 同步挂入：BAR-251 merge 过闸钉（git merge 结构性不触发
# pre-commit——chain 闸在合并路径缺席，0166 §二；pre-merge-commit 薄壳
# 委派被摘/改错 = merge 又绕开 chain 落地）
bash scripts/test-pre-merge-commit.sh || { echo "❌ BAR-251 merge 过闸考题不过"; exit 1; }

echo "=== [chain 12/13] 考卷覆盖矩阵棘轮闸（2026-08-27,自我测试缺口④） ==="
# 治「新功能补题靠自觉」:各模块 pub 项被 tests/ 引用数,棘轮只许降
# （加考题后手改 scripts/check/spec-coverage-baseline.txt 下调基线=进度台账）
bash scripts/check/check-spec-coverage.sh || { echo "❌ 覆盖矩阵恶化"; exit 1; }

echo "=== [chain 13/13] cargo build + android check ==="
cargo build || { echo "❌ 构建不过"; exit 1; }
# android-only 代码在 host 被 cfg 门挡死编不着(na-loader 的 ndk-sys
# internalDataPath 字段名案,2026-08-26:host 全绿、手机才炸)——
# check 免链接,服务器直接判 android 目标
cargo check --target aarch64-linux-android --workspace || { echo "❌ android 目标 check 不过"; exit 1; }

echo "=== [chain] ✅ 全部通过 ==="

# 空间配额提示（MAIN0139 配额制落地，MAIN0140 白露出词照抄）——
# 只看不拦：非步骤非闸，「13 步」口径不变。三约束：|| true（set -e 下
# 不拦提交）／--dry-run（不塞话）／--auto（按 cwd 认人，全线共用不错判，
# 无关目录静默跳过——宁可漏查，不许误判）。
# 过滤按 MAIN0140 楼2 裁决：grep 保关键行前缀，不用 tail -N——行数是
# 变量会切错，行前缀是接口承诺（脚本方可依赖前缀演化，chain 不必跟着改）。
# 四前缀（MAIN0140 楼4 接口成文）：[配额]户级／[总盘]全盘／[跳过]谁没被
# 查到（漏查给假安全感，与「谁超了」同级）／「  → 」动作行。
echo "=== [chain 提示] 空间配额（只看不拦，MAIN0139） ==="
node /root/90-信箱/30-工具/check-空间配额.mjs --auto --dry-run 2>&1 \
  | grep -E '^\[配额\]|^\[总盘\]|^\[跳过\]|^  → ' || true
