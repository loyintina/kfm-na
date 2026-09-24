#!/bin/bash
# na-push-so.sh — 热更新推送(2026-08-26,配套 crates/na-loader)
#
#   bash scripts/na-push-so.sh [--no-restart] [本地.so路径]
#     缺省:从手机仓 ~/kfm-na/target 拿刚编的 release 核心
#     --no-restart:只推不重启(手动划掉重开生效)
#
# 链路:核心 .so → na 沙箱 {files}/hot/libkfm_na.so(先 .new 再 mv 原子
# 防半读,同 keys-in 协议;推前留档 .so.last=秒级回退)→ na-restart.sh
# 自动体面重启 → na-loader
# dlopen 热更核心。判卷:闸门目录 loader-pick 应有 pick=hot 行 +
# boot 报告的构建戳对得上 + na-ping alive。
set -euo pipefail

NA_HOT=/data/data/dev.kfm.na/files/hot

# shellcheck source=scripts/lib/na-ssh.sh
source "$(dirname "$0")/lib/na-ssh.sh"

na() { na_ssh "$1"; }

NO_RESTART=0
SRC=""
for a in "$@"; do
    case "$a" in
        --no-restart) NO_RESTART=1 ;;
        *) SRC="$a" ;;
    esac
done

if [ -n "$SRC" ]; then
    [ -f "$SRC" ] || { echo "❌ 找不到 $SRC" >&2; exit 66; }
    LOCAL_TMP="$SRC"
    SO_TS=""
else
    # 从手机仓拉刚编的核(Termux 私有目录,经 8022 读)
    LOCAL_TMP=/tmp/libkfm_na-hot.so
    # 远端 mtime 先拿下(BAR-060：拉下来的副本 mtime=下载时刻,拿它
    # 比 HEAD 恒新,哨兵形同虚设——09-04 凌晨因此把 9-2 旧核推上机,
    # 期 0③ 被静默降级,靠人肉读 boot 构建戳抓回)
    # BAR-073(2026-09-09)：双候选路径自适应——手机原生编核落
    # target/release/,交叉链落 target/aarch64-linux-android/release/;
    # 写死交叉路径会把残留旧核当事实源(哨兵比对/拉取同源同错)。
    # 取较新者,哨兵与拉取用同一份挑选结果。
    SO_PICK=$(ssh -p 8022 -o BatchMode=yes -o ConnectTimeout=8 localhost \
        'a=$HOME/kfm-na/target/release/libkfm_na.so
         b=$HOME/kfm-na/target/aarch64-linux-android/release/libkfm_na.so
         s=$a
         if [ -f "$b" ] && { [ ! -f "$a" ] || [ "$b" -nt "$a" ]; }; then s=$b; fi
         if [ -f "$s" ]; then stat -c"%Y %n" "$s"; fi' 2>/dev/null || true)
    SO_TS="${SO_PICK%% *}"
    SO_PATH="${SO_PICK#* }"
    [ -z "$SO_TS" ] && SO_TS=0
    [ -z "$SO_PATH" ] || [ "$SO_PATH" = "$SO_TS" ] && {
        echo "❌ 手机仓两条候选路径都没有 libkfm_na.so——先编核" >&2; exit 66; }
    echo "远端核: $SO_PATH ($(date -d "@$SO_TS" '+%m-%d %H:%M'))"
    ssh -p 8022 -o BatchMode=yes -o ConnectTimeout=8 localhost \
        "cat $SO_PATH" > "$LOCAL_TMP"
fi

SIZE=$(stat -c%s "$LOCAL_TMP")
# 陈核哨兵（2026-09-03 二连踩：默认从手机仓拉的 .so 是旧编核、管道
# 掩码让失败构建照样推 stale——两次都靠 boot 构建戳人肉抓回；
# BAR-060 补：默认路径必须比远端 mtime,本地副本 mtime 是下载时刻）。
# .so 比 HEAD 还旧 = 推了白推,当场吼;确认就是要推旧核用 ALLOW_STALE=1
if [ "${ALLOW_STALE:-0}" != 1 ]; then
    HEAD_TS=$(git log -1 --format=%ct 2>/dev/null || echo 0)
    [ -z "$SO_TS" ] && SO_TS=$(stat -c%Y "$LOCAL_TMP")
    if [ "$SO_TS" -lt "$HEAD_TS" ]; then
        echo "❌ 陈核拒推：.so ($(date -d "@$SO_TS" '+%m-%d %H:%M')) 比 HEAD ($(date -d "@$HEAD_TS" '+%m-%d %H:%M')) 还旧——先编核再推；确认推旧核用 ALLOW_STALE=1" >&2
        exit 65
    fi
fi
echo "=== 推送核心 ($SIZE 字节) → hot/ ==="
na "mkdir -p $NA_HOT"
# 原子防半写三道（BAR-150）：①.new 先落地；②**md5 对账过了才许 mv**——
# 旧版 `cat > .new && mv` 的 && 链是假原子：ssh 中途断流（冻结/切网）
# 远端 cat 收到 EOF 照样退出 0，半截核直接落位（2026-09-24 实踩：
# 2MB 半截核落位，靠 .last 回滚）；③推前留档 .so.last = 秒级回退
LMD5=$(md5sum "$LOCAL_TMP" | awk '{print $1}')
na_ssh "cat > $NA_HOT/libkfm_na.so.new" < "$LOCAL_TMP"
RMD5=$(na "md5sum $NA_HOT/libkfm_na.so.new 2>/dev/null | awk '{print \$1}'" || true)
if [ "$RMD5" != "$LMD5" ]; then
    echo "❌ 传输对账失败——半截核不许落位（local=$LMD5 remote=${RMD5:-读不到}）；.new 留远端待续传/诊断，现行核未动" >&2
    exit 65
fi
na "{ [ -f $NA_HOT/libkfm_na.so ] && cp $NA_HOT/libkfm_na.so $NA_HOT/libkfm_na.so.last; mv $NA_HOT/libkfm_na.so.new $NA_HOT/libkfm_na.so; }"
na "ls -la $NA_HOT/"
if [ "$NO_RESTART" = 1 ]; then
    echo "✅ 热更核心已就位(--no-restart:不重启,手动划掉重开生效)"
else
    echo "✅ 热更核心已就位,自动重启生效中——"
    bash "$(dirname "$0")/na-restart.sh"
    # ⑥ 冒烟回归(调试闸门.md §十四):热更刚重启过,SKIP_RESTART 直接判
    # 当前 boot。挂了不拦热更(核心已就位),但报表必须看——挂 = 这次
    # 热更可能带回了已销案的病
    # 名单无 BAR-040(2026-09-19 起):它的前提是「首屏=local shell 横幅」,
    # 默认启动已改 attach kfm-na tmux(用户拍板),首屏永远没横幅=必挂
    echo "=== ⑥ 冒烟回归(挂了不拦热更,但要看) ==="
    SKIP_RESTART=1 bash "$(dirname "$0")/na-regress.sh" \
        PIN-boot PIN-signal \
        || echo "⚠️ 冒烟有挂卷——对照上面报表查案卷" >&2
fi
