# KFM-NA 交接页（state.md)

> 纪律：**每个里程碑必更新此页**。接手者冷启动顺序：本页 → bugs.md →
> AGENTS.md → chain.sh 跑一遍。本页只写「现在进行时」，历史功过在 bugs.md。

## 当前位置（2026-08-23)

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
  （光球/卡片堆）后移到核心层分层落定之后。不为本地 shell 做常驻保活
  （wake lock 烧电；会话永生由服务器端 tmux 扛）。
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
