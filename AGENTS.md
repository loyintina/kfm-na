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
- 固定取包点（用户指定 2026-08-15）：每个包同时拷到手机 `~/w/项目/kfm-na/`——
  安装器没弹/找不到包时去那里拿

## 仓库布局（cargo 视野外的部分）

- `android/java/dev/kfm/na/` — Java 皮：MainActivity + KfmImeView +
  KfmInputConnection。NativeActivity 没有 InputConnection（中文死结根源），
  这层皮把 IME commitText 经 JNI 推进 `src/ime_queue.rs`。**BAR-008 红线：
  不许替换内容 View**——原生渲染路径一行不动，IME 用 1px 焦点占位 View
  叠加。改它必跑 chain 第 4 步（javac 编译检查）+ package-apk.sh 实拍。
- `android/AndroidManifest.xml` — 手工 manifest（package-apk.sh 直打）。
  包名 `dev.kfm.na`、主题、configChanges 与 cargo-apk 时代对齐。

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

- `docs/active/立项.md` — 架构三层 + 尖刺五条验收标准（**动工前必读**）
- `docs/active/工具卡.md` — 工具即卡设计（四个待拍板项定了才准实现）
- `docs/ledger/bugs.md` — BAR 账本：每条修复登记编号/病灶/契约/钉位置
- `/root/kfmv4/docs/ledger/agent-inbox/` — **跨线评审信箱**（评审会话维护，2026-08-15 迁入 kfmv4 文档目录）：
  kfm-na 与 kfmv4 两线设计评审往来信 + 状态列；设计相关评审意见在此收/发。
  kfm-na 侧的单文件信箱（docs/ledger/inbox.md）同日退役，勿重建

## 复用的 kfmv4 资产（只读引用，不复制）

- `kfmv4/docs/active/眼睛与手.md` — 眼睛/手设计思想（NA 落地为网格眼睛 + 按键注入的手）
- kfmv4 服务端协议：terminal-pty ws、/ai/chat、文件树接口（对接时读 kfmv4 源码为准）

## 当前阶段

尖刺 1：手机上亮出终端画面。验收标准五条钉死在 `docs/active/立项.md`，
不达到不扩功能。
