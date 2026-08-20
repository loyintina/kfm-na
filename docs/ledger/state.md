# KFM-NA 交接页（state.md)

> 纪律：**每个里程碑必更新此页**。接手者冷启动顺序：本页 → bugs.md →
> AGENTS.md → chain.sh 跑一遍。本页只写「现在进行时」，历史功过在 bugs.md。

## 当前位置（2026-08-20)

- **阶段 2(Android 终端可用化）收尾完成**。启动慢归因定案
  (BAR-020~023 连环，见 bugs.md)；BAR-023 提示行实拍判卷后取消
  (c73bf61),2.1s 首连唤醒的结构性答案移交 L1。
- **L1 本地 PTY 已落地（待实拍）**:conn-provider-local 插件 + 双会话槽
  （本地秒开为默认活跃，ws 远程后台接为待机，Ctrl-] 切换）；考题 4 道
  全绿（echo 往返/resize 传播/退出事件/双工厂并存）。关键技术债记档：
  多线程 fork fd 继承竞态 → FD_CLOEXEC + FORK_LOCK 串行化。
- **多端分层设计页已送审**(multi-end-layering.md v0，五问待裁决；用户
  终审拍板先行，裁决到达后对账）。
- **L2 探针待定**：私有目录 exec 封锁真机验证（静态 hello-world)，决定
  busybox/tmux 走 jniLibs 伪装还是低 targetSdk 豁免。共享存储是 noexec，
  可执行文件永远不放 /sdcard（用户问过，已答）。
- **方向共识（2026-08-19 与用户定）**: 不重写 Termux(termux-app 是 GPL-3.0
  一行不能抄；终端仿真我们已有 alacritty_terminal)。kfmv4 功能搬家
  （光球/卡片堆）后移到核心层分层落定之后。不为本地 shell 做常驻保活
  （wake lock 烧电；会话永生由服务器端 tmux 扛）。

## 待判卷（实拍未回）

| 项 | 包 | 判卷标准 |
| --- | --- | --- |
| ~~BAR-023 提示行~~ | kfm-na-16777509.apk | **已判卷（2026-08-20 用户实拍）**：字号正常，但观感尴尬，拍板取消——提示行已整体切除，连接前移+握手遥测保留（见 bugs.md BAR-023） |
| L1 双会话 | kfm-na-16777510.apk 起 | ①本地提示符秒出（首轮实拍已过：+118ms `:/ $ `）②启动落家 `/storage/emulated/0/Android/data/dev.kfm.na/files`，裸 `ls` 不再全墙 ③`ls`/`echo` 正常 ④CTRL+`]` 切远程再切回，历史还在 |
| L1 HOME 落家修补 | 待送机 | 首次实拍发现启动 cwd=/ 全墙，已修（HOME 选址+chdir），随下一包判卷 |

## 欠账

- 双指缩放调字号（用户明确要的；要动 CELL_W/CELL_H 常量化几何 +
  TermView set_cell_size 重构路径，壳层活，不挡主线）
- 阶段 2 落地通报（含 BAR-020/021/022/023）投信箱，模板参考
  /root/kfmv4/docs/ledger/agent-inbox/kfm-na-cordis-rs-stage1-landing.md

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
