# na QUIC 隧道设计（quic隧道.md）

> 状态：**设计稿**（2026-09-23 用户拍板立项：「4~5s 压进 1s 的唯一真路径」）。
> 一句话：把 na ↔ 服务器的传输从 ssh/TCP 换成 QUIC——连接用 Connection ID
> 标识而不绑 IP 四元组，**换 IP 不死 = 重连这个行为本身消失**。

## 〇、由来（为什么做）

BAR-126~142 三天七个 BAR 都在治同一个物理层：移动网换 IP 时 TCP 四元组
失效，所有死亡检测（keepalive/探活）、释放（撞口）、重握手都只是「死了
之后更快重来」。BAR-142 后理论恢复 ≈4~6s，已贴近 ssh 方案的极限——
重握手本身（TCP+SSH 认证，移动网 RTT）就占 ~1s 且不可省。

QUIC 的根本差异：

| | ssh/TCP | QUIC |
|---|---|---|
| 连接标识 | IP 四元组 | Connection ID（不绑 IP） |
| 换 IP | 连接死亡，重新握手 | **迁移**：新地址继续收发，验证即恢复 |
| 冻结唤醒 | TCP 必死（NAT 蒸发/服务器收割） | 连接状态在内存，唤醒即用（无握手） |
| 反向通道 | ssh -R 绑口（撞口/收割/释放一串病） | 服务器直接开流（双向是天生的） |
| 恢复（进程真死后） | 全量握手 ~1s | 0-RTT，1 RTT |

省电周场景的全部利好：冻结→唤醒，QUIC 连接还是那条连接——**感知 0s**。

## 一、定位与边界

- **QUIC 隧道是传输层 v2，不是替代协议层**：na 壳与 na-server 的应用协议
  （na-protocol ClientMsg/ServerMsg、/api/na/* 平面）一行不动。QUIC 只做
  「127.0.0.1:9021 这条本地 TCP 的载体」，角色与今天的 ssh -L/-R 完全相同。
- **ssh 隧道退役节奏**：QUIC 验证期内双轨（QUIC 优先，ssh 看门狗降级兜底），
  实机稳定后 ssh 降为设置项。9021 语义不变（只绑回环）。
- **90_21 双向同口不变**；9022 反向路径从「sshd 绑口」变为「na-server
  本机 TCP 监听器 + 服务器开流」，撞口/僵尸/释放三件套整族消失。
- 不做的：HTTP/3、浏览器客户端、公网入口。QUIC 口也只准回环+隧道对端，
  安全语义照抄 8021/9021。

## 二、架构（桥接模型）

```
手机侧（na 核内）                     服务器侧（na-server 内）
┌─────────────────────┐            ┌──────────────────────┐
│ App → 127.0.0.1:9021 │            │   127.0.0.1:9021     │
│        ↓            │            │        ↑             │
│  本机 TCP 监听器      │  QUIC      │   QUIC 监听器         │
│  (今天 ssh 的位置)    │ ════════→  │   (新，quinn)         │
│  conn → quic stream │  CID 迁移  │   stream → TCP 9021  │
│        ↑            │ ←════════  │        ↓             │
│  127.0.0.1:8024     │ 服务器开流  │   TCP 监听 127.0.0.1:9022 │
│  (na sshd，调试路)   │            │   (今天 sshd 的位置)   │
└─────────────────────┘            └──────────────────────┘
```

- **正连**：App 照旧打 127.0.0.1:9021（报表/终端/插件全不需要改——BAR-139
  的「走自持隧道」语义不变，只是载体的壳从 ssh 进程换成核内线程）。每个
  本地连接 = 一条 QUIC bidirectional stream，帧头一字节 kind + 目标口，
  服务器侧回联 127.0.0.1:9021。
- **反连（9022）**：na-server 自己监听 127.0.0.1:9022（它是普通 TCP 服务，
  无主概念）——服务器侧入站连接 → na-server 沿 QUIC 反向开流 → 客户端
  回联 127.0.0.1:8024（na sshd）。多服务器会话 = 多条流，同一 QUIC 连接
  承载，**没有端口绑定动作，撞口在类型层面不存在**。
- 桥接全是无状态字节拷贝（TCP↔stream splice），协议无关性 = 未来新增
  通道（文件树/AI 上 na-server 时）零改动。

### 为什么桥接而不是 QUIC 直吃协议

单源原则：协议编解码只有一份（na-protocol），TCP 接口只有一份（na-server
httpd/wsterm）。QUIC 直吃 = 协议消费点×2，漂移门×2。桥接把 QUIC 贬为纯
传输，QUIC 出问题时降级 = 关掉桥退回 ssh，应用层无感。

## 三、进程形态与分层

- 新 crate `crates/na-quic`：quinn endpoint + 桥接（手机/服务器同一份
  代码，双端同构——na-server 同吃）。**核心层合规**：quinn/rustls 无平台
  依赖（ring 已在依赖树里实证可编 aarch64-android，v1 首航实录）。
- 手机侧：tunnel.rs 增加「QUIC 腿」（核内线程，不是外部进程）。看门狗
  状态机双腿：QUIC 优先生效，QUIC 挂 → ssh 腿降级兜底（现有机能全保留，
  那是一套经过七天风暴考验的资产）。
- 服务器侧：na-server 增加 quinn 监听（UDP 127.0.0.1:9021？**待裁决
  见 §七问题 1**），常驻模式同现有形态。
- **死亡检测范式切换**：ssh 腿的「进程探活+e2e 探活」是查户口；QUIC 腿
  是事件驱动——quinn 直接报告 connected/lost/migrated，僵尸在类型层面
  不存在（连接死了 Rust 侧立刻知道，不用探）。watchdog 的探活三件套
  （ServerAlive/e2e/连败×2）对 QUIC 腿全不需要。

## 四、认证（双向 pinning，复用 ssh 心智）

QUIC 强制 TLS 1.3，方案按 ssh 信任模型抄（**2026-09-23 双向已全落地**）：

- **服务器证**：服务器侧自签证书一张（生成于 /root/kfm-na-certs/（仓外——2026-09-23 教训：仓内路径被 git add -A 扫进公网远端，已轮转）），
  公钥指纹钉进 na 设置（像 ssh known_hosts）。na 首次连接报指纹不符即拒。
  ✅ 已实现（`PinnedVerifier`，考题：错指纹握手即拒）。
- **客户端证**：na 侧预共享 32 字节密钥（与证书同目录 `{前缀}.psk` 首跑
  生成，0600），每条流头 = 2 字节端口 + 32 字节 HMAC 标签
  （`HMAC(psk, "na-quic-auth-v1"‖端口)`，RFC 2104 手卷、RFC 4231 向量
  判卷；标签走 TLS 内部，静态即安全——看不见也伪造不了；绑端口防挪用）。
  验签不过：常量时间比对（`ct_eq`）+ 弃流 + 按来源 IP 记连败，
  满 5 封 10 分钟（`ban_verdict`——防资源消耗，不是防枚举）。
  ✅ 已实现（考题：对钥匙 echo 全还 / 错钥匙零字节）。
- rustls 配置 = `dangerous` 自定义验证器 + 指纹比对，不走 CA 体系
  （我们没有域名，CA 是负资产）。

## 五、迁移与生存期语义（核心章）

- **migration 开启**：quinn `enable_migration`。NAT rebinding（运营商换
  映射）自动处理——服务器看到新源地址，路径验证后继续。
- **keepalive**：客户端 10s 一 ping（保 NAT 映射不蒸发；比 ssh ServerAlive
  便宜——无加密通道重建成本）。省电冻结期 ping 停发，NAT 蒸发无所谓：
  唤醒后从**新地址**发包即迁移，连接仍是那条。
- **idle timeout**：服务器侧设 4h（冻结期连接状态保留的代价 ≈ 一条 CID
  表项）；超时后进程还在 → 0-RTT 会话票据恢复（1 RTT）。
- **进程死亡**：App 被杀 = 连接状态蒸发 → 重启走 0-RTT 恢复。v1 先全量
  握手（~1 RTT 也够），0-RTT 列 v1.1。
- **并发限制**：单连接 streams 上限按现有用量×4 配置；na-server 每条流
  独立 tokio task，背压走 quinn 自带流控。

## 六、测试方案（netns 模拟 IP 迁移，服务器全链可测）

真机迁移难复现 → **网络命名空间+NAT 重映射**在服务器上模拟运营商：

```
nsA（客户端）─veth─ nsR（路由器/NAT）─veth─ 服务器（lo）
```

- nsA 起 quinn echo 客户端，发心跳流；
- 中途在 nsR 上 `nft` 把 masquerade 源 IP 从 10.99.0.1 改成 10.99.0.2
  （= 运营商换 NAT 映射）；
- **判卷**：客户端无感（无重连、无 stream 中断），echo 不丢行——即
  迁移成功。整个考题可挂 chain（A 档：预期行为可编程断言）。
- 反例对照：同样操作对 TCP/ssh = 连接必死（验证测试环境真的在模拟）。

## 七、待裁决（实现前拍板）

1. **QUIC 监听口**：UDP 怎么绑？QUIC 走 UDP，「只绑 127.0.0.1」则公网
   不可达，手机打不进来——QUIC 必须绑**公网 UDP 新口**（安全语义从
   「回环」改为「双向 pinning 认证」），或再套一层穿透。倾向：**公网
   UDP 独立口**（2026-09-23 用户拍板：62633 正连数据路 / 62694 反连推送路），认证见 §四。这突破了 8021/9021 的回环红线，
   需要用户点头。
2. **0-RTT 是否进 v1**：会话票据持久化在手机上（私有目录），风险面小；
   但 v1 先求全量握手把迁移跑通更稳。
3. **ssh 腿降级策略**：QUIC 挂几秒后才允许 ssh 腿接管？还是双腿并行
   （谁先通谁上岗）？倾向双腿并行、QUIC 优先（自愈不新增等待）。

## 八、里程碑

- [x] M1 依赖 spike（✅ 2026-09-23：host + aarch64-android 双端 check 过；
      echo 双通 `spec_m1_quic_echo_双通`。离线 vendor 更新留到 M3 接线时）
- [x] M2 netns 迁移考题（✅ 2026-09-23 首发即过+复跑稳：`scripts/
      test-quic-migration.sh`——conntrack 清空+snat 换源（10→11）模拟
      运营商掐旧映射，QUIC 60/60 行全回还零重连，TCP 反例对照必死。
      环境坑一枚：本机内核 nft 在新 netns 建 nat 链 ENOENT，用 iptables）
- [x] M3 na-server QUIC 监听 + na 核本机桥（✅ 2026-09-23：na-server
      `NA_QUIC_BIND`/`NA_QUIC_CERT` 可选 QUIC 腿（首跑 rcgen 自签落盘，
      指纹打 stderr）；na-quic `run_server`/`run_client` 桥接全链考题
      `spec_m3_桥接全链_echo_逐字节回还` + `spec_m3_na_server_quic_leg_health`
      全绿；pinning 正反两钉（对指纹过/错指纹握手即拒）。redroid 真链
      冒烟待 §七问题 1 裁决后随 M5 走）
- [ ] M4 反连路（9022）迁移 + 真链双腿并行
      （**看门狗双腿状态机已提前就位** 2026-09-23：tunnel.rs `Leg` 裁决
      QUIC 优先、`QUIC_FAIL_TRIP=3` 连挂跳闸降级 ssh、腿在时 ssh 降
      `-R`-only 伴生保推送路、手动重连/回前台即审清零再给 QUIC 一票；
      `servers.json` 增 `"quic": {enable, port=62633, pin, psk}` 段，
      缺省关。
      **M4-1 na-quic 反连双腿**（✅ 2026-09-24）：`run_rev_server`
      （QUIC 监听→psk 注册认领→存活期绑本机 TCP 桥）/ `run_rev_client`
      （拨出注册→accept_bi 回联本机口），`REG_PORT=0` 注册流端口头；
      全链考题 `rev_bridge_spec.rs` 三钉（echo 逐字节回还/错钥匙零服务/
      注册口零非业务）变异双咬。
      **M4-2 na-server 接线**（✅ 2026-09-24）：`NA_QUIC_REV_BIND`
      （不设=不开）/ `NA_QUIC_REV_TCP`（缺省 127.0.0.1:9022，只准回环）/
      `NA_QUIC_REV_TARGET`（缺省 8024 = 手机 na sshd），与正连腿同证同钥。
      **M4-3 看门狗反连腿**（✅ 2026-09-24 代码就位，真链判卷归 M4-5）：
      tunnel.rs `spawn_rev_quic_leg`（UDP 62694，复用正连腿 pin/psk）、
      `ssh_role` 真值表裁决 ssh 娃角色（None/Full/ForwardOnly/ReverseOnly
      ——角色随双腿供应商变必须换娃，摘 -R 顺路同步释放腾 9022 给 QUIC
      桥）、反连腿死只记账不动 TunnelState、手动重连双腿同收；快照新增
      `rev_quic_up`/`rev_quic_fails`。钉 `spec_m4_ssh娃角色_真值表` /
      `spec_m4_正连参数_反连摘除`（变异双咬）。
      待办：M4-4 通道卡两行真状态；M4-5 服务器部署 62694 + 真链并行验证）
      **M4-4 通道卡**（✅ 2026-09-24）：62694 行换 rev_quic_row 四相真
      状态，9022 行加 QUIC 反连相，钉 spec_m4_反连* 双咬。
      **M4-5 部署+真链判卷**（✅ 2026-09-24）：na_server_sup unit/降级
      spawn 双路带 NA_QUIC_REV_BIND=0.0.0.0:62694（钉 spec_常驻_unit
      内容与模式词）；服务器 kfm-na-server.service 常驻双绑 62633+
      62694；手机热更后真链全绿——反连认领→na-server 持 9022、
      na_ssh 走 QUIC 反连通、ssh 娃角色 Full→None 收编（双 QUIC 占
      满）。**兜底演练揪出反连腿僵尸案**：systemd 重启 na-server 后
      反连腿无本地可观测物，4h idle 上限内死信不来、9022 无人绑
      （数据腿同款病灶 BAR-146，但数据腿有 e2e 探活收尸）。修 =
      反连双腿专用死寂判死 REV_IDLE_TIMEOUT=60s（client_config_rev/
      server_config_rev；健康连接 keepalive ACK 续命，死寂 1 分钟
      定罪，ssh 兜底及时接）。钉 spec_m4_反连死寂判死_常量契约
      （变异：改回 4h → 咬）。
      **M4-6 BAR-157 认领循环陈尸憋死案**（✅ 2026-09-26）：62694 上机
      后间歇性全灭（ handshake 超时循环 150+ 次），pcap 逐包定罪——
      手机 Initial 到网卡正常，服务器 20/23 次**零回包**，仅存回包是
      每 60s 一具陈尸的 PTO 序列（60s = REV_IDLE_TIMEOUT）。病灶：
      quinn 的 Incoming **在应用 accept 之前不回第一个包**（等应用
      裁决 accept/refuse/retry），而旧认领循环**串行 await 每个
      Incoming**——客户端 8s 弃连后服务器要等满 60s idle 才收尸，
      一具陈尸把 accept 停摆 60s；服务期 accept 同样停摆（换网络的
      手机要等旧连接 idle 收尸才能握手）。弃尸来得比收尸快 = 队列
      永远排不完，腿永久死。修 = accept 驱动独立任务 + 握手/验签每条
      Incoming 一个任务（超时硬顶 HANDSHAKE_TIMEOUT×2）+ 认领走
      channel（积压只留最新，服务期新认领 close 旧连接当场挤换）。
      契约：**quinn server 的 accept 轮询是握手应答的唯一泵——任何
      「await 单条 Incoming 到终局再 accept 下一条」的串行认领都是
      陈尸放大器；弃尸是公网口的常态不是异常**。考题
      spec_m4_bar157_弃尸风暴_诚实客户端不被憋死（弃尸 = 冻结线程
      保 socket——本机 drop 会回 ICMP 秒收尸演不出 CGNAT 无声陈尸；
      idle 考场加速 2s）：旧码红（3.9s 超时）→ 修复绿（2.4s 全绿）。
- [ ] M5 真机验证（省电周间隙）+ 认证 pinning 落设置页
- [ ] M6（v1.1）0-RTT 会话票据

## 九、部署配置（M3 落地形态）

**服务器侧（na-server）**：两个环境变量，缺省不开——

- `NA_QUIC_BIND`：QUIC 腿监听地址。§七问题 1 裁决前与 TCP 同走回环
  闸（只准回环或显式 0.0.0.0）；裁决后绑公网 UDP 62633。
- `NA_QUIC_CERT`：证书路径前缀，缺省 `/root/kfm-na-certs/quic`（仓外）。
  首跑落盘三件套：`{前缀}.der` / `{前缀}.key.der`（rcgen 自签，指纹
  打 stderr——手机 pin 的比对物，**必须持久**，重生成 = 全设备换 pin）
  + `{前缀}.psk`（32 字节随机客户端证，0600，hex 打一次 stderr 供抄
  进手机 quic.psk）。**开 QUIC 腿即强制客户端证**（HMAC 挑战+连败封禁，
  设计 §四——公网口的前置条件，2026-09-23 已落地）。

**手机侧（servers.json 条目）**：

```json
"quic": { "enable": false, "port": 62633, "pin": "<64 位指纹 hex>", "psk": "<64 位密钥 hex>" }
```

缺省关。`tunnel::quic_configured` 齐件判定：开关开 + 口非 0 + pin/psk
双双恰 64 位 hex（两证齐全才准开腿）——缺任一件静默走 ssh（QUIC 是
加速器，不是单点）。

**看门狗双腿语义**（tunnel.rs，考题 `spec_quic_*` 钉死）：腿裁决
`leg_verdict`——QUIC 优先，连挂 3 次（`QUIC_FAIL_TRIP`）跳闸降级 ssh
兜底；QUIC 腿在时 ssh 只挂 `-R`-only 伴生（`reverse_only_args`，9022
推送路不断，本地口唯一属主是 QUIC 腿）；腿死信事件驱动（run_client
返回即死，免探活三件套）；手动重连/回前台即审清零跳闸账再给 QUIC
一票。状态相新增 `QuicUp`（卡面「自持 QUIC 在线」），与 `Up` 同属
可用相。

**反连路（M4，62694）**：na-server 侧三个环境变量——`NA_QUIC_REV_BIND`
（QUIC 反连监听，不设 = 不开；回环闸同正连腿）/ `NA_QUIC_REV_TCP`
（认领存活期绑的本机 TCP，缺省 127.0.0.1:9022，只准回环）/
`NA_QUIC_REV_TARGET`（回联手机侧目标口，缺省 8024 = na sshd）。与
正连腿同证同钥（`NA_QUIC_CERT` 前缀三件套复用）。**部署形态**：
systemd unit 与降级 spawn 两路都带（na_server_sup `unit_content` /
`ensure_script` 单一源，钉 spec_常驻_unit内容与模式词）——特许
公网仅 62633/62694 两腿（双向认证齐备）。手机侧零新增配置
——`spawn_rev_quic_leg` 复用 quic 段 pin/psk + `QUIC_REVERSE_PORT`
常量。看门狗语义：反连腿与数据腿独立（数据跳闸降级 ssh 时反连照跑）；
`ssh_role` 真值表裁决 ssh 娃角色——双腿都在 = 无娃，各占一路 =
挂剩下那路；角色变必须换娃（运行中的 ssh 改不了转发），摘 -R 时
同步释放腾 9022 给 QUIC 桥（不然桥绑不上口空转）；反连腿死只记
`rev_quic_fails` 账 + 报表，不动 TunnelState（数据路不归它），ssh
兜底零等待接上；手动重连双腿同收（用户在等 = 双腿各再投一票）。
