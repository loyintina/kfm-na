# KFM-NA 交接页（state.md)

> 纪律：**每个里程碑必更新此页**。接手者冷启动顺序：本页 → bugs.md →
> AGENTS.md → chain.sh 跑一遍。本页只写「现在进行时」，历史功过在 bugs.md。

## 当前位置（2026-08-26)

- **自观测第二块：行踪环 + 运行时统计（2026-08-26，本提交）**:
  ①trace ring(src/trace.rs)——report 流本地滚动副本，咽喉单点
  tap(report/report_sync/... 三函数旁路入环,调用点零改动),环帽
  256,两类周期心跳 should_trace 滤掉;活着 trace-req→trace.txt
  随查(na-trace.sh),panic 钩子自动落末 64 行进 panic-trace.txt
  (覆写制)。②stats-req 通道——帧/泵/闸门计数全 AtomicU64 热路径
  加一,stats-req→stats-res 快照随查(na-stats.sh),key=value 钉死。
  SessionRouter 顺手补 names()。考题 tests/trace_spec.rs 4 道全绿。
  全文 docs/active/调试闸门.md §九。
- **热更新闭环收官：自动重启通道 + BAR-037 重跑防御（2026-08-26，
  本提交）**：①gate 第五条通道 restart-req——值守线程见触发文件即
  遗言直报 + `exit(0)`，不经过事件循环（挂起态也杀得死）；②
  BAR-037 防御——android_main 极早期静态 ANDROID_MAIN_RAN 检测，
  二次进门遗言 + `exit(0)` 让位（ROM 冻结打断 exit(0) 的窗口期，
  拉回即重跑的 panic 病根拔除）；③scripts/na-restart.sh 一键闭环：
  触发→等 8024 断连→am start 拉回（冻结案补第二次）→等新 boot
  报告→na-ping 判卷；na-push-so.sh 默认联动自动重启（--no-restart
  可关）。边界诚实写进脚本注释：熄屏/锁屏 am start 可能被系统挡，
  那时提示手动点图标。**装机实证过（cee7cf5，2026-08-26）**：热更推
  核心 6MB → force-stop+拉回（旧核心无 restart-req，最后一趟手动）
  → 新核心上位后再跑 na-restart.sh 全闭环：restart-req 触发 →
  8024 断连 → am start 拉回 → 新 boot 行 → ping 应答，四件套全对，
  panic.log 无新行（BAR-037 旧案两行即全部）。
- **飞行记录仪落地（2026-08-25，本提交）**：自观测路线图第一块——
  确定性回放。泵新增 rec 见证回调（Output 先过记录仪再路由，全部
  会话带名记录，不只活跃方）;resize 在 apply_window_size 落点旁
  tap 一条（网格共享，回放时全应用）。格式 `KFMREC01\n` + 定长头
  变长体记录流（kind 1=Output/2=Resize，未知 kind 按 len 跳过、
  截尾容忍）,rec_encode/rec_decode_all/rec_compact 三纯函数出题
  （tests/rec_spec.rs 5 道，含超帽保新丢旧）。记录仪线程独立落盘
  `$PREFIX/tmp/flight-rec.bin`（帽 2MB，入队即返回、静默丢——观测
  通道不许反咬业务）。host 回放器 `src/bin/na-replay.rs`：按名过滤
  喂 TermView 真渲染，打印末屏文本+统计。一键 `scripts/na-replay.sh`
  = scp 拉回 + 本地回放。判卷法：回放末屏须与 na-text.sh 读屏一致。
  **终判卷过（2026-08-25 装机实拍，逐行 diff 完全一致）**；判卷过程
  自抓三虫：BAR-033（开局横幅绕泵未入带）、BAR-034（记录带跨重启
  追加→开机轮换 .prev）、BAR-035（回放器起手几何与真机不同胚→
  共享常量 BOOT_COLS×BOOT_ROWS)。设计全文 docs/active/调试闸门.md §六。
- **死亡观测落地（2026-08-25，本提交）**：自观测第二块——「它是怎么死
  的」。①panic 落盘：install_panic_hook 三件套（闸门目录 panic.log
  追加为主、report 直报为辅、链默认钩子），行格式钉死一行一案，
  线程 panic 也收（替换旧的「仅异步直报」版——进程死了队列同归于尽
  收不到）。②loop 看门狗：about_to_wait 每圈盖心跳戳（忙轮询泵），
  值守线程比龄期，>3s 卡死/复活迁移写 loop-stall.log + report;
  ping-req→ping-res 随查（scripts/na-ping.sh）。纯被动，不用 proxy
  （挂起态送不达是实锤过的弯路）。考题 gate_spec 3 道（卡死边界/
  panic 行格式/多行压单行）。全文 docs/active/调试闸门.md §八。
- **热更新：loader/核心分离（2026-08-26，本提交）**：manifest lib_name
  改指焊死的加载壳 libna_loader.so（新 crate crates/na-loader，只做
  选择+转发），核心 libkfm_na.so 可被 {files}/hot/ 下的热更件替换——
  改码不再重打 APK 过安装器：na-push-so.sh 推进沙箱 → 重启即生效。
  回落纪律：热更缺失/dlopen 失败自动回落包内捆绑核心，每次选择落档
  loader-pick（跑的是谁必须可查）。考题 tests/loader_spec.rs 3 道。
  这是插件宿主胚胎：壳↔核心只隔 dlopen+固定入口符号。全文
  docs/active/热更新.md。自动重启通道（restart-req）与 BAR-037
  重跑防御押后做。**装机实证过（d7f9bab,2026-08-26）**：首启
  loader-pick = `pick=bundled why=无热更核心`；na-push-so.sh 推核心
  6MB 入 hot/，划掉重开 = `pick=hot`，读屏/看门狗功能全正常。
- **L2 自远程闸门通车（2026-08-23,等长二进制改写方案）**:服务器
  `ssh -p 8024 localhost`(探针钥匙 /root/.ssh/na_probe_key)经 kalo 反隧
  直入 na 沙箱 sshd(回环only/公钥only)——agent 从此能直接操作 na 终端,
  不再靠用户手动测试转述。根治术:`com.termux` 与 `dev.kfm.na` 同 10 字符,
  sed 等长直打 ELF 不挪偏移,sshd 20+ 处焊死路径一遍全愈;已挪进
  overlay-pack.sh 构建侧(0b85924,考题钉等长铁律)。证伪留档:
  LD_PRELOAD(bionic 不让插 libc 符号)、proot(seccomp 沙箱里 ptrace
  失效)、SetEnv LD_LIBRARY_PATH(活不到 exec)——三条都别再试。
  na 主动外传道 = kfm-push(scp 推 Termux ~/w/na-inbox/,na 写不进共享
  存储根 EPERM 实拍)。全文:l2-overlay.md §8/§9;简报已投信箱。
- **BAR-029 前台服务保活封案(2026-08-23,bbcb5f5)**:na 退后台被
  cached-app 冻结器冻僵(sshd 冬眠、8024 断流)→ KfmKeepAliveService
  前台服务(IMPORTANCE_MIN 常驻通知 + partial wake lock,Termux 同款)。
  实拍判卷:修复前后台 10s 必冻 → 修复后 600s 六十探零断流。
  判卷脚本 scripts/test-bg-survival.sh(am start 遥控前后台 + 8024 探针,
  全自动)。**此条推翻 8-19「不做常驻保活」旧共识**——8024 闸门与会话
  现场需要 na 退后台存活,电耗代价用户已拍板接受。
- **zsh/omz + powerline 字体战役(2026-08-23,BAR-027/028/032)**:
  omz 落私有区(FUSE 家目录不支持符号链接),.bashrc 换手行进 zsh;
  agnoster 箭头三案连破——补丁表扩容+借字形(f58433b)→ 全角压半格
  (3d71293)→ 根因定案:FusionPixel 上游 E0B0 是「色块+C形镂空」装饰
  设计(双光栅器复现),烘焙时换合成实心阶梯三角(4cb652e,像素级考题
  spec_bar032)。实拍判卷过。
- **挂单**:BAR-030(~~长行不换行+粘贴撕碎~~ 已销案 2026-08-24:用户
  实拍不复现,四层检查全正常,疑为 zsh 残局期产物)、BAR-031(已封案
  ea5ddec:kfm-pkg 原子性三件套,装机实拍过)。
- **画面回传 na-shot(2026-08-24,本提交)**:8024 闸门配套调试通道——
  na 画面是 Rust 软渲染,帧缓冲本来就在自己手里,倒出来就是截图(不需
  Android 截屏权限)。`touch $PREFIX/tmp/shot-req` → 渲染循环下一帧倒
  shot.rgb+shot.dim → scp 拉回 PIL 转 PNG。一键 scripts/na-shot.sh
  (--watch 循环=近同步直播)。软键盘/系统弹窗不在帧缓冲里,拍不到
  (预期内)。考题 tests/screendump_spec.rs 5 道。
  **后台倒帧(同日二轮)**:第一版想靠 EventLoopProxy 锤醒 about_to_wait
  离屏倒帧,实拍证伪——挂起态下 proxy 事件送达但 winit 不跑
  about_to_wait(循环心跳停跳、触发文件晾着);proxy 只在循环活着时
  叫得醒,blackout 案的锤有效是因为那时循环本身在跑。正解:倒帧全
  收进独立值守线程(spawn_dump_watcher,300ms 轮询触发文件),终端改
  Arc<Mutex<Box<dyn TermEmu>>> 共享,值守线程锁终端离屏光栅化(只画
  网格本体,快捷键行/放大镜是 UI 装帧不进后台视野);draw_frame 每帧
  note_frame_size 记账供后台取尺寸。单消费者,前台后台一个样。
- **调试闸门三件套闭环(2026-08-24 午后,本提交)**:screendump.rs 更名
  gate.rs,从「画面回传」扩为完整调试闸门——看见(shot)+读懂(text)+
  动手(keys)三条文件协议通道,值守线程一轮三查。新增:①text-req →
  dump_text 导当前视野纯文本(TermEmu 新 trait 方法,网格眼睛胚胎,
  滚动跟视野走);②keys-in → 裸字节注入活跃会话(远程键盘,手的胚胎,
  退后台也能注),SessionRouter 随之 Arc 化共享(15 处调用点改
  handle+lock,装配收拢 install_router 一次登记永新鲜)。限制(有意):
  Ctrl-] 会话切换/修饰键粘滞是 UI 层,闸门不过。脚本:na-text.sh /
  na-type.sh(半写防护 = .new+mv 原子协议)。设计参考面:
  docs/active/调试闸门.md。考题:gate_spec 6 道 + termview dump_text
  题。用途:此后 na 调试 agent 自助闭环,不再要用户当手和眼。
- **keys-in 零执行破案(2026-08-24,5260c1e)**:装机后注入无效——
  回执上报(send_checked,eb798cd)先证 drain/登记/send/writer 四环
  全活,读屏见四条命令带字面 `\r` 堆在提示符上:病在 na-type.sh
  `printf '%s'` 把转义当字面发。改 `%b` 后端到端实证(echo 落盘真
  执行)。教训入档:**「通道活了」≠「字节对了」,判案判到最后一厘米**。
  钉:scripts/test-na-type-bytes.sh(假 ssh 注 PATH 判字节)。
- **会话泵:Output 数据面分家(2026-08-24,本提交)**:旧制 Output 走
  SessionEvent 进 UI 事件队列,挂起态不抽干 → 网格冻结,闸门读屏读
  旧画面。新制 `gate::SessionPump` 是全部会话入向唯一消费者:活跃方
  Output 谁 pump 谁喂共享终端(UI 每圈 + 值守 300ms 双 caller,前台
  零延迟后台不冻结);待机方进 replay 缓存(帽 256KB 丢最旧,切换时
  take_replay 补屏);控制事件按名带进控制队列,壳每圈取走记健康账
  (语义不变)。壳的 event_rx/standby/standby_buf 三字段全删,切换不
  再换入向槽,重连 = pump_register 同名登记(自清遗物)。考题
  tests/session_pump_spec.rs 7 道。锁序追加 pump→term 单方向。
- **L2 判卷通过（2026-08-23 实拍）**:`kfm-pkg install base` 全绿,
  `ssh -V`=OpenSSH_10.5p1、`git --version`=2.55.0、`ssh root@服务器`
  登录成功。途中三案:①na 公网出站"不通"破案=境外链路掐 DSCP
  标记包(IPQoS=none 即通;80 口超时是安全组丢包的假线索);
  ②ssh 的 `~` 系路径焊死 com.termux 家目录 → `$HOME/.ssh/config` +
  alias shim(登记 l2-overlay.md §6);③身份 = 复用 Termux moliy_key
  (用户拍板),8027 传递后落 $PREFIX 私有区,副本已删。
  另记:na 终端粘贴长命令会被折行撕碎(待办:粘贴保真)。
- **ANSI 蓝系换品牌蓝（本提交）**:VGA #0000AA/#5555FF 纯黑底不可读
  (ssh 远端 ls 目录名看不清)→ idx4/12 换 #3B82F6/#60A5FA,考题钉死。
- **L2 命令生态定案(实拍修正版):overlay 管线,apt 只做依赖解析器**。
  盘点 bootstrap:coreutils 是多路复用 `bin/coreutils` + SYMLINKS.txt
  136 条链接,grep/sed/gawk/find/tar/curl/top/ps 全齐;真缺口 = ssh/git/
  vim/make。先判「apt 直通」→ 实拍证伪:`apt update` 能成(只拉清单),
  `apt install` 全灭——deb 把 com.termux 前缀三处焊死(data.tar 路径、
  maintainer 脚本、编译期 prefix),dpkg instdir=/ 解包敲别家院门
  EACCES。路线改为三段(设计 docs/active/l2-overlay.md):手机真 Termux
  跑 `scripts/build-overlay.sh`(apt --print-uris 空 status 骗闭包 →
  curl → overlay-pack.sh 剥前缀/改脚本/收链接) → 交接点**手机回环
  HTTP**(实拍修正:na 读共享存储根 EACCES,scoped storage 不吃
  legacy 牌;Termux 侧 serve-overlays.sh 起 127.0.0.1:8027) →
  na 终端 `kfm-pkg install <名>`(shell,assets 每启铺进 $PREFIX/bin)。
  考题 = scripts/test-overlay.sh(fixture 假 deb,chain 第 8 步)。
  交接点服务已并入手机 `kalo` v3.4(随隧道同生同灭,status 可见)。
- **探针脚手架拆除（本提交）**：启动战役归因探针全拆——termview 字体分段
  计时、init_terminal 五段计时、帧#N 三段探针、FIRST_OUTPUT/RTT 探针、
  user_event/new_events 测绘、ATW_N 测绘、resumed 三行计时、唤醒锤成败
  上报、首笔 Redraw 时刻探针。**保留**：构建戳（dex/so 错配防线）、心跳、
  [death]/[panic] 通道、exec 探针本体（后台线程，L2 要它的判词）、
  L3 first_boot 与插件装载失败路径的 report_sync（错误路径不在热循环）、
  唤醒锤 + blackout 补画机制（冗余兜底）、tofu 目击上报（产品级遥测）、
  on_slot_dead 改异步 report（断线瞬间冻 UI 同案犯一并拔）。纪律首例：
  「探针完工要拆」。
- **启动战役 230ms 收官（6449537)**：终版实拍——启动完成 +179ms /
  帧#1 +181ms / 首 output（提示符） +195ms / 补画上屏 +211ms。
  真凶 = 8-13 埋的主线程 report_sync 探针阻塞 HTTP;「系统扣 Redraw
  2.5s」是假象（FIRST_REDRAW_SEEN 注释已写明真实案情）。三刀：
  9c6ba54(exec 后台化）、7cd06bf（首帧快路）、6449537(sync 探针拔刺）。
  用户判卷：秒进、提示符跟画面一起出。通报已投信箱
  kfm-na-startup-230ms-report.md(kfmv4 仓）。
- **壳层交互三轮（3a3a882)**：①拖柄视觉 kfmv4 化——选中底色改品牌正蓝
  `SELECT_BG` #3B82F6（原借用快捷键行私色 0x3E6FB4）；拖柄改水滴/图钉
  （圆头直径 ≈0.7 格宽、柄体连选中条边缘、整柄一整格高），kfmv4「黑边+
  亮色辉光」像素版：近黑 #0A0C10 描边先画大一圈、青 #06B6D4 主体叠上；
  放大镜边框同青色系。②宽字符边界钳制：拖动端点落 CJK spacer 半格按
  方向钳（右 col+1/左 col-1，选词/扩选/拖柄三入口同尺），选词落 spacer
  当归格 0、词尾宽字符带 spacer 收尾；渲染高亮整字扩边不劈字；提取
  一致性用探针 `set_selection_raw` 钉死。**固有取舍（判卷点）：右拖终点
  落 spacer 会包进后一格**（非空白时多带一字）。规格页已同步。考题 +8
  （select_spec 26 道），变异抽检 3 发全抓住（摘描边/摘扩边/摘钳制）。
- **壳层交互二轮（5f43a52)**：实拍三问题修复——①选中态中文只剩左半
  （病灶：单遍渲染 spacer 格高亮盖掉宽字符右半墨 → render_into 改
  两遍制：先全背景后全字形）②选择难操作 → 拖柄（端点 ±1 格宽容命中、
  拖过另一端角色互换）+ 放大镜（触点上方浮窗，帧缓冲源区最近邻 2 倍，
  格心对齐）③⇄(U+21C4) 单格 CJK 字形溢出 → 单格路径格宽裁剪（双宽
  路径不变）。考题 +6，变异抽检 4 发全抓住。
- **壳层交互三件套落地（f4ca09a）**：①默认字号 15x30 → 18x36（用户两次
  抱怨「太小」）；②双指捏合缩放（touch.id 双指跟踪，pinch_cell_size
  钳制纯函数，files/kfm-zoom 持久化 + `[zoom]` 上报，顶带动态化
  margin_top 跟格高走）；③长按选择 + 复制（500ms 计时走 RedrawRequested
  忙轮询泵，词选择/扩选/网格坐标选区/SELECT_BG 高亮，JNI 剪贴板 +
  Toast 薄壳 src/clipboard.rs）。**行为规格页 = docs/active/壳层交互.md**
  （状态机定义 + 坐标换算约定，交接面）。考题 16 道新（select_spec 13 +
  termview_spec 缩放 3），变异抽检过（摘 clamp/恒定顶带均红）。
- **阶段 2(Android 终端可用化）收尾完成**。启动慢归因定案
  (BAR-020~023 连环，见 bugs.md)；BAR-023 提示行实拍判卷后取消
  (c73bf61),2.1s 首连唤醒的结构性答案移交 L1。
- **L1 本地 PTY 已落地（实拍过首轮）**:conn-provider-local 插件 + 双会话槽
  （本地秒开为默认活跃，ws 远程后台接为待机，Ctrl-] 切换）；考题 4 道
  全绿（echo 往返/resize 传播/退出事件/双工厂并存）。关键技术债记档：
  多线程 fork fd 继承竞态 → FD_CLOEXEC + FORK_LOCK 串行化。
- **多端分层评审裁决到达并已落地（f64528e)**：五问全裁总体批准。
  裁决 4 附议考题「切换后输入路由」落地为 `src/session_router.rs`
  （纯路由核，零 IO 零平台依赖）+ 考题 4 道（默认只进活跃/切换翻面/
  无待机无操作/待机槽拒覆盖）；AGENTS.md 增分层纪律三节；chain.sh
  增核心层零依赖闸（chain 2/8,`cargo tree -p cordis-na` 断言）;
  规格书 §9 修订记录 v1.5（kfmv4 仓）。
- **exec 探针两轮实拍定案：targetSdk 28 放行 ✅(exit=42),L3 复活**。
  package-apk.sh 已 TARGET_SDK=28(6d6c144)；副作用窗口被压已修
  （BAR-024,543a239,decorFitsSystemWindows(false)+SHORT_EDGES,
  16777515 实拍满屏正常）。
- **偏差认领（已写进评审信讨论区）**：裁决 1 批 portable-pty，实际用
  nix(bionic 无 openpty,nix 走 posix_openpt，已实证）。对账口径：
  nix 先用，desktop spike 点亮（裁决 2 拆分触发点）时再评 portable-pty。
- **方向共识（2026-08-19 与用户定）**: 不重写 Termux(termux-app 是 GPL-3.0
  一行不能抄；终端仿真我们已有 alacritty_terminal)。kfmv4 功能搬家
  （光球/卡片堆）后移到核心层分层落定之后。~~不为本地 shell 做常驻保活~~
  **已翻案（2026-08-23，用户拍板）**:BAR-029 前台服务保活已落地——
  8024 闸门/会话现场需要 na 退后台存活，电耗代价接受。
- **L3 已闭环（2026-08-21 实拍过 ✅)**:bootstrap-aarch64.zip 32M
  (83 包运行时闭包,剪枝自 222 个 deb)入包,真机解压+second-stage
  成功,本地会话进 bash 生态。fork 链 12 条坑全部记档在
  kfmv4 experiments/dsh-na/na/l3-bootstrap.md §3(含 docker cp 权限
  陷阱、overlay 活体手术禁令、output 剪枝纪律)。ss 类 netlink 诊断
  是安卓内核铁墙(非包问题);curl/git/tmux 等靠 apt 装(keyring
  是官方 key 复刻,官方源/镜像直通)。

## 待判卷（实拍未回）

| 项 | 包 | 判卷标准 |
| --- | --- | --- |
| ~~BAR-023 提示行~~ | kfm-na-16777509.apk | **已判卷（2026-08-20 用户实拍）**：字号正常，但观感尴尬，拍板取消——提示行已整体切除，连接前移+握手遥测保留（见 bugs.md BAR-023） |
| ~~exec 探针第一轮~~ | kfm-na-16777513.apk | **已判：拒绝 ❌ errno=13**——targetSdk 35 进 untrusted_app 新域，私有目录 exec 被封（理论证实） |
| ~~exec 探针第二轮：targetSdk 28~~ | kfm-na-16777514.apk | **已判：放行 ✅ exit=42**——域降级生效，L3 复活（白送 legacy 共享存储访问） |
| ~~BAR-024 窗口被压~~ | kfm-na-16777515.apk | **已判卷：满屏回归正常**（targetSdk 28 副作用已修） |
| L1 双会话 + SessionRouter | kfm-na-16777510.apk 起 | ①本地提示符秒出（首轮已过：+118ms `:/ $ `）②启动落家 `/storage/emulated/0/Android/data/dev.kfm.na/files` ③`ls`/`echo` 正常 ④~~Ctrl-] 切换实拍未回~~ **已实拍（2026-08-21）：切换本身正常，但切到已死远程会话按键无响应**——WS 退后台被掐+服务器 killAll 杀 PTY+无重连，三件套凑齐。修复 = 断线重连（本提交） |
| 壳层四轮（本提交）：拖柄废除改边界直拖 + 断线重连 | 本次包 | ①**不再画任何拖柄**（水滴废除，选区只剩正蓝高亮）②按住选区端点格 ±1 宽容直拖：跨行跟手、拖过另一端角色互换、放大镜还在 ③选区中段按下不抓边界（仍是扩选/复制语义）④**断线重连**：远程死后切过去 → 横幅「已重连 = 新 shell」→ 提示符出现 → `tmux attach` 接回旧现场 ⑤活跃远程死了自动重连一次；再死敲任意键触发 ⑥本地 shell 里 `exit` → 原地复活新 shell ⑦重连中途敲的键不丢（进新 shell 缓存） |
| 壳层三件套（本提交） | 本次包 | ①默认字号 18x36 观感 ②双指捏合：字号实时变、列数跟着变、松手后 `[zoom]` 持久化行 ③杀进程冷启动：字号保持 ④长按键词 500ms 高亮整词 ⑤拖动扩选跨行 ⑥单击任意处：Toast「已复制 N 字符」+ 粘贴他处验证 ⑦keybar 行触摸行为不变 ⑧捏合后顶带仍是一整行（首行不被圆角吃） |
| 壳层二轮（5f43a52）：选中态中文/拖柄/放大镜/⇄ | kfm-na-16777518.apk | ①**选中态中文完整**：高亮下 CJK 两格都有墨、两格都盖高亮色 ②拖柄：定型后两端可拖、跨行跟手、拖过另一端角色互换不塌缩 ③放大镜：拖柄拖动时触点上方浮窗、2 倍放大、格心对齐、不挡手 ④**⇄（U+21C4）不再溢出下一格**（如 lazgit/kimicode 等 TUI 的分隔符）⑤单击非拖柄区仍是复制+清选 |
| 壳层三轮（3a3a882）：拖柄 kfmv4 视觉 + 中文边界钳制 | kfm-na-1787295088.apk | 用户判卷（2026-08-21）：**拖柄观感否决——「还是好丑」，拍板废除**（四轮改边界直拖）；中文边界钳制保留 |

## 欠账

- ~~双指缩放调字号~~（2026-08-21 已落地：18x36 基准 + 捏合 + 持久化 +
  长按选择复制，规格 docs/active/壳层交互.md，待实拍判卷）
- ~~阶段 2 落地通报~~(2026-08-21 已投:
  /root/kfmv4/docs/ledger/agent-inbox/kfm-na-cordis-rs-stage2-landing.md,
  落地 4558f33 对账——30 题全绿/三插件线程排查无风险面/实拍回归过)
- ~~L3 路线规划~~(2026-08-21 已闭环，见「当前位置」L3 条与
  l3-bootstrap.md;apt 自建源换 keyring 路线留档 l3-bootstrap.md §6,
  真需要时再启)

## 构建流程（2026-08-21 用户定案，勿翻）

**服务器出题判卷，手机编包安装。** 分工：

- 服务器（/root/kfm-na):代码事实来源；pre-commit chain 8 步全绿才算数，
  commit-msg 双门照常。任何代码先进这里。
- 手机（Termux,/data/data/com.termux/files/home/kfm-na)：只拉绿了的
  master → 本地编 APK → 本地调安装器。不当判官、不提交代码。
- 一键入口:`bash scripts/build-on-phone.sh`(push master → 手机跑
  deploy-phone.sh --build，其本地模式直接调安装器)。
- 为什么:APK 带 bootstrap 资产后 37M,scp 回传每趟太贵;源码 diff 走
  ssh 隧道秒级;编译负载挪出服务器(多 agent 共线会卡,2026-08-21 实踩
  孤儿链死锁 + 并发踩踏)。
- 一次性铺设(已做):phone remote(ssh://localhost:8022/...)+ 手机侧
  receive.denyCurrentBranch=updateInstead + bootstrap 资产已同步到
  手机 ~/kfm-na-toolchain/bootstrap-aarch64.zip。**bootstrap 重编后要
  重同步这个 zip**,否则手机编出的是裸包(壳会回落系统 sh)。
- **商业字体同理**:assets/fonts/local/ 永不进库 → git push 到手机时
  不会带,手机编出的包回落开源占位字体(2026-08-21 实拍「字体不对」
  的成因)。已 scp 同步 main.ttf(md5 两边一致);**local/ 字体换了要
  重同步手机仓**,否则手机包字体跟服务器包不一致。
- versionCode 用 epoch 秒（package-apk.sh,2026-08-21 改）——跨机天然
  单调，双机都打包也不会降级拒装；同秒连打/时钟回拨取「上次+1」保底。
  （旧方案「双机各自独立计数」已失信废除：手机 16777497 < 已装 16777519
  被拒装，实踩。）
- **第三备份 = GitHub（2026-08-26 接通，同日公開化）**:remote `github` →
  https://github.com/loyintina/kfm-na.git(**public**，跨机传输用），凭据走
  ~/.git-credentials 的 loyintina PAT（credential.helper=store 已配）。
  三份拷贝：服务器（事实来源）/ 手机 / GitHub。提交后顺手
  `git push github master`。公开化前已过泄密扫描（无私钥/token/口令；
  商业字体有 chain.sh 第 1 步防泄漏闸 + gitignore 双保险）。注意：docs
  里有服务器 IP 与端口布局，属用户知情同意的公开面（当晚实表已脱敏）。
- **隧道生命线：kalo v3.13（2026-08-26 晚，手机 ~/bin/kalo，备份
  kalo.bak-v3.12）**：断线根因定案 = 手机 CGNAT 公网 IP 漂移（auth.log
  实证 7 分钟内 39.144.207.45→.203.214→.203.47 三换），漂移即黑洞、
  服务器留僵尸监听挡新 -R 绑定；autossh 指数退避曾滚到 128s+ 拖慢
  自愈。v3.13 对症：GATETIME=0 关退避 + POLL=15s 匀速重试、探死
  15×2=30s、`-E ~/kalo-ssh.log` 记 ssh 死因（autossh 日志只记 exit
  255）、隧道不通且 autossh 活着时主动重启它、判卷放缓 10×3s。
  实证：完整拆建循环（kalo -x && kalo）秒级恢复。看门狗 = crond
  每 5 分钟 kalo-watchdog.sh，断线最坏恢复时长 ≈ 5 分钟（通常 <1 分钟）。

## 日志判读手册（field-reports.log，踩坑攒的）

- 时间戳是**服务器收货时刻**，冲洗节拍量化导致乱序；消息文本里的
  `+Xms`(boot_ms，距 android_main）才是真锚点。
- `grep` 中文要用 `grep -a`（二进制误判）;`strings` 抓不到中文。
- 应用被划掉后，冲洗队列里未发出的行随进程全丢——队列当诊断载体不可靠。
- `[?]` 行 = 自己的 curl 探针（`-d "{}"` 无 stage 字段），不是 bug。
- 手机侧：`ssh -p 8022 localhost`(Termux);dumpsys/netlink 无权限。

## 提交纪律

- **2026-08-20 用户授权：na 线主力 agent 可自行提交**（此前规矩是「其他人
  提交」，已作废）。阶段 2 全部工作已入库：`4558f33`。
- 提交即过双门：pre-commit 重跑 chain；commit-msg 跑文档耦合门 + fix 带钉门
  (scripts/check/check-*.sh,hard fail)。
