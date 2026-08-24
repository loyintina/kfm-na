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
    if [ -x /root/kfmv4/scripts/build-enter-cgroup.sh ] && [ -w /sys/fs/cgroup/agent.slice ]; then
        KFM_CHAIN_NICED=1 exec bash /root/kfmv4/scripts/build-enter-cgroup.sh nice -n 10 ionice -c2 -n7 bash "$0" "$@"
    elif command -v ionice >/dev/null 2>&1; then
        KFM_CHAIN_NICED=1 exec nice -n 10 ionice -c2 -n7 bash "$0" "$@"
    else
        KFM_CHAIN_NICED=1 exec nice -n 10 bash "$0" "$@"
    fi
fi

echo "=== [chain 1/9] 字体防泄漏闸（BAR-021） ==="
# 商业字体（assets/fonts/local/）永不进库：gitignore 是第一道，这道闸是
# 第二道机械执法——误 git add -A 也漏不出去。同时卡住超大字体资产
# （占位字体子集化后应 <4MB，超了就是忘了烘焙）
if git ls-files assets/fonts | grep -qi 'local/\|AaHMKJXST'; then
    echo "❌ 商业字体混入暂存区：git rm --cached 后再试"; exit 1
fi
big=$(git ls-files assets/fonts | while read -r f; do
    [ -f "$f" ] && [ "$(stat -c%s "$f")" -gt 4194304 ] && echo "$f"
done)
[ -z "$big" ] || { echo "❌ 字体资产超 4MB（未子集化？）: $big"; exit 1; }

echo "=== [chain 2/9] 核心层零依赖闸（多端分层纪律 1，评审裁决 5） ==="
# cordis-na = 多端核心层基座：零依赖是公开承诺（crates/cordis-na/Cargo.toml
# 注释钉死）。多一行依赖 = 核心/壳边界破洞——先讨论改闸，不许偷渡
core_deps=$(cargo tree -p cordis-na --depth 1 --prefix none | tail -n +2 | wc -l)
[ "$core_deps" = "0" ] || { echo "❌ cordis-na 染指依赖（$core_deps 个）：核心层必须零依赖"; exit 1; }

echo "=== [chain 3/9] cargo fmt --check ==="
# 2026-08-17 workspace 化（crates/cordis-na)：带根包的 workspace 里裸 cargo
# fmt/clippy/test 只覆盖根包——不加 --all/--workspace 会让 crate 考题静默脱链
cargo fmt --all --check || { echo "❌ fmt 不过：跑 cargo fmt --all 后重试"; exit 1; }

echo "=== [chain 4/9] cargo clippy ==="
cargo clippy --workspace --all-targets -- -D warnings || { echo "❌ clippy 不过"; exit 1; }

echo "=== [chain 5/9] cargo check --target aarch64-linux-android ==="
# Android 代码 cfg 在宿主不可见（fmt/clippy/test 都跳过它）——不查就会烂在盲区
cargo check --target aarch64-linux-android || { echo "❌ Android 目标编译不过"; exit 1; }

echo "=== [chain 6/9] javac（Java 皮编译检查） ==="
# Java 皮（android/java/）是中文输入的命脉，又不在 cargo 视野内——编译检查
# 防「改了 Java 没打过包」的烂尾。APK 全量打包走 scripts/package-apk.sh
# 双环境：服务器用本地 JDK+SDK；手机 Termux 用 openjdk-21 + 拷来的 android.jar
if [ -d /data/data/com.termux ]; then
    JAVAC=javac
    AJAR="$HOME/kfm-na-toolchain/android.jar"
else
    JAVAC=/root/kfm-na-toolchain/jdk/bin/javac
    AJAR=/root/kfm-na-toolchain/sdk/platforms/android-35/android.jar
fi
rm -rf build/java-check && mkdir -p build/java-check
"$JAVAC" -source 8 -target 8 \
    -cp "$AJAR" \
    -d build/java-check android/java/dev/kfm/na/*.java 2>&1 \
    | grep -v 'bootstrap class path' || true
# javac 的告警（-source 8 过时）不挡路，编译失败才挡
[ "${PIPESTATUS[0]}" -eq 0 ] || { echo "❌ Java 皮编译不过"; exit 1; }
rm -rf build/java-check

echo "=== [chain 7/9] cargo test ==="
cargo test --workspace || { echo "❌ 测试不过"; exit 1; }

echo "=== [chain 8/9] overlay 打包核考题（L2,fixture 假 deb) ==="
# 2026-08-22 第 8 步：overlay-pack 是纯 shell 变换，cargo 看不见——
# fixture 考题钉死剥前缀/改写/建链三规则（设计 docs/active/l2-overlay.md)
bash scripts/test-overlay.sh || { echo "❌ overlay 考题不过"; exit 1; }
# 2026-08-24 同步挂入：kfm-pkg 原子性考题（BAR-031——中断标记/重装自愈/
# 装后校验，zsh 卡死案病根）
bash scripts/test-kfm-pkg.sh || { echo "❌ kfm-pkg 考题不过"; exit 1; }

echo "=== [chain 9/9] cargo build ==="
cargo build || { echo "❌ 构建不过"; exit 1; }

echo "=== [chain] ✅ 全部通过 ==="
