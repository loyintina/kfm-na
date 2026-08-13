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
bash scripts/chain.sh    # 唯一检查入口：fmt + clippy + android-check + test + build（提交前必过）
cargo test               # 只跑测试
cargo fmt                # fmt --check 红了的自救

# 打 APK（release，签名用 Android 官方 debug keystore，尖刺期配置见 Cargo.toml）
export ANDROID_HOME=/root/kfm-na-toolchain/sdk
export ANDROID_NDK_ROOT=$ANDROID_HOME/ndk/27.2.12479018
export JAVA_HOME=/root/kfm-na-toolchain/jdk PATH=$JAVA_HOME/bin:$PATH
cargo apk build --release   # 产物：target/release/apk/kfm-na.apk
```

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

## 复用的 kfmv4 资产（只读引用，不复制）

- `kfmv4/docs/active/眼睛与手.md` — 眼睛/手设计思想（NA 落地为网格眼睛 + 按键注入的手）
- kfmv4 服务端协议：terminal-pty ws、/ai/chat、文件树接口（对接时读 kfmv4 源码为准）

## 当前阶段

尖刺 1：手机上亮出终端画面。验收标准五条钉死在 `docs/active/立项.md`，
不达到不扩功能。
