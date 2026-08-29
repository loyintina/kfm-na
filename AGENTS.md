# AGENTS.md — KFM-NA（Kaf Fee Meo Native）

> agent 入职指南。读完这份再动手。

## 这是什么

kfmv4（/root/kfmv4，TypeScript Web 应用）的 **native 手机客户端**，Rust 实现。
核心三件套：光球对话（内置 AI）/ tmux 里的 kimi code（远程操作服务器）/
文件树（仿 Obsidian 手机端交互）。终局愿景：NA 成长到与 kfmv4 同等高度后，
接管现在的 kfm 和数据。设计全貌见 `docs/active/立项.md`。

**服务端一行不动**——kfmv4 的 terminal-pty / /ai/chat / tree 接口是协议层资产，
本仓库只是新客户端，地位与浏览器客户端平等。

## 常用命令

```bash
bash scripts/chain.sh    # 唯一检查入口：fmt + clippy + android-check + java 编译 + test + build（提交前必过）
cargo test               # 只跑测试
cargo fmt                # fmt --check 红了的自救

# 打 APK（2026-08-13 起脱离 cargo-apk：中文输入的 Java 皮它塞不进去。
# 手工管线 javac → d8 → aapt2 → zipalign → apksigner，全本地工具零网络，
# 签名沿用 Android 官方 debug keystore，与旧包同证书可覆盖安装）
bash scripts/package-apk.sh   # 产物：target/release/apk/kfm-na.apk

# 送包到手机（ssh 隧道 localhost:8022 → Termux；scp 到共享存储 + am start
# 调起系统安装器，用户在手机上点「安装」完成最后一步——普通 uid 无
# INSTALL_PACKAGES 权限，静默安装 root 前无解）
bash scripts/deploy-phone.sh           # 送当前已打好的包
bash scripts/deploy-phone.sh --build   # 先打包再送
```

## 双环境（档位 2 手机自举，2026-08-15）

手机 Termux（`ssh -p 8022 localhost`）是第二个完整开发环境：`~/kfm-na`
仓库与服务器同步（服务器 `git push phone master`，手机端
`receive.denyCurrentBranch=updateInstead` 工作树自动更新）。

- 工具链：cargo/rustc/aapt2/apksigner/zipalign/openjdk-21 全部来自 termux 包；
  `d8.jar` + `android.jar` + `debug.keystore` 拷自服务器（`~/kfm-na-toolchain/`，
  d8 是纯 Java，bin/d8 是 wrapper）；.so 链接用 Termux 原生 cc
  （宿主即 aarch64-linux-android，免 NDK 交叉链）
- 脚本双环境自适应：`package-apk.sh`/`chain.sh`/`deploy-phone.sh` 检测
  `/data/data/com.termux` 自动切路径；测试字体夹具（DejaVu/Nimbus）在
  `tests/termview_spec.rs` 按候选路径解析
- 注意：手机 Rust 滚动更新（比服务器新），新 clippy lint 先在手机爆——
  修法是修到两边都绿，不要给手机降版本
- 手机上 `deploy-phone.sh` 走本地模式：跳过 scp 直接调安装器
- 固定取包点（用户指定 2026-08-15）：每个包同时拷到手机
  `/data/data/com.termux/files/home/w/项目/kfm-na/`——安装器没弹/找不到包时去
  那里拿（BAR-019：脚本里必须写绝对路径，`~` 会在本地 shell 展开成 /root）

## 仓库布局（cargo 视野外的部分）

- `android/java/dev/kfm/na/` — Java 皮：MainActivity + KfmImeView +
  KfmInputConnection。NativeActivity 没有 InputConnection（中文死结根源），
  这层皮把 IME commitText 经 JNI 推进 `src/ime_queue.rs`。**BAR-008 红线：
  不许替换内容 View**——原生渲染路径一行不动，IME 用 1px 焦点占位 View
  叠加。改它必跑 chain 第 4 步（javac 编译检查）+ package-apk.sh 实拍。
- `android/AndroidManifest.xml` — 手工 manifest（package-apk.sh 直打）。
  包名 `dev.kfm.na`、主题、configChanges 与 cargo-apk 时代对齐。
- `android/res/` — 应用图标等资源（mipmap-xxxhdpi/ic_launcher.jpg，
  源图 kfmv4/icons/kfm-icon.png，2026-08-16 用户指定；注意源文件扩展名是
  .png 但内容是 JPEG，仓内按内容存 .jpg，aapt2 与 BitmapFactory 都认内容）package-apk.sh
  第 4 步 `aapt2 compile --dir` + link `-R` 进包，不编 R.java。

## 纪律（三门，全部 hard fail，commit-msg/pre-commit 钩子机械化执法）

1. **chain 全绿**：pre-commit 跑 `scripts/chain.sh`，红了提交不了。
2. **fix 必须带钉**：提交信息首行 `fix:`/`fix(范围):` 必须触及测试
   （tests/ 或 `*_test.rs`）。回归钉 = `#[test]` 名带 BAR 编号 +
   `docs/ledger/bugs.md` 登记一行。确属无需补钉（纯配置/文案/构建修复），
   提交信息**独立一行**写 `tests:na` 豁免。
3. **文档耦合**：提交触及 src/ 或 scripts/ 必须同时触及 docs/；
   确认无文档影响则提交信息**独立一行**写 `docs:na` 豁免。
   （独立行语法：防正文讨论豁免标记时 prose 字面串误认——kfmv4 2026-07-30 教训）

提交信息语言：中文，格式同 kfmv4（`类型(范围): 主题`，类型 feat/fix/chore/docs/test）。

## 跨线运维公约（2026-08-28 评审裁决，全线生效）

1. **重 IO 窗口制**：连续型重 IO 任务（变异抽检/全仓多轮编译/批量
   索引）只准 22:00-07:00 空载窗跑，必须 `ionice -c3 nice -n 19`
   双甲（nice 只挡 CPU 不挡磁盘——08-28 两次 IO 事故的根因）；白天
   单发离散编译豁免。
2. **push 遇阻分流**：未提交闸（别线在途活）→留本地+信箱知会当事
   线，不空转重试；链超时（重活占场）→查 PSI 错峰，不连环重推；
   机械合规红→当场修当场推。
3. **信箱计数投影**由 kfmv4 侧 gen-agent-inbox 自动回写，na 侧不再
   手改计数（改也活不过下一次 gen）。

判例与全文：kfmv4 仓信箱 kfm-na-ops-convention-submission.md +
kfmv4-review-ops-convention-verdict.md。

## 分层纪律（2026-08-20 多端分层设计，评审五问全裁落地）

设计页：`/root/kfmv4/experiments/dsh-na/na/multi-end-layering.md`。三条：

1. **核心层禁碰平台依赖**：cordis-na（及未来的核心 crate）不许依赖
   winit/softbuffer/jni/android 系——chain 机械检查执法，不靠自觉；
2. **终端仿真归核心，渲染归壳**：alacritty 网格状态是数据，画像素是壳的事；
3. **新能力先问「核心还是壳」**：答不上来的不许写。

## 开发方法论（2026-08-13 用户拍板：考题先行，分三档）

**agent 写的考题，代码是根据考题生成的答案**——但按判卷成本分三档，不搞一刀切：

- **A 档·考题先行**（纯逻辑：协议解析/终端网格/按键编码/手势状态机/几何）：
  先写考题并验证红，答案生成到绿。**考题必须带变异抽检**——故意改坏答案
  看考题抓不抓得住（kfmv4 教训：考题弱 → 测试全绿行为全错，谁判判卷人）。
- **B 档·答案先行，考题钉住**（胶水/平台代码：Manifest/wgpu 初始化/生命周期）：
  这类代码的对错是「系统让不让你活」，没有输入输出可判卷。正常写，冒烟钉防退化。
- **C 档·感官判卷**（渲染手感/手势/中文 IME）：判卷人是眼睛和手指，
  实拍即判卷（尖刺五条验收标准就是 C 档考题），自动化只覆盖可测的边角。

判卷成本倒挂的不出考题（getter/装配/常量表）。

## 文档地图

- `docs/ledger/state.md` — **交接页：现在进行时**（当前位置/待判卷/欠账/日志
  判读手册，里程碑必更新；接手冷启动第一读）
- `docs/active/排障手册.md` — **用户报 bug 第一读**：症状 → 工具 →
  字段 → 判卷的速查表与八条走位（操作层；机制原理在调试闸门.md)
- `docs/active/调试闸门.md` — 8024 闸门机制全集 + §十一 排障闭环六步/
  逃逸条款/观测矩阵（设计层）
- `scripts/README.md` — 脚本索引（我要干什么 → 拿哪件）
- `docs/active/立项.md` — 架构三层 + 尖刺五条验收标准（**动工前必读**）
- `docs/active/ui-base.md` — UI 基础层契约（硬切基座+采样缝+动画全插件；
  含三行块排版纪律。动任何 UI 前必读）
- `docs/active/工具卡.md` — 工具即卡设计（四个待拍板项定了才准实现）
- `docs/ledger/bugs.md` — BAR 账本：每条修复登记编号/病灶/契约/钉位置
- `/root/kfmv4/docs/ledger/agent-inbox/` — **跨线评审信箱**（评审会话维护，2026-08-15 迁入 kfmv4 文档目录）：
  kfm-na 与 kfmv4 两线设计评审往来信 + 状态列；设计相关评审意见在此收/发。
  kfm-na 侧的单文件信箱（docs/ledger/inbox.md）同日退役，勿重建

## 复用的 kfmv4 资产（只读引用，不复制）

- `kfmv4/docs/active/眼睛与手.md` — 眼睛/手设计思想（NA 落地为网格眼睛 + 按键注入的手）
- kfmv4 服务端协议：terminal-pty ws、/ai/chat、文件树接口（对接时读 kfmv4 源码为准）

## 当前阶段

**阶段 3：多端核心层抽层（L1 已落地）**。尖刺 1/阶段 2 已闭环（终端可用化：
内嵌字体/快捷键行/触摸滚动/中文 IME/启动归因 BAR-020~024 全链，详见
bugs.md 与 state.md）。当前主线：L1 本地 PTY 双会话（本地秒开 + ws 远程
后台接 + Ctrl-] 切换）→ L3 本地 apt 生态（exec 探针两轮实拍：targetSdk 28
域降级放行私有目录 exec）。设计宪法：multi-end-layering.md（评审已批）。
