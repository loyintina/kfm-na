# bugs.md — KFM-NA BAR 账本

> 每条 bug 修复登记一行。钉 = `#[test]` 名带 BAR 编号，位置列在右列。
> 纪律见 AGENTS.md 门 2（fix 提交不带钉 = 提交不了）。
>
> 分级：I = 事故级（用户实拍踩到/数据损坏）· L = 逻辑错（测试/审计发现）· V = 观感类

| 编号 | 模块 | 病灶与契约 | 级 | 状态 | 钉位置 |
|------|------|-----------|----|------|--------|
| BAR-001 | termview | 字形竖直居中 → 高矮字母底边错位（里倒歪斜）。契约：同基线字母底边对齐、高字母顶边更高、下伸字母探过基线 | V | 已修 | tests/termview_spec.rs `spec_bar001_基线对齐_同基线字母底边对齐` |
| BAR-002 | termview | 真机 NotoSansCJK-Regular.ttc 在 fontdue 0.9 下光栅全空（能载不能画）→ 只见光标不见字。契约：load_font 必须跳过空光栅字体（探针字符 'M' 无墨即弃） | I | 已修（跳过 → Roboto 兜底，11:30 实拍字已出） | tests/termview_spec.rs `spec_字体_空光栅判不合格` / `spec_字体_真字形判合格` |
| BAR-003 | termview | 真机落到 Roboto（比例字体）→ 定宽格摆比例字形，间距忽近忽远。契约：'i'/'M' 步进宽不等即弃；路径候选全灭落内嵌 DejaVuSansMono | I | 已修（等宽判定 + 内嵌兜底） | tests/termview_spec.rs `spec_字体_等宽判定` / `spec_字体_加载跳过比例字体` / `spec_字体_内嵌字节可直接用` / `spec_字体_候选全灭落内嵌等宽` |
| BAR-004 | android_app | 切后台再进页面消失：Android 退后台销毁 native 表面，壳不弃窗不弃 softbuffer 表面，回前台对着死柄画。契约：suspended 弃窗弃表面，resumed 重建；Term/会话还活着就不重开会话 | I | 已修（B 档生命周期胶水，C 档实拍判卷，无输入输出可出考题） | —（tests:na，平台胶水） |
| BAR-005 | termview | 网格从 (0,0) 画起，边缘字符被屏幕圆角/曲面切半。契约：帧缓冲四周 MARGIN 带内必须是纯背景，墨水全在带内之后 | V | 已修（MARGIN 12px 黑带，不画框） | tests/termview_spec.rs `spec_边距_首格不贴边` |
| BAR-006 | android_app/insets | 软键盘盖住底部内容不上滚。初版走 Ime::Enabled 事件 + 42% 估计——实拍判卷：该事件在本机（OriginOS）从未触发（全日志零条），估计式避让是死代码。正道：JNI 直调 WindowInsets.Type.ime() 拿真实高度，500ms 节流轮询驱动 resize | V | 已修（JNI insets.rs，C 档实拍判卷） | —（tests:na，平台胶水） |
| BAR-007 | termview | TUI 转动点（盲文 U+2800 块）画方框：字体选择按 Unicode 段（needs_cjk）路由，盲文 < U+2E80 被分给主字体 DejaVuSansMono——它没有盲文字形。契约：按字形覆盖挑（lookup_glyph_index），主缺备用有才换；双缺记 tofu 目击名单上报 | V | 已修（覆盖路由 + tofu census） | tests/termview_spec.rs `spec_cjk_按覆盖挑选` / `spec_渲染_tofu目击名单` |
| BAR-008 | android/java | 中文输入 Java 皮第一刀把内容 View 替换成自定义 SurfaceView → 原生层绑到不可见 surface：全黑屏、触摸/键盘全哑，只有切后台间隙能瞥见真终端一行。NativeContentView 与窗口 surface（takeSurface）的回调时序是原生渲染命脉，动不得。契约：原生渲染路径一行不动，IME 用 1px 焦点占位 View（KfmImeView）正交叠加——input queue 被 NativeActivity 整窗接管，焦点给谁只决定 IMM 用谁的 InputConnection | I | 已修（占位叠加，B 档平台胶水 C 档实拍判卷） | —（tests:na，平台胶水） |
| BAR-009 | android/java | BAR-008 修完点击不弹软键盘：占位 View 持了焦点，但 View.onCheckIsTextEditor() 默认 false——InputMethodManager.showSoftInput 检查输入目标不是文本编辑器就拒绝弹键盘（此前无 View 持焦点时 IMM 无 served view 反而照弹）。契约：IME 占位 View 必须覆写 onCheckIsTextEditor → true（SDL DummyEdit 同款）；Java 侧链路探针（nativeImeLog）上报 IMM 询问/焦点变化 | I | 已修（覆写 + 探针，B 档平台胶水 C 档实拍判卷） | —（tests:na，平台胶水） |
| BAR-010 | termview | 圆角屏吃掉首行首字符（首行第一个字符完全看不到）。契约：顶边距 MARGIN_TOP = MARGIN_Y + CELL_H（再下探一整行），底/左右维持 MARGIN；顶带内纯背景 | V | 已修 | tests/termview_spec.rs `spec_边距_首格不贴边` |
| BAR-011 | android/java/scripts | e44fdaa 零日志闪退（连 android_main 进入都没有）：e44fdaa 与上一版 versionCode 相同（16777473），同版本号覆盖安装可能不重解压 .so——「新 dex 调 nativeImeLog + 旧 so 没符号」→ UnsatisfiedLinkError 在启动期炸死。契约：①versionCode 每次打包必须递增（package-apk.sh 注释红线）；②Java 侧任何 JNI 调用不许裸奔——imeLog/commitText/sendKey 统一防护入口，Throwable 全吞，探针/输入哑火绝不等于闪退 | I | 已修（versionCode 递增 + JNI 防护，B 档平台胶水 C 档实拍判卷） | —（tests:na，平台胶水） |
| BAR-012 | android/java/insets | IME 三杂症（73eefc8 实拍）：①进场没点任何东西键盘自弹——占位 View 是文本编辑器且持焦点，IMM 进场自动弹；②什么内容都输不进——Gboard 对英文也开组词（自动纠错），字母走 setComposingText 攒词不 commit，setComposingText 是 no-op 则终端永远不见字；③关掉键盘再点召唤不出——winit set_ime_allowed 走 SHOW_IMPLICIT，用户显式收键盘后 IMM 按策略拒弹。契约：①MainActivity 设 SOFT_INPUT_STATE_HIDDEN，键盘只能触摸主动召唤；②inputType = TYPE_CLASS_TEXT \| TYPE_TEXT_VARIATION_VISIBLE_PASSWORD（Termux 同款），禁英文组词逐字 commit，中文拼音组词不受影响；③触摸召唤走 JNI showSoftInput SHOW_FORCED（insets::force_show_keyboard） | I | ①已修（自弹止住，实拍确认）；③未愈反加重为「完全不弹」。一轮埋点实锤：触摸派发正常、无卡死、IMM showSoftInput=Some(false) 拒弹 → 无 served view（焦点丢失/IMM 拒认）嫌疑。二轮：onWindowFocusChanged 重请求焦点 + 强弹目标换焦点 View 本身 + 每次触摸报「焦点类名/isActive/强弹结果」三数 | —（tests:na，平台胶水） |
| BAR-013 | scripts/insets/manifest | 476ed14 实拍「完全不弹键盘」+ [java] 探针全体沉默实锤：设备 .so 不随覆盖安装重解压——dex 是新的（STATE_HIDDEN 压住自弹）但 .so 是旧的（触摸处理里没有 force_show_keyboard、探针符号缺失被静默吞）。BAR-011 的 versionCode 递增判决下错了，止不住。契约：①manifest extractNativeLibs=false + 打包 .so STORED 不压缩 + zipalign -p 页对齐——.so 直从 APK mmap，与 dex 原子同版本，错配链连根拔；②构建戳 option_env!(KFM_NA_BUILD) 编进 android_main 首行上报，设备跑哪个构建日志首行一读便知；③force_show_keyboard 首调结果上报（None=JNI 失败 / Some(false)=IMM 拒弹） | I | 已修（B 档平台胶水 C 档实拍判卷） | —（tests:na，平台胶水） |
