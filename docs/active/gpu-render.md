# GPU 渲染立项（gpu-render）

> 2026-09-04 立。状态：**期 0① 动工**（wgpu 30 × NativeActivity，
> spikes/gpu/——里程碑+心跳走飞鸽传书，判卷 tail field-reports.log）。
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
- 渲染底座 = 期 0③ 尖刺骨架（glow/GLES 直连定案，§九——wgpu 本机
  铁案犯冲已封档，② GameActivity 封存不用）；
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

- 分层不变：渲染归壳，核心不见 GPU 后端（glow/wgpu/softbuffer 皆壳）；
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

## 八、期 0① 实录（2026-09-04，wgpu 30 × NativeActivity）

- **API 漂移实锤**（风险登记第二条兑现，照 wgpu-types 30.0.1 源码修）：
  `InstanceDescriptor::default()` 构造器化（`new_without_display_handle()`，
  `Instance::new` 改传值）；`RequestAdapterOptions` 增 `apply_limit_buckets`；
  `DeviceDescriptor` 增 `experimental_features`；`SurfaceConfiguration` 增
  `color_space`；`RenderPipelineDescriptor.multiview` 改名 `multiview_mask`；
  **`get_current_texture()` 去 Result 化改返 `CurrentSurfaceTexture` 枚举**
  （Success/Suboptimal/Timeout/Occluded/Outdated/Lost/Validation——表面
  丢失语义被类型系统整个接管，老防御逻辑的接力点在这）；
  **`present()` 从 `SurfaceTexture` 挪到 `Queue::present(frame)`**。
  期 1 GPU 化时 renderer 抽象必须把版本漂移计入接口设计。
- **打包坑**：纯 NativeActivity 无 classes.dex 的包，manifest 必须显式
  `android:hasCode="false"`——默认 true 被 vivo 安装器判「软件包无效」
  （主包有 dex 不受影响，cargo-apk 时代它自动写 false）。
- **流程坑**：chain-phone.sh 会把手机工作树回齐到 HEAD——scp 直改手机
  源码后若先跑 chain 闸，改动被冲掉。正确序：改码 → 提交落账 → 手机
  重编打包（或 scp 后立刻编包再补账）。
- **上报协议坑**：/na-report 只认 JSON `{stage, msg}`——spike_report 首版
  text/plain 裸发，内容全落成空 `[?]` 行（首跑 14 条报告只剩行数可数）。
  但「同毫秒 12 条 + 2 秒后断流」本身已是证据：进程活了 ~2s 死在早帧段。
- **ROM 包名拉黑实锤**：vc=...378（hasCode=false 首包）装上跑了 2 秒
  暴毙，用户卸载后，紧接的 ...763（同管线同证书同结构，仅 .so 与 vc
  不同）被 vivo 判「软件包无效」——zipalign -c/apksigner verify/md5
  三验全过、file:// 与 content://（termux-open）双通道同拒、主 app
  对照组同刻装得上。换包名 gpuspike→gpuspikeb 即装即走。**尖刺纪律：
  每次暴毙卸载后换包名重生**（manifest b/c/… 尾缀递增）。
- **期 0① 判决书（双后端验尸）**：wgpu 30 × NativeActivity——
  Vulkan 轮 adapter（Mali-G720 Immortalis，r44p1）/device 全过，
  +89ms 死 `surface.configure()`；GL 轮（OpenGL ES 3.2 同驱动）
  adapter/device 全过，+72ms 死同一行。零 panic，与 2026-08-13 判词
  **同一个死亡点**。**wgpu 版本变量排除**——病灶不在 wgpu API 层，
  在 wgpu-hal configure 路径 × Mali r44p1 × OriginOS 的交汇处。
  ①封档。按分流进 ②③；③（glow/GLES 直连）另有一个诊断价值：
  它不过 wgpu-hal——若裸 EGL/GLES 能 configure 上屏，病灶即坐实在
  wgpu-hal 而非驱动，GPU 化路线就还有「绕开 wgpu」这条命。

## 九、期 0③ 判决书（2026-09-04，glow/GLES 直连 × NativeActivity）——**活**

- **全链路通关**：dlopen libEGL.so → eglInitialize v1.5 → choose_config
  （RGB888 ES3 window）→ GLES3 context → **eglCreateWindowSurface 过了**
  → **eglMakeCurrent 过了（wgpu 双后端从未活着走到这）** → shader/VAO
  → **首帧 eglSwapBuffers 过了（+138ms）**，紫底橙三角上屏（肉眼对拍
  与①同视觉判据）。
- **帧率**：首秒 107 帧（catch-up）后稳锁 60fps（vsync），suspend/resume
  两轮实测：拆 surface/context+窗 → 重建全栈 → 帧计数续走无暴毙。
- **病灶坐实**：同机同驱动（Mali-G720 r44p1）同 ROM，裸 EGL 一遍活、
  wgpu-hal 两个大版本四个组合全死 configure——**wgpu 抽象层与该机
  犯冲是铁案，驱动无罪**。
- **路线定案**：GPU 化走 **glow/GLES 直连**（khronos-egl dlopen
  libEGL.so + glow）——依赖纯 Rust crates.io，手工打包管线零改动；
  **② GameActivity 变量封存不用**（窗体供给层无罪，不必为它引
  gradle/AAR 链）。期 1 终端网格 GPU 化的渲染底座 = 本尖刺骨架
  （EGL 生命周期 + suspend/resume 拆建纪律）+ 字形图集纹理 + 网格
  实例化绘制。
- **链接坑实录**：Termux 的 libEGL.so SONAME 是 libEGL.so.1，静态链
  会把 .so.1 写进 NEEDED 而 app 命名空间只有 libEGL.so——必须
  khronos-egl dynamic（libloading dlopen）路线。
- **验收闭环（期 0③ 正式封卷）**：suspend/resume **15 循环**（标准 10）
  每次拆建后帧计数续走零暴毙；前台连续心跳 **15.8 分钟每秒一条零断流**
  （全程累计 714 条心跳 / 存活 25 分钟）；前台稳锁 ~60fps（vsync）。
  期 0 三项验收标准全绿，GPU 化地基打完，期 1 可动工。

## 十一、期 1 第 2 层 A 档：字形图集 + 实例转换（2026-09-05）

- `src/glyph_atlas.rs`（核心层，零平台依赖）：GlyphAtlas 行架装箱
  （同键幂等 / 换行 / 行架高 / 页满翻页 / 超页违约 panic）；
  GpuCell→BgInstance/GlyphInstance 两遍制转换（背景缺省过滤、
  spacer/paintable 不落墨、宽字符 2 格 clip 折进四边形与 UV、
  图集未命中记 misses 补装载后重生成）。
- 字体路由（prefer_cjk）归调用方闭包——本模块只认路由键与槽位；
  coverage 8bit 原样进图集（R8+NEAREST 1:1 → 覆盖率与 CPU 同源，
  对拍差异只剩整数/浮点舍入 ±1/255）。
- 考题 `tests/glyph_atlas_spec.rs` 12 题（考题先行，两轮考题自错
  修正——实现是标准行架语义）；变异抽检 2/2 咬住（拆换行条件、
  反转背景过滤）。
- 待办（B 档）：GL 侧图集纹理 + 双实例流绘制接入 gles_present，
  实拍对拍 + 动画场景 <8ms 验收。
- **判卷（2026-09-05）**：视觉 ✓（缩略图回传 + 用户肉眼双确认——
  字形/布局/栏带/光球全对；缩略图回传为新增自观测仪器，整套
  GLS_READBACK_PROBE 基础设施保留可复启）；性能 ✗——动画窗口实测
  190 帧均 41.8ms（靶 <8ms）。瓶颈 = 全画布条件 alpha 遍历（~4ms）
  + 全画布 14MB 纹理上传（~15-20ms）+ AI 面板过渡帧全屏 scratch
  重渲染（~15ms）——字形图集省下的被这三样吃回。修复路径：
  ①chrome 脏 hash 跳上传（流式场景白捡）②面板滑动 scratch 缓存
  ③长期 chrome GPU 原生绘制（期 2）。
- **B 档落地（2026-09-05）**：gles_present 扩为分层合成——清屏 →
  网格背景实例（不透明）→ 图集字形实例（R8 采样 × 前景色 alpha 混合，
  按图集页分组 draw）→ chrome 层（CPU 画栏带/输入栏/AI 页/放大镜，
  透明底 + `|= 0xFF000000` alpha 标记，RGBA 上传 alpha 混合叠上）。
  实例四边形 = 3 倍超界大三角（整格全覆盖无半像素缝）；attrib divisor
  1 每实例；图集页纹理增量上传。TermEmu trait 扩 gpu_cells/
  rasterize_for_atlas 两方法（调用方先例 = android_app GLES 分支）；
  字体路由闭包双键回退（主缺 → CJK 顶）。rasterize 加 gpu_term 开关
  ——CPU 不再画终端网格（第 2 层的省，就在这一刀）。

## 十、期 1 第 1 层：壳内 EGL 基建（2026-09-04）

- **切法**：先换「present」不换「墨」——全部光栅化照旧 CPU 进帧缓冲，
  仅把呈现从 softbuffer 换成 `gles_present::GlesPresent`（纹理上传 +
  全屏三角 + eglSwapBuffers）。尖刺③骨架移植：EGL 生命周期、
  suspend/resume 拆建纪律照搬。字形图集是第 2 层的事，不在本层。
- **后端开关**：`Gfx` 枚举化（Soft/Gles），`GLES_FIRST=true` 优先试
  GLES，init 任何一步 Err 自动回退 softbuffer（立项书红线「永久保留」
  在此兑现）；`FrameBuf` 借用层统一 `&mut [u32]` 喂 rasterize，present
  各回各家——rasterize 与全部控件零改动。
- **像素格式**：XRGB u32 小端按 RGBA8 上传，片元 swizzle（b,g,r），
  零 CPU 转换零扩展依赖；NEAREST 采样保字形/SDF 边；`swap_interval(0)`
  不堵 vsync（条件帧泵怕堵输入派发，帧率治理在 fx_frame_due）。
- **判卷**（C 档实拍）：GLES 起得来、亮得对（na-shot 与 softbuffer
  同屏对拍）、后台切回不崩、stats 帧耗画像对照。
- **判卷闭环（2026-09-04 深夜，全绿）**：构建戳 ccf988a-09041525 上屏；
  na-shot 双包截图 md5 逐字节一致；后台往返一轮——suspended → resumed
  GLES 全栈重建（+50ms）、会话保留零暴毙；全帧画像基线 avg 12ms /
  max 20ms（第 2 层字形图集的靶子）。
- **热更核遮蔽案（排障新钉，两小时教训）**：装新包后启动报告仍是旧
  构建戳 → 先查 `files/hot/libkfm_na.so`（na-push-so 热更通道，na-loader
  优先 dlopen 它）与 `usr/tmp/loader-pick`——APK md5 对 ≠ 加载的核对。
  今晚 hot 目录躺着 20:06 推入的旧核，四轮重装全被遮蔽。处置：
  hot 旧核移名清场（.so.stale-* 留档），loader 回 bundled。
- **versionCode 计数器双线撞车（叠加案，真实存在）**：`build/`
  gitignore 跨机不同步，kfm-na 线与 nz hotfix 线共用同一计数器文件，
  三轮同数互拒（「已安装更高版本」「相同版本」）。过渡：kfm-na 线
  暂占 1789600000+ 段；分治提案见信箱
  kfm-na-versioncode-counter-collision-notice.md。
