# scripts/ 索引(2026-08-27 立)

> 20+ 个脚本平铺在此,按用途分五组。每个脚本头部自带用法注释,
> 本索引只答「我要干什么 → 拿哪件」。排障场景先读
> `docs/active/排障手册.md` 速查表。

## 纪律链(提交前)

- `chain.sh` — **唯一检查入口**(10 步)：防泄漏闸 → 零依赖闸 →
  **stats 字段咬合闸**(check/check-stats-format.sh,StatsSnap 加字段
  忘进 format! 不许过链，2026-08-27 评审建议落地）→ fmt → clippy →
  android-check → javac → test → overlay/kfm-pkg 考题 → build。
  pre-commit 自动跑。
- `check/` — chain 调用的单项检查（提交纪律闸门、stats 咬合闸等）。

## 构建与部署(改 APK 层时)

- `package-apk.sh` — 手工打包 APK(javac→d8→aapt2→zipalign→apksigner)。
- `build-on-phone.sh` — 手机编译回路:服务器推 master,手机本地编
  APK + 调安装器。
- `deploy-phone.sh` — 送包到手机并调起安装器(`--build` 先打包再送)。
- `font-bake.py` — 字体烘焙管线(子集化/借形/monoify)。

## 热更回路(只改核心 .so 时,日常主力)

- `na-push-so.sh [--no-restart]` — 推核心进沙箱 hot/ → 默认联动
  自动重启 → ping 判卷,全自动闭环;推前留档 `.so.last`(秒级回退)。
- `na-restart.sh` — 体面重启:restart-req → 等断连 → am start 拉回
  → 等新 boot → 判卷。

## 观测(看)——8024 闸门配套

- `na-ping.sh` — 事件循环死活四态(alive/stall/background/未起跳)。
- `na-stats.sh` — 运行时统计快照:帧耗/CPU/RSS/泵与闸门计数/
  分桶吞吐/会话死亡。
- `na-history.sh` — stats 水位环:最近 24 分钟每 30s 一张快照,
  一行一张(趋势类判卷尺,awk 取列即曲线)。
- `na-trace.sh [行数]` — 行踪环全量或末 N 行(事件流,带毫秒戳)。
- `na-text.sh` — 当前视野纯文本(读屏)。
- `na-shot.sh` — 当前帧拍图,落 /tmp/na-shot.png。
- `na-replay.sh` — 飞行记录仪拉回 host 确定性回放,末屏 diff 判卷。
- `na-autopsy.sh [备注]` — **一键收尸包**:触发落盘 + 八件档案拉回
  `autopsy/<时间戳>/` + 摘要。出异常先跑它。
- `na-case.sh BAR-xxx "现象"` — 开案脚手架:收尸 + bugs.md 案卷骨架
  + 复现脚本模板(结晶条款配套,见调试闸门.md §十一)。

## 注入(控)

- `na-type.sh 'cmd\r'` — 裸字节注入活跃会话 PTY(远程键盘;
  `\r`/`\x03` 等转义由 printf '%b' 翻成真字节)。
- `na-touch.sh 'scroll 3' [...]` — 触摸注入(通道八):tap/down/move/
  up/scroll/sleep 脚本化,与真手指同一入口;手势类 bug 的复现腿。
- `na-orb.sh 'tap' [...]` — AI 外显事件注入(通道十):tap/drag/run/end/
  dismiss,直调 AiPresenceState 状态核,落 orb-inject-res 回执;
  判卷配 stats 的 ai_* 字段族 + na-shot 实拍。

## 判卷(实证脚本)

- `na-regress.sh [名字...]` — **真机回归套件**(调试闸门.md §十四):
  cases/*-accept.sh 全跑或点名跑,一案/一钉一卷,exit 0 过 / 非 0 挂 /
  77 跳过;「重启类」自动排尾。热更后必跑(已挂进 na-push-so.sh ⑥)。
- `cases/BAR-040-accept.sh` — 首屏标题不得被顶出(重启类)。
- `cases/PIN-boot-accept.sh` — boot 段末行 <3000ms(启动族绊线)。
- `cases/PIN-pump-accept.sh` — 泵速率 <1000/s(57k 空转回潮闸)。
- `cases/PIN-touch-accept.sh` — scroll ±5 首行精确往返(通道八)。
- `cases/PIN-signal-accept.sh` — kill -URG 探针 SIGNAL 行 +1 且活着。
- `cases/PIN-standby-death-accept.sh` — ss -K 掐 ws:远程死亡记账+活跃不受扰+自愈重连。
- `cases/PIN-switch-accept.sh` — switch-req 切换往返 X→Y→X(通道九)。
- `cases/PIN-remote-active-death-accept.sh` — 活跃=远程死亡自动重孵(弹一次远程,宜空闲时跑)。
- `cases/PIN-rehatch-accept.sh` — 故障注入:exit 杀会话→自动重孵→回显(§十五)。
- `test-na-type-bytes.sh` — na-type 字节语义(假 ssh 判字节)。
- `test-na-regress-meta.sh` — 回归套件的套件:跳过语义/boot 解析/泵速率/重启排尾四元契约(假 ssh 桩,零编译秒级)。
- `check-spec-coverage.sh` — 考卷覆盖矩阵棘轮闸(调试闸门.md §十六):模块×考题对照表落 docs/ledger/test-coverage-matrix.md,未覆盖数只许降。
- `probe-overnight-power.sh` — 过夜/昼间电耗画像采集:双源(电池 termux-battery-status + na stats)对账,GAP 行=冻结窗口即数据。
- `test-bg-survival.sh` — BAR-029:遥控前后台 + 8024 探针判后台存活。
- `test-kfm-pkg.sh` — kfm-pkg 原子性三案(挂 chain 第 8 步)。
- `test-overlay.sh` / `test-serve-overlays.sh` — L2 overlay 考题。

## 运维(crond 自动)

- `na-nightly-quiesce.sh` — NA-QUIESCE 夜间熄灯(00:55):na 存活且后台才投
  restart-req 体面退出不复活;前台活跃/keep-alive 旗豁免。与 `na-restart.sh`
  的唯一区别=无 am start 拉回腿。判据链:电耗对照夜简报(wake lock 10x 定罪)。

## L2 overlay(本地 apt 生态)

- `build-overlay.sh` / `overlay-pack.sh` / `serve-overlays.sh` —
  在手机 Termux 里跑的打包/文件服务管线,设计见 docs/active/l2-overlay.md。
