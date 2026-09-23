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

QUIC 强制 TLS 1.3，方案按 ssh 信任模型抄：

- **服务器证**：服务器侧自签证书一张（生成于 /root/kfm-na/certs/，不进仓），
  公钥指纹钉进 na 设置（像 ssh known_hosts）。na 首次连接报指纹不符即拒。
- **客户端证**：na 侧预共享 32 字节密钥（现有 key_path 的同类物），首流
  HMAC 挑战——服务器认密钥不认 IP。
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
   UDP 独立口**（如 9023），认证见 §四。这突破了 8021/9021 的回环红线，
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
      `servers.json` 增 `"quic": {enable, port=9023, pin=指纹hex}` 段，
      缺省关。待办：反连路 QUIC 化 + 公网 UDP 裁决后的真链并行验证）
- [ ] M5 真机验证（省电周间隙）+ 认证 pinning 落设置页
- [ ] M6（v1.1）0-RTT 会话票据

## 九、部署配置（M3 落地形态）

**服务器侧（na-server）**：两个环境变量，缺省不开——

- `NA_QUIC_BIND`：QUIC 腿监听地址。§七问题 1 裁决前与 TCP 同走回环
  硬闸（只准 127.0.0.1）；裁决后绑公网 UDP（倾向 9023）。
- `NA_QUIC_CERT`：证书路径前缀，缺省 `/root/kfm-na/certs/quic`。
  首跑 rcgen 自签落盘 `{前缀}.der` / `{前缀}.key.der`，指纹（DER 的
  SHA-256 hex）打 stderr——手机 pinning 的比对物，**必须持久**（重生成
  = 全设备换 pin）。

**手机侧（servers.json 条目）**：

```json
"quic": { "enable": false, "port": 9023, "pin": "<64 位指纹 hex>" }
```

缺省关。`tunnel::quic_configured` 齐件判定：开关开 + 口非 0 + 指纹
恰 64 位 hex——缺任一件静默走 ssh（QUIC 是加速器，不是单点）。

**看门狗双腿语义**（tunnel.rs，考题 `spec_quic_*` 钉死）：腿裁决
`leg_verdict`——QUIC 优先，连挂 3 次（`QUIC_FAIL_TRIP`）跳闸降级 ssh
兜底；QUIC 腿在时 ssh 只挂 `-R`-only 伴生（`reverse_only_args`，9022
推送路不断，本地口唯一属主是 QUIC 腿）；腿死信事件驱动（run_client
返回即死，免探活三件套）；手动重连/回前台即审清零跳闸账再给 QUIC
一票。状态相新增 `QuicUp`（卡面「自持 QUIC 在线」），与 `Up` 同属
可用相。
