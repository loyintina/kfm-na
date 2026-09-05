# KFM-NA 交接页（state.md)

> 纪律：**每个里程碑必更新此页**。接手者冷启动顺序：本页 → bugs.md →
> AGENTS.md → chain.sh 跑一遍。**用户报 bug 第一读 docs/active/排障手册.md**
> (速查表:症状 → 工具 → 字段 → 判卷)。本页只写「现在进行时」,
> 历史功过在 bugs.md。

## 当前位置（2026-09-05)

- **装机首验反馈修复二批（2026-09-06）**：BAR-067 输入栏多行黑墙——
  栏带还原 kfmv4 rgba(18,18,26,.85) 半透（CHROME_BAND_ALPHA +
  blend_px 保 α），终端内容 15% 透出。两钉 + 变异咬红。
- **装机首验反馈修复（2026-09-05，用户实看两案）**：①BAR-066 光球
  黑块——加法 sprite 在透明画布上被条件 alpha 强转，修 = (α,E) 半透
  写出 + mark_chrome_alpha 扩版直通（三钉）；②面板曲线换 Material
  emphasized 族（0.2,0,0,1 / 0.3,0,0.8,0.15），350/250 照旧，求解器
  带解析锚点钉。棘轮：orb 维持 0、fx_ease 下调 0、三新模块入账
  （gles_present 9/glyph_atlas 1）。全量 470/0。
- **期 1 第 2 层 C 档落账（2026-09-05，d014d77，三仓推 ✓）**：AI 页
  文字接入图集管线（ras 48ms 病根拆除）+ GLES 双层合成（下层=键行+
  AI 面板底，上层=输入栏/光球，两层夹 AI 文字 GPU 实例；过渡帧
  scratch+blit 整条删除，FrameBuf 退役）。考题 7 道全绿（整帧逐像素
  对拍逮住 UV 假凶与 off_y 截断两案）；棘轮 termview 7 维持，新模块
  入账 gles_present=9 / glyph_atlas=1。设计全文 = gpu-render.md §十二。
  **装机包 kfm-na-1789600036.apk 已调起安装器，待用户点装**。实机
  验收判据：①AI 页稳态 gles-stage ras <10ms（原 48ms）②过渡动画无
  卡顿（scratch 已删）③双层合成无回归（键行被面板盖住、输入栏/光球
  浮在 AI 文字上、中英 CJK 混排正常）。上探次序（若仍超靶）：chrome
  脏 hash/带状上传（双层化后收益翻倍）→ chrome GPU 原生化（期 2）。
- **手机工具链适配教训（本轮 6 轮闸的学费，后续写码前置规避）**：
  新 rustc/clippy 不做的三件事——①跨 &mut 借用的闭包提升（slot_of
  要内联成临时）②&mut Box<dyn T> 参数（borrowed_box，用 &mut dyn T）
  ③两段 deref 强转（MutexGuard<Box<dyn>> → &dyn 须显式 &**）。死代码
  棘轮：专线化后无构造点的枚举变体（FrameBuf::Gles）会红，删净。
- **夜班一行（2026-09-05 01:43 窗）**:期 1 第 2 层 B 档落账
  f117ec3（GPU 图集管线接入主 app：终端网格归 GPU 实例绘制，CPU 只
  画 chrome 层；考题 3 道钉收集口契约，棘轮闸咬出顶带裁剪语义）。
  双推 github/gitee ✓；**phone 推送欠账**（手机隧道 01:10 起断，
  看门 60 轮未复，隧道通后 `git push phone master` 补上）。待装机
  实拍：na-shot 对拍 + 动画场景 <8ms 验收。
- **GPU 渲染立项（2026-09-04 晚，用户拍板）**：触发 = 用户实拍
  「AI 页面下落掉帧明显，成熟 app 都是 90 帧」；查机制确认非设置
  问题——stats 实测全帧 avg 40ms/max 63ms（~25fps 天花板），
  过渡帧每帧全量重光栅化（GPU 合成机制缺失）。立项书 =
  **docs/active/gpu-render.md**：期 0 复活尖刺先行（wgpu 25→30 /
  GameActivity / glow 三变量矩阵，对照 2026-08-13 本机 wgpu 暴毙
  判词逐项复跑，15 分钟存活零原生崩溃才许往下）；期 0 全灭则封档
  转 CPU 优化线（动画分层缓存+字形 LRU）。期 0 前不投 CPU 微优化
  （动画分层缓存例外，与 GPU 合成同构）。下一步 = 期 0 动工。
- **夜班一行（2026-09-04 01:43 窗）**:chain 手机端全绿;工作树
  干净无攒账;变异下批(评审点将 gate.rs alert_check/ring_push/
  parse_touch_line)46 针 45 抓/0 存活/1 废——三域零漏网,r2 通报
  已投评审信箱(kfm-na-gate-mutants-r2-report.md)。
- **ai-presence 期 0③ 真对话闭环+首验（2026-09-04 凌晨，b31c13f）**:
  消息状态核 ai_chat.rs + 换脑 DirectApiBrain（配置驱动，回退 echo
  兜底）+ AI 页真渲染（纯文本消息行）。三路 key 已推手机私有目录
  （scripts/deploy-ai-config.sh，key 不进 git）。**闸门后台自验一轮
  全通**:bar-inject 注入问答 → Kimi 真回复上 AI 页（na-shot 实拍
  实锤，脑装配日志确认非 echo）。首验抓出 BAR-059（思考流混入可见
  回复）当期修复——ai_chat 思考/正文分账，期 0 消息行只画正文，
  正文空思考归位（kfmv4 陷阱 10 同判据）;A 档两钉+变异双咬。
  同夜登记 BAR-058(am start 打活进程双杀，既存病待修，临时纪律:
  am start 仅限进程死透后，排障手册已改）。**装机复验已过**:
  5f3e0f7 核热更后闸门注入同题问答，AI 页实拍只剩干净回复，
  思考流零混入，BAR-059 结案。同场事故:BAR-060(陈核哨兵对默认
  拉取路径失效——本地副本 mtime=下载时刻恒通过，把 9-2 旧核推上机
  静默降级;修=拉取前比远端 mtime,修后同景拒推复现)。**待办**:
  用户手动终验（真机打字问答一轮）。
  下一步主线 = 期 0④ 浮层+手（ai-presence.md 唯一设计文档）。
- **输入栏复测批次:旧核假象破案+BAR-043/044 修复上机(2026-09-02)**:
  昨晚 23:38 灌的 hot 是旧核(缺 04c34be/ae68e27,连 scrollpx 解析臂都
  没有)——此前「修复没生效/拖动无效」全是旧核假象;判别指纹=旧核把
  scrollpx 当坏行拒绝。两案修复已上机(90da251,regress 3/3):
  ①BAR-043 尾锚→手动交接不播种(用户「第一下失效/比例失真」真根因:
  raw scroll_px 语义=距头顶偏移,尾锚期间恒 0,首笔 clamp 后瞬移文本
  头;修=交接首笔播种 max_eff 再叠加钳制)②BAR-044 bar-inject 空读
  竞态(writer cat> 截断窗口被值守撞见,applied=0 静默吞指令;修=空
  内容不消费不删文件)。考题 3 新 + 1 旧题按新契约改写(换算意图保留),
  全绿。**三场景装机判卷卡前台**:后台 stale lines 使闸门侧
  max_eff=0、滚动全部塌缩到头(BAR-039 同族,只影响后台注入判卷,
  真指无碍);termux uid 无 INJECT_EVENTS 点不亮屏。待用户点亮手机
  后跑:真拖 200px=1:1 / 点按柄在该行行底 / 柄两相位稳显。
- **na-tunnel 隧道韧性工具(2026-09-02,9f9998e)**:跨隧道动作统一
  入口(probe/status/wait/ssh/scp/shot)——带预算退避重试、断连史
  (~/.na-tunnel/history.log)、截图 mtime 新鲜度校验;ssh 只对连接层
  失败重试,不拿重试掩盖真错。安全红线:只碰 127.0.0.1 隧道端口,
  不新增公网暴露面(用户定案)。排障手册前置一已改写(两种死法分流:
  TCP 不通=等自愈,通但 reset=半开,8022 am start 拉回)。
- **ai-presence 期 0 组件一落地（2026-08-30）**：AI 外显状态核+光球。
  `src/ai_presence.rs` 两布尔状态核（ai_running×page；浮层=f(running,
  dismissed,驻留3000ms)；球位/pressed/per-run dismissed；时钟注入零墙钟）
  + cordis-na 插件（provides AiPresenceState，disabled 一键关默认开）
  + 雾球 sprite 四态 alpha 硬切（kfmv4 base.scss:23 配方）+ AI 页占位空壳
  + 触摸路由（球命中优先终端/拖动钳制/长按=fake_run debug 钩子）
  + 观测闭环：stats `ai_*` 六格 + 通道十 orb-inject（na-orb.sh，回执
  orb-inject-res）+ na-shot 倒帧装帧含球（gate dump_now 组件一修订）。
  考题 tests/ai_presence_spec.rs 27 道全绿；热更实拍四连（终端页球/
  drag 移位/灯亮/AI 页占位）通过。后续组件②③④（浮层/输入栏/合成网格）
  未动工，设计以 ai-presence.md v3 为准。
- **探针③ PIN-remote-active-death 落地（2026-08-28）**：活跃=远程
  死亡自动重孵契约入考官——switch-req 切远程 + ss -K 掐 ws,判五步
  （deaths+1/active 保持 remote/横幅/新 shell 回显/local 不受扰）。
  可重复无锁存（活跃死亡有自动重孵）。运行代价=弹一次远程,注释
  醒目。**故障注入探针族至此盖满两种传输×两种会话态**:本地死亡
  (rehatch)/待机远程死亡(standby-death)/活跃远程死亡(remote-active-
  death),回归套件九卷。

- **通道九 switch-req 落地（2026-08-28，本提交）**：会话切换注入
  通道——Ctrl-] 缺口（观测矩阵最后一块登记在案的空白）闭合。
  gate SWITCH_IN 标志位+值守 switch_req_check+App about_to_wait
  取走调 switch_session（与 Ctrl-] 完全同入口，遥控器非旁门）。
  考官 PIN-switch（往返判卷）。解锁探针③（活跃=远程死亡自动重孵）。
  回归套件八卷。

- **故障注入探针② PIN-standby-death 落地（2026-08-28）**：远程会话
  死亡记账契约入考官——ss -K 掐服务器侧 8021 ws（=网络断,服务端
  零接触）,判五步:deaths+1/活跃不受扰/ws 自愈重连/一生一发锁存
  （marker=na.pid,重启重新武装）/阴性对照（杀 tmux attach 客户端
  不产生死亡事件——death 定义=ws 断开,阴性知识入 §十五）。首跑绿
  +锁存跳过验证过。回归套件七卷。

- **跨线运维公约生效+gen 补丁 na 代改落地（2026-08-28）**：三公约
  （重 IO 窗口制/push 遇阻分流/投影自动回写）评审全批，na AGENTS.md
  与调试闸门 §十七 已收录。gen-agent-inbox.mjs 补投影回写（只准替换
  「N 封信」数字）+考题 test-gen-agent-inbox-projections.mjs 四断言
  （字节安全/计数统一/漂移检出/表生成），生产实证:gen 真跑自动同步
  三处计数、机检绿。同批:侦察#3 修订 v3（九行对拍清单+tie-break 口
  径+5:3 落预判形状）投复核。

- **决策轨迹三样本战役收官（2026-08-28）**：侦察#3 nz ranger
  runaway 跨线标注完成（迁移性通过，14 类型零新建）+ schema v2
  合稿生效（trace-schema-v2.md:13 类/四分类转移计数/三链合表）。
  评审收官裁决：链型命题记「方向验证」不记「证实」（n=3+链型事后
  贴标，事前化判据留 v3）；烂尾链「无尺裸奔型」预测批准挂起（模型
  自觉虚高+仪器缺位，下次遇烂尾链优先标）。产物全在 kfmv4 仓
  experiments/dsh-na/na/ 与信箱（四信往返）。

- **电耗专题开局:过夜/昼间画像采集上线（2026-08-28，本提交）**：
  scripts/probe-overnight-power.sh 双源对账（电池
  termux-battery-status:电量/电流µA/温度 + na stats:cpu_jiffies/
  rss/pump/deaths），300s 一拍落 overnight-power-MMDD.log；GAP 行
  =8024 失联窗口（Doze 冻结）本身即数据；结尾 drain 摘要算 jiffies
  差分。昼间 12h 档已挂（今日活跃画像），过夜档待挂。首测读数：
  na 后台 CPU ≈1.8 jiffies/s（单核 2% 量纲）。**首验自抓一处**：
  stats 键名误写 uptime_ms（实为 uptime=）致采样恒 GAP，修正后
  双源通。

- **考卷覆盖矩阵棘轮闸上线（2026-08-27，本提交）**：自我测试缺口④
  ——scripts/check/check-spec-coverage.sh 生成「功能×考题」对照表
  （23 模块入账，落 docs/ledger/test-coverage-matrix.md gen 区），
  基线棘轮只许降；chain 第 10/11 步常驻。首跑读法：零覆盖模块清一色
  A 档纯逻辑=纪律健康；缺口集中 gate.rs 胶水与 report/trace 接线层
  =下一批考题打点图。边界（词级近似/函数级豁免不做/胶水由真机考官
  判卷）写进 §十六。

- **C4 宽字符契约考题 na 半边落地（2026-08-27，本提交）**：按评审
  指引换判据——同串直喂网格断光标推进列数（cursor_col 新访问器），
  契约串表四例对齐 term-contract.md §C4（A中A=4/中中=4/E0B0=1/
  中文A=5）+ 劈格防御题（行尾半格不拆字）。教训焊注释：经 PTY/
  shell 测宽度混入 ZLE 回显，必须直喂网格。53 题全绿，待 nz 同表
  对拍后回贴期望值。
- **故障注入探针族开张（2026-08-27，本提交）**：自我测试缺口③
  第一枚——PIN-rehatch：exit 字节注入合法杀会话 → session_deaths
  计数 +1 → 活跃方自动重孵横幅「已重连 = 新 shell」→ 新 shell
  回显活着，四步判卷全闸门内零核心改动。配套调试闸门.md §十五
  「沙箱观测三铁律」（pgrep -f 必自匹配禁用；/proc 对 sshd 不可见，
  扫不到 ≠ 死了——SELinux 同 uid 异类别隔离实锤；唯一可靠活性判据
  = kill -0 $(cat na.pid)）。回归套件升六卷全绿。恢复路径自此有
  对抗性考题：重孵失灵自动进报表。


- **真机回归套件通车（2026-08-27，本提交）**：自我测试缺口①落地
  ——已结案案卷的判卷法固化成真机考官，案卷从档案变成站岗的。
  scripts/cases/*-accept.sh 五卷（BAR-040/PIN-boot/PIN-pump/
  PIN-touch/PIN-signal)+ 共享库 scripts/lib/gate-lib.sh + runner
  na-regress.sh(「重启类」自动排尾，报表 + exit 码）+ na-push-so.sh
  第⑥步冒烟挂钩（SKIP_RESTART 直判当前 boot)。纪律升级：结案
  硬条件加「判卷法能自动化的必须固化成 accept 脚本」。首验自抓
  三案（seq 50 零 scrollback 假空转/熄屏 boot 误报/sshd 晚起
  8024 误伤），全套件真机 5/5 绿、冒烟路径 3/3 绿。全文
  docs/active/调试闸门.md §十四。

- **scrollback 显式钉值（2026-08-27，本提交）**：两线横向审计漂移 #1
  用户拍板——各线显式钉值，na 保持 10000 行。termview.rs
  `SCROLLBACK_LINES` 常量（此前裸用 `Config::default()` 纯继承上游
  默认，审计实锤 alacritty_terminal 0.25 原值 10000),`history_size()`
  公开，容量考题 spec_scrollback_容量钉死显式值（灌超帽输出实测
  正好压帽，退回裸默认必红）。顺手清 keymap.rs 粘滞注释旧口径
  （状态机早迁 Rust keybar.rs,「Java 侧」是迁移前的话）。通报信
  kfmv4-audit-term-parity-na-landing.md 已落 kfmv4 仓信箱。

- **自观测第四块三件套落地（2026-08-27，本提交）**:①crash.rs
  信号级坠机记录——panic 钩子够不着的 SIGSEGV/SIGBUS/SIGILL/
  SIGABRT 由 last-gasp handler 写 `SIGNAL sig=N addr=0x...` 进
  panic.log 再交还系统，SIGURG 为装机判卷探针（写行后继续活）;
  ②异常自报告警——值守每 3s 过三规则（帧耗新峰值/RSS 绝线或窗
  净涨/会话死亡窗新增），越线自动 report 进 trace 环 + field-
  reports，方向从「人来查」补了「异常找人」;③stats 历史水位环
  ——每 30s 一张快照、帽 48（≈24 分钟回望），通道九 history-req
  + scripts/na-history.sh，趋势类判卷尺。考题 tests/
  selfwatch_spec.rs 9 道全绿，全文 docs/active/调试闸门.md §十三。
  装机判卷法：URG 探针验①、na-history.sh 累积验③、②只验
  「不误报」（不主动制造异常，诚实标注）。
  **实证（f949730 热更后）**:kill -URG → panic.log 落
  `SIGNAL sig=23 addr=0x...` 且进程活着；水位环 30s 一张正常
  累积（rss 146-150MB、draw 峰值 16ms、零死亡）；告警侧干净
  无误报。**首验自抓一案**：初版探针用 SIGUSR1，装机实测被
  ART 认领（堆转储/GC,libsigchain 不下传用户 handler),panic.log
  一字未落——改 SIGURG 修复并钉 PROBE_SIG 常量防回退。

- **交接盲测终审通过，正式闭案（2026-08-27)**：评审二轮答卷
  (kfm-na-blind-test-round2-verdict.md)——五洞全实证补齐（含
  na-case 脚手架实弹探针，模板态 exit=1 契约兑现），三道排障题
  按手册走位真跑全通（注入对照/touch-in 新通道/trace 归因）。
  **排障文档体系认定为接手 agent 冷启动级**。微瑕顺手清：
  na-case.sh heredoc 转义符直出（\" 残留），已修+实弹验证
  （BAR-999 空弹，产物干净后清场）。
  另：评审角色转两线审计（kfmv4-review-role-shift-notice.md),
  na 侧无实质变化，结案质量自扛的纪律不变。

- **触摸注入通道通车（2026-08-27，本提交）**：观测矩阵输入侧空格
  销案。①gate 通道八 touch-in——脚本行协议（tap/down/move/up/
  scroll/sleep），解析器纯函数 A 档考题 tests/touch_spec.rs 4 道；
  ②android_app 原 WindowEvent::Touch 臂 278 行机械搬家成
  handle_touch，真手指与注入**同一入口**（行界切片+词边界断言+
  编译验证，零逻辑改动）;③App 侧 drain_touch_in 执行器（Sleep
  节拍挂起、Scroll 按真实格高展开成滑动序列）;④scripts/
  na-touch.sh（原子写协议同 na-type）;⑤stats 添 touches 字段。
  「截图→滚动→再截图」路径自此全闸内化，手势类 bug 不再需要
  用户当手。全文 docs/active/调试闸门.md §十二。**首验自抓一案**
  (c59b637):scroll 语法糖 window 早退在挂起态静默空转（裸事件反通），
  几何改取 last_grid——挂起态实证过：scroll 8 → 读屏/拍图首行
  132（分毫不差）,scroll -8 回底 140，双路径交叉一致。同批：chain
  第 3 步 stats 字段咬合闸（评审裁决建议落地，别名须登记）。

- **交接盲测一轮答卷落地（2026-08-27，本提交）**：评审 agent 冷读
  判卷——三道排障题（输入无反应/后台黑屏/启动慢）经典走位全通，
  结论「经典案例能闭环；趋势+竞态两类缺判据」。洞清单五条全补：
  ①②排障手册加「环境与前置」段（脚本在服务器跑/8024 隧道/
  探针钥匙/闸门目录绝对路径/field-reports 位置）;③§六扩趋势
  采样法（连拍求差速率 + 多次冷启动对比，趋势案结案须前后斜率
  对比）;④观测矩阵时序侧空格补现行判据（边界加密探针或遥控
  判卷脚本二选一，不许裸修）;⑤同尺复验机械面（案卷判卷法栏
  必须是可复跑命令序列，na-case.sh 模板退出码=判卷结果，模板态
  恒 exit 1 防空脚本误判结案）。原信按 MECH-FLOW-12 改名
  kfm-na-handover-blind-submission.md。回函请评审复测二轮
  (kfm-na-handover-blind-response.md)。

- **排障闭环工程化：结晶条款落地（2026-08-27，本提交）**——把
  「bug → 新观测资产」从自觉变纪律：①docs/active/排障手册.md
  （操作层 playbook：速查表 + 八条症状走位，每条带判卷法）;
  ②调试闸门.md §十一——闭环六步（定位→复现→找因→测试→送机→
  同尺复验）+ 逃逸条款（看不见/一修不好/复现不了的 bug 结案必须
  留永久资产：新观测点/回归考题/可复用脚本）+ 观测矩阵（五侧
  分类账，空格 = 已知盲区：触摸注入、跨进程竞态、电耗画像）;
  ③bugs.md 案卷区——逃逸 bug 开六栏案卷（现象/复现序列/盲区/
  新观测点/考题/判卷法）,「盲区」栏是灵魂；④scripts/na-case.sh
  开案脚手架（收尸+案卷骨架+复现模板一条命令）;⑤scripts/README.md
  脚本索引；⑥AGENTS.md 文档地图挂排障手册。**待判卷：评审 agent
  冷读盲测**（只凭仓库答排障题，答不上来 = 文档的洞，信在
  kfmv4/docs/ledger/agent-inbox/kfm-na-handover-blind-test.md)。

- **自观测第三块：资源画像 + 一键收尸包（2026-08-27，本提交）**:
  stats 从「计数」升「画像」——①帧耗时（draw_frame 全帧计时，
  `draw_avg_ms`/`draw_max_ms`);②CPU/内存（读 /proc/self/stat 的
  utime+stime jiffies 与 status 的 VmRSS，解析器纯函数钉死，失败
  静默给 0);③会话分桶吞吐（泵 rec 回调按名记账，
  bytes_local/remote/other);④会话死亡计数（on_slot_dead 每次
  +1,重连频度温度计）。②scripts/na-autopsy.sh 一键收尸包：触发
  trace/stats 落盘 → panic.log/panic-trace.txt/loop-stall.log/
  trace.txt/stats-res/loader-pick/flight-rec.bin 全量拉回
  autopsy/<时间戳>/ → 打印摘要。考题 tests/trace_spec.rs 增至
  6 道。全文 docs/active/调试闸门.md §十。

## 历史位置（2026-08-26)

- **BAR-039:热更裂脑案——IME 绑错库实例(2026-08-26,本提交)**:
  loader 分离后 Java loadLibrary("kfm_na") 绑到包内捆绑核心,与 hot/
  运行核心两个实例,IME 落字进副本队列,输入全灭(用户实拍:键盘
  弹得出字进不来,commit 计数恒 0)。修复:loader 导出三个 IME JNI
  符号 tail-call 进当前核心,Java 焊死 loadLibrary("na_loader")。
  连带提交 818370c 泵降频(WaitUntil 4ms+条件 redraw)与 03c0244
  BAR-038(exit 换干净线程,两轮重跑让位实拍 panic.log 零新增)。
  本次改动沾了 Java/dex 与 loader——热更盖不住,要重打 APK 过一回
  安装器,之后 IME 转发层焊死,核心继续热更。
- **自观测第二块：行踪环 + 运行时统计（2026-08-26，本提交）**:
  ①trace ring(src/trace.rs)——report 流本地滚动副本，咽喉单点
  tap(report/report_sync/... 三函数旁路入环,调用点零改动),环帽
  256,两类周期心跳 should_trace 滤掉;活着 trace-req→trace.txt
  随查(na-trace.sh),panic 钩子自动落末 64 行进 panic-trace.txt
  (覆写制)。②stats-req 通道——帧/泵/闸门计数全 AtomicU64 热路径
  加一,stats-req→stats-res 快照随查(na-stats.sh),key=value 钉死。
  SessionRouter 顺手补 names()。考题 tests/trace_spec.rs 4 道全绿。
  全文 docs/active/调试闸门.md §九。**装机实证过（4da2bc5，热更全
  自动闭环：推送→restart-req→拉回→判卷，零手动）**:na-trace 拉出
  真实事件流（boot 时序/ws 握手分段耗时/tofu 目击/挂起档）,
  na-stats 首拍即抓货——泵空转 ~57k 次/s（记挂单①）。
- **热更新闭环收官：自动重启通道 + BAR-037 重跑防御（2026-08-26，
  本提交）**：①gate 第五条通道 restart-req——值守线程见触发文件即
  遗言直报 + `exit(0)`，不经过事件循环（挂起态也杀得死）；②
  BAR-037 防御——android_main 极早期静态 ANDROID_MAIN_RAN 检测，
  二次进门遗言 + `exit(0)` 让位（ROM 冻结打断 exit(0) 的窗口期，
  拉回即重跑的 panic 病根拔除）；③scripts/na-restart.sh 一键闭环：
  触发→等 8024 断连→am start 拉回（冻结案补第二次）→等新 boot
  报告→na-ping 判卷；na-push-so.sh 默认联动自动重启（--no-restart
  可关）。边界诚实写进脚本注释：熄屏/锁屏 am start 可能被系统挡，
  那时提示手动点图标。**装机实证过（cee7cf5，2026-08-26）**：热更推
  核心 6MB → force-stop+拉回（旧核心无 restart-req，最后一趟手动）
  → 新核心上位后再跑 na-restart.sh 全闭环：restart-req 触发 →
  8024 断连 → am start 拉回 → 新 boot 行 → ping 应答，四件套全对，
  panic.log 无新行（BAR-037 旧案两行即全部）。
- **飞行记录仪落地（2026-08-25，本提交）**：自观测路线图第一块——
  确定性回放。泵新增 rec 见证回调（Output 先过记录仪再路由，全部
  会话带名记录，不只活跃方）;resize 在 apply_window_size 落点旁
  tap 一条（网格共享，回放时全应用）。格式 `KFMREC01\n` + 定长头
  变长体记录流（kind 1=Output/2=Resize，未知 kind 按 len 跳过、
  截尾容忍）,rec_encode/rec_decode_all/rec_compact 三纯函数出题
  （tests/rec_spec.rs 5 道，含超帽保新丢旧）。记录仪线程独立落盘
  `$PREFIX/tmp/flight-rec.bin`（帽 2MB，入队即返回、静默丢——观测
  通道不许反咬业务）。host 回放器 `src/bin/na-replay.rs`：按名过滤
  喂 TermView 真渲染，打印末屏文本+统计。一键 `scripts/na-replay.sh`
  = scp 拉回 + 本地回放。判卷法：回放末屏须与 na-text.sh 读屏一致。
  **终判卷过（2026-08-25 装机实拍，逐行 diff 完全一致）**；判卷过程
  自抓三虫：BAR-033（开局横幅绕泵未入带）、BAR-034（记录带跨重启
  追加→开机轮换 .prev）、BAR-035（回放器起手几何与真机不同胚→
  共享常量 BOOT_COLS×BOOT_ROWS)。设计全文 docs/active/调试闸门.md §六。
- **死亡观测落地（2026-08-25，本提交）**：自观测第二块——「它是怎么死
  的」。①panic 落盘：install_panic_hook 三件套（闸门目录 panic.log
  追加为主、report 直报为辅、链默认钩子），行格式钉死一行一案，
  线程 panic 也收（替换旧的「仅异步直报」版——进程死了队列同归于尽
  收不到）。②loop 看门狗：about_to_wait 每圈盖心跳戳（忙轮询泵），
  值守线程比龄期，>3s 卡死/复活迁移写 loop-stall.log + report;
  ping-req→ping-res 随查（scripts/na-ping.sh）。纯被动，不用 proxy
  （挂起态送不达是实锤过的弯路）。考题 gate_spec 3 道（卡死边界/
  panic 行格式/多行压单行）。全文 docs/active/调试闸门.md §八。
- **热更新：loader/核心分离（2026-08-26，本提交）**：manifest lib_name
  改指焊死的加载壳 libna_loader.so（新 crate crates/na-loader，只做
  选择+转发），核心 libkfm_na.so 可被 {files}/hot/ 下的热更件替换——
  改码不再重打 APK 过安装器：na-push-so.sh 推进沙箱 → 重启即生效。
  回落纪律：热更缺失/dlopen 失败自动回落包内捆绑核心，每次选择落档
  loader-pick（跑的是谁必须可查）。考题 tests/loader_spec.rs 3 道。
  这是插件宿主胚胎：壳↔核心只隔 dlopen+固定入口符号。全文
  docs/active/热更新.md。自动重启通道（restart-req）与 BAR-037
  重跑防御押后做。**装机实证过（d7f9bab,2026-08-26）**：首启
  loader-pick = `pick=bundled why=无热更核心`；na-push-so.sh 推核心
  6MB 入 hot/，划掉重开 = `pick=hot`，读屏/看门狗功能全正常。
- **L2 自远程闸门通车（2026-08-23,等长二进制改写方案）**:服务器
  `ssh -p 8024 localhost`(探针钥匙 /root/.ssh/na_probe_key)经 kalo 反隧
  直入 na 沙箱 sshd(回环only/公钥only)——agent 从此能直接操作 na 终端,
  不再靠用户手动测试转述。根治术:`com.termux` 与 `dev.kfm.na` 同 10 字符,
  sed 等长直打 ELF 不挪偏移,sshd 20+ 处焊死路径一遍全愈;已挪进
  overlay-pack.sh 构建侧(0b85924,考题钉等长铁律)。证伪留档:
  LD_PRELOAD(bionic 不让插 libc 符号)、proot(seccomp 沙箱里 ptrace
  失效)、SetEnv LD_LIBRARY_PATH(活不到 exec)——三条都别再试。
  na 主动外传道 = kfm-push(scp 推 Termux ~/w/na-inbox/,na 写不进共享
  存储根 EPERM 实拍)。全文:l2-overlay.md §8/§9;简报已投信箱。
- **BAR-029 前台服务保活封案(2026-08-23,bbcb5f5)**:na 退后台被
  cached-app 冻结器冻僵(sshd 冬眠、8024 断流)→ KfmKeepAliveService
  前台服务(IMPORTANCE_MIN 常驻通知 + partial wake lock,Termux 同款)。
  实拍判卷:修复前后台 10s 必冻 → 修复后 600s 六十探零断流。
  判卷脚本 scripts/test-bg-survival.sh(am start 遥控前后台 + 8024 探针,
  全自动)。**此条推翻 8-19「不做常驻保活」旧共识**——8024 闸门与会话
  现场需要 na 退后台存活,电耗代价用户已拍板接受。
- **zsh/omz + powerline 字体战役(2026-08-23,BAR-027/028/032)**:
  omz 落私有区(FUSE 家目录不支持符号链接),.bashrc 换手行进 zsh;
  agnoster 箭头三案连破——补丁表扩容+借字形(f58433b)→ 全角压半格
  (3d71293)→ 根因定案:FusionPixel 上游 E0B0 是「色块+C形镂空」装饰
  设计(双光栅器复现),烘焙时换合成实心阶梯三角(4cb652e,像素级考题
  spec_bar032)。实拍判卷过。
- **挂单**:①~~泵空转~~(2026-08-26 当晚销案:WaitUntil 4ms 节拍 +
  有脏才 redraw 双闸,长按计时挪 about_to_wait;判卷 = stats 前后
  对照,proxy 全事件驱动彻底版留电耗专题)。
  ②BAR-030(~~长行不换行+粘贴撕碎~~ 已销案 2026-08-24:用户
  实拍不复现,四层检查全正常,疑为 zsh 残局期产物)、BAR-031(已封案
  ea5ddec:kfm-pkg 原子性三件套,装机实拍过)。
- **画面回传 na-shot(2026-08-24,本提交)**:8024 闸门配套调试通道——
  na 画面是 Rust 软渲染,帧缓冲本来就在自己手里,倒出来就是截图(不需
  Android 截屏权限)。`touch $PREFIX/tmp/shot-req` → 渲染循环下一帧倒
  shot.rgb+shot.dim → scp 拉回 PIL 转 PNG。一键 scripts/na-shot.sh
  (--watch 循环=近同步直播)。软键盘/系统弹窗不在帧缓冲里,拍不到
  (预期内)。考题 tests/screendump_spec.rs 5 道。
  **后台倒帧(同日二轮)**:第一版想靠 EventLoopProxy 锤醒 about_to_wait
  离屏倒帧,实拍证伪——挂起态下 proxy 事件送达但 winit 不跑
  about_to_wait(循环心跳停跳、触发文件晾着);proxy 只在循环活着时
  叫得醒,blackout 案的锤有效是因为那时循环本身在跑。正解:倒帧全
  收进独立值守线程(spawn_dump_watcher,300ms 轮询触发文件),终端改
  Arc<Mutex<Box<dyn TermEmu>>> 共享,值守线程锁终端离屏光栅化(只画
  网格本体,快捷键行/放大镜是 UI 装帧不进后台视野);draw_frame 每帧
  note_frame_size 记账供后台取尺寸。单消费者,前台后台一个样。
- **调试闸门三件套闭环(2026-08-24 午后,本提交)**:screendump.rs 更名
  gate.rs,从「画面回传」扩为完整调试闸门——看见(shot)+读懂(text)+
  动手(keys)三条文件协议通道,值守线程一轮三查。新增:①text-req →
  dump_text 导当前视野纯文本(TermEmu 新 trait 方法,网格眼睛胚胎,
  滚动跟视野走);②keys-in → 裸字节注入活跃会话(远程键盘,手的胚胎,
  退后台也能注),SessionRouter 随之 Arc 化共享(15 处调用点改
  handle+lock,装配收拢 install_router 一次登记永新鲜)。限制(有意):
  Ctrl-] 会话切换/修饰键粘滞是 UI 层,闸门不过。脚本:na-text.sh /
  na-type.sh(半写防护 = .new+mv 原子协议)。设计参考面:
  docs/active/调试闸门.md。考题:gate_spec 6 道 + termview dump_text
  题。用途:此后 na 调试 agent 自助闭环,不再要用户当手和眼。
- **keys-in 零执行破案(2026-08-24,5260c1e)**:装机后注入无效——
  回执上报(send_checked,eb798cd)先证 drain/登记/send/writer 四环
  全活,读屏见四条命令带字面 `\r` 堆在提示符上:病在 na-type.sh
  `printf '%s'` 把转义当字面发。改 `%b` 后端到端实证(echo 落盘真
  执行)。教训入档:**「通道活了」≠「字节对了」,判案判到最后一厘米**。
  钉:scripts/test-na-type-bytes.sh(假 ssh 注 PATH 判字节)。
- **会话泵:Output 数据面分家(2026-08-24,本提交)**:旧制 Output 走
  SessionEvent 进 UI 事件队列,挂起态不抽干 → 网格冻结,闸门读屏读
  旧画面。新制 `gate::SessionPump` 是全部会话入向唯一消费者:活跃方
  Output 谁 pump 谁喂共享终端(UI 每圈 + 值守 300ms 双 caller,前台
  零延迟后台不冻结);待机方进 replay 缓存(帽 256KB 丢最旧,切换时
  take_replay 补屏);控制事件按名带进控制队列,壳每圈取走记健康账
  (语义不变)。壳的 event_rx/standby/standby_buf 三字段全删,切换不
  再换入向槽,重连 = pump_register 同名登记(自清遗物)。考题
  tests/session_pump_spec.rs 7 道。锁序追加 pump→term 单方向。
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
  （光球/卡片堆）后移到核心层分层落定之后。~~不为本地 shell 做常驻保活~~
  **已翻案（2026-08-23，用户拍板）**:BAR-029 前台服务保活已落地——
  8024 闸门/会话现场需要 na 退后台存活，电耗代价接受。
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
- **第三备份 = GitHub（2026-08-26 接通，同日公開化）**:remote `github` →
  https://github.com/loyintina/kfm-na.git(**public**，跨机传输用），凭据走
  ~/.git-credentials 的 loyintina PAT（credential.helper=store 已配）。
  三份拷贝：服务器（事实来源）/ 手机 / GitHub。提交后顺手
  `git push github master`。公开化前已过泄密扫描（无私钥/token/口令；
  商业字体有 chain.sh 第 1 步防泄漏闸 + gitignore 双保险）。注意：docs
  里有服务器 IP 与端口布局，属用户知情同意的公开面（当晚实表已脱敏）。
- **隧道生命线：kalo v3.13（2026-08-26 晚，手机 ~/bin/kalo，备份
  kalo.bak-v3.12）**：断线根因定案 = 手机 CGNAT 公网 IP 漂移（auth.log
  实证 7 分钟内 39.144.207.45→.203.214→.203.47 三换），漂移即黑洞、
  服务器留僵尸监听挡新 -R 绑定；autossh 指数退避曾滚到 128s+ 拖慢
  自愈。v3.13 对症：GATETIME=0 关退避 + POLL=15s 匀速重试、探死
  15×2=30s、`-E ~/kalo-ssh.log` 记 ssh 死因（autossh 日志只记 exit
  255）、隧道不通且 autossh 活着时主动重启它、判卷放缓 10×3s。
  实证：完整拆建循环（kalo -x && kalo）秒级恢复。看门狗 = crond
  每 5 分钟 kalo-watchdog.sh，断线最坏恢复时长 ≈ 5 分钟（通常 <1 分钟）。

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

## 2026-09-01 · 重载窗口制收紧（用户拍板，第 3 次 IO 事故后）

大负载任务（chain 全量/交叉编译/变异/覆盖）**白天只准手机端跑**，服务器
只留 **01:00-07:00** 窗口（原 22:00 起点作废）；双甲 ionice -c3 nice -n 19
（此前 nice 10 不达标）。白天提交闸 = `scripts/chain-phone.sh`（补丁推手机
跑全量 chain → stamp，pre-commit 白天校验哈希咬合+6h）；夜间 01:43 定时
任务兜底重载与攒账提交。当日事故复盘：白天全量交叉编译 + 4 kimi 并行 →
IO 挤兑（some=0.93/full=0.76），编译进程被外部冻结，负载自然回落。

### 2026-09-01 补记 · 取包点清旧包
取包点攒多个 APK 导致旧包误装（弹「已安装更高版本」）。deploy-phone.sh
部署后自动清到最新 2 个。同批：计数器抬至 1789300000 重打包（组合态批
8d28412 的装机载体）。
