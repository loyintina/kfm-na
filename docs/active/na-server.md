# na-server 设计（na 自持服务端）

> 状态：**设计定稿待实现**（2026-09-20 用户立项拍板）。
> 一句话：把 na 远程终端的会话层从 kfmv4 进程手里拿回来，na 自己养一个
> 只属于 na 的服务端；同一份代码二期跑上手机，双端同构。

## 〇、由来（为什么做）

na 对 8021（kfmv4 后端）的实证用量 ≈ 全面的 5%：WS 四类消息
（terminal-open/input/resize/close）+ `POST /kfmv4/api/na-report`，
tmux 管理靠 tmux_exec 短命 PTY 会话顺带完成，AI/文件树/obs 一概没碰。
用量小本身不是理由（不调用零开销），真正的理由是**生命周期耦合**：

- kfmv4 重启/崩溃（开发期高频），na 终端会话全断——哪怕 tmux、sshd 都活着；
- L3 自持隧道 + 断线卡 + 续链（BAR-117）让传输层自持了，会话层却寄人篱下，
  架构自相矛盾；
- 协议本来就是 na 定义的（`src/protocol.rs` ClientMsg/ServerMsg，kfmv4
  只是恰好实现），服务端重写面极小——这是「成本突然变低」的关键事实。

> 对 AGENTS.md「服务端一行不动」的修订：该条指「不改动 kfmv4 服务端」，
> 仍然有效；na-server 是 **na 仓自有的新资产**，不是改 kfmv4。实现提交时
> 同步修 AGENTS.md 该段表述。

## 一、定位与边界

- **第一性原则（2026-09-20 用户拍板）：na 是主体**。na-server 不是
  服务器上独立养着的系统服务，而是 **na 连接时由 na 拉起、归 na 看管的
  娃**——服务器侧、手机侧同此理。na 走到哪里，服务跟到哪里。
- **na-server = na 的会话层后端**：终端 PTY over WebSocket + na-report
  回传口 + 健康面（服务卡数据源）。三件事，没有第四件。
- 不做：AI 对话、obs、provider——那些仍是 kfmv4 的资产，na 用到时走 kfmv4
  （后端可双挂，见 §五）。**2026-09-26 修订：文件树的「数据面」入驻本仓**
  （`GET /api/fs/list` + `GET /api/fs/read`，§三）——它属于会话层同构的一部分
  （树卡片读的后端该是 na 自己拉起的那个），纯函数核 `crates/na-protocol/
  src/fsapi.rs` 双端共享；文件树 **UI 与 @ 引用**仍归客户端，语义规格照抄
  nz `docs/file-tree-v1-design.md`（本仓不重造手感）。
- 安全语义照抄 8021：**只绑 127.0.0.1**，公网不可达，入口只有 SSH 隧道。
  无浏览器客户端 ⇒ 不需要 origin 校验；鉴权 = SSH 本身。

## 二、进程形态

- 新 crate：`crates/na-server`（workspace 第三成员）。
- 依赖：tokio + tokio-tungstenite + portable-pty + serde_json。全是 na
  已在用的件，无新生态。
- **协议单源**：`src/protocol.rs`（壳 crate 私有）上提为共享——迁入
  `cordis-na`（纯 JSON 编解码，符合核心层禁平台依赖纪律）或独立
  `crates/na-protocol` 小 crate。实现时二选一，倾向后者（cordis-na 语义
  是终端核心，协议是传输层，别混居）。na 壳与 na-server 同吃一份，
  协议漂移在类型层面不可能。
- 监听：`127.0.0.1:9021`（服务器侧）。隧道目标从
  `-L 9021:127.0.0.1:8021` 改 `-L 9021:127.0.0.1:9021`——**双端同口
  9021**，手机侧无感。（实现第一步先实证服务器侧 9021 无占用。）
- **生命周期（2026-09-21 用户拍板改判：常驻模式为主路）**：
  - **常驻（主路）**：「服务常驻在服务器，但它依然是 na 的触手」——unit
    内容**随 na 走**（`na_server_sup::unit_content` 单一源，na 编译期内嵌），
    na 每次连接经既有 SSH exec 通道**幂等**装/更新 unit（内容变了才重写 +
    `daemon-reload`）+ `systemctl enable --now`（在跑的不重启）；活着归
    systemd（`Restart=always` 收尸、`WantedBy=multi-user.target` 随机器
    自启、`NA_IDLE_EXIT_SECS=0` 永不自退）。na 侧探针因此从 15s 放宽到
    60s——它只是**看状态**，不再是伺候一个随时会死的自持娃。承载模式
    进 `SupSnap.mode`（常驻/自持/借用）上卡面，na 由此「监控服务器上这个
    服务的状态」。**迁移注意（一次性）**：从自持模式切常驻时，占着 9021
    的老 spawn 进程要先停（否则 unit 绑不上口，`Restart=always` 会空转）；
    其上的 ws 会话断一次、由 tmux 续上（会话不丢）。**这条已写进 ensure
    脚本**（一次性迁移段：unit 未 active 而 9021 上有**本仓** na-server =
    自持遗留 → 只认 cmdline 精确停掉它，别的一律不动；实测迁移后 unit
    一次绑定成功）。
  - **降级（无 systemd 的机器：Termux 等）**：仍走自持 spawn + idle 自退
    1800s，且**先探活接管**（活 = 不重启别人的进程）。
  - 历史（v1 主体拉起制，2026-09-20 设计）：
  - **拉起**：na app 连上服务器后，经 SSH exec 通道在服务器侧
    `setsid`  detached 启动 na-server（通道与隧道同一条 SSH 或独立
    exec，实现时定）；app 侧归 tunnel.rs 看门狗同级看管（新 supervisor
    或 tunnel supervisor 扩一席，实现时定）。
  - **归属语义照抄隧道的 Up/ExternalUp**（BAR-117 时代已验证的模式）：
    自己 spawn 的归自己管（死链重拉）；连上时发现已有 na-server 在跑
    （上次留守/别人起的）=「外部借用」，只接管使用，不重启不杀。
  - **留守与自退**：na 断开时 na-server 留在服务器上（安卓管不到
    服务器进程）；带 idle 自退保卫生——无会话且无人连接满 N 分钟
    （初值 30）自行退出，下次 na 连接时重新拉起。
  - **二进制投递**：一期 = 服务器仓内 `cargo build -p na-server`
    （服务器有完整工具链 + repo 双推同步，拉起命令先检查后建）；
    二期 = na 推静态 musl x86_64 二进制到服务器固定路径
    （无工具链也能起）。
  - systemd unit 降级为**可选兜底**（服务器长期无人连也想活着的
    场景），不是主路，一期不写。

## 三、接口面（就这么多）

| 面 | 规格 | 说明 |
|---|---|---|
| `GET /ws`（Upgrade） | 消息协议 = 现 protocol.rs 一字不改 | terminal-open(cwd?, cmd?) → spawned PTY；input/resize/close；回 opened/output/exit。语义对齐 kfmv4 terminal-pty.ts |
| `POST /api/na-report` | body 原样 append `/root/kfm-na/field-reports.log` | 与 kfmv4 files.ts:378 同行为；接报表路的迁移见 §五 |
| `GET /api/na/health` | `{uptime_s, sessions:[{id, cmd, cols, rows, alive, idle_s}]}` | **新增**，服务卡唯一数据源（kfmv4 没有此面——后端是 kfmv4 时服务卡显示「kfmv4 托管」态）。idle_s = **真空闲**（距最后 input/output 活动，2026-09-20 修约：此前借 opened_epoch_s 充数 = 年龄冒充空闲，服务卡显形后修约，wsterm input/output 接线 registry.touch） |
| `GET /api/na/sys` | `{load:[l1,l5,l15]\|null, procs:[running,total]\|null, mem_total_kb\|null, mem_avail_kb\|null, swap_total_kb\|null, swap_free_kb\|null, disk_total_b\|null, disk_avail_b\|null, uptime_s\|null, cores\|null}`（2026-09-20 晚三路扩：进程/交换/在线；**2026-09-21 加 cores——负载判色口径**：负载占比 = l1/核数，客户端凭它把负载轨并入三档判色；旧版缺键 → 客户端 None → 负载轨回退窗内峰值归一 + 中性档，契约向旧兼容） | **新增（2026-09-20）**，环境卡唯一数据源——「中央终端所在环境的自身体征」，与设备无关的通用面。采集/解析 = `crates/na-sys`（na-server 与 na 客户端同一份，双端同构第二面；手机本地相 = 客户端 `collect("/data")` 直读，卡面零改动）。**逐路显形契约（同日修约）**：键永远在（客户端凭键认版本），采不到的路 = null——Android SELinux 拒 /proc/loadavg 实锤（手机 Termux EACCES，meminfo 可读），collect 永不整组失败，一路塌不连坐 |
| `GET /api/fs/list?dir=<相对路径>` | `{"ok":true,"dir":…,"entries":[{"name","kind":"dir"\|"file","size":u64,"mtime":i64(ms)}]}` | **新增（2026-09-26）文件树数据面**：只列**直接子层**（懒加载，子目录里的东西不冒头），排除规则过滤后按名字排序。`dir` 缺省 = 空串 = 允许根本身 |
| `GET /api/fs/read?path=<相对路径>&max=<字节>` | 文本 `{"ok":true,"path","binary":false,"truncated","size","text"}` / 二进制 `{…,"binary":true,…}`（不带 text） | **新增（2026-09-26）**：NUL 探测判二进制；`max` 缺省 64KB、上限 1MB；`truncated = 读到的 > max ‖ 读到的 < size`；text 按 UTF-8 字符边界收口（绝不劈出半个字）。`size` 永远是全文长度 |
| 文件面失败 | 404 `{"ok":false,"error":"not found"}` | 越界 / 不存在 / 类型不符（列文件、读目录）**塌成同一条响应**——不透露存在性；IO 故障才 500 |

**文件树数据面（2026-09-26 立项入驻）**：纯函数核 = `crates/na-protocol/src/fsapi.rs`
（`list_json`/`read_json`/`resolve`/`safe_rel`/`excluded`/`query_get`/`pct_decode`），
na-server 的 `httpd::route` 只切 query 出参、`main.rs` 只做 IO 搬运——
放 na-protocol 是为了**双端一份**（客户端要同一套 query 编解码与出参形状，
各写一份必漂移）。语义真相源 = nz `src/server/fs.ts` + `docs/file-tree-v1-design.md`
（na 本地化换皮，手感/安全口径照抄）。

- **安全模型**：路径一律相对允许根；`..`/绝对路径/根组件在 `safe_rel` 就拒；
  `canonicalize` 后必须 `starts_with(根 canonical)`——**软链逃出根 = 与不存在
  同一种 404**。canonical 路径一路带到 open/readdir（不拿拼接路径复用）。
- **允许根 `NA_FS_ROOTS`**（环境变量，冒号分隔；每请求现读，运行时可改）：
  缺省 = `/root/00-Loyintina`（库本体）存在则用它，否则 `$HOME`——全量 HOME
  会把源码树/toolchain 全索进来（nz 8.3MB 索引实锤）。**设成空串 = 零根
  fail-closed**（一切都 404），不退缺省，防「以为收窄了其实放开了」。
- **执行纪律**：na-server 是 `current_thread` 运行时（全部连接含 WS 终端流
  共用一条线程）——fs 面**必须**走 `tokio::task::spawn_blocking` 的阻塞池，
  handler 里直接 `std::fs` 会把所有连接一起冻住（2026-09-26 实拍：40000 条
  目录慢列在飞时，health 面 29ms 应答）。
- **已知差异**：排序 = Rust `sort()` 字节序（Node 是 UTF-16 码元序，
  仅 U+10000 以上与 U+E000..U+FFFF 混排时理论序不同）；出参键序 =
  serde_json 的字母序（JSON 对象键序无语义，两端都走 serde 解析）。

## 四、可视化落位（解析页服务卡）

解析页三级卡纵排：tmux 卡（已有）→ 连接卡（已有，传输层：隧道）
→ **服务卡（新，会话层：na-server）**。

- 位置：连接卡正下方，同宽；几何照旧例由 tmux 卡 INSET 同源预留。
- 内容 v1：卡头「服务 · na-server 在线/托管中/未上线」+ 字段行
  （进程/ uptime / 会话数）+ 每会话一行（绑的 tmux 窗/空闲时长）。
- 数据：health 轮询（解析页可见时 2s 一拍，不可见不轮——照 tmux 卡
  现有节拍惯例）。
- 二期（手机侧 na-server 上线）：卡头加「本地/远程」切换即复用，不重造。

**v1 已落地（2026-09-20）**：`src/ui/svc_card.rs`（文案三源合成 +
动态高几何，A 档 13 考题三变异）+ `src/svc_health.rs`（health 轮询
器：页可见 2s/不可见不轮，http1 手写客户端打隧道本地口，零新依赖）。
落地与设计的两处偏差：①字段行定四（后端/在线/会话/错误——「进程」
并入状态词，错误行接 nasup 旧账与轮询错，连接卡同尺）；②Error 相
保留旧数据（闪断不清卡面）。kfmv4 后端 = 卡显「kfmv4 托管」态
（§三表口径照旧）。

**环境卡 v2 已落地（2026-09-20，解析页第三张二级卡，连接服务合并
卡下）**：「中央终端所在环境的自身体征」可视化（用户立项：与设备
无关的通用面）。`src/ui/sys_card.rs`（恒定高几何 + 三源合成，A 档
考题六变异）+ `crates/na-sys`（负载/进程/内存/交换/磁盘/在线解析
采集，na-server 与客户端同一份）+ na-server `/api/na/sys` 面。轮询
与 health 同器同拍（svc_health 扩面，sys 错误只报不换位）。字段
定六两竖列（行主序：负载/进程 | 内存/交换 | 磁盘/在线——同日晚
用户拍板三路扩，「服务器的信息能不能更详细一些」），IO/网络/温度
后续再议。**逐路显形（同日修约，chain-phone 红出来的真需求）**：
SysInfo 各路全 Option——单路采不到 = 该字段卡面「—」，不许编造
零值不连坐（手机本地相的合法常态：SELinux 拒 /proc/loadavg 与
/proc/uptime，meminfo 可读，statvfs 可用）。

**连接服务合并卡 v2 已落地（2026-09-20 晚，用户拍板「第二张和第三
张卡能合并一下吗？占空间太大了，可以做成两竖列」）**：连接卡 +
服务卡合并成 `src/ui/link_card.rs` 一张二级卡两竖列——左列连接
（mini 卡头/四字段/[重连]钮钉列底），右列服务（mini 卡头/四字段/
会话行表，无分隔线）。文案面零改动（conn_card/svc_card 退役为文案
面，同两份快照同两份考题）；几何/命中归 link_card（A 档 6 考题四
变异：两列等宽半分/卡高取高列/钮钉列底/预留量同源）。省一段卡头
+ 一段卡间距的纵高。

## 五、切换与回退（双挂期）

1. na-server 上线后 kfmv4 **原样保留**，双后端并存。
2. na 设置页「服务器配置」加后端项：`na-server`（默认，9021）/
   `kfmv4`（回退，隧道指回 8021）。切换 = 重建会话，与切服务器同语义。
3. 报表路：一期不动（kalo `-L 8021` → kfmv4 /na-report 照旧）；na-server
   的 /na-report 先实现备着，二期把 kalo 转发指过去后 kfmv4 对 na 彻底
   变成可选。
4. 回退标准：na-server 判卷期出任何「kfmv4 不出」的问题 → 设置切回
   kfmv4 即回旧世界，零代码回滚。

## 六、测试标准（判卷人：redroid 考场 + 宿主 A 档）

**A 档（宿主，考题先行带变异）**
- 协议编解码共享 crate 迁移后，na 侧现有 protocol 钉全绿不改动 = 迁移
  零漂移的直接证据。
- PTY 生命周期：spawn `/bin/sh` → echo 往返字节级一致 → resize 生效
  → exit 码透传。kfmv4 行为做参照系（同一条消息序列打两边，输出流
  等价）。
- health 面字段与实况一致（起 N 条假会话断言 N 条记录）。

**redroid 考场（B/C 档，全程实录判卷）**
1. 切 na-server 后端：会话开/输入/输出/重排/tmux_exec 短命会话，全绿。
2. **解耦判决（本项目的存在性证明）**：na 会话挂着，服务器上
   `systemctl restart kfmv4`——na 侧零感知（不断、不卡、不出断线卡）。
   这一条不过 = 项目白做。
3. 断线回归：掐 na-server → 断线卡出 → 双钮命中 → 起回 → BAR-117
   续链自动接回，行为与 kfmv4 时代逐项对齐。
4. tmux 窗口表/新建/关闭/重排经 na-server 全环绿。
5. 压测：150 行重排 + htop 满屏刷新 5 分钟，帧账不比 kfmv4 后端差
   （预期更好——少一层 Node 转发）。

**结案纪律**：照观测先行三条款，仪器判绿 ≠ 结案，用户真机终验后才许
写「结案」。

**PTY 层 nix 直造（2026-09-20，手机 chain 红的裁决）**：pty_sess 曾用
portable-pty，其 serial → termios 0.2 依赖链没有 android cfg——Termux
整编 na-server 必红（E0432/E0433，`os::target` 空）。裁决：换 nix 直造
（主 crate local_pty.rs 同款 posix_openpt/fork/waitpid + FORK_LOCK +
CLOEXEC 纪律），Android 与 host 同一份代码——这是二期双端同构的前置，
不是绕路。接口不变：spawn/resize(TIOCSWINSZ)/try_wait(WNOHANG)/kill。

**增量 UTF-8 解码（2026-09-20，redroid tofu 病灶结案）**：PTY 读线程
不许按块 `from_utf8_lossy`——块界劈开多字节字符时两侧各产一个 U+FFFD
（redroid 终端实录目击）。正身 `utf8x.rs`：半截合法序列（≤3 字节）攒
carry 下一口续拼，非法字节替换符顶一个，EOF 尾巴强制出场不吞字节。
判卷仪器：`NA_READ_BUF` 环境变量拧小读块（默认 8192，下限 4）——
live 考题拧 7（7%3=1 块块劈字）。两条教训编码：①内核 PTY 分包可能
恰好字符对齐，默认块大的变异抓不到病灶，仪器旋钮让劈字成必然；
②帧界判读必须对累积流做——结束标记（如 LIVE-DONE）会被小帧劈进
两帧，逐帧 contains 永远等不到（探针实证）。

## 七、二期：双端同构与路网（2026-09-20 用户定调）

na 自己也能起服务，且**任何机器上的 na 都是同一个 na-server**——
同 crate、同协议、同接口面，区别只在谁拉起、谁连它。

**端口地图（统一登记，新增必改此表）**

| 口 | 在哪 | 是什么 |
|---|---|---|
| 9021 | 手机本地 ↔ 服务器 | na 数据口：手机 `-L 9021:127.0.0.1:9021` → 服务器 na-server |
| 9031 | 服务器本地 ↔ 手机 | **手机侧 na-server**（na app 内嵌监听 127.0.0.1:9031）；服务器经 `-R 9031:127.0.0.1:9031` 反连访问 |
| 8021 | 手机本地 → 服务器 | 报表路（kalo `-L 8021` → kfmv4），一期照旧，二期可切 na-server |

**三种访问关系**

1. **na → 服务器服务**：现状主线（9021 正连），本文档一期。
2. **服务器 agent → 手机服务**：同一条 SSH 加 `-R` 反连——服务器上的
   agent（kimi 会话、kfmv4、脚本）访问 `127.0.0.1:9031` 即摸到手机
   na-server：手机的本地终端、文件、传感器面都能经此暴露。
3. **其他机器 → na**：服务器做枢纽（它有公网 IP + 两条转发都在它
   手上）。别机 SSH 到服务器后走路由口即达任意一端 na；na 世界对外
   只有服务器一个门。

**手机侧 na-server 形态**：开发期 Termux 托管；产品态内嵌 na app
进程内监听（na 内置终端已证明手机具备完整进程托管能力）。二期主要
成本在安卓生命周期管理，不在新开发。
