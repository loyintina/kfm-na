#!/usr/bin/env bash
# package-apk.sh — KFM-NA 手工 APK 打包（2026-08-13 起替代 cargo apk build）
#
# 为什么脱离 cargo-apk：中文输入的 Java 皮（android/java/，MainActivity +
# InputConnection）cargo-apk 塞不进去——它只会生成裸 NativeActivity 的
# manifest，不带 Java 编译。本脚本用 SDK 自带工具手工走完全程，零 Gradle、
# 零网络：
#   cargo build（.so）→ javac → d8 → aapt2 link → zip 装 dex/lib
#   → zipalign → apksigner（debug.keystore，与旧包同证书可覆盖安装）
set -euo pipefail
cd "$(dirname "$0")/.."

# 工具解析（双环境）：服务器 = SDK 全套本地路径；手机 Termux = 系统包
# （cargo/javac/aapt2/zipalign/apksigner 在 PATH）+ 服务器拷来的
# d8.jar/android.jar（~/kfm-na-toolchain）。档位 2 手机自举（2026-08-15）
if [ -d /data/data/com.termux ]; then
    TOOLBOX="$HOME/kfm-na-toolchain"
    AJAR="$TOOLBOX/android.jar"
    JAVAC=javac
    D8="$TOOLBOX/bin/d8"
    AAPT2=aapt2
    ZIPALIGN=zipalign
    APKSIGNER=apksigner
    KEYSTORE="$HOME/.android/debug.keystore"
    # Termux 的 cc 原生就是 aarch64-linux-android clang，无需 NDK 交叉链
    LINKER=cc
else
    SDK=/root/kfm-na-toolchain/sdk
    BT="$SDK/build-tools/34.0.0"
    AJAR="$SDK/platforms/android-35/android.jar"
    JAVAC=/root/kfm-na-toolchain/jdk/bin/javac
    D8="$BT/d8"
    AAPT2="$BT/aapt2"
    ZIPALIGN="$BT/zipalign"
    APKSIGNER="$BT/apksigner"
    KEYSTORE=/root/.android/debug.keystore
    LINKER="$SDK/ndk/27.2.12479018/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang"
fi
TARGET=aarch64-linux-android
MIN_API=24
# targetSdk 定案（exec 探针 vc16777513 实拍）：35 → 28。targetSdk≥29 进
# untrusted_app 新域，SELinux 摘除私有目录 exec 权（errno 13 实锤）；
# ≤28 留旧域（Termux 同款姿态，其 uid 语境 untrusted_app_27 亲见），
# app_data_file exec 保留——L2 busybox / L3 apt 生态的总开关。
# 代价：安装时系统或提示「为旧版 Android 打造」；Android 14+ 安装下限是
# targetSdk<23，28 不受影响。行为变更按 targetSdk 门控的全部回落旧制
# （含 legacy 共享存储访问，白送）
TARGET_SDK=28
# versionCode 必须大于已装包才能覆盖安装——旧包是 cargo-apk 默认的 16777472。
# 红线：每次打包必须递增（2026-08-13 零日志闪退教训）——同 versionCode
# 覆盖安装可能不重解压 .so，设备上「新 dex + 旧 so」JNI 符号缺失即闪退。
# 2026-08-18 手工递增已失信一次（16777496 连打两包）——改计数器自动递增。
# 2026-08-21 计数器方案再失信：手机/服务器各自独立计数，双机都打包后
# 手机包 versionCode(16777497) 低于已装(16777519)，降级拒装——改 epoch 秒，
# 天然跨机单调；同秒连打/时钟回拨时取「上次+1」保底严格递增
LAST=$(cat build/version-code.current 2>/dev/null || echo 0)
NOW=$(date +%s)
VERSION_CODE=$(( NOW > LAST ? NOW : LAST + 1 ))
# deploy-phone.sh 从这里取已解析的值（别再从本脚本 grep 字面值——
# 计数器表达式 grep 出来是未展开的源码串，2026-08-18 实踩）
echo "$VERSION_CODE" > build/version-code.current
VERSION_NAME=0.1.0
BUILD=build/apk
OUT=target/release/apk/kfm-na.apk

# 构建戳编译进 Rust（BAR-013）：设备跑的 .so 是哪个构建，
# field-reports.log 首行一读便知
export KFM_NA_BUILD="$(git rev-parse --short HEAD 2>/dev/null || echo nogit)-$(date -u +%m%d%H%M)"
# versionCode 同进编译期（BAR-022）：field-reports 自报家门，
# 判「跑的是不是刚装的包」不再靠猜（16777504 那次实踩：旧包没装上，锚点全无）
export KFM_NA_VC="$VERSION_CODE"

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$LINKER"

echo "=== [package 1/6] cargo build --release ($TARGET) ==="
# 双库:kfm_na = 核心(可被热更替换),na_loader = 焊死的加载壳
# (manifest lib_name 指它,启动时 dlopen 热更/捆绑核心,见 crates/na-loader)
cargo build --release --target "$TARGET" -p kfm-na -p na-loader

echo "=== [package 2/6] javac（Java 皮） ==="
rm -rf "$BUILD"
mkdir -p "$BUILD/classes" "$BUILD/dex" "$BUILD/stage/lib/arm64-v8a" target/release/apk

# L3 bootstrap 资产(可选):找到就入包 assets/,找不到就裸包
# (app 侧 asset 缺失会优雅回落系统 sh——报告 [boot] L3 行)
BOOTSTRAP_ZIP="${KFM_BOOTSTRAP_ZIP:-}"
if [ -z "$BOOTSTRAP_ZIP" ]; then
    for cand in \
        /root/kfm-na-toolchain/termux-packages/output/bootstrap-aarch64.zip \
        /root/kfm-na-toolchain/termux-packages/bootstrap-aarch64.zip \
        "$HOME/kfm-na-toolchain/bootstrap-aarch64.zip"; do
        [ -f "$cand" ] && BOOTSTRAP_ZIP="$cand" && break
    done
fi
if [ -n "$BOOTSTRAP_ZIP" ]; then
    mkdir -p "$BUILD/stage/assets"
    cp "$BOOTSTRAP_ZIP" "$BUILD/stage/assets/bootstrap-aarch64.zip"
    echo "bootstrap 资产入包 ← $BOOTSTRAP_ZIP"
else
    echo "bootstrap 资产缺席——裸包(本地会话回落系统 sh)"
fi
# kfm-pkg 运行时安装器(L2 overlay,docs/active/l2-overlay.md):常驻资产,
# app 侧每启覆盖铺进 $PREFIX/bin,版本随 APK 滚动
mkdir -p "$BUILD/stage/assets"
cp android/assets/kfm-pkg "$BUILD/stage/assets/kfm-pkg"

$JAVAC -source 8 -target 8 -cp "$AJAR" -d "$BUILD/classes" \
    android/java/dev/kfm/na/*.java 2>&1 | grep -v 'bootstrap class path' || true
# javac 的告警（-source 8 过时）不挡路，编译失败才挡
[ "${PIPESTATUS[0]}" -eq 0 ] || { echo "❌ Java 皮编译不过"; exit 1; }

echo "=== [package 3/6] d8（class → dex） ==="
"$D8" --min-api "$MIN_API" --lib "$AJAR" --output "$BUILD/dex" \
    $(find "$BUILD/classes" -name '*.class')

echo "=== [package 4/6] aapt2 compile+link + 装 dex/lib ==="
# res 先 compile 成 .flat 打包，再 -R 喂给 link（图标等二进制资源进包的正路；
# 不编 R.java——Java 皮不引用资源，manifest 里 @mipmap 引用由 aapt2 解析）
"$AAPT2" compile --dir android/res -o "$BUILD/res.zip"
"$AAPT2" link -o "$BUILD/unsigned.apk" -I "$AJAR" \
    --manifest android/AndroidManifest.xml \
    -R "$BUILD/res.zip" \
    --min-sdk-version "$MIN_API" --target-sdk-version "$TARGET_SDK" \
    --version-code "$VERSION_CODE" --version-name "$VERSION_NAME"
cp "$BUILD/dex/classes.dex" "$BUILD/stage/"
cp "target/$TARGET/release/libna_loader.so" "$BUILD/stage/lib/arm64-v8a/"
cp "target/$TARGET/release/libkfm_na.so" "$BUILD/stage/lib/arm64-v8a/"
# BAR-013：.so 不压缩（STORED）+ 下方 zipalign -p 页对齐，配 manifest 的
# extractNativeLibs="false"——.so 直从 APK mmap 加载，与 dex 天然原子，
# 「重解压被跳过 → dex 新 so 旧」整条错配链连根拔掉
python3 - "$BUILD/stage" "$BUILD/unsigned.apk" <<'EOF'
import os, sys, zipfile
stage, apk = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(apk, "a") as z:
    for root, _, files in os.walk(stage):
        for f in files:
            p = os.path.join(root, f)
            arc = os.path.relpath(p, stage)
            ct = zipfile.ZIP_STORED if arc.endswith((".so", ".zip")) else zipfile.ZIP_DEFLATED
            z.write(p, arc, ct)
EOF

echo "=== [package 5/6] zipalign（-p 页对齐 .so） ==="
"$ZIPALIGN" -f -p 4 "$BUILD/unsigned.apk" "$BUILD/aligned.apk"

echo "=== [package 6/6] apksigner（debug.keystore） ==="
"$APKSIGNER" sign --ks "$KEYSTORE" --ks-pass pass:android \
    --out "$OUT" "$BUILD/aligned.apk"

ls -lh "$OUT"
echo "=== [package] ✅ $OUT ==="
