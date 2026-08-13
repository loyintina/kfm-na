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

SDK=/root/kfm-na-toolchain/sdk
BT="$SDK/build-tools/34.0.0"
AJAR="$SDK/platforms/android-35/android.jar"
JDK=/root/kfm-na-toolchain/jdk
NDK="$SDK/ndk/27.2.12479018"
KEYSTORE=/root/.android/debug.keystore
TARGET=aarch64-linux-android
MIN_API=24
# versionCode 必须大于已装包才能覆盖安装——旧包是 cargo-apk 默认的 16777472。
# 红线：每次打包必须递增（2026-08-13 零日志闪退教训）——同 versionCode
# 覆盖安装可能不重解压 .so，设备上「新 dex + 旧 so」JNI 符号缺失即闪退
VERSION_CODE=16777474
VERSION_NAME=0.1.0
BUILD=build/apk
OUT=target/release/apk/kfm-na.apk

export PATH="$JDK/bin:$PATH"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android${MIN_API}-clang"

echo "=== [package 1/6] cargo build --release ($TARGET) ==="
cargo build --release --target "$TARGET"

echo "=== [package 2/6] javac（Java 皮） ==="
rm -rf "$BUILD"
mkdir -p "$BUILD/classes" "$BUILD/dex" "$BUILD/stage/lib/arm64-v8a" target/release/apk
javac -source 8 -target 8 -cp "$AJAR" -d "$BUILD/classes" \
    android/java/dev/kfm/na/*.java 2>&1 | grep -v 'bootstrap class path' || true
# javac 的告警（-source 8 过时）不挡路，编译失败才挡
[ "${PIPESTATUS[0]}" -eq 0 ] || { echo "❌ Java 皮编译不过"; exit 1; }

echo "=== [package 3/6] d8（class → dex） ==="
"$BT/d8" --min-api "$MIN_API" --lib "$AJAR" --output "$BUILD/dex" \
    $(find "$BUILD/classes" -name '*.class')

echo "=== [package 4/6] aapt2 link + 装 dex/lib ==="
"$BT/aapt2" link -o "$BUILD/unsigned.apk" -I "$AJAR" \
    --manifest android/AndroidManifest.xml \
    --min-sdk-version "$MIN_API" --target-sdk-version 35 \
    --version-code "$VERSION_CODE" --version-name "$VERSION_NAME"
cp "$BUILD/dex/classes.dex" "$BUILD/stage/"
cp "target/$TARGET/release/libkfm_na.so" "$BUILD/stage/lib/arm64-v8a/"
python3 - "$BUILD/stage" "$BUILD/unsigned.apk" <<'EOF'
import os, sys, zipfile
stage, apk = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(apk, "a", zipfile.ZIP_DEFLATED) as z:
    for root, _, files in os.walk(stage):
        for f in files:
            p = os.path.join(root, f)
            z.write(p, os.path.relpath(p, stage))
EOF

echo "=== [package 5/6] zipalign ==="
"$BT/zipalign" -f 4 "$BUILD/unsigned.apk" "$BUILD/aligned.apk"

echo "=== [package 6/6] apksigner（debug.keystore） ==="
"$BT/apksigner" sign --ks "$KEYSTORE" --ks-pass pass:android \
    --out "$OUT" "$BUILD/aligned.apk"

ls -lh "$OUT"
echo "=== [package] ✅ $OUT ==="
