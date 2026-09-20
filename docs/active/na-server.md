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
- 不做：AI 对话、文件树、obs、provider——那些仍是 kfmv4 的资产，na 用到
  时走 kfmv4（后端可双挂，见 §五）。
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
- **生命周期（主体拉起制，取代 systemd）**：
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
| `GET /api/na/health` | `{uptime_s, sessions:[{id, cmd, cols, rows, alive, idle_s}]}` | **新增**，服务卡唯一数据源（kfmv4 没有此面——后端是 kfmv4 时服务卡显示「kfmv4 托管」态） |

## 四、可视化落位（解析页服务卡）

解析页三级卡纵排：tmux 卡（已有）→ 连接卡（已有，传输层：隧道）
→ **服务卡（新，会话层：na-server）**。

- 位置：连接卡正下方，同宽；几何照旧例由 tmux 卡 INSET 同源预留。
- 内容 v1：卡头「服务 · na-server 在线/托管中/未上线」+ 字段行
  （进程/ uptime / 会话数）+ 每会话一行（绑的 tmux 窗/空闲时长）。
- 数据：health 轮询（解析页可见时 2s 一拍，不可见不轮——照 tmux 卡
  现有节拍惯例）。
- 二期（手机侧 na-server 上线）：卡头加「本地/远程」切换即复用，不重造。

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
