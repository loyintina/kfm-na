#!/usr/bin/env bash
# package-spike.sh — GPU 期 0 尖刺③（glow/GLES 直连）打包（gpu-render.md §四）
# ①同款管线：cargo build → aapt2 link → 装 lib（STORED + 页对齐）→
# zipalign → apksigner。无 Java 皮无 d8 无资源。
# 用法：
#   bash package-spike.sh           # 打包
#   bash package-spike.sh deploy    # 打包 + 调起系统安装器（最后一下用户点）
set -euo pipefail
cd "$(dirname "$0")"

# 工具解析（双环境，同主管线）
if [ -d /data/data/com.termux ]; then
    TOOLBOX="$HOME/kfm-na-toolchain"
    AJAR="$TOOLBOX/android.jar"
    AAPT2=aapt2
    ZIPALIGN=zipalign
    APKSIGNER=apksigner
    KEYSTORE="$HOME/.android/debug.keystore"
    TARGET_DIR=target/release            # 宿主即 aarch64-linux-android
    CARGO_TARGET_ARGS=""
else
    SDK=/root/kfm-na-toolchain/sdk
    BT="$SDK/build-tools/34.0.0"
    AJAR="$SDK/platforms/android-35/android.jar"
    AAPT2="$BT/aapt2"
    ZIPALIGN="$BT/zipalign"
    APKSIGNER="$BT/apksigner"
    KEYSTORE=/root/.android/debug.keystore
    NDK_BIN="$SDK/ndk/27.2.12479018/toolchains/llvm/prebuilt/linux-x86_64/bin"
    export CC_aarch64_linux_android="$NDK_BIN/aarch64-linux-android24-clang"
    export AR_aarch64_linux_android="$NDK_BIN/llvm-ar"
    export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$NDK_BIN/aarch64-linux-android24-clang"
    TARGET_DIR=target/aarch64-linux-android/release
    CARGO_TARGET_ARGS="--target aarch64-linux-android"
fi

# versionCode：epoch 秒天然跨机单调（主管线同款纪律，红线：必须递增）
mkdir -p build
LAST=$(cat build/version-code.current 2>/dev/null || echo 0)
NOW=$(date +%s)
VERSION_CODE=$(( NOW > LAST ? NOW : LAST + 1 ))
echo "$VERSION_CODE" > build/version-code.current

echo "=== [spike③ 1/4] cargo build（glow/GLES 直连） ==="
# shellcheck disable=SC2086
cargo build --release $CARGO_TARGET_ARGS

echo "=== [spike③ 2/4] aapt2 link + 装 lib（STORED） ==="
BUILD=target/spike-build
rm -rf "$BUILD"
mkdir -p "$BUILD/stage/lib/arm64-v8a"
"$AAPT2" link -o "$BUILD/unsigned.apk" -I "$AJAR" \
    --manifest AndroidManifest.xml \
    --min-sdk-version 24 --target-sdk-version 28 \
    --version-code "$VERSION_CODE" --version-name "spike-$VERSION_CODE"
cp "$TARGET_DIR/libgles_spike.so" "$BUILD/stage/lib/arm64-v8a/"
python3 - "$BUILD/stage" "$BUILD/unsigned.apk" <<'EOF'
import os, sys, zipfile
stage, apk = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(apk, "a") as z:
    for root, _, files in os.walk(stage):
        for f in files:
            p = os.path.join(root, f)
            arc = os.path.relpath(p, stage)
            ct = zipfile.ZIP_STORED if arc.endswith(".so") else zipfile.ZIP_DEFLATED
            z.write(p, arc, ct)
EOF

echo "=== [spike③ 3/4] zipalign（-p 页对齐 .so） + apksigner ==="
"$ZIPALIGN" -f -p 4 "$BUILD/unsigned.apk" "$BUILD/aligned.apk"
mkdir -p target/release/apk
OUT=target/release/apk/gles-spike.apk
"$APKSIGNER" sign --ks "$KEYSTORE" --ks-pass pass:android --out "$OUT" "$BUILD/aligned.apk"
ls -lh "$OUT"

if [ "${1:-}" = "deploy" ]; then
    echo "=== [spike③ 4/4] 送包 + 调起安装器 ==="
    NAME="gles-spike-$VERSION_CODE.apk"
    if [ -d /data/data/com.termux ]; then
        cp "$OUT" "/storage/emulated/0/$NAME"
        am start -a android.intent.action.VIEW \
            -d "file:///storage/emulated/0/$NAME" \
            -t application/vnd.android.package-archive
    else
        scp -P 8022 -o BatchMode=yes "$OUT" localhost:/data/data/com.termux/files/home/downloads/"$NAME"
        ssh -p 8022 -o BatchMode=yes localhost \
            "cp /data/data/com.termux/files/home/downloads/$NAME /storage/emulated/0/$NAME && \
             am start -a android.intent.action.VIEW -d file:///storage/emulated/0/$NAME \
             -t application/vnd.android.package-archive"
    fi
    echo "=== [spike③] ✅ 安装器已调起：手机上点「安装」，然后开「GLES尖刺」==="
    echo "    判卷：服务器 tail -f /root/kfm-na/field-reports.log | grep gles-spike"
fi
echo "=== [spike③] ✅ $OUT（vc=$VERSION_CODE） ==="
