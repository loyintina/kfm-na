# AGENTS.md — KFM-NA（Kaf Fee Meo Native）

> agent 入职指南。读完这份再动手。
>
> **PARADIGM v5（2026-09-25 晨班会：BAR-145 五度再现定罪——「内部账
> 自洽 ≠ 无罪」升观测矩阵硬行（命中漂移三方对表：系统截屏×GL 帧×
> [touch]/[bake] 账+「指针位置」开发者选项）；v4 2026-09-24 晨班会：
> BAR-143 密钥防泄漏 纯段落→代码守卫
> check-no-secrets.sh 挂 chain 第 1 步；BAR-148 提交纪律第 5 门硬编码——
> 永不许 -c core.hooksPath 拆闸；v3 2026-09-23：BAR-134 报表行挂
> [arch/pid] 设备/实例标识——field-reports 混流分道闸从判读纪律升为数据
> 自带；v2 2026-09-21 晨班会：
> BAR-114 段落级→代码守卫 check-modal-cursor.sh；v1 2026-09-17 立，nz 唯一
> 产出迭代范式 na 落地）**：本文件
> 与 `docs/active/排障手册.md` 合为 na 的方法论唯一产出——每次任务它们
> 是输入（本文件机械注入，手册由本文件指认），每次踩坑把教训编码回来并
> 版本 +1（两文件头版本号同步）。形态效力阶梯：代码守卫 > 清单硬编码项
> > 文档段落——新教训能升形态就不许停在段落（漏 cd 三天三犯/BAR-098~106
> 三天三部曲：挂账≠入节奏）。**反哺审计**：`scripts/check/lesson-audit.sh`
> 按形态阶梯分级近 N 天 BAR 行，07:47 晨班会第一道议程逐条过 PROSE 级
> （升形态或写明挂账理由）——升级闭环有闹钟催，不靠自觉。

## 这是什么

kfmv4（/root/kfmv4，TypeScript Web 应用）的 **native 手机客户端**，Rust 实现。
核心三件套：光球对话（内置 AI）/ tmux 里的 kimi code（远程操作服务器）/
文件树（仿 Obsidian 手机端交互）。终局愿景：NA 成长到与 kfmv4 同等高度后，
接管现在的 kfm 和数据。设计全貌见 `docs/active/立项.md`。

**服务端一行不动**——kfmv4 的 terminal-pty / /ai/chat / tree 接口是协议层资产，
本仓库只是新客户端，地位与浏览器客户端平等。（2026-09-20 修订：此条指
「不改动 kfmv4 服务端」。na-server（crates/na-server）是 na 仓自有的会话层
后端，与 kfmv4 双挂并存，设计见 docs/active/na-server.md。）

## 常用命令

```bash
bash scripts/chain.sh    # 唯一检查入口：fmt + clippy + android-check + java 编译 + test + build（提交前必过）
cargo test               # 只跑测试
cargo fmt                # fmt --check 红了的自救

# 打 APK（2026-08-13 起脱离 cargo-apk：中文输入的 Java 皮它塞不进去。
# 手工管线 javac → d8 → aapt2 → zipalign → apksigner，全本地工具零网络，
# 签名沿用 Android 官方 debug keystore，与旧包同证书可覆盖安装）
bash scripts/package-apk.sh   # 产物：target/release/apk/kfm-na.apk
# （WITH_X86=1 出 arm64+x86_64 胖包——redroid 云安卓用，见 state.md
#  redroid 条；一键起场 scripts/redroid-up.sh）

# 送包到手机（ssh 隧道 localhost:8022 → Termux；scp 到共享存储 + am start
# 调起系统安装器，用户在手机上点「安装」完成最后一步——普通 uid 无
# INSTALL_PACKAGES 权限，静默安装 root 前无解）
bash scripts/deploy-phone.sh           # 送当前已打好的包（走 Termux 8022）
bash scripts/deploy-via-na.sh          # 自持通道：QUIC 桥 + FileProvider（na 含 Provider 后零 Termux）
bash scripts/deploy-phone.sh --build   # 先打包再送
```

## 双环境（档位 2 手机自举，2026-08-15）

手机 Termux（`ssh -p 8022 localhost`）是第二个完整开发环境：`~/kfm-na`
仓库与服务器同步（服务器 `git push phone master`，手机端
`receive.denyCurrentBranch=updateInstead` 工作树自动更新）。

- 工具链：cargo/rustc/aapt2/apksigner/zipalign/openjdk-21 全部来自 termux 包；
  `android.jar` + `debug.keystore` 拷自服务器（`~/kfm-na-toolchain/`）；
  d8 是纯 Java，bin/d8 是 wrapper——**核已换 R8 8.5.27**（BAR-078，
  2026-09-10：旧 d8.jar R8 8.2.2-dev 吃 javac 21 产物必 NPE，
  r8-8.5.27.jar 来自 Google 官方 maven，旧 jar 留档同目录）；.so 链接用
  Termux 原生 cc（宿主即 aarch64-linux-android，免 NDK 交叉链）
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

## 纪律（五门，全部 hard fail，commit-msg/pre-commit 钩子机械化执法）

1. **chain 全绿**：pre-commit 跑 `scripts/chain.sh`，红了提交不了。
2. **fix 必须带钉**：提交信息首行 `fix:`/`fix(范围):` 必须触及测试
   （tests/ 或 `*_test.rs`）。回归钉 = `#[test]` 名带 BAR 编号 +
   `docs/ledger/bugs.md` 登记一行。确属无需补钉（纯配置/文案/构建修复），
   提交信息**独立一行**写 `tests:na` 豁免。
3. **文档耦合**：提交触及 src/ 或 scripts/ 必须同时触及 docs/；
   确认无文档影响则提交信息**独立一行**写 `docs:na` 豁免。
   （独立行语法：防正文讨论豁免标记时 prose 字面串误认——kfmv4 2026-07-30 教训）
4. **仪器证据门（2026-09-17，BAR-098~106 三部曲裁决）**：`fix(渲染/动画/
   设置页/手势/平移):` 提交必须引 BAR-NNN 且该行含观测证据通道词
   （panend/panc/差分/遥测/实录/录屏/截屏/na-rec/epoch/帧账/redroid）——
   没有仪器定罪的修复 = 假设驱动修复（BAR-098/099「修了真 bug 但不是用户
   报的那个」）。纯逻辑病灶豁免：提交信息**独立一行**写 `instrument:na`。
   commit-msg 钩子机械执法（check-fix-instrument.sh，八言考题
   test-check-fix-instrument.sh 挂 chain 第 9 步）；门被摘/考题被删
   chain 第 3 步自守卫拦红。
5. **提交永不许 `-c core.hooksPath` 覆盖（2026-09-24，BAR-148）**：
   闸门在 `.githooks`（git config core.hooksPath），`-c core.hooksPath=...`
   一覆盖 pre-commit chain 整体旁路——cfg 盲区代码（android 目标才可见）
   的唯一兜底就是 chain 第 6 步 android check，拆闸 = 拆这唯一兜底。
   提交命令只许裸 `git commit`（需要免交互加 `</dev/null`，不许动 hooksPath）。

**提交精确 add（2026-09-26，双线共享工作树）**：提交前只许精确 add 本工单
涉及的文件，禁止 `git add -A` / `git add -u` 一扫了之——双线共享工作树，
在途活会互污染（实例：2026-09-26 工单①提交时一次 `git add -A` 把研究线
在途的 scripts/na-winstate-watch.sh 扫进主开发线提交，幸是成品）。机制面
挂账：`.githooks/pre-commit` 第 9 行为算变更哈希自带 `git add -A`，目前
仍会重 Stage 全树，待立项修。

## 观测先行条款（2026-09-17 立三条，2026-09-26 增第四条，C 档问题铁律）

1. **立案先指认仪器**：C 档（感官）问题立案第一步 = 指认
   `docs/active/排障手册.md` 观测矩阵的行；**没有行覆盖 = 先造仪器再修**，
   不许凭假设动手（BAR-098/099 弯路）。
2. **问「你能观测到吗」= 当场实跑**：用户问观测能力时，唯一合法应答是
   当场跑一遍仪器并贴原始输出——禁止口头回答「能/不能」。
3. **修复声明不许跳级**：状态词只许 `立案 → 定罪(仪器证据) → 已修待判
   → 用户终验结案`；钉绿≠修复，仪器判卷≠修复，**「结案」一词只许在用户
   肉眼终验后出现**（账本判卷列照此填写）。
4. **静默判过（2026-09-26 用户拍板）**：修复上机后用户无报障即视为
   终验通过（原话「目前来看应该是没有问题，如果有我会直接跟你说，没说
   就是默认没问题」）——账本判卷列标「用户终验通过（静默判过，日期）」
   结案；用户报障即翻案重开。

提交信息语言：中文，格式同 kfmv4（`类型(范围): 主题`，类型 feat/fix/chore/docs/test）。

## 跨线运维公约（2026-08-28 评审裁决，全线生效）

1. **重 IO 窗口制（2026-09-01 用户收紧，取代原 22:00-07:00 版）**：
   **大负载任务只准两处跑——手机端，或服务器 01:00-07:00 窗口**。
   大负载判据：连续占用多核 ≥1 分钟或引发可感 IO 压力（全量交叉编译/
   全量测试 chain/变异抽检/覆盖矩阵/批量索引均属之）。白天服务器只许
   轻量操作：fmt / git / ssh 探针 / push-so / 增量 check（秒级）。
   双甲不变：`ionice -c3 nice -n 19`（08-28 教训：nice 挡 CPU 不挡
   磁盘；09-01 事故：白天全量交叉编译 + 多 kimi 并行 → IO 挤兑复发，
   且 nice 10 不达标）。
   **2026-09-01 晚二次修订（内存升至 16G 后实测定档）**：实测——增量
   chain 全量 52s / 冷全量交叉编核 38s（nice19 双甲），PSI 三项峰值全
   0.00（空载 16G 下均无感）。档位放宽：**白天允许 chain 全量与单发
   release 编核（双甲照旧）**；仍守夜间/手机的只剩连续型重载（变异
   抽检、批量重编、cargo clean 级）；软规则：4 个 kimi 会话都满负荷时
   自觉错峰——事故真根因是并发挤兑，不是任务本身。
   **白天代码提交的 chain 闸 = 服务器本地（2026-09-23 用户拍板，全天
   服务器闸）**：pre-commit 全天跑 `scripts/chain.sh`（白天双甲，2026-09-01
   修订实测 52s/PSI 0.00 合规）；`scripts/chain-phone.sh` 降级为可选工具
   （手机端工具链可用时想双保险就跑，不再落 stamp、不再作闸）。动机：
   Termux 自主化——日常闸不依赖 Termux 资产，陌生设备开机的编译自举另立
   「编译舱」项（docs/active/编译舱.md）；夜间窗口（01:00-07:00）照旧，
   另有 01:43 定时任务兜底夜间重载与攒账提交。
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
  **变异回退禁用 `git checkout`**——未提交的工作区改动会一并陪葬
  （2026-09-19 parser_page.rs 整批重写实录）；改坏前先 `cp` 备份，
  验完用备份恢复。
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
- `docs/active/ai-presence.md` — AI 外显插件唯一设计文档（living；
  光球/浮层/AI页/输入栏+眼睛手嘴。动 AI 对话前必读）
- `docs/active/theme.md` — **KFM 主题宪法**（唯一来源：网格×像素总则/
  随机 accent/边框反转/双池/标签栏/三层目录语义。动任何 UI 视觉前先修宪）
- `docs/active/设置页.md` — 设置页（配置卡）内容设计（living；服务器
  配置/切换键/会话切换行为决策。动设置页前必读）
- `docs/active/工具卡.md` — 工具即卡设计（四个待拍板项定了才准实现）
- `docs/active/解析页.md` — **解析页两轴插件契约**（对象轴 Endpoint ×
  能力轴插件卡正交注册；卡链排布器；新终端/新功能适配律。动解析页
  结构/加卡/加终端前必读）
- `docs/active/编译舱.md` — **na 自持编译+热更链**（陌生设备自举：工具链
  包/9022 传输/私有目录落点/两版驱动/spike 三项。动设备侧编译前必读）
- `docs/ledger/bugs.md` — BAR 账本：每条修复登记编号/病灶/契约/钉位置
- `/root/kfmv4/docs/ledger/agent-inbox/` — **跨线评审信箱**（评审会话维护，2026-08-15 迁入 kfmv4 文档目录）：
  kfm-na 与 kfmv4 两线设计评审往来信 + 状态列；设计相关评审意见在此收/发。
  kfm-na 侧的单文件信箱（docs/ledger/inbox.md）同日退役，勿重建

## 复用的 kfmv4 资产（只读引用，不复制）

- `kfmv4/docs/active/眼睛与手.md` — 眼睛/手设计思想（NA 落地为网格眼睛 + 按键注入的手）
- kfmv4 服务端协议：terminal-pty ws、/ai/chat、文件树接口（对接时读 kfmv4 源码为准）
- **nz 可直抄规格索引（2026-09-11 nz 结项通报移交，维护态）**：`/root/kfmv4/nz/docs/`
  file-tree-v1-design.md（文件树+@引用全套，含 §七 实施定案：右滑仲裁/纯色行底 α 公式/长按复制三级链）/
  ai-chat-a1-design.md / config-pool-a2a-design.md / keybar-v3-state-machine.md /
  tmux-tabs-v2-state-machine.md / plugin-contract.md / dev-flow-case-001~006——
  参数全实证，Rust 本地化照抄得同一手感。避坑：注册 tmux 钩子必加
  NZ_NO_BELL_HOOK 式闸（临时实例禁注册；退出时钩子指向自己则摘除）

## 当前阶段

**阶段 3 现实主线（2026-09-26 重写，旧「L1 双会话 → L3 apt 生态」表述
过期——L1 早已落地，详见 bugs.md 与 state.md）**：①**QUIC 双通道自持**——
数据面 UDP 62633 主 / ssh 备（跳闸降级），反连 9022 已 QUIC 化（UDP
62694，M4 全里程碑判卷毕；BAR-157 认领循环陈尸憋死案修复上机实证通过）；
②**外置视口 v4 推流画布**——tmux -C 控制模式 %output 即时推流 + 隐藏画布
后台生长，v3 轮询降级保底（BAR-155/156 用户终验结案）；③**全会话预热池 +
滚动 2 倍增益**——用户真机判卷过关销账；④**BAR-145 仪器观察中**（tmux 列
点击漂移 = Android 输入边界坐标偏移，[touch]/[bake] 账常驻，观察期至
2026-10-02）。设计宪法：multi-end-layering.md（评审已批）。
