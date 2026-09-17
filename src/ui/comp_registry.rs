//! comp_registry.rs — 组件注册表（主题宪法 §五 目录语义 7「组件池页」，
//! 2026-09-13 九修，用户拍板；核心层纯数据零 IO）。
//!
//! **唯一信息源纪律**：组件池页直接读本表渲染——本表是什么，页面上
//! 就是什么；禁止在涂装/壳层手抄第二份组件清单。每个条目钉一对
//! (symbol, file) = 实现坐标，考题棘轮核对 symbol 字符串真实出现在
//! file 里（tests/comp_registry_spec.rs）——表与代码漂移 = 考题红。
//!
//! 十修增订（同日，用户拍板「重点是这个内容，而不是档案」）：
//! 条目加 **preview 维**——跳框预览画板的渲染种类（宪法 §六 跳框
//! 预览画板条款）；涂装归 termview 预览段，原语全复用共享件。
//!
//! 分类（大类 = 组件池页下池行）：
//! - 装修框：页面的装饰骨架（宪法 §三 装修框家）
//! - 组件：无边框的文字容器（§六 组件条款）
//! - 功能光标：选中态指示框（§三 功能光标家——现役空缺，封存待复用）
//! - 控件：自包含交互单元（ui/ 控件库，registry.md 登记的正式成员）
//! - 动效引擎：弹簧/缓动/手势仲裁/视口平移（ui-base.md §八 动画全插件）
//!
//! 状态三档：现役 / 封存（退役留档，待复用）/ 计划（立了项没动工）。

/// 组件状态（上池行 value 列原样显示）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompStatus {
    /// 现役：在页面上跑着呢
    Active,
    /// 封存：退役留档（如开口框，待文件树光标复用）
    Mothballed,
    /// 计划：立了项没动工
    Planned,
}

impl CompStatus {
    pub fn label(self) -> &'static str {
        match self {
            CompStatus::Active => "现役",
            CompStatus::Mothballed => "封存",
            CompStatus::Planned => "计划",
        }
    }
}

/// 预览画板渲染种类（十修 §六 跳框预览画板条款）：涂装侧 match 本枚举
/// 出微缩实时渲染；命名即语义，涂装实现归 termview 预览段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preview {
    /// 圆角矩形边框环（页环/池框同配方微缩）
    Ring,
    /// 三级框行：选中（全包框：左粗 10+三细 3 整环渐变 α255）与未选中
    /// （无框纯渐变暗底）各一条
    RowFrame,
    /// 均匀细框：四边 3px 渐变细框两条（非池行场合通用件）
    ThinFrame,
    /// 跳框自指：迷你压暗区 + 小卡 + 小关闭钮
    ModalMini,
    /// 标签页块：选中（c1→c2 竖向均匀渐变满填）+ 未选中（条带薄态，
    /// 异色演示每标签独立双色）+ 底线（配合标签模式纯色）
    TabChip,
    /// 底线组件：模式①自身反转渐变细线（c2→c1；模式②配合标签纯色
    /// 在 TabChip 预览里展出）
    Underline,
    /// 下拉面板：触发器（三级框）+ 圆角深底下弹 panel（选中项均匀细框）
    Dropdown,
    /// 字段标签列：标签（36 亮+提亮背衬）+ 值框（30 灰）mini 行
    FieldLabel,
    /// 功能光标开口框（封存件复活展出）
    OpenCursor,
    /// 光球真渲染（orb sprite 加法合成微缩）
    Orb,
    /// 输入栏 mini：栏框 + 占位灰字 + 发送钮
    InputBar,
    /// 快捷键行 mini：两排键格
    Keybar,
    /// 设置钮：程序化齿轮（gear paint_at 共享件）
    Gear,
    /// 弹簧响应曲线（过冲可见）
    CurveSpring,
    /// 缓动曲线（ease-out/in 两族）
    CurveEase,
    /// 手势仲裁示意：圆点 + 水平轨迹 + 方向箭头
    Swipe,
    /// 视口平移示意：新页推入旧页挤出
    ViewportPush,
    /// 池高伸缩演示（十五修语义化）：双小池，上池高 25%→55% 乒乓伸缩、
    /// 下池顶与行跟随
    PoolGlide,
    /// 标签栏层演示（十五修语义化）：两未选 chip + 选中块在两者间滑行
    /// + 底线（配合模式纯色）
    TabSlide,
    /// 光标滑行/光标层演示（十五修语义化）：三行小行（真文字）+ 光标框
    /// 行间乒乓滑行，框动字不动（BAR-107：文字在框上层直出）
    CursorSlide,
    /// 下拉开合演示（十五修语义化）：触发器 + ▼三角旋转 p×180° +
    /// 抽屉面板生长，选项行钉面板顶随面 clip
    DropdownAnim,
    /// 视口平移切页演示（十五修语义化）：两个迷你页（各含双小池）整体
    /// 横移换页，面与内容一体
    PagePan,
}

/// 组件条目（跳框字段区+预览画板的数据源：名/状态/位置/规范/考题/
/// 说明/预览种类）
pub struct CompEntry {
    /// 组件名（上池行 label + 跳框标题）
    pub name: &'static str,
    /// 大类（= CATEGORIES 之一；下池行归属）
    pub cat: &'static str,
    pub status: CompStatus,
    /// 实现坐标：symbol 字符串必须真实出现在 file 里（考题棘轮）
    pub symbol: &'static str,
    pub file: &'static str,
    /// 规范出处（宪法条款/设计文档）
    pub spec: &'static str,
    /// 考题位置
    pub tests: &'static str,
    /// 一句话说明（跳框折行显示）
    pub desc: &'static str,
    /// 预览画板渲染种类（十修）
    pub preview: Preview,
}

/// 大类表（顺序 = 组件池页下池行序）
pub const CATEGORIES: [&str; 5] = ["装修框", "组件", "功能光标", "控件", "动效引擎"];

/// 组件总表（唯一信息源；排序 = 大类内上池行序）
pub const COMPONENTS: &[CompEntry] = &[
    // ---- 装修框 ----
    CompEntry {
        name: "页环",
        cat: "装修框",
        status: CompStatus::Active,
        symbol: "paint_page_frame_ring",
        file: "src/termview.rs",
        spec: "宪法 §三/§四",
        tests: "tests/termview_spec.rs",
        desc: "全屏页面的外框环：左粗三边细、135° 双色渐变、外发光。三公民页面与终端卡片壳同源同配方。",
        preview: Preview::Ring,
    },
    CompEntry {
        name: "池框",
        cat: "装修框",
        status: CompStatus::Active,
        symbol: "paint_rect_ring",
        file: "src/termview.rs",
        spec: "宪法 §三/§五",
        tests: "tests/termview_spec.rs",
        desc: "圆角矩形边框环核：外发光 + 渐变外环 + 内芯分路（grad_fill：平色 punch / 渐变暗底，十二修）。双池、跳框卡、页环本体全从这里出；终端卡壳与 AI 页主题基座走平色。",
        preview: Preview::Ring,
    },
    CompEntry {
        name: "三级框行",
        cat: "装修框",
        status: CompStatus::Active,
        symbol: "paint_row_frame",
        file: "src/termview.rs",
        spec: "宪法 §五 池行",
        tests: "tests/termview_spec.rs",
        desc: "圆角深色框行（十二修）：内芯渐变暗底（dark(c1)→dark(c2) 135°，不透明直写）。选中 = 全包框（左粗竖线 10px+三细边 3px 整环渐变 α255，下池目录行/下拉项）；未选中 = 无框纯渐变暗底剪影。角部渐细只渐形状不渐色（十一修）。",
        preview: Preview::RowFrame,
    },
    CompEntry {
        name: "均匀细框",
        cat: "装修框",
        status: CompStatus::Active,
        symbol: "paint_thin_frame",
        file: "src/termview.rs",
        spec: "宪法 §五 池行",
        tests: "tests/termview_spec.rs",
        desc: "四边 3px 渐变均匀细框 + 渐变暗底内芯（十二修：4% 白填退役）：跳框关闭钮、预览展台等非池行场合的通用细框。不挂左粗缘——左粗是选择语言的视觉载荷（十一修新立）。",
        preview: Preview::ThinFrame,
    },
    CompEntry {
        name: "跳框",
        cat: "装修框",
        status: CompStatus::Active,
        symbol: "paint_modal_impl",
        file: "src/termview.rs",
        spec: "宪法 §六 跳框",
        tests: "tests/modal_spec.rs",
        desc: "模态详情卡：压暗层 + 居中卡 + 题注/内容字段区 + 全宽关闭钮。点框外或关闭钮收起。",
        preview: Preview::ModalMini,
    },
    // ---- 组件 ----
    CompEntry {
        name: "标签页块",
        cat: "组件",
        status: CompStatus::Active,
        symbol: "paint_tab_chip",
        file: "src/termview.rs",
        spec: "宪法 §四 十一修",
        tests: "tests/tab_bar_spec.rs",
        desc: "无边框色块标签：上两角圆角、下缘直边。每标签独立随机双色：选中 = c1→c2 竖向均匀渐变满填 α255（十二修）+ 深色字；未选中 = 同一把 t 尺均匀渐变薄态 α48 满块（十三修：三段条带硬切退役，只降 alpha 不降连续性）。",
        preview: Preview::TabChip,
    },
    CompEntry {
        name: "底线",
        cat: "组件",
        status: CompStatus::Active,
        symbol: "paint_tab_bar_layer",
        file: "src/termview.rs",
        spec: "宪法 §四 十一修（BAR-096 拆层：span 由壳层喂 snap.line_span）",
        tests: "tests/tab_bar_spec.rs",
        desc: "标签行下缘紧挨的 1px 细线，池区同宽。两模式：自身反转渐变（c2→c1，留档）/配合标签 = 选中标签 c2 纯色（配置页采用，与选中块下 1/3 同色一体）。空态也画。BAR-096：随标签栏独立成小画布层，span 由壳层每帧喂（层画布不知屏高/键盘 inset）。",
        preview: Preview::Underline,
    },
    CompEntry {
        name: "下拉面板",
        cat: "组件",
        status: CompStatus::Active,
        symbol: "paint_cfg_pool_content_impl",
        file: "src/termview.rs",
        spec: "宪法 §六 下拉栏",
        tests: "tests/cfg_page_spec.rs",
        desc: "自绘下拉（十三修重订）：触发器 = 三级框全包框 + 右缘 ▼；展开面板 = 整面圆角无边框深底（近黑 α252）；选项行方形无个体背景，选中行 = 均匀细框。顶部栏向下弹——方向反了会弹出屏外。",
        preview: Preview::Dropdown,
    },
    CompEntry {
        name: "字段标签列",
        cat: "组件",
        status: CompStatus::Active,
        symbol: "field_label_rect",
        file: "src/ui/cfg_page.rs",
        spec: "宪法 §五 字段框行条款",
        tests: "tests/cfg_page_spec.rs",
        desc: "上池字段行的标签块：无边框组件，36px 亮（七修字档反转）+ 圆角背衬 = 渐变暗底 + 8% 白提亮（α20，十三修）。十四修动态宽度：块宽随标签文字实量宽，锚行左缘；值框锚右缘，间隔 ≥3 格，超长换行 ≤2 行。",
        preview: Preview::FieldLabel,
    },
    // ---- 功能光标 ----
    CompEntry {
        name: "开口框",
        cat: "功能光标",
        status: CompStatus::Mothballed,
        symbol: "paint_open_cursor",
        file: "src/termview.rs",
        spec: "宪法 §三 功能光标",
        tests: "tests/cursor_spec.rs",
        desc: "左强调线 + 顶底随机长发丝 + 绿青底垫的选中光标。标签栏八修改用填色标签块后封存，待文件树光标复用。",
        preview: Preview::OpenCursor,
    },
    // ---- 控件 ----
    CompEntry {
        name: "光球",
        cat: "控件",
        status: CompStatus::Active,
        symbol: "build_orb_sprite",
        file: "src/ui/orb.rs",
        spec: "ai-presence.md",
        tests: "tests/ai_presence_spec.rs",
        desc: "雾状光球 sprite + 呼吸光晕，AI 外显入口。点击召唤 AI 对话面板（上缘下落动画）。",
        preview: Preview::Orb,
    },
    CompEntry {
        name: "输入栏",
        cat: "控件",
        status: CompStatus::Active,
        symbol: "render_inputbar",
        file: "src/ui/prompt_bar.rs",
        spec: "ai-presence.md 期 0",
        tests: "tests/input_bar_spec.rs",
        desc: "全局输入栏：压键盘顶，多行折行、像素级滚动、长按选区与拖动锚点、发送口直进 AI 面板。",
        preview: Preview::InputBar,
    },
    CompEntry {
        name: "快捷键行",
        cat: "控件",
        status: CompStatus::Active,
        symbol: "render_keybar",
        file: "src/ui/keybar.rs",
        spec: "ui-base.md",
        tests: "tests/keybar_spec.rs",
        desc: "终端两行快捷键：Ctrl/Alt 修饰 + Esc/Tab/方向键。手机端没有物理键盘的补偿层。",
        preview: Preview::Keybar,
    },
    CompEntry {
        name: "设置钮",
        cat: "控件",
        status: CompStatus::Active,
        symbol: "hit_rect",
        file: "src/ui/gear.rs",
        spec: "宪法 §四 配置卡入口",
        tests: "tests/gear_spec.rs",
        desc: "终端页右上角齿轮，两行高。配置卡的唯一入口——画进终卡槽，面板靠泊时整层自隐。",
        preview: Preview::Gear,
    },
    CompEntry {
        name: "标签栏",
        cat: "控件",
        status: CompStatus::Active,
        symbol: "rects_of",
        file: "src/ui/tab_bar.rs",
        spec: "宪法 §四 标签栏",
        tests: "tests/tab_bar_spec.rs",
        desc: "配置卡首行标签行：横滑 + 点选 + 弹簧滑块。手势仲裁边界单源——行带上的横向滑动不触发面板拖拽。",
        preview: Preview::TabChip,
    },
    // ---- 动效引擎 ----
    CompEntry {
        name: "弹簧",
        cat: "动效引擎",
        status: CompStatus::Active,
        symbol: "spring_pos",
        file: "src/ui/fx_spring.rs",
        spec: "ui-base.md §八",
        tests: "tests/fx_spring_spec.rs",
        desc: "欠阻尼弹簧：标签滑块、键盘 inset 同核。select 瞬间从当前位置重定基续弹，600ms 兜底贴死。预览 = 语义化演示（十五修）：白球点触，响应点沿 spring_pos 实曲线往返骑行（去程 0→100、回程 100→0），1400ms 乒乓无缝。",
        preview: Preview::CurveSpring,
    },
    CompEntry {
        name: "缓动",
        cat: "动效引擎",
        status: CompStatus::Active,
        symbol: "ease_out_cubic",
        file: "src/ui/fx_ease.rs",
        spec: "ui-base.md §八 + 宪法 §五 曲线单一源",
        tests: "tests/fx_ease_spec.rs",
        desc: "全局曲线库三族：ease-in-out cubic 250ms = 位移类唯一尺（池平移/光标/池高/标签游标，BAR-095 定律）；ease-out/in = 面板下落收起与下拉开合；rise_release/power2_out = 面板松手补间。预览 = 语义化演示（十五修）：白球点触，小面板 power2_out 下落、停靠、rise_release 收起，1400ms 乒乓。",
        preview: Preview::CurveEase,
    },
    CompEntry {
        name: "手势仲裁",
        cat: "动效引擎",
        status: CompStatus::Active,
        symbol: "completion_progress",
        file: "src/ui/panel_drag.rs",
        spec: "ui-base.md §五B",
        tests: "tests/panel_drag_spec.rs",
        desc: "面板跟手拖拽：横向锁定制，松手按完成度+速度裁决去留。一滑一义——纵向滚动时横向锁未起。预览 = 语义化演示（十五修）：白球 1:1 拖小卡片到 70%、松手 power2_out 补到终点，回程纯 1:1 拖回，1400ms 乒乓。",
        preview: Preview::Swipe,
    },
    CompEntry {
        name: "视口平移",
        cat: "动效引擎",
        status: CompStatus::Active,
        symbol: "viewport_push",
        file: "src/ui/viewport_push.rs",
        spec: "ui-base.md §五B",
        tests: "tests/viewport_push_spec.rs",
        desc: "四公民页面视口平移合成：新页推入、旧页挤出。被覆盖面板保持覆盖态，收起覆盖者即露出。预览 = 语义化演示（十五修）：白球左拖，满宽新页推入旧页完全挤出（真换页），ease-in-out 往返，1400ms 乒乓。",
        preview: Preview::ViewportPush,
    },
    CompEntry {
        name: "池高伸缩",
        cat: "动效引擎",
        status: CompStatus::Active,
        symbol: "glide_upper_content_h",
        file: "src/ui/dual_pool.rs",
        spec: "宪法 §五 池区分域律（BAR-095）",
        tests: "tests/dual_pool_spec.rs",
        desc: "上池高度分域动画：点下池 = 250ms ease-in-out 与光标/平移同钟同步（「光标到位池高也到位」，零过冲——BAR-094 弹簧废除）；切标签 = 直通（新页池高起步帧就位，贴死零二次动画）。预览 = 语义化演示（十五修）：双小池，上池高 25%↔55% 乒乓伸缩，下池顶与行跟随。",
        preview: Preview::PoolGlide,
    },
    CompEntry {
        name: "标签栏层",
        cat: "动效引擎",
        status: CompStatus::Active,
        symbol: "TAB_LAYER_H",
        file: "src/ui/tab_bar.rs",
        spec: "ui-base §八 渲染成本模型（BAR-096 拆槽）",
        tests: "tests/termview_spec.rs",
        desc: "标签行独立小画布槽（屏宽×118 ≈0.65MB）：游标滑行逐帧重烘只脏这一层——原配置槽每次 14MB 全页重光栅+上传是 21fps 帧饥饿的三路真凶之一（draw_avg 47ms）。签名已钉「与整页版逐像素等价」（减 y_shift 一个常量）。预览 = 语义化演示（十五修）：两未选 chip + 选中块在两者间滑行 + 底线。",
        preview: Preview::TabSlide,
    },
    CompEntry {
        name: "光标层",
        cat: "动效引擎",
        status: CompStatus::Active,
        symbol: "paint_lower_cursor_layer",
        file: "src/termview.rs",
        spec: "ui-base §八 渲染成本模型（BAR-096 拆槽）",
        tests: "tests/termview_spec.rs",
        desc: "下池光标独立小画布槽（池内容宽×行高 ≈0.69MB）：位置全进合成期 placement，滑行逐帧零重烘；渐变参照吃「框在页上原位」的页坐标页尺（保真条，防层画布尺漂色）。预览 = 语义化演示（十五修）：与光标滑行件同款三行+滑框（框动字不动，BAR-107）。",
        preview: Preview::CursorSlide,
    },
    CompEntry {
        name: "光标滑行",
        cat: "动效引擎",
        status: CompStatus::Active,
        symbol: "cursor_fx_active",
        file: "src/ui/cfg_page.rs",
        spec: "宪法 §五 池区动画（BAR-094 改判缓动核）",
        tests: "tests/cfg_page_spec.rs",
        desc: "下池选中光标跨行滑行：250ms ease-in-out 与视口平移同钟同曲线（BAR-094 改判——弹簧过冲判「瞬移+过冲」废除），全程单调无过冲，涂装选中全包框吃瞬时值。预览 = 语义化演示（十五修）：三行真文字 + 光标框行间乒乓滑行，框动字不动（BAR-107）。",
        preview: Preview::CursorSlide,
    },
    CompEntry {
        name: "下拉开合",
        cat: "动效引擎",
        status: CompStatus::Active,
        symbol: "dropdown_progress",
        file: "src/ui/cfg_page.rs",
        spec: "宪法 §六 开合动画两件",
        tests: "tests/cfg_page_spec.rs",
        desc: "下拉面板开合（十五修+两段时序+十七修两件）：展开 250ms ease-out 生长；点选他行 = 选中细框 160ms ease-out 滑行（面板冻结等它）→ 180ms ease-in 收起；点当前行/外点直接收。十七修：①抽屉随面——选项行/选中细框钉在全高刚体上随面板滑出滑回（下方先入场、上方先没入）；②▼三角矢量旋转 progress×180°。预览 = 语义化演示（十五修）：触发器 + ▼旋转 + 抽屉生长开合乒乓。",
        preview: Preview::DropdownAnim,
    },
    CompEntry {
        name: "视口平移切页",
        cat: "动效引擎",
        status: CompStatus::Active,
        symbol: "pan_active",
        file: "src/ui/cfg_page.rs",
        spec: "宪法 §六 面与内容一体",
        tests: "tests/cfg_page_spec.rs",
        desc: "面与内容一体平移（十七修通则）：①标签切换 = 页面级（双池框+内容整体平移，视口 = 页环）；②下池选行 = 上池级（上池内容平移，视口 = 上池框）；③下拉 = 垂直实例（抽屉随面）。方向律：选择前进 = 内容左移。双代同画（旧代冻结快照带偏移出、新代活态带偏移进，禁两拍），250ms ease-out cubic。预览 = 语义化演示（十五修）：两个迷你页（各含双小池）整体横移换页，面与内容一体。",
        preview: Preview::PagePan,
    },
];

/// 预览是否动效演示（十四修 §六 立、十五修扩到 10 件：动效引擎分类
/// 全部语义化动画；帧泵与烘焙 sig 时间桶维的开关单源）
pub fn preview_is_animated(p: Preview) -> bool {
    matches!(
        p,
        Preview::CurveSpring
            | Preview::CurveEase
            | Preview::Swipe
            | Preview::ViewportPush
            | Preview::PoolGlide
            | Preview::TabSlide
            | Preview::CursorSlide
            | Preview::DropdownAnim
            | Preview::PagePan
    )
}

/// 某大类的条目下标表（组件池页：下池聚焦大类 → 上池行表；
/// 返回 COMPONENTS 下标——跳框按它取详情，眼手同尺）
pub fn entries_of(cat: &str) -> Vec<usize> {
    COMPONENTS
        .iter()
        .enumerate()
        .filter(|(_, e)| e.cat == cat)
        .map(|(i, _)| i)
        .collect()
}

/// 大类内的组件计数（下池行 meta 列）
pub fn count_of(cat: &str) -> usize {
    COMPONENTS.iter().filter(|e| e.cat == cat).count()
}
