# AI 外显插件（ai-presence）设计文档

> 这是什么：AI 外显系统的唯一插件文档——设计讨论逐条沉淀于此，实现以本文为准。
> 性质：插件设计文档（living：需要什么讨论什么；用户拍板一条，待定项转正进 §八）。
> 关联：`ui-base.md`（UI 契约：硬切基座/采样缝/分界表）/ `立项.md` /
> `term-contract.md`（kfmv4，网格契约）/ cordis-na `fiber.rs`（Plugin 面）。
> 状态：期 0 组件一动工（2026-08-30 v3：两布尔模型取代三档旋钮，雾球样式/拖动/遥控/观测闭环入档 D7-D9）。
> 当前期：期 0 组件一（状态核+光球）。

## 一、插件定位

- **名字**：`ai-presence`，cordis-na 编译期插件，全程热更覆盖（不碰 Java 皮/manifest）。
- **职责**：AI 的「外显」——光球（AI 页切换钮+运行指示灯）+ 运行时浮层 + AI 全屏页
  + 全局输入栏，以及眼睛/手/嘴三件套的宿主。
- **边界**：不做「脑」——推理与工具编排在服务器/agent 侧；本插件是外显+通道。
- **分层（宪法回应「核心还是壳」）**：插件 = 状态与数据（消息网格/模式状态/协议流）；
  壳 = 绘制与命中（光球/浮层/输入栏与 keybar 同级的 chrome 绘制）。两边都在热更面内。

## 二、形态（三层结构，定稿）

- **会话层（分页）**：本地 / 远程 / AI——AI 页 = 全屏纯历史网格：**合成网格**
  （经 `dyn TermEmuFactory` 建仿真实例，把 markdown-lite 渲染成带样式文本喂进去），
  scrollback 吃 term-contract 的 10000 行钉。
- **常驻 chrome 一 · 底部全局输入栏+发送按钮**：任何会话下都在，压底紧贴键盘，
  快捷键行上移一层。聚焦时键盘按键全归输入栏，Enter=发送。
- **常驻 chrome 二 · 光球 = AI 页切换钮 + 运行指示灯**（D7 取代三档旋钮）：
  点击 = 终端页 ↔ AI 全屏页往返；**可拖动换位**（D9）；闲=暗、运行=亮
  （静态换图报信，零常动帧）；**右缘中部偏下默认位**（D6）。
- **运行时浮层**（D7：AI 在跑 → 在；停 → 走）：**顶部、高=屏 25%（约 8-10 行）、
  80% 不透明暗底、直角**（D5），消息尾随上滚 = **同一聊天网格的第二视口**
  （尾随锁定，零状态复制）；v1 纯显示、触摸穿透；生命周期规则见 §五。
- **手** = 按键注入进来源会话（逐字符天然可见）；**工具调用** v1 = 浮层内样式块
  （🔧 行），独立悬浮框 v1.5；命令出现在同一终端。
- **眼睛** = 活跃会话视口文本快照附上下文（复用读屏路径），v1 承重件。

## 三、插件面（cordis-na 契约视角）

### 三层模型（壳源分离；推广注记：tmux-bridge/文件树照此办理）

- **机制层（共享壳，一个就够）**：源无关逻辑——外显 UI、眼睛、手的注入机制、
  渲染。不随数据源拆分。
- **数据源层（一源一插件一键）**：脑 = `direct-api-brain`（本地配 key 直连服务商——
  **默认本地脑，期 0② 主力**）/ `echo-brain`（考题夹具）/ `server-brain`（kfmv4
  /ai/chat 走隧道——**未来「服务器空间」数据源，非地基**，D11），三个插件
  三个服务键；先例 = 终端连接（`LocalPtyFactory` / `TermFactory`）。粒度与回退纪律
  绝配：某源出 bug，disabled 它一个，其余照常。
- **路由层（薄）**：按会话所属空间把能力查询路由到对应源。

**需新建的本地壳只有三个**：本地脑（echo→direct-api）、本地 tmux 适配器、
本地文件树适配器；终端本地壳已有（local PTY）；眼/手/外显天然源无关不拆。

**本地壳先行（D11 再确认，曾险些违背）**：任何能力先落本地壳（可闭环可考题），
服务器源后插。**na = 独立本地软件，服务器只是未来可切换的数据源**——把服务器
脑当地基 = 变成 nz 的劣化版（用户 2026-08-30 原话：「把服务器上的东西拉到本地
是 nz 干的事」）。期 0 echo-brain→direct-api-brain 即首实践。

**双源对拍（用户 2026-08-30 提出）**：本地壳 = 天然测试环境（自测自闭环），
服务器源 = 现成第二环境——**同一套考题跑两个数据源，行为一致性机械可验**，
与 term-contract 双线对拍同构。每个能力的考题应在两源下各跑一遍。

**新插件上线纪律**：一律带 disabled 开关上线，默认开、可一键关（回退第一层）。

### deps / provides

**deps（需要，已点名的现有服务键）**：

- `dyn TermEmuFactory`（term_alacritty 提供）——合成聊天网格：建仿真实例、喂样式
  文本、取网格，CJK 宽度/换行/scrollback 全部继承 C4 双线契约，零新布局引擎。
- `dyn ImeInsets`（input_ime 提供）——键盘避让让位公式的 inset 读数。
- 待定①：会话注册/切换、输入焦点路由、chrome 触摸命中——这套目前长在 harness
  （session_router/gate/keybar），是包成服务键让插件 deps，还是 ai-presence 只出
  状态、壳层直读直挂？倾向前者（插件化纯度），但影响面大，须拍板。

**provides（出口）**：

- `AiPresenceState`（两布尔 ai_running×page + 球位置/pressed + 浮层可见性）——
  光球绘制、AI 遥控、探针观测的同源读数，未来状态栏类插件可用。
- `AiChatGrid`（聊天网格句柄）——AI 页与浮层两个视口的同一数据源；判卷通道复用
  （na-text/replay 天然可读）。
- `AiSendSink`（发送入口）——壳层输入栏的「发送」只是把文本推进这个口。

**apply 纪律**：瞬时返回契约（50ms 预算）——网络/流式接收全部自开线程，
apply 只注册服务与监听。

## 四、协议契约

§四A = **内部九事件协议**（UI 面，脑无关——任何脑都吐这套事件，UI 零感知换脑）；
§四B = **上游 OpenAI 协议**（direct-api-brain 的对外面，期 0② 直连主力）；
§四C = **kfmv4 /ai/chat 协议**（未来 server-brain 数据源的契约，2026-08-30 侦察
回填：源码 + 活服务器探针双证——注意：这是数据源契约，不是 na 的地基，D11）。

### 四A·内部九事件协议（content-block，kfmv4 血统）

- 事件全集：`message_start` / `content_block_start{index,blockType:text|tool_use,
  toolUseId?,toolName?}` / `content_block_delta{index,deltaType:text_delta|
  thinking_delta|input_json_delta,deltaText}` / `content_block_stop{index}` /
  `tool_result{toolUseId,toolResult{content:[{type,text}],isError?},filesChanged?}` /
  `message_stop` / `done` / `error{content}` / `rule_warning{content}`
- block 布局：`index=0` 恒为 text（**thinking+正文同块混排，靠 deltaType 分流**）；
  tool_use 从 1 起连续编号
- 一轮工具循环 = message_start → blocks → (rule_warning*/tool_result*) → message_stop；
  最终 message_stop → done
- 判卷基准 fixture：`tests/fixtures/ai-chat/probe-kimi-k3-256k-20260830.sse`（44 事件）/
  `probe-glm-5.3-flash-20260830.sse`（40 事件）——kfmv4 服务端吐出的真流，
  双 provider 互证分帧形状与上游无关

### 四B·上游 OpenAI 协议（direct-api-brain 对外面；2026-08-30 双路活探针回填）

- chat/completions 流式：`stream:true` + `stream_options.include_usage`；
  SSE 帧 = `data: {chunk}\n\n`，终结 = `data: [DONE]`；
  chunk = `{id,created,object,model,choices[0]{index,delta{…},finish_reason}}`
- delta 字段：`role`（可忽略）/ `content` → text_delta / `reasoning_content`
  → thinking_delta / `tool_calls`（OpenAI 碎片格式，期 2 再抓）→ tool_use 块；
  `finish_reason:"stop"` 帧即收尾；usage 帧只记账不进事件流
- 翻译职责（复刻 chat.ts 的角色）：外加 **reasoning 归位**（text 空且
  reasoning 非空 → 正文，R3）
- **方言差异登记（双 fixture 互证）**：
  | 维度 | Kimi（k2.7-highspeed） | 智谱（glm-5.3-flash） |
  |---|---|---|
  | `role:"assistant"` | 仅首帧 | **每帧都重复**（容忍） |
  | usage 位置 | stop 帧内联 **+ 独立 `choices:[]` 帧**（双份） | 仅 stop 帧内联 |
  | `system_fingerprint` | 有 | 无 |
  | 401 错误体 | `{error:{message,type}}` | `{error:{code:"401",message}}`（code 是字符串） |
  | `reasoning_content` | 有（39 帧） | 有（37 帧）——两家同字段，统一处理 |
- 容错判据（解析器考题要吃）：空 `delta:{}` 帧、`choices:[]` 帧、未知字段、
  role 重复——全是常态不是错误
- fixture：`upstream-kimi-k2.7-highspeed-20260830.sse`（kimi-for-coding-highspeed，
  用户 2026-08-30 点名——比 k3-256k 便宜约 3 倍刊例价）/
  `upstream-glm-5.3-flash-20260830.sse` / `upstream-error-cases-20260830.txt`
  （双路 401 实录，坏 key 零额度）；tool_calls 流随期 2 手再抓
- 配置复刻：na 私有目录落 `providers.json` + `.env`，代字 `${VAR}` fuse 语义
  照搬 kfmv4（resolveKey：先 env 后 .env，缺失 → error 事件，绝不裸发代字）
- TLS：`rustls` + `webpki-roots`（纯 Rust，Android 交叉编译安全，不碰 openssl）；
  HTTP/1.1 手写（chunked/SSE 解析器与四C共用考题）

### 四C·kfmv4 /ai/chat 协议（server-brain 数据源契约，暂缓实施）

#### 端点（基址 `/api`，镜像 `/kfmv4/api`；服务绑 127.0.0.1:8021）

| 端点 | 用途 | 响应 |
|---|---|---|
| `POST /api/ai/chat/start` | 开 run | 200 `{runId, fromIndex:0, done:false}`；400 `{error}` 参数非法 |
| `GET /api/ai/chat/:runId/stream?from=N` | SSE 事件流 | `text/event-stream`（**无 `event:` 行**，仅 `data:` 行+空行分隔） |
| `POST /api/ai/chat/:runId/cancel` | 中断（空 body） | `{ok:bool}`（不存在→`ok:false`，**不 404**） |
| `GET /api/ai/chat/:runId/status` | 探活 | `{exists,done,eventCount}` |
| `GET /api/ai/chat/active?sessionId=X` | 找回 runId（重连入口） | `{runId,eventCount,done}` 或 `{runId:null}` |
| `GET /api/ai/tools` | 工具清单 | `{categories,tools}` |

#### start body

- `sessionId`（必填；白名单 `^[\p{L}\p{N}_-]{1,128}$/u` + UTF-8 ≤200B——**含中文**）
- `messages`（必填非空；OpenAI 投影 role/content/tool_calls/tool_call_id/reasoning_content，
  **客户端全量上传**；服务端另以会话文件 `~/.kfmv4/sessions/<id>.json` 为真相源落盘）
- `model`/`provider`（可选，默认 deepseek-v4-flash/deepseek；provider 按 providers.json
  的 id 或 name 匹配，**无静默回退**，失败 → SSE error 事件）
- `tools`（可选 string[] 白名单；服务端执行层 fail-closed 再拦一道）
- roleFile/userText/extraSystem/maxTokens/params/sessionClass/sandboxRoot/readRoot（可选，v1 不用）

#### SSE 分帧（实录 fixture：`tests/fixtures/ai-chat/probe-kimi-k3-256k-20260830.sse` 44 事件全程；
第二路互证：`probe-glm-5.3-flash-20260830.sse` 40 事件，分帧形状逐帧一致）

- 帧 = `data: {"index":N,"event":{...}}\n\n`；`index` = 重连 cursor（客户端存 index+1）
- 终结帧 = `data: {"type":"__end__"}`（SSE 级收尾非业务事件；不存在/已淘汰 runId
  挂 stream 直接只发这一条，**不 404**）
- 事件全集（`shared/chat-protocol/events.ts:16-44`）：
  `message_start` / `content_block_start{index,blockType:text|tool_use,toolUseId?,toolName?}` /
  `content_block_delta{index,deltaType:text_delta|thinking_delta|input_json_delta,deltaText}` /
  `content_block_stop{index}` / `tool_result{toolUseId,toolResult{content:[{type,text}],isError?},filesChanged?}` /
  `message_stop` / `done` / `error{content}` / `rule_warning{content}`
- block 布局：`index=0` 恒为 text（**thinking+正文同块混排，靠 deltaType 分流**）；
  tool_use 从 1 起连续编号
- 一轮工具循环 = message_start → blocks → (rule_warning*/tool_result*) → message_stop；
  最终 message_stop → done → `__end__`

#### 中断 / 重连 / 缓冲

- 中断 = POST cancel（服务端 abort；**客户端断开不取消后台生成**）
- 重连：知 runId → `stream?from=cursor`；丢 runId → `active?sessionId` 找回；
  attachRun 先同步回放 `events[from:]` 再实时尾随，已完成则回放完即 `__end__`
- 缓冲：done/error 后 **5min 淘汰**；6min 无事件看门狗以 error 收尾
- 同 session 新 start = **取代**旧 run（旧的一律取消）

#### 错误语义（实录 fixture：`tests/fixtures/ai-chat/probe-error-cases-20260830.txt`）

- 参数非法 → 400 `{error}`（sessionId 白名单 / 空 messages，均有实录）
- 跨源 → 403 `{error}`（verifyLocalOrigin；**无 Authorization 概念**，防护 = Origin
  同源或 loopback——na 走 127.0.0.1/ssh 隧道天然过）
- provider 不存在 / apiKey 代字缺失 → 200 立即返 runId + SSE error 事件（人话 content）
- 上游 4xx/5xx → error 事件 `'API 请求失败: <status> — <body前300字>'`；
  网络层重试 2 次后 → `'网络错误…'`；用户取消 → error `'已取消'`

#### apiKey 与 provider 配置（复刻要点）

- 配置 `~/.kfmv4/providers.json`，条目 `{id,name,baseUrl,apiKey,models[],contextWindow?}`
- 代字 fuse：`apiKey=${VAR}` → resolveKey 查 process.env 再查 `~/.kfmv4/.env`；
  缺失 → error 事件，**绝不裸发代字**
- 用户点名两路（2026-08-30 均活探针验证）：Kimi 卡（id `Kimi`，api.kimi.com/coding/v1）
  model `k3-256k`；智谱 coding plan 卡（id `智谱`，open.bigmodel.cn/api/coding/paas/v4）
  model `glm-5.3-flash`（即 kimi-code 里的「GLM 5.3 Flash Coding (套餐)」，
  `bigmodel-coding/glm-5.3-flash`）。**model 字段服务端不校验 models[] 白名单、
  直接透传上游**（chat.ts:289）——卡上登记模型只是面板可选项
- 配置事故实录（2026-08-30 修复）：智谱卡原与聚光卡**共用 `${KFM_PROVIDER_KEY}`**
  （中文 id 经 envNameForProvider 全塌缩成同名代字），.env 里存的是聚光的 key →
  智谱 401「令牌已过期」。修复：智谱卡改 `${KFM_PROVIDER_ZHIPU}` + .env 独立条目
  （key 取自 kimi-code `providers.bigmodel-coding`），并补登记 `glm-5.3-flash` 进卡

### 风险清单（侦察登记）

- **R1 鉴权零成本但零防护**：无 token、绑 loopback——复刻成本≈0，但公网暴露即裸奔，
  ssh 隧道纪律不能破
- **R2 cursor 是客户端纪律**：信封 index 必须跟踪，否则重连重复消费（服务端不兜底）
- **R3 reasoning 归位陷阱**：text 空且 reasoning 非空 → 归位正文（kfmv4 陷阱 10，
  na 解析器同须处理）
- **R4 input_json_delta 是碎片**：tool_use 参数须累积到 stop 才 JSON.parse，半截 JSON 是常态
- **R5 token 成本不可控**：全局 system 预设（prompts/global）在服务端注入，客户端零感知
- **R6 历史双写**：客户端上传投影 + 服务端文件真相源并存；na v1 = 只上传投影，
  本地持久化另议（待定③）
- **R7 WS 旁路独立于 SSE**：眼睛/手/心跳走 WS，对话走 SSE；期 0 只复刻 SSE，
  WS 是期 1 眼睛的前置

### `BrainEndpoint` trait 草案（从协议反推，非凭空设计）

```rust
/// 一个「脑」= 能开 run、吐事件流、可中断的后端。
/// direct-api-brain（na 直连 provider，rustls+手写 HTTP）= 默认本地脑，期 0② 主力；
/// echo-brain 回放 fixture 做考题夹具；server-brain（HTTP/SSE 到 kfmv4）= 期 3 数据源。
trait BrainEndpoint {
    /// 开一轮对话，返回 run 句柄 + 事件流（自有 content-block 协议，与上游无关）。
    fn start(&self, req: ChatStartReq) -> (RunHandle, BoxStream<ChatEvent>);
    /// 中断 run（尽力而为；已终结的 run 返回 false）。
    fn cancel(&self, run: &RunHandle) -> bool;
    /// 重连接回：从 cursor 回放+尾随。server-brain 有（5min 缓冲）；echo/direct 可空实现。
    fn attach(&self, run: &RunHandle, from: u64) -> Option<BoxStream<ChatEvent>>;
}
// ChatStartReq = {session_id, messages, model, provider, tools}
// ChatEvent    = 平移 events.ts 九类型；BrainError 二分：传输错误(可重试) vs
//                业务 error 事件(入流不例外)
```

- 解析器 + markdown-lite 转换器 = 纯逻辑，A 档考题先行 + 变异抽检。
  md 范围 v1：代码块/粗体/斜体/列表/标题。
- 协议实现对 `BrainEndpoint` 接口编程（三层模型），**direct-api-brain 是第一个
  后端，server-brain 只是可插的数据源之一**（D11）。

## 五、状态机（A 档考题化；D7 两布尔模型，2026-08-30 取代三档）

- **核心两布尔**：`ai_running`（AI 在跑与否）× `page`（终端 / AI 全屏）——
  状态从真事实派生，用户不管模式、只理解因果。
- **浮层可见性 = f(ai_running, dismissed)**：`run_start` → 现；`run_end` →
  驻留 LINGER_MS（初值 3000，常量可调）后自动隐；运行中上滑甩掉 → 本次运行
  不再出现（per-run dismissed），下一次 `run_start` 复位；点浮层 → 跳 AI 全屏；
  人在 AI 全屏时 `run_end` 不踢人。非用户发送触发的运行同样现浮层
  （浮层 = 「AI 活着」的一致信号）。
- **光球**：`tap` → page 往返；拖动 → 更新 (x,y)（状态字段，边界钳制：
  不出屏、让位快捷键行/键盘）；pressed（按下/长按/拖动中）→ 光晕加大静态态。
- **焦点二态**：终端 / 输入栏（点输入栏聚焦，Esc 或点终端区失焦）——组件三落。
- **发送流**：文本入格（本地即时）→ `run_start`（浮层自动现，不抢全屏）→
  流式分片入格（尾随视口自动跟）→ `run_end`。
- **调试钩子**：长按球 = fake_run(3000)（标注 debug，echo-brain 就位后可拆）。
- **时钟注入**：所有时间判定吃 `now_ms` 参数，不碰墙钟——考题喂时间戳即判。
- **组件一落地入口**：状态核 `src/ai_presence.rs`（考题 tests/ai_presence_spec.rs）；
  观测 = stats `ai_*` 字段族（na-stats.sh）；驱动 = 通道十 orb-inject
  （na-orb.sh 'tap'/'drag x y'/'run ms'/'end'/'dismiss'，回执 orb-inject-res）；
  视觉 = na-shot 实拍（倒帧装帧含光球/AI 页占位——gate dump_now 组件一修订）。

## 六、分期（每期独立可验收，慢慢来）

- **期 0（2026-08-30 用户拍板重排 D10 + 纠偏 D11：先接通真 AI，浮层/手靠后；
  脑 = 本地直连，服务器只是未来数据源）**：
  ① 状态核+光球 ✅（组件一闭环，`b2c4ffa` 在机）
  ② AI 接通（本地直连）：上游协议侦察（curl 直打 Kimi/智谱原生端点抓 SSE
     fixture，§四B）→ OpenAI SSE→九事件翻译器+解析器考题（四A fixture 当
     内部协议标准答案）→ `BrainEndpoint` 接口定形 → **direct-api-brain**
     （rustls+手写 HTTP，providers.json/.env 代字复刻）；
     echo-brain = **考题夹具**（协议解析器/断网回归的零网络基准，非组件）
  ③ 基本对话页+输入栏：AI 页占位壳 → 真对话页（简版纯文本消息行，
     不做 markdown-lite）+ 全局输入栏发送；验收 = 真机问答一轮
  ④ 浮层+手：观战增强（此时浮层显示的是真 AI 的真输出，脚手架一次到位）
  ⑤ 合成网格美化：markdown-lite / 样式块 / 浮层数据源换第二视口
  每组件独立考题+热更+实拍。
  **②进度（2026-08-31）**：协议层落地 `src/brain.rs`（纯逻辑零 IO）——
  SseParser（碎喂/粘包/半帧/CRLF/注释容忍）+ OpenAiTranslator（上游 chunk→
  四A 九事件，方言全容忍）+ RunAccumulator（reasoning 归位 R3）+
  error_event_from_http + build_chat_request；考题 tests/brain_spec.rs
  **18 题全绿**，变异抽检双发双咬（thinking↔text 换轨咬 4 题、归位删除咬 1 题）。
  脑插座落地 `src/brain_ep.rs`：BrainEndpoint trait（mpsc 通道模型——start/
  attach 返回 Receiver，脑自开线程推事件，守 apply 50ms 预算；BoxStream 草案
  按 Rust 线程模型改通道）+ EchoBrain 夹具（from_upstream_sse 走真解析管造
  节目单、pace 节奏注入、取消→Error 已取消 收尾、attach 历史后缀回放）；
  考题 tests/brain_ep_spec.rs **6 题全绿**，变异双咬（取消检查删除咬、
  attach 游标 +1 咬）。direct-api-brain 落地 `src/direct_brain.rs` +
  `src/http1.rs`（手写 HTTP/1.1：chunked 分帧状态机碎喂安全，tick 钩子=
  读超时醒来过取消检查）+ `src/providers.rs`（providers.json/.env 代字
  fuse 复刻：process env 优先，缺失报错绝不裸发）+ rustls ring 后端
  （纯 Rust TLS，Android 交叉安全）；考题 http1_spec 8 题 + providers_spec
  8 题全绿，变异双咬（trailer 跳过不咬=登记盲区：body 已读完不影响，
  连接复用才有差，一期不做）。**live 双线一次全通**（2026-08-31）：
  Kimi/kimi-for-coding-highspeed 28 事件 Done 正文暗号咬、
  智谱/glm-5.3-flash 98 事件同尺咬、坏 provider 立即人话 Error 事件——
  tests/direct_brain_live_spec.rs（#[ignore]，手动 --ignored）。
  **③进度（2026-08-31）**：全局输入栏落地 `src/input_bar.rs`（状态核：
  文本缓冲 UTF-8 安全退格/焦点二态/enter 取文/submit 发送口三路同源）
  + `src/plugins/input_bar.rs`（共享实例直挂）+ 壳层全链接线（几何：
  压底紧贴键盘、keybar 上移一层、usable_h/orb 钳制同步让位；触摸：
  栏带命中→文本区聚焦弹键盘/发送钮 submit；IME 三路分流：
  drain_ime_inject/handle_key/Ime::Commit 聚焦时按键全归栏）+
  termview render_inputbar（聚焦亮底硬切，零动画帧）+ 通道十一
  bar-inject（focus/unfocus/text/backspace/clear/submit，回执
  bar-inject-res）+ stats bar_* 字段族。**期 0②收尾同车完成**：
  发送闭包接 echo-brain 真 run——run_start/run_end 驱动光球亮灭
  （fake_run 仅留长按 debug 钩子）；考题 input_bar_spec **12 题全绿**
  + 变异双咬（enter 不清空咬/退格撕字节咬）。
- **期 1 眼睛**：视口快照附上下文。验收 = 问「屏幕上有什么」答得对。
- **期 2 手**：工具执行注入来源会话。验收 = 让 AI 跑一条命令，终端可见。
- **期 3 打磨**：server-brain（服务器空间数据源，§四C 契约已就位——**服务器
  以数据源身份回归，不再是地基**）/ 工具调用独立悬浮框 / 展开动画（ui-fx
  第二插件，缝的首批属性落位）/ 浮层可触摸滚动。

## 七、待定清单（讨论一条落一条，拍板后转正进 §八）

① 会话/焦点/命中的插件化方式（服务键 vs 壳层直挂）
② ~~/ai/chat 协议契约~~ ✅ 已回填（2026-08-30 侦察：源码+活探针双证，见 §四）
③ 消息本地持久化格式（飞行记录仪风格）
④ 逐能力目标反选（默认跟随空间，显式覆盖待真实场景冒头再立项，parked）

## 八、决策记录（三行块）

【D1】三层结构：会话分页 + 两条常驻 chrome + 浮层第二视口
决定：AI 页=全屏会话；光球与全局输入栏常驻；浮层=同一聊天网格的尾随视口
理由：手机稀缺的是同屏面积，分页不分屏；视口方案零状态复制；输入栏=全局入口
（用户否决「随面板开合」：全局入口价值 > 两行屏幕成本）
否决案：底部抽屉分屏（两半残废+多面触摸分发）；独立消息缓冲（状态复制/滚动分裂）
标签：打脸结晶

【D2】光球 = 存在感三档旋钮
决定：隐身/伴随/主场三档循环；AI 状态静态换图
理由：一个开关覆盖全部场景，语义比「往返跳转」纯；零常动帧
否决案：光球仅做来源↔AI 页往返跳转（两态表达不了「纯终端」需求）
标签：成本不对称
状态：已废止（2026-08-30 D7 取代——用户判「三档是替 AI 管模式」，化简为两布尔）

【D3】空间模型 + 覆盖链
决定：本地=完整底层空间（每能力有本地默认实现）；服务器=覆盖层，连接时能力探测
（handshake）决定覆盖哪些槽位，探不到落回本地；覆盖作用域=空间（会话），非全局；
数据源全程 UI 可见（树顶/AI 页标注来源）
理由：本地闭环使 na 成为独立软件（接入 api 即用）；覆盖链使降级成为模型自带行为；
无镜像即无双源漂移（各空间只持自有配置+现场探测）
否决案：镜像配置文件夹（双源漂移）；全局本地/远程总开关（与会话粒度冲突）
标签：打脸结晶（用户 2026-08-30 提出「本地闭环+数据源可换」，推翻「脑只在服务器」前案）

【D4】发送后档位行为
决定：发送后当前=隐身则升到伴随，不强制切主场
理由：短回复浮层内读完，观战场景不被拽离现场；长回复/历史由光球主动进入
否决案：发送即切主场（观战看不到手）；纯文字才自动切（规则分叉，行为不可预期）
标签：成本不对称
状态：已并入 D7（升档规则废弃——发送即 run_start，浮层自动现、不抢全屏）

【D5】浮层几何定稿
决定：高=屏 25%（约 8-10 行）、80% 不透明暗底、直角、顶对齐
理由：顶部压旧输出不挡手（底部是活区）；80%=「感到终端在后面」与可读性的折中起点；
直角贴终端网格风格
否决案：全透明（文字打架）；圆角卡片风（与终端风格断裂）
标签：外部判据（C 档真机实拍可调）

【D6】光球位置与 AI 会话环位
决定：光球右缘中部偏下常驻，默认档=伴随；AI 会话排 Ctrl-] 切换环末尾
理由：右手拇指热区，避开右下角发送键与终端底部活区；默认伴随=AI 存在感默认可感知
否决案：右下角（与发送键冲突）；默认隐身（新用户感知不到 AI 存在）
标签：外部判据（C 档真机实拍可调）
状态：档位部分随 D7 废止；出生位 2026-08-30 用户真机截屏指定——
比例 (0.859, 0.556)（Screenshot_20260830_214836 实测球心 (1082,1557)@1260×2800，
取代「右缘×60%」：拇指热区内收、避开右缘手势区；D9 起可拖走）

【D7】两布尔模型（取代 D2 三档 / D4 升档）
决定：状态核 = ai_running × page 两布尔；浮层 = 运行时观察窗自动来走（run_end 驻留
3000ms / 上滑甩掉 per-run / 点浮层跳全屏 / 全屏时 run_end 不踢人）；光球唯一职责 =
切全屏 + 运行指示灯；输入栏发送 → run_start → 浮层现、不抢全屏；非用户触发的运行
同样现浮层（浮层 = 「AI 活着」的一致信号）
理由：三档是让用户替 AI 管模式；化简后状态从真事实派生，功能定位更精准
（用户 2026-08-30 提出并拍板）；两布尔转移表比三档模式更好考题化
否决案：三档旋钮（模式虚、需用户理解档位）；发送即切主场（观战被拽离现场）
标签：打脸结晶

【D8】光球视觉 = 雾状光球 sprite（kfmv4 血统；2026-08-30 拟合定稿）
决定：**样式以用户提供的专用参考图为准逐像素拟合**（非凭文字猜）——
三层参数化模型（长度量均以球半径 Rs 归一，任意尺寸可缩放）：
① 光晕层（底）：alpha a(r) = clip((1-r/Rg)^p + tamp·exp(-r/tsig))，Rg=2.93·Rs、
p=2.05、tamp=0.12、tsig=1.02·Rs，色 = C_lit=(99,50,198)
② 球体层：Lambert 明暗 I = max(0, -lx·nx - ly·ny + lz·nz)^k，光向 (lx,ly)=(0.37,0.45)
（左上）、k=2.24；色 = mix(DARK=(9,8,13), C_lit, I)；整盘 alpha As=0.77——**暗面
靠高 alpha 暗色遮挡光晕成形，不是透明**（这是「确实有个球」的知觉来源，前两次
「纯渐隐/独立遮挡盘」模型 RMSE 15.2/11.3 证伪，Lambert 模型 7.8 证实方向）
③ 高光点：光源方向 0.55·Rs 处小高斯，amp=0.22、sigma=0.10·Rs，过曝略往白
四态硬切 = 整 sprite 增益系数：闲/运行/pressed/AI页（初值 0.85/1.0/1.25/1.0+晕增益，
实拍可调）；拟合器 `scripts/orb-fit.py`（坐标下降，换参考图重跑即再校准），
拟合证据 RMSE 4.66/255 + 对比图 `docs/assets/orb-fit-compare.png`
理由：用户首要诉求 = 几乎透明不挡后面内容、但确实可见有球；参考图实证球感来自
「径向亮度落差 + 暗面遮挡 + 光晕托底」三件套，无轮廓线；sprite 预渲染零动画帧
合 ui-base 纪律；参数化模型使 Rust 生成器与 Python 拟合器同公式，逐像素可回归
否决案：实心渐变盘（覆盖率 100% 挡字）；空心细环（非原版血统）；纯径向渐隐
（无暗面遮挡，出不来「球」的体积感，RMSE 15.2 证伪）
标签：外部判据（C 档真机实拍可调）
**加法合成决定（2026-08-30 用户实拍反馈后修订）**：sprite 合成从 alpha
混合改加法（每像素 = 三层公式在 BG=(11,10,15) 上的合成结果减 BG 裁剪 ≥0
的光贡献量，绘制 = 饱和加）。机理证据：alpha 混合时球体暗面（As=0.77、
色 (9,8,13)）把底下文字盖暗——球内笔画亮度 p90=325 vs 球外 478，遮挡
−32%（用户说的「压字/偏暗」实锤）；参考效果（orb-on-white-ref.jpg）是
球内 p90=539 vs 球外 284，提亮 +90%。样式参考图暗面 ≈ BG，故加法在
黑底上 底+加值 **精确复现**拟合图（逐像素钉目标值不变），文字底上文字
全亮透过+球加光——一个模型两个环境通吃。球径 96→**120px**（Rs=60）：
px 标准说明——kfmv4 网页用 CSS px（36px × 手机 DPR≈3 ≈ 108-120 物理
px），na 用物理 px，Rs=60 与 kfmv4 同级且合用户实测。增益按加法语义
重调：闲 0.7 / 运行 1.0+晕 1.2 / pressed 1.3 / AI 页 1.0（alpha 时代
旧值不复用）；**二调（同日，用户实机裁图定量反馈）**：闲 0.7 时峰值/
球区/光晕全面为样式参考的 ~60%（「偏暗不明显」实锤）——闲态即应 =
样式参考基准亮度，「几乎透明」由加法结构（暗面=无光贡献）保证、不靠
整体压暗：闲 **1.0** / 运行 **1.15**+晕 1.2 / pressed **1.4** / AI 页 1.0。
评测尺 `scripts/orb-on-text-measure.py`（球内/球晕/球外
三区笔画 p90 与底 p10）
校准锚（增补）：`docs/assets/orb-on-white-ref.jpg` /
`docs/assets/orb-on-gray-ref.jpg`（文字环境参考=穿透可读性对拍基准）
校准锚：`docs/assets/orb-style-ref-20260830.jpg`（样式参考=逐像素对拍基准）/
`docs/assets/orb-ref-20260830.jpg`（实机截图=大小/位置/场景参考，右下大紫雾球，
图中右上小原子标是粒子时代装饰非光球）/ `docs/assets/orb-fit-generated.png`
（拟合产物=Rust 生成器的验收基准）

【D9】拖动 / 按压 / AI 遥控 / 观测闭环
决定：球位置 (x,y) 入状态核（按下命中球区 + 位移超阈值 → 拖动；边界钳制不出屏、
让位快捷键行/键盘；位置持久化 v1 不做 parked）；pressed = 第四视觉态硬切；
AI 遥控 = AiPresenceState 服务方法（toggle_page / move_orb / fake_run），人走触摸、
AI 走服务，同一状态核同一套考题；观测闭环 = stats 添 ai_presence 字段族（机器轨）
+ 探针合成事件注入口（驱动轨）+ na-shot 实拍（视觉轨）
理由：kfmv4 已有拖动与 AI 遥控（wsChannel expand-orb 等），na 不能退化；
「agent 能观测自己的交互结果」是自我测试闭环的地基（用户 2026-08-30 明确要求）
否决案：位置常量（用户明确要求可拖）；只截图断言（状态对画错抓不到，须双轨）
标签：成本不对称

【D10】期 0 组件序列重排（2026-08-30 用户拍板）
决定：期 0 重排为 ①状态核+光球✅ → ②AI 接通（协议侦察→契约回填→BrainEndpoint
定形→server-brain，echo-brain 降为考题夹具）→ ③基本对话页+输入栏（简版纯文本）
→ ④浮层+手 → ⑤合成网格美化；原期1/2/3 顺延为眼睛/手/打磨
理由：真价值先做（接通真 AI 才是产品本体）；真风险先排（/ai/chat 协议是最大
未知项，待定②若留到期 1 才回填，期 0③ 的对话页会建在猜测上）；echo-brain
保留双源对拍基准价值但不再占组件位（零网络夹具服务解析器考题）
否决案：原序列（先浮层后接通——脚手架建在协议猜测上，返工风险后置）
标签：成本不对称

【D11】脑的顺序纠偏：本地直连脑是地基，server-brain 降数据源（2026-08-30 用户拍板）
决定：期 0② 的脑 = **direct-api-brain**（na 本地配 key 直连 Kimi/智谱，rustls
纯 Rust TLS + 手写 HTTP/1.1，providers.json/.env 代字 fuse 照搬 kfmv4）；
server-brain（kfmv4 /ai/chat 走隧道）从期 0② 主力**降为期 3「服务器空间」
数据源插件**（§四C 契约保留备用）；内部九事件协议（§四A）不变——UI 面与脑
解耦，换脑零改动；direct-api-brain 须复刻 chat.ts 的上游→九事件翻译职责
（含 reasoning 归位）
理由：na = **独立安卓本地软件，服务器只是未来可切换的数据源**——把服务器脑
当地基，na 就退化成 nz（kfmv4 安卓化线）的劣化版，而「把服务器的东西拉到本地」
恰是 nz 的赛道且其更优（用户 2026-08-30 原话）。§三「本地壳先行」本是文档
自有纪律，D10 落地时险些违背——纠偏即回到自己的纪律上
否决案：server-brain 先行（na 变成 kfmv4 远程终端，身份错位；本地闭环无从谈起）
标签：打脸结晶

——kfm-na(Kimi Code) · 2026-08-30 立 · v3 两布尔模型+雾球入档 · 实现以期为单位，每期过 chain+regress+实拍
