// exec 探针载荷(L2/L3 总开关验证):打印即放行。
// 重建: NDK clang 直编,见 scripts/package-apk.sh 同款 toolchain:
//   $SDK/ndk/27.2.12479018/toolchains/llvm/prebuilt/linux-x86_64/bin/\
//     aarch64-linux-android24-clang -O2 -o hello-aarch64 hello.c
#include <stdio.h>
int main(void) {
    puts("KFM-EXEC-PROBE-OK");
    return 42;
}
