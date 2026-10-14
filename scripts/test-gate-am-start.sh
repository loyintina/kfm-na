#!/bin/bash
# test-gate-am-start.sh — BAR-248 钉（追踪信 NA0177）：Termux 腿（8022）
# 拉起 app 前先身份核验。8022 是「会漂的编号」——neo6s Termux → 11 机 →
# 9 机三易其主（谁占谁应答），无核验的 am start 误拉 9 机旧核引爆互踢
# 风暴。修法 = 动作时身份核验（getprop 机型 ≠ 期待即拒），不是改常量。
#
# 桩法：PATH 前置假 ssh/adb——ssh 桩按 FIXTURE_MODEL 答 getprop（空 =
# 取不到/不通，exit 255）、见 am start 落 AM_START_MARK；adb 桩同落
# mark（adb 臂 -s 序列号即身份，不核）。
set -euo pipefail
cd "$(dirname "$0")/.."

T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
mkdir "$T/bin"

cat > "$T/bin/ssh" <<'EOF'
#!/bin/bash
# 假 ssh：每次被调落 SSH_MARK；末参数是远命令——getprop 答 FIXTURE_MODEL
#（空 = 取不到，exit 255 仿连接失败）；am start 落 AM_START_MARK。
touch "$SSH_MARK"
for last; do :; done
case $last in
    *getprop*)
        if [[ -n ${FIXTURE_MODEL:-} ]]; then echo "$FIXTURE_MODEL"; exit 0; fi
        exit 255 ;;
    *"am start"*) touch "$AM_START_MARK"; exit 0 ;;
esac
exit 0
EOF
cat > "$T/bin/adb" <<'EOF'
#!/bin/bash
# 假 adb：末参数（shell 子命令串）含 am start → 落 AM_START_MARK
for last; do :; done
case $last in
    *"am start"*) touch "$AM_START_MARK"; exit 0 ;;
esac
exit 0
EOF
chmod +x "$T/bin/ssh" "$T/bin/adb"

export PATH="$T/bin:$PATH"
export AM_START_MARK="$T/am_start.mark"
export SSH_MARK="$T/ssh.mark"

die() { echo "❌ BAR-248 钉：$1" >&2; exit 1; }

# ① 异机拒：桩答 9 机机型，默认期待 11 机 → 拒；无 mark、stderr 含 BAR-248
err=$(FIXTURE_MODEL=V2339FA bash -c '
    source scripts/lib/gate-lib.sh; gate_am_start' 2>&1) \
    && die "①异机未拒（竟放行）"
[[ ! -e $AM_START_MARK ]] || die "①异机拒却仍发 am start"
[[ $err == *BAR-248* ]] || die "①拒辞不含 BAR-248：$err"
echo "  ①异机拒 ✓"

# ② 同机放：桩答 11 机机型（默认期待值）→ 放行
FIXTURE_MODEL=V2573A bash -c '
    source scripts/lib/gate-lib.sh; gate_am_start' || die "②同机被误拒"
[[ -e $AM_START_MARK ]] || die "②同机放行但 am start 未发"
rm -f "$AM_START_MARK"
echo "  ②同机放 ✓"

# ③ NA_TERMUX_MODEL 覆盖放：判卷机显式指定 9 机 → 放行
FIXTURE_MODEL=V2339FA NA_TERMUX_MODEL=V2339FA bash -c '
    source scripts/lib/gate-lib.sh; gate_am_start' || die "③覆盖后被误拒"
[[ -e $AM_START_MARK ]] || die "③覆盖放行但 am start 未发"
rm -f "$AM_START_MARK"
echo "  ③NA_TERMUX_MODEL 覆盖放 ✓"

# ④ 取不到机型拒：口不通/无 getprop（FIXTURE_MODEL 空）→ 拒
err=$(FIXTURE_MODEL= bash -c '
    source scripts/lib/gate-lib.sh; gate_am_start' 2>&1) \
    && die "④取不到机型竟放行"
[[ ! -e $AM_START_MARK ]] || die "④拒却仍发 am start"
[[ $err == *BAR-248* ]] || die "④拒辞不含 BAR-248：$err"
echo "  ④取不到机型拒 ✓"

# ⑤ adb 臂直发不核：即使 ssh 侧会拒（异机），adb 臂（-s 序列号即身份）
# 不经过 ssh → 放行，且 ssh 全程未被调用（先清前四钉的 ssh 调用痕）
rm -f "$SSH_MARK"
FIXTURE_MODEL=V2339FA NA_TRANSPORT=adb NA_ADB="$T/bin/adb" bash -c '
    source scripts/lib/gate-lib.sh; gate_am_start' || die "⑤adb 臂被误拒"
[[ -e $AM_START_MARK ]] || die "⑤adb 臂 am start 未发"
[[ ! -e $SSH_MARK ]] || die "⑤adb 臂竟走了 ssh"
rm -f "$AM_START_MARK" "$SSH_MARK"
echo "  ⑤adb 臂直发不核 ✓"

# ⑥ 源码锚：gate_am_start 与 na-back.sh 不再有裸「ssh -p 8022」，
# 都收口到 gate_termux_am（防回退再开一条无核验的拉起路）
if grep -n 'ssh -p 8022' scripts/lib/gate-lib.sh scripts/na-back.sh \
    | grep -v '^[0-9]*: *#' | grep .; then
    die "⑥仍有裸 ssh -p 8022 拉起路"
fi
grep -q 'gate_termux_am' scripts/na-back.sh \
    || die "⑥na-back.sh 未走 gate_termux_am"
grep -q 'gate_termux_am "-n dev.kfm.na/.MainActivity"' scripts/lib/gate-lib.sh \
    || die "⑥gate_am_start 未走 gate_termux_am"
echo "  ⑥源码锚（裸 8022 清零＋两调用点收口）✓"

echo "✅ BAR-248 钉全绿（六钉）"
