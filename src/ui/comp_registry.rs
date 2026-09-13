//! comp_registry.rs — 组件注册表（主题宪法 §五 目录语义 7「组件池页」，
//! 2026-09-13 九修，用户拍板；核心层纯数据零 IO）。
//!
//! **唯一信息源纪律**：组件池页直接读本表渲染——本表是什么，页面上
//! 就是什么；禁止在涂装/壳层手抄第二份组件清单。每个条目钉一对
//! (symbol, file) = 实现坐标，考题棘轮核对 symbol 字符串真实出现在
//! file 里（tests/comp_registry_spec.rs）——表与代码漂移 = 考题红。
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

/// 组件条目（跳框字段区的数据源：名/状态/位置/规范/考题/说明）
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
    },
    CompEntry {
        name: "池框",
        cat: "装修框",
        status: CompStatus::Active,
        symbol: "paint_rect_ring",
        file: "src/termview.rs",
        spec: "宪法 §三/§五",
        tests: "tests/termview_spec.rs",
        desc: "圆角矩形边框环核：外发光 + 渐变外环 + 底色 punch 内芯。双池、跳框卡、页环本体全从这里出。",
    },
    CompEntry {
        name: "三级框行",
        cat: "装修框",
        status: CompStatus::Active,
        symbol: "paint_row_frame",
        file: "src/termview.rs",
        spec: "宪法 §五 池行",
        tests: "tests/termview_spec.rs",
        desc: "圆角深色框行：4% 白填、左粗缘在角部渐细入 8% 白细边。下池目录行、上池值框、下拉项、跳框关闭钮共用。",
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
    },
    // ---- 组件 ----
    CompEntry {
        name: "标签页块",
        cat: "组件",
        status: CompStatus::Active,
        symbol: "paint_tab_chip",
        file: "src/termview.rs",
        spec: "宪法 §四 八修",
        tests: "tests/tab_bar_spec.rs",
        desc: "无边框色块标签：上两角圆角、下缘直边。选中 = accent 渐变满填 + 深色字，未选中 = 6% 白薄填。",
    },
    CompEntry {
        name: "底线",
        cat: "组件",
        status: CompStatus::Active,
        symbol: "paint_cfg_tab_bar_impl",
        file: "src/termview.rs",
        spec: "宪法 §四 八修",
        tests: "tests/tab_bar_spec.rs",
        desc: "标签行下缘紧挨的 1px 渐变细线，池区同宽，色向 = 内卡反转 c2→c1。空态也画——装修不是内容。",
    },
    CompEntry {
        name: "下拉面板",
        cat: "组件",
        status: CompStatus::Active,
        symbol: "paint_cfg_pool_content_impl",
        file: "src/termview.rs",
        spec: "宪法 §六 下拉栏",
        tests: "tests/cfg_page_spec.rs",
        desc: "自绘下拉：触发器 6% 白底，面板 96% 近黑 + 选中项 accent 描边。顶部栏向下弹——方向反了会弹出屏外。",
    },
    CompEntry {
        name: "字段标签列",
        cat: "组件",
        status: CompStatus::Active,
        symbol: "LABEL_COL_W",
        file: "src/ui/cfg_page.rs",
        spec: "宪法 §六 组件条款",
        tests: "tests/cfg_page_spec.rs",
        desc: "上池字段行的标签列：无边框文字容器，36px 亮——标签是行的标题，字大且亮（七修字档反转）。",
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
        desc: "欠阻尼弹簧：标签滑块、键盘 inset 同核。select 瞬间从当前位置重定基续弹，600ms 兜底贴死。",
    },
    CompEntry {
        name: "缓动",
        cat: "动效引擎",
        status: CompStatus::Active,
        symbol: "ease_out_cubic",
        file: "src/ui/fx_ease.rs",
        spec: "ui-base.md §八",
        tests: "tests/fx_ease_spec.rs",
        desc: "面板入场/出场曲线库：ease-out 下落 350ms、ease-in 收起 250ms（真机逐帧标定）。",
    },
    CompEntry {
        name: "手势仲裁",
        cat: "动效引擎",
        status: CompStatus::Active,
        symbol: "completion_progress",
        file: "src/ui/panel_drag.rs",
        spec: "ui-base.md §五B",
        tests: "tests/panel_drag_spec.rs",
        desc: "面板跟手拖拽：横向锁定制，松手按完成度+速度裁决去留。一滑一义——纵向滚动时横向锁未起。",
    },
    CompEntry {
        name: "视口平移",
        cat: "动效引擎",
        status: CompStatus::Active,
        symbol: "viewport_push",
        file: "src/ui/viewport_push.rs",
        spec: "ui-base.md §五B",
        tests: "tests/viewport_push_spec.rs",
        desc: "四公民页面视口平移合成：新页推入、旧页挤出。被覆盖面板保持覆盖态，收起覆盖者即露出。",
    },
];

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
