# GPU 渲染立项（gpu-render）

> 2026-09-04 立。状态：期 0 复活尖刺待动工。
> 触发：用户实拍「AI 页面下落掉帧明显，成熟 app 动画都是 90 帧」——
> 查机制后确认不是设置问题，是架构缺 GPU 合成；用户拍板立项
> （「只靠优化肯定不够，未来跨端也需要，现在内容少正是好时机」）。

## 一、动机（数据实锤）

- 帧耗实测（自观测 stats 直出，2026-09-04）：全帧 CPU 软渲染
  **avg 40ms / max 63ms**（含 present）→ 持续重绘场景（滚动/动画/刷屏）
  天花板 ~25fps，峰值压力 ~16fps。
- 机制差距：成熟 app 的动画 = **GPU 合成**（内容层渲染一次成纹理，
  动画只动纹理，90/120fps 是硬件合成器白送）；我们的过渡帧分支
  （android_app.rs）每帧把终端页 + 快捷键行 + AI 页**全量重光栅化**
  再 blit 压盖。
- 字形零缓存：`draw_glyph`（termview.rs）每帧对屏上每字重新
  `fontdue.rasterize`（一屏终端近 2000 字）。
- 跨端：未来桌面 exe（Linux/Windows）同样需要 GPU 后端；wgpu 一处
  抽象覆盖 Android（Vulkan/GLES）与桌面（Vulkan/DX12/Metal）。
- 时机：UI 内容尚少（光球/快捷键行/输入栏/AI 页/放大镜），渲染迁移
  面每天都在长——越晚越贵。

## 二、历史判决（动工前必读）

2026-08-13 已判过一次（立项.md §十，commit a44f936 判词）：

- wgpu 25 × **Mali-G720 Immortalis r44p1 + OriginOS**：
  adapter/surface/configure **随机原生暴毙**，Vulkan/GLES 双后端同病，
  零 Rust panic；裸 winit 窗对照组稳定（15s+）。六次实拍定罪：
  「本机 GPU 驱动栈与 wgpu 抽象层犯冲，非代码逻辑病」→ 转 softbuffer。

判词没变，**变的是变量**：

| 变量 | 2026-08-13 死亡时 | 现在 |
|---|---|---|
| wgpu 版本 | 25 | **30**（五个大版本，surface/adapter 层多次重写） |
| Activity 底座 | NativeActivity | GameActivity 后路未试（立项.md §十备忘：Bevy 默认，生态共识） |
| 抽象层 | wgpu 一家 | glow/GLES 直连（更低抽象，犯冲面更小）未试 |

因此本立项第一颗钉 = **复活验证**：不过此钉，后面分期全部免谈，
CPU 优化线顶上（§六分流）。

## 三、目标 / 非目标

目标：
- 动画/滚动场景稳定 **≥60fps**（面板进出场 350ms 内 21 帧画满）；
- 稳态交互帧耗 **<16ms**；
- 渲染后端可切换：wgpu（首选）/ softbuffer（永久降级路径），
  chain 加双后端 check。

非目标：
- 不追 90/120Hz 硬指标（屏驱/合成器链路不可控因素多，流畅判卷归
  C 档人眼）；
- 不动核心层（cordis-na 零平台依赖纪律不变，chain 禁依赖表加 wgpu）；
- 不动 IME/Java 皮（渲染换心与输入链路无关）；
- 不重写控件逻辑——控件只换「墨的去处」（像素缓冲 → GPU 纹理/图集），
  排版/状态核/既有考题一行不动。

## 四、分期与验收

**期 0 · 复活尖刺**（唯一判卷点：本机能不能活）
- 变量矩阵按序试：① 现版本 wgpu × NativeActivity（打包管线零改动，
  最先试）→ ② 现版本 wgpu × GameActivity（需评打包改造，①死才启用）
  → ③ glow/GLES 直连对照组。
- 验收（对照 2026-08-13 死亡点逐项复跑）：adapter/surface/configure/
  present 全过 + suspend/resume 10 循环 + **15 分钟存活零原生崩溃** +
  三角渲染上屏实拍。
- 全灭 → 封档：GPU 路线本机二审判死，转 CPU 优化线（§六），
  桌面端 GPU 另行立项。

**期 1 · 终端网格 GPU 化**
- 字形图集（atlas）+ instanced quad；终端页帧耗 <8ms；
- 与 softbuffer 同屏像素对拍（眼手同尺：网格眼睛读数两后端一致）。

**期 2 · chrome/控件层 + GPU 合成动画**
- SDF shader 化（圆角/渐变/发光按控件分诊：shader / 九宫格 / 预渲精灵）；
- 面板进出场 = 移动纹理，内容层零重绘；动画场景实测 ≥60fps；
- 挂电耗对账（GPU 常驻 vs CPU 间歇，probe-overnight-power.sh 方法现成）。

**期 3 · 跨端**
- 桌面 shell（Linux 先），同一 renderer 抽象；桌面端 wgpu 成熟度远高
  于本机 Mali 栈，风险与 Android 期 0 解耦。

## 五、架构纪律

- 分层不变：渲染归壳，核心不见 wgpu/softbuffer；
- **renderer 抽象是期 1 的真正难点**：现状 `Frame{buf,w,h}` 像素直写
  是全部控件的物质基础（termview SDF 图元族/draw_items/orb sprite/
  blit_panel）——抽象设计要先回答「逐像素 SDF 的墨怎么进 GPU」，
  答案按控件分诊，不搞一刀切；
- softbuffer 永久保留：GPU 二审判死的保险 + 老设备兜底。

## 六、与 CPU 优化线的分流（防白做）

- 期 0 动工前**不投** CPU 微优化：字形 LRU 会被 GPU 图集取代，
  GPU 活 = 白做；
- **动画分层缓存例外**：它的「层」思路与 GPU 合成同构（内容静止只动
  偏移），且是 softbuffer 降级路径的保命优化——可在期 0 准备期间做，
  优先级低于期 0；
- 判卷纪律：每期动刀前后同尺复验（stats 帧耗画像 + na-regress 全卷）。

## 七、风险登记

- **最大风险 = 期 0 同机同病再死**——退路已写明（封档转 CPU 线），
  沉没成本限尖刺规模；
- wgpu 25→30 API 迁移成本（surface 生命周期模型已变）；
- GameActivity 引入 gradle/AAR 链，与手工打包管线（package-apk.sh）
  冲突——②启用前先评打包改造（或 xbuild）；
- 电耗：GPU 常驻渲染 vs CPU 间歇渲染，期 2 验收挂电耗对账，超标回评。
