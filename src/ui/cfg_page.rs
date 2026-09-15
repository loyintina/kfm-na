//! cfg_page.rs — 配置页三层目录状态核（主题宪法 §五「双池的目录语义」
//! 二版，2026-09-13 用户拍板；核心层纯逻辑零 IO，A 档钉）。
//!
//! 四版增订（2026-09-13，kfmv4 实证 4 图量化拍板）：
//! - 字段框行 4 格 / 下池行 4.5 格 / 行隙 1.5 格 / 内边距 2 格
//!   （标签列 12 格已于十四修退役——见下）
//! - **上池像素滚动**（§五「超出部分上池内滚动」条款兑现，v1a 欠账）：
//!   upper_scroll px 状态 + scroll_upper_by（clamp [0, 内容高−池高]，
//!   到位不空涨代际）；几何自由函数全带 scroll 维（眼手同尺不漏维）
//!
//! 十四修增订（2026-09-14，宪法 §五 字段框行条款重订，用户拍板）：
//! - **标签块/值框宽随文字动态**（实量宽入参 + 双侧文内边距 1.5 格；
//!   标签锚左、值锚右、间隔 ≥3 格、值框最小 4 格+边距、下拉行 +▼ 位
//!   45px）——固定 12 格标签列退役
//! - 超长**逐字贪心换行 ≤2 行**（wrap_field_lines），再超涂装裁剪
//! - **末行距池底 1 格**（upper_content_h 末尾 +FIELD_BOTTOM_PAD）
//!
//! 十五修增订（2026-09-14，宪法 §五/§六 动画条款，用户拍板）：
//! - **下池光标滑行**：点选聚焦 = 光标行号弹簧滑向新行（fx_spring
//!   欠阻尼同核）——**BAR-094 改判（2026-09-15 用户拍板）**：弹簧
//!   换 250ms ease-in-out cubic 定时缓动（与视口平移同钟同曲线，
//!   光标滑行与上池内容平移严格同步；欠阻尼过冲从未被用户要求）；
//!   内容与页色即时切换不等光标；切标签页光标直接落首行不滑行；
//!   set_rows clamp 同步落点不动画
//! - **下拉开合两件**：展开生长 0→全高 250ms ease-out（fx_ease
//!   同族）；选中收起 = 选中细框即时落新行 + 面板 180ms ease-in 收起；
//!   开合中选项不命中（壳侧闸），收起中余影不穿透触摸
//!   （dropdown_dismiss_now 归零）
//!
//! 二版条款兑现（kfmv4 池卡实证 4 图对齐）：
//! - 根目录（大类）= 首行标签栏（tab_bar.rs 已有，本册不管）
//! - **子目录选择 = 下池**：行表由壳喂（系统管理大类目前仅一行
//!   「系统管理」）；行 = 3 格高圆角框，选中 = accent 渐变描边（涂装侧）
//! - **二级选项 = 上池**：字段框行表（标签列 + 值框），首行是
//!   **下拉行**（如「默认服务器」）——下拉服务上池内容自身的选项集，
//!   **不与下池联动**（推翻初版双向联动条款）
//! - 三级展开 = 全屏页（v1b，本册不及）
//!
//! 九修增订（2026-09-13，宪法 §五 目录语义 7 + §六 跳框条款）：
//! - tab 维（0=系统管理 1=组件池）+ set_tab 切页副作用清零
//!   （聚焦/滚动/下拉/跳框归初态，内容重建归壳）
//! - modal 维（COMPONENTS 下标）= 跳框开合；upper_row_at_y 上池命中
//!   （组件池页点行开跳框，眼手同尺吃 scroll 维）
//!
//! 眼手同尺：涂装与触摸命中读本册同一份几何（lower_row_rect/
//! upper_row_rect/field_label_rect/field_value_rect/trigger_rect/
//! dropdown_panel_rect），壳层不许另算。
//!
//! 数据流：壳持有 settings 解析结果（servers.json/terminal.json），
//! 重建时喂 `set_rows`（下池行）+ `set_upper`（上池字段框行）+
//! `set_options`（下拉选项与选中）；本册管 focus/dropdown/代际。
//! 下拉点选只改 option_sel——换选的业务动作（写 terminal.json 等）
//! 归壳（核心零 IO），壳点选后读 `option_sel()` 执行。任何影响涂装
//! 的变更 bump epoch——配置槽 sig 吃 epoch 一维（漏维 = 陈旧像素
//! 鬼影，ui-base §八 纪律）。

use crate::termview::{CELL_H, CELL_W};
use crate::ui::dual_pool::PoolRect;

/// 下池池行高 = 4.5 格（四版 §五 池行条款：3 格 ×1.5，半格网 §一）
pub const LOWER_ROW_H: u32 = CELL_H * 9 / 2;
/// 上池字段框行高 = 4 格（四版：2 格 ×2，行高 ×2 后 36px 圆角比例 0.25
/// 与 kfmv4 实证偏方正观感同源）
pub const FIELD_ROW_H: u32 = CELL_H * 4;
/// 字段行内值框高 = 3 格，居中于行（六修：上下线各往中心缩半格——
/// 正合 §三 最小容量律：字 1 格 + 上下各 1 格 ≥0.5 格）
pub const FIELD_BOX_H: u32 = CELL_H * 3;
/// 池内容距池框缘的内缩 = 2 格（四版：1 格实机太窄，kfmv4 实证 ≈40px）
pub const POOL_CONTENT_INSET: i64 = CELL_W as i64 * 2;
/// 行间留隙 = 1.5 格（四版 ×1.5；三版 1 格拍板「三级框跟二级框一样
/// 得有间隔」不变，行高放大后同比例跟放）——下池行专用
pub const ROW_GAP: i64 = CELL_W as i64 * 3 / 2;
/// 上池字段框行留隙 = 1 格（七修 2026-09-13 用户拍板：1.5 格减半格——
/// 字段行是同质表项，比下池目录行可密半格）
pub const FIELD_ROW_GAP: i64 = CELL_W as i64;
/// 字段框文内边距（单侧）= 1.5 格（十四修：动态宽度的双侧留白，
/// 与涂装 text_inset 同尺）
pub const FIELD_TEXT_INSET: u32 = CELL_W * 3 / 2;
/// 标签块与值框最小间隔 = 3 格（十四修 2026-09-14 用户拍板）
pub const FIELD_BOX_GAP: u32 = CELL_W * 3;
/// 值框最小宽 = 4 格 + 双侧文内边距（收缩顺序：先保值框下限，
/// 标签块让到上限）
pub const FIELD_VALUE_MIN_W: u32 = CELL_W * 4 + FIELD_TEXT_INSET * 2;
/// 下拉行值框附加宽（右缘 ▼ 三角位；加宽向左吃，右缘不动）
pub const FIELD_TRIANGLE_PAD: u32 = 45;
/// 上池末行距池底框线 = 1 格（十四修用户拍板；upper_content_h 末尾
/// 加上，池高跟随与滚动 clamp 自动多吃这一格）
pub const FIELD_BOTTOM_PAD: u32 = CELL_H;

/// 下池行（子目录）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView {
    pub title: String,
    pub meta: String,
}

/// 上池字段框行：标签列 + 值框；首行 is_dropdown = 下拉行
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpperRow {
    pub label: String,
    pub value: String,
    pub is_dropdown: bool,
}

/// 涂装/判卷快照（D9：gate 值守倒帧与前台帧同一份读数）
#[derive(Debug, Clone)]
pub struct CfgPageSnap {
    pub rows: Vec<RowView>,
    pub focus: usize,
    pub upper: Vec<UpperRow>,
    pub options: Vec<String>,
    pub option_sel: usize,
    /// 选中细框行号浮点位置（两段时序Ⅰ段滑行的瞬时值；收敛后 ==
    /// option_sel as f32）。涂装选中细框吃这维
    pub option_sel_f: f32,
    pub dropdown_open: bool,
    /// 上池滚动 px（四版 §五 滚动条款兑现；0 = 顶）
    pub upper_scroll: i64,
    /// 当前标签（九修：0=系统管理 1=组件池；宪法 §五 目录语义 7）
    pub tab: usize,
    /// 开着的跳框 = COMPONENTS 下标（九修 §六 跳框条款；None = 无模态）
    pub modal: Option<usize>,
    pub epoch: u64,
    /// 下池光标行号（十五修 §五：弹簧滑行的瞬时值，涂装选中框吃这维；
    /// 收敛后 == focus as f32）
    pub cursor_row: f32,
    /// 下拉面板开合进度 0..1（十五修 §六：1 = 全开展开毕；收起动画
    /// 中途 dropdown_open 已 false 而 progress > 0——面板照画渐缩）
    pub dropdown_progress: f32,
    /// 视口平移切页（十七修 §六「面与内容一体」通则）：Some = 平移
    /// 进行中——涂装双代同画（旧代冻结快照带偏移出、新代活态带偏
    /// 移进）；None = 稳态单代
    pub pan: Option<PanSnap>,
}

/// 视口平移域（十七修 §六）：Page = 页面级（标签切换：双池框+内容
/// 整体平移，视口 = 页环）；Upper = 上池级（下池选行：上池内容平移，
/// 视口 = 上池框，双池框不动）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanScope {
    Page,
    Upper,
}

/// 旧代冻结快照（十七修 §六 双代同画：旧代 = 切换瞬间的状态封存）
#[derive(Debug, Clone)]
pub struct EpochSnap {
    pub rows: Vec<RowView>,
    pub upper: Vec<UpperRow>,
    pub options: Vec<String>,
    pub option_sel: usize,
    pub upper_scroll: i64,
    pub focus: usize,
    pub cursor_row: f32,
    /// 页面级专用：旧双池几何（新代池框可随池高弹簧变，旧代冻结）
    pub pool: crate::ui::dual_pool::DualPoolSnap,
    /// 页面级专用：旧页色（整页换色瞬时随新代，旧代带旧色出）
    pub accent: crate::ui::accent::AccentPair,
}

/// 平移瞬时值（snap 按 now 求值后的涂装读数）
#[derive(Debug, Clone)]
pub struct PanSnap {
    pub scope: PanScope,
    /// +1 = 选择前进（内容左移：旧左出、新右进）；−1 = 后退（右移）
    pub dir: i8,
    /// 缓动后进度 0..1（十八修：ease-in-out cubic；1 = 贴死收敛）
    pub t: f32,
    pub old: Box<EpochSnap>,
}

/// 视口平移切页时长 ms（十七修 §六：250ms；十八修 §七：曲线换
/// ease-in-out cubic——ease-out 起步即满速读感「太块」，用户实机
/// 录屏拍板）
pub const PAN_MS: u64 = 250;
/// 视口平移双代留隙 G（十八修 §七 留隙律）：页面级 = 2 格（静态时
/// 池卡外缘距页内容带缘 1 格的两倍）；平移距 = 视口宽 + G——内容
/// 轴上恒为「旧代 | 隙 G | 新代」隐藏布局
pub const PAN_GAP_PAGE: i64 = crate::ui::dual_pool::POOL_SIDE_PAD as i64;
/// 上池级留隙 G = 2×POOL_CONTENT_INSET（内容缘距池框缘 2 格的两倍）
pub const PAN_GAP_UPPER: i64 = POOL_CONTENT_INSET * 2;

/// 平移双代偏移（十九修 D8：合成域换算唯一来源，涂装域 softbuffer
/// 兜底同吃这把尺）：dir=+1 前进（旧左出新右进）、−1 后退镜像；
/// 返回 (d_old, d_new) 屏像素。留隙律：任意时刻 新代左缘 − 旧代右缘
/// = dir×G（考题 pan_offsets_forward_backward_and_gap_law 钉死）
pub fn pan_offsets(dir: i8, t: f32, travel: i64) -> (i64, i64) {
    (
        -i64::from(dir) * (t * travel as f32).round() as i64,
        i64::from(dir) * ((1.0 - t) * travel as f32).round() as i64,
    )
}

/// 下拉展开时长 ms（十五修 §六：生长 0→全高 ease-out）
pub const DROPDOWN_ENTER_MS: u64 = 250;
/// 下拉收起时长 ms（十五修 §六：选中细框即时落新行，面板 ease-in 收）
pub const DROPDOWN_EXIT_MS: u64 = 180;
/// 下拉点选Ⅰ段：选中细框滑向新行的时长 ms（2026-09-14 用户拍板两段
/// 时序：Ⅰ段滑行（面板冻结）→ Ⅱ段面板才收；点当前行/外点无Ⅰ段）
pub const DROPDOWN_PICK_MOVE_MS: u64 = 160;

pub struct CfgPage {
    rows: Vec<RowView>,
    focus: usize,
    upper: Vec<UpperRow>,
    options: Vec<String>,
    option_sel: usize,
    dropdown_open: bool,
    upper_scroll: i64,
    tab: usize,
    modal: Option<usize>,
    epoch: u64,
    /// 下池光标缓动起点（十五修立，BAR-094 改缓动核）：起点位（像素
    /// 域——重定基时 = 当时缓动瞬时值 × 行步进；见 cursor_row 注）
    cursor_from: f32,
    cursor_start_ms: u64,
    /// 下拉进度弹簧（十五修 §六）：起点进度（重定基时 = 当时进度）
    dd_from: f32,
    dd_start_ms: u64,
    /// 下拉点选两段时序（2026-09-14 用户拍板）：Some((sel_from 行号
    /// 浮点, panel_from 冻结进度, start_ms)) = Ⅰ段选中细框滑行中或
    /// Ⅱ段收起账挂在这条上（惰式求值：progress/sel_f/fx 全从 now
    /// 推，不收尾归一化——收起终点恒 0、sel 终点恒 option_sel）
    pick_move: Option<(f32, f32, u64)>,
    /// 视口平移切页（十七修 §六）：Some((scope, dir, start_ms, 旧代
    /// 冻结快照)) = 平移账；惰式求值同 pick_move（snap/fx 从 now 推，
    /// 贴死后 snap 出 None，账留待下一次切换覆盖）
    pan: Option<(PanScope, i8, u64, EpochSnap)>,
}

impl CfgPage {
    pub fn new() -> Self {
        CfgPage {
            rows: Vec::new(),
            focus: 0,
            upper: Vec::new(),
            options: Vec::new(),
            option_sel: 0,
            dropdown_open: false,
            upper_scroll: 0,
            tab: 0,
            modal: None,
            epoch: 0,
            cursor_from: 0.0,
            cursor_start_ms: 0,
            dd_from: 0.0,
            dd_start_ms: 0,
            pick_move: None,
            pan: None,
        }
    }

    /// 喂下池行表（壳重建：大类切换后）。focus 越界 clamp；
    /// 行表变了 bump 代际。clamp 落点不动画（十五修：光标同步落点
    /// ——clamp 不是用户点选，不播滑行）
    pub fn set_rows(&mut self, rows: Vec<RowView>) {
        if rows != self.rows {
            self.rows = rows;
            self.epoch += 1;
        }
        if self.focus >= self.rows.len() {
            self.focus = self.rows.len().saturating_sub(1);
            self.cursor_from = self.focus as f32 * (LOWER_ROW_H as i64 + ROW_GAP) as f32;
            self.epoch += 1;
        }
    }

    /// 喂上池字段框行表（下拉换选/数据变更后由壳重建）
    pub fn set_upper(&mut self, upper: Vec<UpperRow>) {
        if upper != self.upper {
            self.upper = upper;
            self.epoch += 1;
        }
    }

    /// 喂下拉选项表与选中项（壳从 terminal.json defaultSession 解析）。
    /// sel 越界 clamp；变了才 bump
    pub fn set_options(&mut self, options: Vec<String>, sel: usize) {
        let sel = sel.min(options.len().saturating_sub(1));
        if options != self.options || sel != self.option_sel {
            self.options = options;
            self.option_sel = sel;
            self.pick_move = None; // 选项表换版，挂账的两段时序作废
            self.epoch += 1;
        }
    }

    pub fn focus(&self) -> usize {
        self.focus
    }

    pub fn option_sel(&self) -> usize {
        self.option_sel
    }

    pub fn dropdown_open(&self) -> bool {
        self.dropdown_open
    }

    /// 当前标签（九修：0=系统管理 1=组件池）
    pub fn tab(&self) -> usize {
        self.tab
    }

    /// 切标签（标签栏点选后壳调用）：内容重建归壳（set_rows/set_upper
    /// 判等不空涨）；本册负责切页副作用清零——聚焦归首行（十五修：
    /// 光标直接落首行不滑行）、上池滚动归零、下拉/跳框全收（新页不
    /// 继承旧页的浮层，下拉余影也即时清零）。同标重点不空涨。
    /// 十七修 §六「面与内容一体」：切标签 = **页面级视口平移**——
    /// 旧代（行表/上池/选项/滚动/双池几何/页色）冻结挂账，涂装双代
    /// 同画；方向律：标签右移（i 变大）= 前进 = 内容左移（dir +1）
    pub fn set_tab(
        &mut self,
        i: usize,
        now_ms: u64,
        pool: crate::ui::dual_pool::DualPoolSnap,
        accent: crate::ui::accent::AccentPair,
    ) {
        if i == self.tab {
            return;
        }
        let dir: i8 = if i > self.tab { 1 } else { -1 };
        let old = self.epoch_snap(now_ms, pool, accent);
        self.tab = i;
        self.focus = 0;
        self.cursor_from = 0.0;
        self.upper_scroll = 0;
        self.dropdown_open = false;
        self.dd_from = 0.0;
        self.pick_move = None;
        self.modal = None;
        self.pan = Some((PanScope::Page, dir, now_ms, old));
        self.epoch += 1;
    }

    /// 当前状态冻结成旧代快照（十七修 §六；光标/滚动吃 now 瞬时值，
    /// 下拉面板不进快照——切页前已收）
    fn epoch_snap(
        &self,
        now_ms: u64,
        pool: crate::ui::dual_pool::DualPoolSnap,
        accent: crate::ui::accent::AccentPair,
    ) -> EpochSnap {
        EpochSnap {
            rows: self.rows.clone(),
            upper: self.upper.clone(),
            options: self.options.clone(),
            option_sel: self.option_sel,
            upper_scroll: self.upper_scroll,
            focus: self.focus,
            cursor_row: self.cursor_row(now_ms),
            pool,
            accent,
        }
    }

    /// 开着的跳框（COMPONENTS 下标；None = 无模态）
    pub fn modal(&self) -> Option<usize> {
        self.modal
    }

    /// 开跳框（组件池页上池行点按；宪法 §六 跳框条款）
    pub fn open_modal(&mut self, i: usize) {
        if self.modal != Some(i) {
            self.modal = Some(i);
            self.epoch += 1;
        }
    }

    /// 收跳框（点框外/关闭钮）；关着再关 = 不空涨代际
    pub fn close_modal(&mut self) {
        if self.modal.is_some() {
            self.modal = None;
            self.epoch += 1;
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// 点选下池行（子目录切换 = 上池内容跟换，重建归壳）。
    /// 同标重点不重掷（代际不空涨）。十五修 §五：光标从当前位置
    /// 重定基缓动滑向新行（内容/页色即时切换不等光标）；BAR-094：
    /// 缓动核 = 250ms ease-in-out 与本刻挂账的平移同钟同曲线（同步
    /// 滑行，无过冲）。
    /// 十七修 §六「面与内容一体」：选行 = **上池级视口平移**——旧
    /// 上池内容冻结挂账双代同画，双池框/下池不动；方向律同切标签
    /// （光标下移 = 前进 = 内容左移）
    pub fn select(
        &mut self,
        i: usize,
        now_ms: u64,
        pool: crate::ui::dual_pool::DualPoolSnap,
        accent: crate::ui::accent::AccentPair,
    ) {
        if self.rows.is_empty() {
            return;
        }
        let i = i.min(self.rows.len() - 1);
        if i != self.focus {
            let dir: i8 = if i > self.focus { 1 } else { -1 };
            let old = self.epoch_snap(now_ms, pool, accent);
            let stride = (LOWER_ROW_H as i64 + ROW_GAP) as f32;
            self.cursor_from = self.cursor_row(now_ms) * stride;
            self.cursor_start_ms = now_ms;
            self.focus = i;
            self.pan = Some((PanScope::Upper, dir, now_ms, old));
            self.epoch += 1;
        }
    }

    /// 下池光标行号（十五修 §五 立滑行；BAR-094 改判 2026-09-15）：
    /// 250ms ease-in-out cubic 定时缓动（PAN_MS 同钟同曲线——点选时
    /// select 同刻挂平移账，光标滑行与上池内容平移严格同步），收敛后
    /// == focus。缓动在像素域跑（from 起点像素域，对外仍报行号——
    /// 涂装/快照零改尺）。**无过冲**：ease-in-out 全程单调不过靶，
    /// 欠阻尼弹簧的回弹（≈2.5% 屏高「墩一下」）从未被用户要求，
    /// 真机逐帧判「瞬移+过冲」即此病灶
    pub fn cursor_row(&self, now_ms: u64) -> f32 {
        let stride = (LOWER_ROW_H as i64 + ROW_GAP) as f32;
        let from = self.cursor_from;
        let target = self.focus as f32 * stride;
        if from == target {
            return self.focus as f32;
        }
        let elapsed = now_ms.saturating_sub(self.cursor_start_ms);
        let t = (elapsed.min(PAN_MS)) as f32 / PAN_MS as f32;
        (from + (target - from) * crate::ui::fx_ease::ease_in_out_cubic(t)) / stride
    }

    /// 光标滑行活性探针（帧泵闸）：缓动未到时长且确有位移 = true
    pub fn cursor_fx_active(&self, now_ms: u64) -> bool {
        let stride = (LOWER_ROW_H as i64 + ROW_GAP) as f32;
        self.cursor_from != self.focus as f32 * stride
            && now_ms.saturating_sub(self.cursor_start_ms) < PAN_MS
    }

    /// 两段时序挂账未收敛 = true（Ⅰ段滑行中或Ⅱ段收起中）——
    /// toggle/pick/dismiss 的有效开合态判定单源
    fn pick_move_live(&self, now_ms: u64) -> bool {
        self.pick_move
            .is_some_and(|(_, _, s)| now_ms < s + DROPDOWN_PICK_MOVE_MS + DROPDOWN_EXIT_MS)
    }

    /// 上池下拉框开合（十五修 §六：进度从当前值重定基续走——开着
    /// 再点 = 从当前展开度收，关着再点 = 从当前余影长）。两段时序
    /// 挂账未收敛时 = 取消挂账按收处理（挂账已收敛 = 关着，重开
    /// fresh——账面烂账不许把重开误判成收；普通收起中途无挂账，
    /// raw 关态重开续自余影，与旧契一致）
    pub fn toggle_dropdown(&mut self, now_ms: u64) {
        let p = self.dropdown_progress(now_ms);
        let was_open = self.dropdown_open || self.pick_move_live(now_ms);
        self.dd_from = p;
        self.dd_start_ms = now_ms;
        self.dropdown_open = !was_open;
        self.pick_move = None;
        self.epoch += 1;
    }

    /// 下拉面板开合进度 0..1（十五修 §六）：开着 = 从 dd_from 长向 1
    /// （250ms ease-out），关着 = 从 dd_from 收向 0（180ms ease-in）；
    /// 超时贴死端点。两段时序挂账中（2026-09-14 用户拍板）：Ⅰ段
    /// （选中细框滑行 160ms）面板**冻结**在点选时进度，Ⅱ段从冻结
    /// 进度 180ms ease-in 收向 0
    pub fn dropdown_progress(&self, now_ms: u64) -> f32 {
        if let Some((_, panel_from, start)) = self.pick_move {
            let move_end = start + DROPDOWN_PICK_MOVE_MS;
            if now_ms < move_end {
                return panel_from; // Ⅰ段：面板冻结等选中细框滑到
            }
            let e = now_ms - move_end;
            let t = (e.min(DROPDOWN_EXIT_MS)) as f32 / DROPDOWN_EXIT_MS as f32;
            return panel_from * (1.0 - crate::ui::fx_ease::ease_in_cubic(t));
        }
        let elapsed = now_ms.saturating_sub(self.dd_start_ms);
        if self.dropdown_open {
            let t = (elapsed.min(DROPDOWN_ENTER_MS)) as f32 / DROPDOWN_ENTER_MS as f32;
            if t >= 1.0 {
                1.0
            } else {
                self.dd_from + (1.0 - self.dd_from) * crate::ui::fx_ease::ease_out_cubic(t)
            }
        } else {
            let t = (elapsed.min(DROPDOWN_EXIT_MS)) as f32 / DROPDOWN_EXIT_MS as f32;
            if t >= 1.0 {
                0.0
            } else {
                self.dd_from * (1.0 - crate::ui::fx_ease::ease_in_cubic(t))
            }
        }
    }

    /// 选中细框的行号浮点位置（两段时序：Ⅰ段从旧行 ease-out 滑向
    /// 新行；无挂账/收敛后 == option_sel）。涂装选中细框吃这维
    pub fn option_sel_f(&self, now_ms: u64) -> f32 {
        if let Some((sel_from, _, start)) = self.pick_move {
            let t = (now_ms.saturating_sub(start).min(DROPDOWN_PICK_MOVE_MS)) as f32
                / DROPDOWN_PICK_MOVE_MS as f32;
            sel_from + (self.option_sel as f32 - sel_from) * crate::ui::fx_ease::ease_out_cubic(t)
        } else {
            self.option_sel as f32
        }
    }

    /// 下拉开合活性探针（帧泵闸）：展开未满 / 收起未尽 / 两段时序
    /// Ⅰ段滑行中或Ⅱ段未收尽 = true
    pub fn dropdown_fx_active(&self, now_ms: u64) -> bool {
        if let Some((_, _, start)) = self.pick_move {
            if now_ms < start + DROPDOWN_PICK_MOVE_MS {
                return true; // Ⅰ段滑行中
            }
            return self.dropdown_progress(now_ms) > 0.0; // Ⅱ段未收尽
        }
        let p = self.dropdown_progress(now_ms);
        if self.dropdown_open { p < 1.0 } else { p > 0.0 }
    }

    /// 下拉框点选（2026-09-14 用户拍板两段时序，宪法 §六②）：
    /// 点**其他**行 = 换选 + dropdown_open 即翻 false（壳的触发器
    /// 重开路由不被账面欺骗）+ 挂账两段——Ⅰ段选中细框 160ms 滑向
    /// 新行（面板冻结等它），Ⅱ段面板 180ms ease-in 自动收；点
    /// **当前**行 = 无Ⅰ段直接收（挂账已收敛的重点 = 不空涨代际）。
    /// 业务动作（写配置/重建上池）归壳——壳在本调用后读
    /// `option_sel()` 执行
    pub fn dropdown_pick(&mut self, i: usize, now_ms: u64) {
        if self.options.is_empty() {
            return;
        }
        let i = i.min(self.options.len() - 1);
        if i != self.option_sel {
            let panel_from = self.dropdown_progress(now_ms);
            let sel_from = self.option_sel_f(now_ms);
            self.option_sel = i;
            self.dropdown_open = false; // 有效态即关，动画账全挂 pick_move
            self.pick_move = Some((sel_from, panel_from, now_ms));
            self.epoch += 1;
        } else if self.dropdown_open || self.pick_move_live(now_ms) {
            self.dd_from = self.dropdown_progress(now_ms);
            self.dd_start_ms = now_ms;
            self.dropdown_open = false;
            self.pick_move = None;
            self.epoch += 1;
        }
    }

    /// 下拉框开着时点别处 = 收（宪法 §六 下拉栏常规语义；十五修：
    /// 同选中收起的 180ms ease-in 动画）。两段时序挂账中 = 取消滑行
    /// 从当前进度直接收；挂账已收敛/普通收起中途 = 不空涨代际
    pub fn dismiss_dropdown(&mut self, now_ms: u64) {
        if self.dropdown_open || self.pick_move_live(now_ms) {
            self.dd_from = self.dropdown_progress(now_ms);
            self.dd_start_ms = now_ms;
            self.dropdown_open = false;
            self.pick_move = None;
            self.epoch += 1;
        }
    }

    /// 收起中余影被点 = 即时清零（十五修 §六：余影不穿透触摸——面板
    /// 已判定收，余影只是视觉尾巴，点按处交互必须立即让位）。两段
    /// 时序挂账残留一并作废
    pub fn dropdown_dismiss_now(&mut self) {
        if self.pick_move.take().is_some() {
            self.dropdown_open = false;
            self.dd_from = 0.0;
            self.epoch += 1;
        } else if !self.dropdown_open && self.dd_from > 0.0 {
            self.dd_from = 0.0;
            self.epoch += 1;
        }
    }

    /// 上池滚动 px（0 = 顶；四版 §五 滚动条款）
    pub fn upper_scroll(&self) -> i64 {
        self.upper_scroll
    }

    /// 上池像素滚动（四版：1:1 跟手；dy>0 = 内容上移看更后）。
    /// clamp [0, 内容高−池高]；到位 = false 不空涨代际（回弹感留 v1b）
    pub fn scroll_upper_by(&mut self, dy: i64, pool_h: u32) -> bool {
        let max = self.upper_content_h().saturating_sub(pool_h) as i64;
        let new = (self.upper_scroll + dy).clamp(0, max.max(0));
        if new != self.upper_scroll {
            self.upper_scroll = new;
            self.epoch += 1;
            true
        } else {
            false
        }
    }

    // ---- 几何（眼手同尺唯一来源）----

    /// 下池命中：y 落第几行（出池/行间隙 = None）
    pub fn lower_row_at_y(&self, y: i64, lower: &PoolRect) -> Option<usize> {
        for i in 0..self.rows.len() {
            let r = lower_row_rect(i, lower);
            if y >= r.y && y < r.y + r.h as i64 {
                return Some(i);
            }
        }
        None
    }

    /// 上池命中：y 落第几行字段框（九修，组件池页点行开跳框用；
    /// 吃 upper_scroll 同一维——眼手同尺；行间隙/池外 = None）
    pub fn upper_row_at_y(&self, y: i64, upper: &PoolRect, scroll: i64) -> Option<usize> {
        for i in 0..self.upper.len() {
            let r = upper_row_rect(i, upper, scroll);
            if y >= r.y && y < r.y + r.h as i64 {
                return Some(i);
            }
        }
        None
    }

    /// 上池下拉触发器矩形（首行字段框的值框位，§六 触发器条款）——
    /// 触发器随上池内容一起滚（本页 scroll 喂 self.upper_scroll）。
    /// 十四修动态宽度：label_tw/value_tw = 首行标签/值的实量宽
    /// （调用方 measure_items 同尺量出——眼手同尺不漏维）
    pub fn trigger_rect(&self, upper: &PoolRect, label_tw: u32, value_tw: u32) -> PoolRect {
        let dd = self.upper.first().is_none_or(|r| r.is_dropdown);
        trigger_rect(upper, self.upper_scroll, dd, label_tw, value_tw)
    }

    /// 下拉 panel 矩形（顶部栏向下弹——宪法 §六：方向反了会弹出屏外）：
    /// 触发器下缘起，行数 = 选项数，最高不出配置页可视区（壳喂 max_h）。
    /// 十七修 BAR-090：content_w_min = 选项最长文实量宽 + 双侧边距
    pub fn dropdown_panel_rect(
        &self,
        upper: &PoolRect,
        max_h: u32,
        label_tw: u32,
        value_tw: u32,
        content_w_min: u32,
    ) -> PoolRect {
        let dd = self.upper.first().is_none_or(|r| r.is_dropdown);
        dropdown_panel_rect(
            self.options.len(),
            upper,
            max_h,
            self.upper_scroll,
            dd,
            label_tw,
            value_tw,
            content_w_min,
        )
    }

    /// 下拉 panel 命中：y 落第几行（panel 外 = None）
    pub fn dropdown_item_at_y(
        &self,
        y: i64,
        upper: &PoolRect,
        max_h: u32,
        label_tw: u32,
        value_tw: u32,
        content_w_min: u32,
    ) -> Option<usize> {
        let p = self.dropdown_panel_rect(upper, max_h, label_tw, value_tw, content_w_min);
        if y < p.y || y >= p.y + p.h as i64 {
            return None;
        }
        let i = ((y - p.y) as u32 / FIELD_ROW_H) as usize;
        (i < self.options.len()).then_some(i)
    }

    /// 上池内容高（喂 dual_pool.set_upper_content_h）：字段框行 + 留隙
    /// + 末行底距 1 格（十四修：末行不许贴池底框线）
    pub fn upper_content_h(&self) -> u32 {
        let n = self.upper.len() as u32;
        if n == 0 {
            return 0;
        }
        POOL_CONTENT_INSET as u32
            + n * FIELD_ROW_H
            + (n - 1) * FIELD_ROW_GAP as u32
            + FIELD_BOTTOM_PAD
    }

    /// 快照（十五修：吃 now_ms——光标弹簧/下拉进度是时间函数）
    pub fn snap(&self, now_ms: u64) -> CfgPageSnap {
        CfgPageSnap {
            rows: self.rows.clone(),
            focus: self.focus,
            upper: self.upper.clone(),
            options: self.options.clone(),
            option_sel: self.option_sel,
            option_sel_f: self.option_sel_f(now_ms),
            dropdown_open: self.dropdown_open,
            upper_scroll: self.upper_scroll,
            tab: self.tab,
            modal: self.modal,
            epoch: self.epoch,
            cursor_row: self.cursor_row(now_ms),
            dropdown_progress: self.dropdown_progress(now_ms),
            pan: self.pan_snap(now_ms),
        }
    }

    /// 平移瞬时值求值（十七修 §六：250ms；十八修 §七：ease-in-out
    /// cubic；贴死出 None，账留待下一次切换覆盖——fresh 账直接换掉
    /// 旧账不续弹）
    fn pan_snap(&self, now_ms: u64) -> Option<PanSnap> {
        self.pan.as_ref().and_then(|(scope, dir, start, old)| {
            let raw = (now_ms.saturating_sub(*start)).min(PAN_MS) as f32 / PAN_MS as f32;
            if raw >= 1.0 {
                None
            } else {
                Some(PanSnap {
                    scope: *scope,
                    dir: *dir,
                    t: crate::ui::fx_ease::ease_in_out_cubic(raw),
                    old: Box::new(old.clone()),
                })
            }
        })
    }

    /// 平移活性探针（帧泵闸）：账未贴死 = true
    pub fn pan_active(&self, now_ms: u64) -> bool {
        self.pan
            .as_ref()
            .is_some_and(|(_, _, start, _)| now_ms < *start + PAN_MS)
    }

    /// Upper 域平移进行中（BAR-095 分域律）：壳层据此选池高喂入
    /// 方式——Upper 域 = glide 缓动（池高与光标/平移同步），Page 域
    /// 与无平移 = set 直通（新页池高起步帧就位）
    pub fn pan_upper_active(&self, now_ms: u64) -> bool {
        self.pan.as_ref().is_some_and(|(scope, _, start, _)| {
            *scope == PanScope::Upper && now_ms < *start + PAN_MS
        })
    }
}

impl Default for CfgPage {
    fn default() -> Self {
        Self::new()
    }
}

// ---- 几何自由函数（眼手同尺唯一来源：CfgPage 方法与涂装侧共用）----

/// 下池第 i 行的框矩形（池内缘内缩 1 格，逐行 3 格高 + 留隙）
pub fn lower_row_rect(i: usize, lower: &PoolRect) -> PoolRect {
    PoolRect {
        x: lower.x + POOL_CONTENT_INSET,
        y: lower.y + POOL_CONTENT_INSET + (i as i64) * (LOWER_ROW_H as i64 + ROW_GAP),
        w: lower.w.saturating_sub((POOL_CONTENT_INSET * 2) as u32),
        h: LOWER_ROW_H,
    }
}

/// 上池第 i 行字段框矩形（池内缘内缩 2 格，逐行 4 格高 + 留隙 1 格
/// ——七修：上池行隙减半格 ROW_GAP→FIELD_ROW_GAP）——
/// scroll = 上池滚动 px（四版：内容随滚动整体上移，触发器不例外）
pub fn upper_row_rect(i: usize, upper: &PoolRect, scroll: i64) -> PoolRect {
    PoolRect {
        x: upper.x + POOL_CONTENT_INSET,
        y: upper.y + POOL_CONTENT_INSET + (i as i64) * (FIELD_ROW_H as i64 + FIELD_ROW_GAP)
            - scroll,
        w: upper.w.saturating_sub((POOL_CONTENT_INSET * 2) as u32),
        h: FIELD_ROW_H,
    }
}

/// 字段框的标签块矩形（十四修动态宽度，宪法 §五 字段框行条款）：
/// 锚行左缘，宽 = 标签实量宽 + 双侧文内边距，上限 = 行宽 − 3 格间隔
/// − 值框最小宽（收缩顺序：先保值框下限）；3 格高居中于行
pub fn field_label_rect(row: &PoolRect, label_text_w: u32) -> PoolRect {
    let min = FIELD_TEXT_INSET * 2;
    let max = row
        .w
        .saturating_sub(FIELD_BOX_GAP + FIELD_VALUE_MIN_W)
        .max(min);
    let w = (label_text_w + FIELD_TEXT_INSET * 2).clamp(min, max);
    PoolRect {
        x: row.x,
        y: row.y + (FIELD_ROW_H - FIELD_BOX_H) as i64 / 2,
        w,
        h: FIELD_BOX_H,
    }
}

/// 字段框的值框矩形（十四修动态宽度）：锚行右缘，宽 = 值实量宽 +
/// 双侧文内边距（+ 下拉行 ▼ 三角位），下限 FIELD_VALUE_MIN_W，
/// 上限 = 行宽 − 3 格间隔 − 标签块实际宽；3 格高居中于行（六修不变）
pub fn field_value_rect(
    row: &PoolRect,
    label_w: u32,
    value_text_w: u32,
    is_dropdown: bool,
) -> PoolRect {
    let tri = if is_dropdown { FIELD_TRIANGLE_PAD } else { 0 };
    let min = FIELD_VALUE_MIN_W + tri;
    let max = row.w.saturating_sub(FIELD_BOX_GAP + label_w).max(min);
    let w = (value_text_w + FIELD_TEXT_INSET * 2 + tri).clamp(min, max);
    PoolRect {
        x: row.x + row.w as i64 - w as i64,
        y: row.y + (FIELD_ROW_H - FIELD_BOX_H) as i64 / 2,
        w,
        h: FIELD_BOX_H,
    }
}

/// 字段框文逐字贪心换行（十四修 §五：单行装不下换行，最多 2 行，
/// 再超进末段由涂装裁剪——行数不爆炸）。widths = 逐字步进宽
/// （measure_items 同尺）；返回 (起, 止, 行宽) 段表，空入 = 零行
pub fn wrap_field_lines(widths: &[f32], max_w: f32) -> Vec<(usize, usize, f32)> {
    const MAX_LINES: usize = 2;
    if widths.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let (mut start, mut acc) = (0usize, 0.0f32);
    for (i, &wd) in widths.iter().enumerate() {
        let full = lines.len() + 1 >= MAX_LINES; // 末段不再断行
        if !full && acc + wd > max_w && i > start {
            lines.push((start, i, acc));
            start = i;
            acc = wd;
        } else {
            acc += wd;
        }
    }
    lines.push((start, widths.len(), acc));
    lines
}

/// 上池下拉触发器矩形 = 首行字段框的值框位（十四修：吃首行标签/值
/// 实量宽与 is_dropdown 维）
pub fn trigger_rect(
    upper: &PoolRect,
    scroll: i64,
    is_dropdown: bool,
    label_tw: u32,
    value_tw: u32,
) -> PoolRect {
    let row0 = upper_row_rect(0, upper, scroll);
    let lb = field_label_rect(&row0, label_tw);
    field_value_rect(&row0, lb.w, value_tw, is_dropdown)
}

/// 下拉 panel 矩形（顶部栏向下弹）：触发器值框下缘起，行数 = 选项数，
/// 最高不出配置页可视区（调用方喂 max_h）；scroll = 上池滚动 px。
/// 十七修 BAR-090：宽 = max(触发器宽, content_w_min)（选项最长文
/// 不裁剪），**右缘与触发器右缘对齐、加宽向左长**——触发器右缘 ≡
/// 池内容内缘（值框锚行右缘恒等式），「左对齐 + 右钳」会把宽度
/// 锁死成触发器宽（钉考题实锤修宪）；左缘钳池内容左内缘
#[allow(clippy::too_many_arguments)]
pub fn dropdown_panel_rect(
    opt_count: usize,
    upper: &PoolRect,
    max_h: u32,
    scroll: i64,
    is_dropdown: bool,
    label_tw: u32,
    value_tw: u32,
    content_w_min: u32,
) -> PoolRect {
    let t = trigger_rect(upper, scroll, is_dropdown, label_tw, value_tw);
    let want = (opt_count as u32) * FIELD_ROW_H;
    let right = t.x + t.w as i64; // 触发器右缘 ≡ 池内容右内缘
    let left_min = upper.x + POOL_CONTENT_INSET;
    let w = t.w.max(content_w_min).min((right - left_min).max(0) as u32);
    PoolRect {
        x: right - w as i64,
        y: t.y + t.h as i64,
        w,
        h: want.min(max_h),
    }
}

// ---- 共享句柄（D9 同源：gate 值守倒帧与前台帧同一份配置页读数）----

use std::sync::{Arc, Mutex, RwLock};

pub type SharedCfgPage = Arc<Mutex<CfgPage>>;

static CFG_PAGE_HANDLE: RwLock<Option<SharedCfgPage>> = RwLock::new(None);

/// 注册（android_app 装配时调一次）；重注册 = 覆盖（热更核新实例）
pub fn register_cfg_page(page: SharedCfgPage) {
    *CFG_PAGE_HANDLE.write().unwrap() = Some(page);
}

/// 读句柄（gate 值守倒帧取快照用；未注册 = None 兜底不画内容）
pub fn cfg_page_handle() -> Option<SharedCfgPage> {
    CFG_PAGE_HANDLE.read().unwrap().clone()
}
