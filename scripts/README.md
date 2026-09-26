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

## 隧道韧性(跨隧道动作统一入口,2026-09-23 改版)

- `lib/na-ssh.sh` — 手机闸门传输唯一入口（na-tunnel 2026-09 退役，
  BAR-117 看门狗接管隧道本体）：na_ssh/na_scp_pull/na_scp_push，
  **9022 首选**（na 自持隧道反连，na 活着口就在）**/8024 备援**
  （Termux 代维旧路）。8022 = Termux 专用（am start/手机仓/
  工具链），一般闸门活不许碰。安全红线:只碰 127.0.0.1 隧道端口,
  不新增任何公网暴露面(2026-09-01 用户定案,第一层公网直开永不采用)。

## 构建与部署(改 APK 层时)

- `package-apk.sh` — 手工打包 APK(javac→d8→aapt2→zipalign→apksigner)。
  `WITH_X86=1` 出 arm64+x86_64 胖包(redroid 云安卓用;不带开关=日常
  手机包,体积不变)。
- `redroid-up.sh` — 云安卓(redroid)一键起场(幂等:binder 内核件 →
  容器 → adb connect → 等 boot → adbd root → ④.6 llkd 宿主恐慌引信
  拆除 → ④.5 报表接力保活)。④.6 由来:2026-09-16 nz 线 12:45 内核
  恐慌根因通报——容器内 llkd 经 sysrq Panic 宿主内核,stop/kill 无效
  (disabled 服务死亡即重生),引信拆除 = sysrq-trigger 绑空文件。
  起完后闸门走 adb 直读沙箱(无 sshd/无隧道,协议不变);判卷平台差异
  见 state.md redroid 条。
- `redroid-report-relay.py` + `redroid-loop-relay.sh` — redroid 报表
  通道接力(2026-09-12:adb reverse 数据面死亡的兜底,容器 8021 →
  网桥 172.18.0.1:8021 → kfmv4 8021;由 redroid-up.sh ④.5 自动保活,
  一般不用手跑;病灶与拓扑见两脚本头注释)。
- `build-on-phone.sh` — 手机编译回路:服务器推 master,手机本地编
  APK + 调安装器。
- `deploy-phone.sh` — 送包到手机并调起安装器(`--build` 先打包再送)。
- `deploy-via-na.sh` — 自持安装通道（2026-09-26）：QUIC 反连桥推包进 na
  私有目录 incoming/ → **投闸门**（通道十五 install-apk-req）→ na 值守线程
  JNI 甩 MainActivity（UI 线程）→ FileProvider content:// 一次性授权调起
  安装器，零 Termux/存储权限依赖（首次使用前提：在跑的 na 已含
  KfmFileProvider）。**2026-09-26 改道**：原先第二步走桥 shell 的 am start
  被 vivo 按进程态判 BAL 静默吞（连浏览器 VIEW 都不弹、exit=0 无输出），
  安装意图只能由前台 Activity 发起——BAR-162。
- `na-install-apk.sh [APK]` — **na 自更新原语**（BAR-162，2026-09-26）：
  推包进 incoming/ + 投同一道闸门，读回 usr/tmp/install-status 判决。
  通用口（deploy-via-na.sh 的同源同法）——agent 给自己/工具链推包走这条。
  现行装机 APK 没有 Java 新方法时自动走引导腿（门线程直调
  activity.startActivity），故**不装包也能用**。
- `deploy-ai-config.sh` — 三路 key 配置(Kimi 默认/智谱/DeepSeek 官网)
  抽自服务器 kfmv4,经闸门(na-ssh.sh:9022 首选/8024 备援)推 na 私有目录 ai/(key 不进 git)。
- `font-bake.py` — 字体烘焙管线(子集化/借形/monoify)。

## 热更回路(只改核心 .so 时,日常主力)

- `na-push-so.sh [--no-restart] [本地.so路径]` — 推核心进沙箱 hot/ → 默认联动
  自动重启 → ping 判卷,全自动闭环;推前留档 `.so.last`(秒级回退)。
  缺省从手机仓拉刚编的核;陈核哨兵:.so 比 HEAD 旧当场拒推
  (ALLOW_STALE=1 豁免)——2026-09-03 二连踩(手机仓旧核/管道掩码
  推 stale)后的机械闸;BAR-060 补:默认路径比的是远端 .so mtime
  (本地副本 mtime=下载时刻,比了白比),显式路径比本地文件 mtime。
- `na-restart.sh` — 体面重启:restart-req → 等断连 → am start 拉回
  → 等新 boot → 判卷。

## 观测(看)——闸门配套

**传输层开关（2026-09-11 redroid 接线）**：以下 na-*.sh 全部经
`lib/gate-lib.sh` 单源传输——默认走 `lib/na-ssh.sh` 统一入口
（2026-09-23 起 **9022 首选/8024 备援**：9022 = na 自持隧道反连，
na 看门狗自营，Termux 冻不冻都不看脸色；8024 = 旧反连路）。
`NA_TRANSPORT=adb` 时走云安卓 adbd root 直读沙箱（串口
`NA_ADB_SERIAL` 默认 localhost:5555），文件触发协议不变。
整条回归套件上云安卓：`NA_TRANSPORT=adb bash scripts/na-regress.sh`。
依赖活会话的卷用 `need_alive`/`need_any_alive` 前置探针
（stats 的 local_dead/remote_dead 字段是事实源），平台不适用
自动跳过不挂卷。云安卓特判两处：na-shot 走 CPU 倒帧路（GL 回读
翻转）、na-restart 死活探针看 pidof（adbd 常连无断连语义）。
**8022 = Termux 专用路**（am start/手机仓 ~/kfm-na/工具链/
termux-battery-status），只有 Termux 的 rootfs 干得了的活才用，
一般闸门活不许碰它——Termux 空闲冻结时 8022 会消失（实测抖动）。

- `na-front.sh` / `na-back.sh` — 前台拉起/退回后台并确认（2026-09-11
  用户拍板工作流：agent 自拉前台自测，退回后台 = 完成信号。熄屏时
  vivo 限制拉不起会报红；回后台必须走 launcher intent——KEYCODE_HOME
  会被 NA 当终端按键吃掉）。
- `na-ping.sh` — 事件循环死活四态(alive/stall/background/未起跳)。
- `na-stats.sh` — 运行时统计快照:帧耗/CPU/RSS/泵与闸门计数/
  分桶吞吐/会话死亡。
- `na-history.sh` — stats 水位环:最近 24 分钟每 30s 一张快照,
  一行一张(趋势类判卷尺,awk 取列即曲线)。
- `na-trace.sh [行数]` — 行踪环全量或末 N 行(事件流,带毫秒戳)。
- `na-text.sh` — 当前视野纯文本(读屏)。
- `na-shot.sh` — 当前帧拍图,落 /tmp/na-shot.png。
- `orb-on-text-measure.py <图> <cx> <cy> <R>` — 光球文字穿透三区指标
  (球内/球晕/球外 笔画 p90 与底 p10;D8 加法合成校准尺,判据见脚本头)。
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
- `na-anim-cap.sh` — 点播下一轮动画的渲染源采样(BAR-076 起点播制):
  投 anim-cap-req,动画开表消费,采样帧走 [anim-strip] 报表,
  服务器 anim-strip-png.py 拼 PNG。不点播 = 动画零 readPixels 开销。
  **BAR-101 警告(2026-09-16 实锤)：点播武装的轮次整轮被 glReadPixels
  停顿压成 2-3 帧——anim-strip 只判像素内容对错,它的帧数/帧耗时
  一律不作性能证据;判动画性能用无点播的 panel-anim/panc。**
- `na-panend-cap.sh` — BAR-104 贴死交接差分机一键入口:点播后触发一次
  Upper 平移(点下池行),自动抓「平移全序列帧 panend-aNN + 首帧稳态 b」
  (BAR-105 复判 2026-09-17 从末帧单帧升级为全序列——只留末帧说不清
  仪器逐帧看见什么),拉回逐帧对 b 差分(均值/热区/diff PNG)。判读:
  末帧 a≈b=交接无缝;差大且平移解释不了=贴死闪变病灶。帧数/耗时
  同样不作性能证据(BAR-101 纪律)。
- `na-rec.sh` — 真机软件内录(MediaProjection,rec.mp4):帧级真相的正路,
  每次 app 重启后需用户在授权弹窗点「立即开始」一次。**注意:内录有效
  帧率仅 ~14fps(2026-09-17 实测隔帧重复),逐帧判连续性请用系统录屏。**
- `redroid-anim-watch.sh page|upper|custom 'cmds'` — 云安卓动画帧级监控
  (2026-09-16):screenrecord 整段录虚拟屏 + imageio 拆全帧到
  /tmp/redroid-anim/<case>/。两个坑已钉进脚本:screenrecord 必须重定向
  脱离 tty(--time-limit 兜底,否则 adb shell 阻塞到自然结束);redroid
  虚报 15Hz 刷新率,有效帧率个位数,中间帧稀少是平台本色不是 bug。

## 判卷(实证脚本)

- `na-anim-bench.sh` — 池区动画帧数一键判卷(两轮:Page 切标签/Upper 点
  下池,各给 panc 帧数+明细+panel-anim 仪表;`NA_TRANSPORT=adb` 上云安卓)。
  **判卷口径是水位线之后的新行**(2026-09-16 仪器病实锤:trace 是 256 帽
  内存环,`rm trace.txt` 不清环,不过滤会把多次平移的累积旧行当帧数)。
  两个配套口径:①注入后等 3.5s 再落盘——touch 脚本由 app 内执行器
  消费,na-touch 返回 ≠ 执行完;②Upper 用例行命中以输出里 gest
  「下池点按: 聚焦行 N」回执为准,N≠0 才有效(同行再点不挂账,panc=0
  是正确行为不是仪器坏)。前提:设置页停在「系统管理」默认标签。

- `na-regress.sh [名字...]` — **真机回归套件**(调试闸门.md §十四):
  cases/*-accept.sh 全跑或点名跑,一案/一钉一卷,exit 0 过 / 非 0 挂 /
  77 跳过;「重启类」自动排尾。热更后必跑(已挂进 na-push-so.sh ⑥)。
- `cases/BAR-040-accept.sh` — 首屏标题不得被顶出(重启类)。
- `cases/PIN-boot-accept.sh` — boot 段末行 <3000ms(启动族绊线)。
- `cases/PIN-pump-accept.sh` — 泵速率 <1000/s(57k 空转回潮闸)。
- `cases/PIN-touch-accept.sh` — scroll ±5 首行精确往返(通道八)。
- `cases/PIN-signal-accept.sh` — kill -URG 探针 SIGNAL 行 +1 且活着。
- `cases/PIN-standby-death-accept.sh` — ss -K 掐 ws:远程死亡记账+活跃不受扰+自愈重连。
- `test-quic-migration.sh` — QUIC M2 迁移考题（服务器本地，root+netns+
  iptables+conntrack）：nsA 客户端经 NAT 出网，中途清 conntrack+snat 换源
  模拟运营商掐映射——判卷：QUIC echo 全回还零重连 + TCP 反例必死。
  载体 crates/na-quic/examples/quic_echo.rs（`cargo build -p na-quic
  --example quic_echo` 先编）。不进 chain 必修闸（CI 无 root/netns）。
- `cases/PIN-switch-accept.sh` — switch-req 切换往返 X→Y→X(通道九)。
- `cases/PIN-remote-active-death-accept.sh` — 活跃=远程死亡自动重孵(弹一次远程,宜空闲时跑)。
- `cases/PIN-rehatch-accept.sh` — 故障注入:exit 杀会话→自动重孵→回显(§十五)。
- `test-na-type-bytes.sh` — na-type 字节语义(假 ssh 判字节)。
- `test-na-regress-meta.sh` — 回归套件的套件:跳过语义/boot 解析/泵速率/重启排尾四元契约(假 ssh 桩,零编译秒级)。
- `check-spec-coverage.sh` — 考卷覆盖矩阵棘轮闸(调试闸门.md §十六):模块×考题对照表落 docs/ledger/test-coverage-matrix.md,未覆盖数只许降。
- `probe-overnight-power.sh` — 过夜/昼间电耗画像采集:双源(电池 termux-battery-status + na stats)对账,GAP 行=冻结窗口即数据。
- `test-bg-survival.sh` — BAR-029:遥控前后台 + 闸门探针判后台存活。
- `test-kfm-pkg.sh` — kfm-pkg 原子性三案(挂 chain 第 8 步)。
- `test-overlay.sh` / `test-serve-overlays.sh` — L2 overlay 考题。
- `test-relay-timeout.sh`(+`.py` 行为核) — BAR-109:relay 不得掐静默长连接(4s 死亡线命案;行为级判卷,dummy 上游静默 8s,越线一问一答。挂 chain 第 9 步)。

## 运维(crond 自动)

- `na-nightly-quiesce.sh` — NA-QUIESCE 夜间熄灯(00:55):na 存活且后台才投
  restart-req 体面退出不复活;前台活跃/keep-alive 旗豁免。与 `na-restart.sh`
  的唯一区别=无 am start 拉回腿。判据链:电耗对照夜简报(wake lock 10x 定罪)。

## L2 overlay(本地 apt 生态)

- `build-overlay.sh` / `overlay-pack.sh` / `serve-overlays.sh` —
  在手机 Termux 里跑的打包/文件服务管线,设计见 docs/active/l2-overlay.md。
