#!/bin/bash
# na-nightly-quiesce.sh — NA-QUIESCE 夜间熄灯公约(2026-08-30 用户拍板电耗选项A)
#
#   crond: 55 0 * * * bash /root/kfm-na/scripts/na-nightly-quiesce.sh >> /var/log/na-quiesce.log 2>&1
#   干跑:  NA_QUIESCE_DRY=1 bash scripts/na-nightly-quiesce.sh
#
# 判据链:docs/active/电耗对照夜简报-2026-08-30.md——na 后台常驻唤醒
# =8.3%/h,缺席=0.84%/h(~10x),罪在 BAR-029 wake lock 整夜挡 Doze。
#
# 动作:00:55 查 na 状态——存活且在后台 → touch restart-req 体面退出
# (gate.rs 通道五:记遗言 exit(0),不复活,晨间用户点图标即用);
# 前台活跃不动;已死无事。**没有 am start 拉回腿**——这是它与
# na-restart.sh 的唯一区别。
#
# 豁免:na 终端里 touch $NA_TMP/keep-alive(usr/tmp/keep-alive)保当夜,
# 用完自己删;重启/清 tmp 自然失效。
set -u

SSHOPTS="-p 8024 -i /root/.ssh/na_probe_key -o BatchMode=yes -o ConnectTimeout=6 -o StrictHostKeyChecking=no"
NA_TMP=/data/data/dev.kfm.na/files/usr/tmp
DRY=${NA_QUIESCE_DRY:-0}

if ssh $SSHOPTS localhost "test -f $NA_TMP/keep-alive" 2>/dev/null; then
    echo "$(date '+%F %T') [quiesce] keep-alive 旗在,豁免一晚"
    exit 0
fi

FG=$(bash "$(dirname "$0")/na-stats.sh" 2>/dev/null | grep -oE '^foreground=(true|false)')
case "$FG" in
    *false*)
        if [ "$DRY" = "1" ]; then
            echo "$(date '+%F %T') [quiesce][dry] na 在后台,将触发 restart-req"
            exit 0
        fi
        if ssh $SSHOPTS localhost "touch $NA_TMP/restart-req" 2>/dev/null; then
            echo "$(date '+%F %T') [quiesce] na 在后台 → restart-req 已投,体面退出"
        else
            echo "$(date '+%F %T') [quiesce] restart-req 投递失败"
        fi
        ;;
    *true*)
        echo "$(date '+%F %T') [quiesce] na 在前台活跃,不动"
        ;;
    *)
        echo "$(date '+%F %T') [quiesce] na 不可达(已死/未起),无事"
        ;;
esac
