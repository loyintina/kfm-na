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

echo "=== [chain 1/7] 字体防泄漏闸（BAR-021） ==="
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

echo "=== [chain 2/7] cargo fmt --check ==="
# 2026-08-17 workspace 化（crates/cordis-na)：带根包的 workspace 里裸 cargo
# fmt/clippy/test 只覆盖根包——不加 --all/--workspace 会让 crate 考题静默脱链
cargo fmt --all --check || { echo "❌ fmt 不过：跑 cargo fmt --all 后重试"; exit 1; }

echo "=== [chain 3/7] cargo clippy ==="
cargo clippy --workspace --all-targets -- -D warnings || { echo "❌ clippy 不过"; exit 1; }

echo "=== [chain 4/7] cargo check --target aarch64-linux-android ==="
# Android 代码 cfg 在宿主不可见（fmt/clippy/test 都跳过它）——不查就会烂在盲区
cargo check --target aarch64-linux-android || { echo "❌ Android 目标编译不过"; exit 1; }

echo "=== [chain 5/7] javac（Java 皮编译检查） ==="
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

echo "=== [chain 6/7] cargo test ==="
cargo test --workspace || { echo "❌ 测试不过"; exit 1; }

echo "=== [chain 7/7] cargo build ==="
cargo build || { echo "❌ 构建不过"; exit 1; }

echo "=== [chain] ✅ 全部通过 ==="
