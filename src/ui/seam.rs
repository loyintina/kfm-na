//! seam.rs — 采样缝（ui-base.md §三）：可动画属性渲染时过一道槽位。
//!
//! 无插件占槽 → 渲染值 == 目标值（硬切基座，功能等价只是变糙）；
//! ui-fx 插件占槽驱动插值。**缝不能后补**（改缝 = 改每个控件）——
//! 随第一消费者（AI 对话面板 Y 偏移，2026-09-04）一次落位。
//! 组合策略 v1：每属性单槽，后占者赢，不做混合栈。
//!
//! 注册表（ui-base.md §二）的代码镜像——登记一个属性开一道槽：
//! - AI 面板 Y 偏移：目标值语义在基础层（AI 页=0 靠泊 / 终端页=-屏高
//!   屏外），动画只许在缝内插值，不许改目标值。

use std::sync::{Arc, Mutex};

/// 占槽件：采样器（目标值, 时刻 → 当前渲染值）+ 活性探针
/// （帧时钟按需启停的判据——ui-base §四：无活跃动画 = 零额外帧）+
/// 入场重播踢（可选，BAR-079）：面板栈坍缩②「覆盖再召唤=重播入场」——
/// 同一栈态有两种历史，目标值纯函数算不出「该动」；壳层见状态核入场代
/// bump 即踢：(屏外位, 时刻) → 采样器重定基从屏外位起播。动画仍在缝内
/// 插值（铁律不破），踢的只是采样器的重定基输入。无入场概念的缝
/// （键盘 inset）= None
pub struct Occupier {
    pub sampler: Arc<dyn Fn(f32, u64) -> f32 + Send + Sync>,
    pub is_active: Arc<dyn Fn() -> bool + Send + Sync>,
    pub replay: Option<Arc<dyn Fn(f32, u64) + Send + Sync>>,
}

static AI_PANEL_OFFSET_Y: Mutex<Option<Occupier>> = Mutex::new(None);

/// 占槽（后占者赢，ui-base §三 v1）
pub fn occupy_ai_panel_offset_y(o: Occupier) {
    *AI_PANEL_OFFSET_Y.lock().unwrap() = Some(o);
}

/// 拔槽回硬切（插件卸载/禁用——na-regress 禁用 ui-fx 全卷绿的兜底）
pub fn release_ai_panel_offset_y() {
    *AI_PANEL_OFFSET_Y.lock().unwrap() = None;
}

/// 采样（渲染时过缝）：无占槽直通目标值——硬切基座语义（纪律条款
/// 「缝在基础层：无插件占槽，下一帧渲染值 == 目标值」）
pub fn sample_ai_panel_offset_y(target: f32, now_ms: u64) -> f32 {
    let g = AI_PANEL_OFFSET_Y.lock().unwrap();
    match g.as_ref() {
        Some(o) => (o.sampler)(target, now_ms),
        None => target,
    }
}

/// 该槽有活跃动画（帧时钟启停判据；无占槽恒 false = 零额外帧）
pub fn ai_panel_offset_y_active() -> bool {
    AI_PANEL_OFFSET_Y
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|o| (o.is_active)())
}

/// 入场重播踢（BAR-079，坍缩②）：壳层见 AI 入场代 bump 即踢——采样器
/// 重定基到屏外位（目标不变）→ 重播入场动画。无占槽/无 replay = 空操作
pub fn replay_ai_panel_offset_y(offscreen: f32, now_ms: u64) {
    if let Some(r) = AI_PANEL_OFFSET_Y
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|o| o.replay.as_ref())
    {
        r(offscreen, now_ms);
    }
}

// ---- 第二道缝：键盘 inset（chrome 跟随，2026-09-04）----
// 目标值语义在基础层（真实键盘 inset，BAR-006 轮询）；动画只许插值。
// 消费方 = 输入栏/快捷键行渲染与触摸命中（眼手同尺吃同一份采样值）、
// AI 页视口下沿。ui-fx 弹簧平滑：100ms 轮询轨迹是阶梯，纯镜像实看
// 太硬（用户拍板改 ui-base §五 旧判据「只许镜像逐帧 insets」）。
// 注意：终端网格 resize 不过缝——pty resize 抖动红线，永远吃真实值。

static CHROME_IME_INSET: Mutex<Option<Occupier>> = Mutex::new(None);

/// 占槽（后占者赢，ui-base §三 v1）
pub fn occupy_chrome_ime_inset(o: Occupier) {
    *CHROME_IME_INSET.lock().unwrap() = Some(o);
}

/// 拔槽回硬切（插件卸载/禁用）
pub fn release_chrome_ime_inset() {
    *CHROME_IME_INSET.lock().unwrap() = None;
}

/// 采样（渲染/触摸几何时过缝）：无占槽直通目标值——硬切基座语义
pub fn sample_chrome_ime_inset(target: f32, now_ms: u64) -> f32 {
    let g = CHROME_IME_INSET.lock().unwrap();
    match g.as_ref() {
        Some(o) => (o.sampler)(target, now_ms),
        None => target,
    }
}

/// 该槽有活跃动画（帧时钟启停判据）
pub fn chrome_ime_inset_active() -> bool {
    CHROME_IME_INSET
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|o| (o.is_active)())
}

// ---- 第三道缝：配置面板 X 偏移（面板栈 §五B，2026-09-10）----
// 目标值语义在基础层（配置页在栈=0 靠泊 / 不在栈=+屏宽 屏外右缘——
// 左滑召唤、右滑推回，抽屉对称的来向）。动画只许在缝内插值。
// 方向分档吃 fx_ease 同一对臂：入场（+w→0）走减速臂，立场（0→+w）
// 走加速臂——与 AI 面板 Y 缝同一条曲线族，零新曲线。

static CONFIG_PANEL_OFFSET_X: Mutex<Option<Occupier>> = Mutex::new(None);

/// 占槽（后占者赢，ui-base §三 v1）
pub fn occupy_config_panel_offset_x(o: Occupier) {
    *CONFIG_PANEL_OFFSET_X.lock().unwrap() = Some(o);
}

/// 拔槽回硬切（插件卸载/禁用）
pub fn release_config_panel_offset_x() {
    *CONFIG_PANEL_OFFSET_X.lock().unwrap() = None;
}

/// 采样（渲染时过缝）：无占槽直通目标值——硬切基座语义
pub fn sample_config_panel_offset_x(target: f32, now_ms: u64) -> f32 {
    let g = CONFIG_PANEL_OFFSET_X.lock().unwrap();
    match g.as_ref() {
        Some(o) => (o.sampler)(target, now_ms),
        None => target,
    }
}

/// 该槽有活跃动画（帧时钟启停判据；无占槽恒 false = 零额外帧）
pub fn config_panel_offset_x_active() -> bool {
    CONFIG_PANEL_OFFSET_X
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|o| (o.is_active)())
}

/// 入场重播踢（BAR-079，坍缩②）：壳层见配置入场代 bump 即踢——采样器
/// 重定基到屏外右缘（目标不变）→ 重播抽屉入场。无占槽/无 replay = 空操作
pub fn replay_config_panel_offset_x(offscreen: f32, now_ms: u64) {
    if let Some(r) = CONFIG_PANEL_OFFSET_X
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|o| o.replay.as_ref())
    {
        r(offscreen, now_ms);
    }
}

// ---- 第四道缝：文件树面板 X 偏移（面板栈 §五B 三公民，2026-09-11）----
// 目标值语义在基础层（文件树在栈=0 靠泊 / 不在栈=-屏宽 屏外左缘——
// 右滑召唤、左滑推回，与配置家镜像对称的来向）。动画只许在缝内插值。
// 曲线族复用 fx_ease 同一对减速臂，零新曲线（同配置缝纪律）。

static FILETREE_PANEL_OFFSET_X: Mutex<Option<Occupier>> = Mutex::new(None);

/// 占槽（后占者赢，ui-base §三 v1）
pub fn occupy_filetree_panel_offset_x(o: Occupier) {
    *FILETREE_PANEL_OFFSET_X.lock().unwrap() = Some(o);
}

/// 拔槽回硬切（插件卸载/禁用）
pub fn release_filetree_panel_offset_x() {
    *FILETREE_PANEL_OFFSET_X.lock().unwrap() = None;
}

/// 采样（渲染时过缝）：无占槽直通目标值——硬切基座语义
pub fn sample_filetree_panel_offset_x(target: f32, now_ms: u64) -> f32 {
    let g = FILETREE_PANEL_OFFSET_X.lock().unwrap();
    match g.as_ref() {
        Some(o) => (o.sampler)(target, now_ms),
        None => target,
    }
}

/// 该槽有活跃动画（帧时钟启停判据；无占槽恒 false = 零额外帧）
pub fn filetree_panel_offset_x_active() -> bool {
    FILETREE_PANEL_OFFSET_X
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|o| (o.is_active)())
}

/// 入场重播踢（BAR-079，坍缩②）：壳层见文件树入场代 bump 即踢——采样器
/// 重定基到屏外左缘（目标不变）→ 重播抽屉入场。无占槽/无 replay = 空操作
pub fn replay_filetree_panel_offset_x(offscreen: f32, now_ms: u64) {
    if let Some(r) = FILETREE_PANEL_OFFSET_X
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|o| o.replay.as_ref())
    {
        r(offscreen, now_ms);
    }
}
