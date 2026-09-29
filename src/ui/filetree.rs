//! filetree.rs — 文件树页内容核（右滑公民 `ChromeSlot::FileTree` 的内容层，
//! 2026-09-26 立）
//!
//! 这是什么：照 kfmv4/nz `src/client/plugins/file-tree/index.tsx`（判据稿
//! §3.1 令牌表，逐像素复刻）的**纯逻辑核**——缩进/密度/行高几何、命中、
//! 懒加载展开状态机、抽屉与光标框的帧值。没有网络、没有像素：壳取数据
//! （`GET /api/fs/list` 的 body 原样喂 `entries_of`），本册记账，涂装只吃
//! `snap_at(now)`。
//!
//! 为什么行高是 90：BAR-178 半格网标定（单行 2.5 格 × CELL_H 36 = 90，
//! 换行长名 3 格 = 108，见 `ROW_H`/`ROW_H_WRAP`；原 86/118 = kfmv4 行高
//! 26 CSS px 的真机实测 pitch 众数，留档）。NA 全物理 px（`FT_CSS` = 本机 dpr），
//! CSS 单位只活在缩进表与设计稿里，进代码即换算。缩进实测锚点（深度
//! 0..4）= 0 / 55 / 104 / 147 / 184 物理 px（= 三角左缘 18 起的**层间距**，
//! 六调后 `tri_x` 0..4 = 18 / 73 / 122 / 165 / 202 逐字保住）。
//!
//! **六调（2026-09-27 用户工单，一页六改）**：①名字与三角盒留
//! `TRI_NAME_GAP_PX`；②根层不画行带（露页底）；③展开/收起两头都让**兄弟行
//! 整体平移**（下半树刚体推下/连续上滑，不再瞬移，见 `SnapShift`/`RetractAnim`）；
//! ④曾展开账 `expanded_mem`（**显式展开入账 / 显式收起摘自己那条 / 级联摘行不动它**），
//! 及其级联恢复（关根再开 = 恢复用户离开时的样子）；
//! ⑤竖条吊到**父行三角**正下方（`bar_x`）；⑥深层保缝——每层步进下限
//! `STEP_MIN_PX` 由「⑤ 的对齐偏量 + 竖条宽 + 保缝」反解，表尾递减档一律兜住。
//! 三角与竖条由此**分成两根轴**（`tri_x` 累进 / `bar_x` 对齐父缘），
//! `indent_px` 只剩「行内容基准 = bar_x」一个义。
//!
//! 坐标口径（眼手同尺的唯一尺，三处同吃一份，谁都不许自己再算）：
//! - 行矩形 = **内容坐标**（`row_rects` 的 y 已减 scroll，动画期再加
//!   `SnapShift::dy`）——涂装画它、命中落它；
//! - `hit(x, y, view_h)` 吃**页面坐标**（未减 scroll 的屏幕坐标），内部
//!   按 y 与行矩形（内容坐标）对表；
//! - 光标 y 是**内容坐标的框心**（框属于行，随行滚 + 随兄弟行平移）——涂装
//!   画在 y − scroll 处、框顶 = cursor_y − (行高 − CURSOR_INSET)/2，滚出
//!   视口由列表裁剪带裁掉。
//!
//! 状态机语义（懒加载，nz 同源）：`rows` 即扁平树（DFS 行序，展开的目录
//! 其后紧跟其子层）——子层没到就没有行。`expanded`（行上的 expanded 位）
//! 记「视觉展开」，`loaded` 记「子层已取」：**已展开 ⟺ 子行在树**（收起
//! 即行即走、账即清，再展开重取），所以 `toggle` 的 `NeedList` 判定可以
//! 只认行在不在。锁窗（同一目录 240/180ms 内二次 toggle 忽略）是 nz
//! §3.2「动画锁只防视觉抖动」的直译。**唯一例外（六调④）**：级联恢复出来的
//! 目录是「展开（▼）但在途（loading）」——那个瞬间行还没到，正是同一条
//! 不变量的在途态（等价于用户刚点下去那一刻）。
//!
//! 抽屉动画复用 cfg 下拉的「刚体 + 顶缘裁剪」机制（cfg_page 十七修 §六③）：
//! 子行钉全高刚体，`drawer_dy = cur_h − full_h`（≤0），涂装按 cur_h 裁顶缘
//! ——展开时刚体从父行下缘滑出（下方子行先入场）。**展开的刚体在子层到位
//! 那一刻起步**（`apply_list` 记账 240ms）；收起 180ms = 锁窗 + 三角回转 +
//! **退场账**（六调③：行即刻走，但被摘走行高总和换成下半树的连续上滑，
//! 见 `RetractAnim`/`retract_dy`——路线 b 保守解，不变量「展开 ⟺ 子行在树」
//! 一行不动）。
//!
//! 颜色常量按 **RRGGBBAA** 书写（规格原文口径：`0x00D4FFFF` = rgba(0,212,
//! 255)、`0x0094B2FF` = teal(0,148,178)，末位 FF 是 alpha）——与 termview
//! 的 AARRGGBB 惯例**相反**，涂装取色走 `rrggbbaa()`，别直搬另一套解包
//! （直搬 = `0x00D4FFFF` 解出 alpha 0，整条光标线画不出来）。
//!
//! 考题：tests/filetree_spec.rs（A 档，带变异抽检）。

use std::collections::{BTreeMap, BTreeSet};

use crate::ui::fx_ease::{ease_in_out_cubic, ease_out_cubic};

// ── 令牌表（§3.1；CSS→物理的一次性换算都集中在这里）────────────────

/// 物理换算系数（本机 dpr，实测）
pub const FT_CSS: f32 = 3.06;
/// 缩进增量表（CSS px，index = 深度）：递减增量，深层钳 2px（防深层目录出屏）
pub const SHIFT_TABLE: [i32; 20] = [
    18, 16, 14, 12, 10, 9, 8, 7, 6, 5, 4, 3, 3, 2, 2, 2, 2, 2, 2, 2,
];
/// 累计缩进总量钳制的 **CSS 原值**（深到表尾仍在加，没这道闸深层会走出屏）。
/// **六调⑤⑥ 后只作留档**：现役钳制吃物理身 `SHIFT_CLAMP_PX`（`tri_x` 里那
/// 一道），本值记录「489 是怎么来的」——160 CSS × 3.06 = 489.6 → 489
pub const SHIFT_CLAMP_CSS: i32 = 160;
/// 钳制上限的物理身（实测锚点取整：160 × 3.06 = 489.6 → 489）
pub const SHIFT_CLAMP_PX: i64 = 489;
/// 单行行高（物理 px）——**BAR-178 半格网标定**：2.5 格 × CELL_H 36 = 90
/// （原 86 = 真机截屏实测 pitch 众数 85.5，见 §三原表）。运行时刻度随
/// pinch 变、本表是基准格（18×36）下的 px 身——基准常量 CELL_W/CELL_H
/// 不变时两账一致；pinch 联动重标定挂账（docs/active/文件树.md 挂账段）
pub const ROW_H: i64 = 90;
/// 换行长名行高（物理 px）——**BAR-178 半格网标定**：3 格 × 36 = 108
/// （两行 36 行距 + 上下各 18 余量；原 118 = 实测两行文本）
pub const ROW_H_WRAP: i64 = 108;
/// 三角盒宽（物理 px）——**三修（2026-09-27 用户终验症①）**：26×28 仍太小。
/// 原版比例取证：nz 判据稿 §3.1 = 行高 26 / 字 11 / 三角 9 CSS（字:三角 ≈
/// 1.22）；真机实测字高 32~33 物理 px 配三角 24 → **字:三角墨高 ≈ 1.37**。
/// 本实现字 44 → 墨高目标 ≈ 32（44/1.37）⇒ 盒 32×34、基形 26×32（26:32 ≈
/// 原版实测 19~26 : 19~22 的横竖比）。常量收拢可调。
pub const TRI_W: i64 = 32;
/// 三角盒高（物理 px，同上）
pub const TRI_H: i64 = 34;
/// 行左强调竖条宽（物理 px）——**单源**：涂装（termview）与几何（本册）
/// 同吃这一个数（旧版 termview 里另有一份 FT_ROW_BAR_W，双源必漂）
pub const ROW_BAR_W: i64 = 6;
/// 三角左缘与竖条的间隙（物理 px）——**三修症⑤**：用户要「跟竖线有一定
/// 距离」（旧版三角左缘 = 缩进量本身，贴着竖条）。基线（ROW_BAR_W + 本值）
/// = 三角左缘；**深度 0 一并右移**（对齐观感，无竖条时也缩）
pub const TRI_GAP_PX: i64 = 12;
/// 三角盒与名字的间隔（物理 px，**六调①**）：名字左缘 = 三角盒右缘 + 本值
/// ——旧版名字紧贴三角盒（`tri_x + TRI_W`），字与三角挤成一团
pub const TRI_NAME_GAP_PX: i64 = 12;
/// 竖条与**父行**三角盒左缘的对齐偏量（物理 px，**六调⑤**，原版实测：竖线
/// 吊在父三角正下方略偏右）
pub const BAR_ALIGN_DX: i64 = 5;
/// 同层「本行三角盒左缘 − 竖条右缘」的保缝下限（物理 px，**六调⑥**）
pub const GAP_MIN_PX: i64 = 6;
/// 每层步进下限（物理 px，由 ⑥ 反解）：步进再小，竖条（对齐父三角 +BAR_ALIGN_DX）
/// 就要压进本行三角盒里——表尾递减档（≤5 CSS）一律由本值兜住
const STEP_MIN_PX: i64 = BAR_ALIGN_DX + ROW_BAR_W + GAP_MIN_PX;
/// 曾展开账上限（**六调④**：超帽丢字典序最小者，确定性）
pub const MEM_CAP: usize = 4096;
/// 兄弟块首尾圆角半径（物理 px = 4 CSS × 3）
pub const ROW_RADIUS: i64 = 12;
/// 光标框内缩（盒高 = 行高 − 本值）
pub const CURSOR_INSET: i64 = 4;
/// 抽屉展开时长 ms（子层到位起算）
pub const DRAWER_OPEN_MS: u64 = 240;
/// 抽屉收起时长 ms（= 收起锁窗；行即走，见模块头注）
pub const DRAWER_CLOSE_MS: u64 = 180;
/// 三角旋转时长 ms（实心 ▶/▼，展开转 90°）
pub const TRI_ROT_MS: u64 = 180;
/// 光标移动时长 ms（ease-out cubic）
pub const CURSOR_MOVE_MS: u64 = 180;
/// 光标上下线色（RRGGBBAA：rgba(0,212,255) + alpha FF）
pub const CURSOR_LINE_RGB: u32 = 0x00D4_FFFF;
/// 光标线 α（kfmv4 rgba(0,212,255,0.7) 的 0.7）
pub const CURSOR_LINE_ALPHA: f32 = 0.7;
/// 光标盒底 accent 垫 α（15%）
pub const CURSOR_FILL_ALPHA: f32 = 0.15;
/// 三角色**留档**（RRGGBBAA：teal(0,148,178)）——**打回改约（症③）**：
/// 那是原版那次召唤随机到的 accent，不是固定色。现役涂装取**页 accent
/// 渐变同源采样**（`RingGradient::sample`），本常量只在考题里钉原版实测值。
pub const CHEVRON_RGB: u32 = 0x0094_B2FF;
/// 光标上线最短（kfmv4 topLineW 下限 20 同款）
pub const CURSOR_NAME_MIN: i64 = 20;
/// 光标上线最远 = 行宽 − 本值（kfmv4 totalW−10 同款）
pub const CURSOR_NAME_INSET: i64 = 10;
/// 光标左强调竖线宽（物理 px）
pub const CURSOR_BAR_W: i64 = 3;
/// 光标上下线线宽（物理 px）
pub const CURSOR_HAIR_W: i64 = 1;

/// RRGGBBAA 解包（见模块头注：本册颜色常量与 termview 的 AARRGGBB 相反）
pub fn rrggbbaa(c: u32) -> (u8, u8, u8, u8) {
    ((c >> 24) as u8, (c >> 16) as u8, (c >> 8) as u8, c as u8)
}

// ── 数据模型 ────────────────────────────────────────────────────────

/// 条目类型（服务端 `kind` 字段的词汇表：只有这两档，别的不认）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Dir,
    File,
}

impl RowKind {
    /// 线上词汇（请求/日志两用——`/api/fs/list` 的 `kind` 原文）
    pub fn as_str(self) -> &'static str {
        match self {
            RowKind::Dir => "dir",
            RowKind::File => "file",
        }
    }

    pub fn is_dir(self) -> bool {
        matches!(self, RowKind::Dir)
    }
}

/// `/api/fs/list` 的一条子项（已解析，未入树）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: RowKind,
    /// 字节数（缺省 0——显示元数据不挡树）
    pub size: u64,
    /// mtime 毫秒（缺省 0，同上）
    pub mtime: u64,
}

/// 平面树的一行（`rows` 就是 DFS 行序的扁平树本身，不是索引）
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// 相对路径（根的子项 = 名字；根本身不做行）
    pub path: String,
    pub name: String,
    /// 深度（0 = 根子层）
    pub depth: usize,
    pub kind: RowKind,
    /// 视觉展开位（与 `FileTreeState::expanded` 同一事实，涂装读这维）
    pub expanded: bool,
    /// 长名换行（行高 108 的那档；量宽侧回填，见 `set_wrap`——BAR-178
    /// redroid 判卷查实生产侧尚无调用方，折行档未接线，挂账）
    pub wrap: bool,
    pub size: u64,
}

// ── 几何（A 档纯函数；涂装与命中同吃）──────────────────────────────

/// 单层缩进增量（CSS px；超表长按末档兜——nz `shift(d)` 的 `?? 2` 同款）
pub fn shift_css(depth: usize) -> i32 {
    SHIFT_TABLE[depth.min(SHIFT_TABLE.len() - 1)]
}

/// 每层步进（物理 px；`depth` = **目标层**，0 层无步进）= max(表值换算,
/// STEP_MIN_PX)。几何核重排（**六调⑤⑥**）：三角独立成轴逐层累进，本函数是
/// 唯一来源；表尾递减档（2 CSS ≈ 6px）不够让开「竖条 + 缝」，由下限兜住
pub fn step_px(depth: usize) -> i64 {
    if depth == 0 {
        return 0;
    }
    ((shift_css(depth - 1) as f32 * FT_CSS).round() as i64).max(STEP_MIN_PX)
}

/// 三角盒左缘（页面 x）：基线 (ROW_BAR_W + TRI_GAP_PX) + Σ_{d=1..depth}
/// step_px(d)，总量钳 SHIFT_CLAMP_PX（深到表尾仍步进，没这道闸深层走出屏）。
/// 0 层 = 基线本身（无竖条也按同尺对齐）。**实测锚点保住**：深度 0..4 的
/// 表值换算与旧累加表同值 → 18 / 73 / 122 / 165 / 202
pub fn tri_x(depth: usize) -> i64 {
    let mut x = ROW_BAR_W + TRI_GAP_PX;
    for d in 1..=depth {
        x += step_px(d);
    }
    x.min(SHIFT_CLAMP_PX)
}

/// 竖条左缘（页面 x）：0 层 = 0（根层没有「嵌套在谁里面」）；否则吊在**父行**
/// 三角盒左缘正下方（+ BAR_ALIGN_DX，**六调⑤**），再钳到「本行三角盒左缘 −
/// (ROW_BAR_W + GAP_MIN_PX)」——三角封顶 489 后父缘不再右移，钳制档由后半段
/// 兜住 ⑥ 的缝（**自证**：∀d 本行三角左缘 − 竖条右缘 ≥ GAP_MIN_PX）
pub fn bar_x(depth: usize) -> i64 {
    if depth == 0 {
        return 0;
    }
    (tri_x(depth - 1) + BAR_ALIGN_DX)
        .min(tri_x(depth) - (ROW_BAR_W + GAP_MIN_PX))
        .max(0)
}

/// 行内容基准（= `bar_x`；0 层 = 0）——**语义已改（六调⑤⑥）**：旧版是缩进表
/// 的累加量（竖条与三角共用一个数），现三角走 `tri_x` 独立成轴，本函数只剩
/// 「竖条左缘 / 行内容左缘」一个义（涂装/探针同吃）
pub fn indent_px(depth: usize) -> i64 {
    bar_x(depth)
}

/// 名字文本左缘（页面 x）：三角盒 + 间隔（六调①）
pub fn name_x(depth: usize) -> i64 {
    tri_x(depth) + TRI_W + TRI_NAME_GAP_PX
}

/// 逐层加深（深层更浓不是更淡）：浅≈0 → 深→0.89
pub fn density(depth: usize) -> f32 {
    1.0 - shift_css(depth) as f32 / 18.0
}

/// 行底纯色平涂 α（2026-09-11 用户裁决「逐行渐变太花」→ 纯色，纵深保留）。
/// **六调②（2026-09-27 用户拍板）**：根层恒 0 = 不画行带，露出页底
/// CARD_PAGE_BG（根层没有「嵌在谁里面」的纵深可言）；深度 ≥1 公式不动
pub fn band_alpha(depth: usize) -> f32 {
    if depth == 0 {
        return 0.0;
    }
    0.05 + density(depth) * 0.26
}

/// 行左强调边 opacity
pub fn border_op(depth: usize) -> f32 {
    0.3 + density(depth) * 0.5
}

/// 行左强调边的绘制门：**深度 0 不画**（nz 原版 `row.depth > 0 ? span : null`
/// ——根层没有「嵌套在谁里面」可言）
pub fn left_bar_on(depth: usize) -> bool {
    depth > 0
}

/// 行高（单行 / 换行两档）
pub fn row_h(kind_wrap: bool) -> i64 {
    if kind_wrap { ROW_H_WRAP } else { ROW_H }
}

/// 树总高（= 各行行高和；滚动上界的唯一来源）
pub fn total_h(rows: &[Row]) -> i64 {
    rows.iter().map(|r| row_h(r.wrap)).sum()
}

/// 第 idx 行的行顶（**内容坐标**；idx 越界按行表末尾之后算）
pub fn row_top(rows: &[Row], idx: usize) -> i64 {
    rows.iter().take(idx).map(|r| row_h(r.wrap)).sum()
}

/// 第 idx 行的行心（内容坐标；光标目标就是它——框心对齐行心）
pub fn row_center(rows: &[Row], idx: usize) -> i64 {
    rows.get(idx)
        .map_or(0, |r| row_top(rows, idx) + row_h(r.wrap) / 2)
}

/// 可见行表 → (行下标, 行顶 y, 行高)，y 为**内容坐标**（已减 scroll）；
/// 只出与视口 [0, view_h) 相交的行（半行压边也出——裁剪是涂装的事）。
/// `sib` = 兄弟行整体平移账（③；None = 无动画）。
///
/// **眼手同尺**：命中吃的那一帧必须与屏上那一帧同位移——抽屉/退场动画期
/// 下半树整体被推移，命中框若按静态内容位判就会点到隔壁行（BAR-145 判例：
/// 命中唯一合法源 = 屏上正显示的那一代）
pub fn row_rects(
    rows: &[Row],
    scroll: i64,
    view_h: i64,
    sib: Option<&SnapShift>,
) -> Vec<(usize, i64, i64)> {
    let mut out = Vec::new();
    if view_h <= 0 {
        return out;
    }
    let mut top = 0i64;
    for (i, r) in rows.iter().enumerate() {
        let h = row_h(r.wrap);
        let shift = match sib {
            Some(s) if i >= s.from_idx => s.dy,
            _ => 0,
        };
        let y = top - scroll + shift;
        top += h;
        if y >= view_h || y + h <= 0 {
            continue;
        }
        out.push((i, y, h));
    }
    out
}

/// 兄弟块首/尾判定（→ 涂装圆角：块首圆上、块尾圆下）。行表是 DFS 行序，
/// 「前一行更浅或无前一行」即块首（nz flatten 的 first/last 等价物）
pub fn sibling_ends(rows: &[Row], idx: usize) -> (bool, bool) {
    let Some(r) = rows.get(idx) else {
        return (false, false);
    };
    let first = idx == 0 || rows[idx - 1].depth < r.depth;
    let last = idx + 1 >= rows.len() || rows[idx + 1].depth < r.depth;
    (first, last)
}

/// 抽屉刚体全高 = 父行之后连续子块的行高和（无子行 = 0，dy 恒 0）
pub fn drawer_full_h(rows: &[Row], parent_idx: usize) -> i64 {
    let Some(p) = rows.get(parent_idx) else {
        return 0;
    };
    rows.iter()
        .skip(parent_idx + 1)
        .take_while(|r| r.depth > p.depth)
        .map(|r| row_h(r.wrap))
        .sum()
}

/// 光标上线长 = 名字实量宽 clamp(20, 行宽 − 10)（量宽由涂装喂入，本函数是
/// 唯一钳制源；行窄到 clamp 下限都放不下时给下限，让线短到看得见）
pub fn cursor_line_w(name_w: i64, row_w: i64) -> i64 {
    name_w.clamp(
        CURSOR_NAME_MIN,
        (row_w - CURSOR_NAME_INSET).max(CURSOR_NAME_MIN),
    )
}

// ── 出参解析（`GET /api/fs/list`）───────────────────────────────────

/// 解析 list 出参：`{"ok":true,"dir":..,"entries":[{name,kind,size,mtime}]}`
/// （na-server 侧形状：crates/na-protocol/src/fsapi.rs `list_json`）。
/// 容错两处：①`kind` 缺位时认 kfmv4/nz 的 `type`（两代服务端同名不同词）；
/// ②`ok` 缺位当成功（nz 服务端不发 `ok`）。**name/kind 是行的本体，缺即
/// Err**；size/mtime 只是显示元数据，缺/型不符按 0（缺一个 mtime 不该让
/// 整棵树空）。
pub fn entries_of(body: &str) -> Result<Vec<Entry>, String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("fs/list JSON 坏: {e}"))?;
    let obj = v.as_object().ok_or("fs/list 响应不是对象")?;
    if let Some(ok) = obj.get("ok")
        && ok != &serde_json::Value::Bool(true)
    {
        return Err("fs/list 响应 ok 非 true".into());
    }
    let arr = obj
        .get("entries")
        .and_then(|e| e.as_array())
        .ok_or("fs/list 缺 entries 数组")?;
    let mut out = Vec::with_capacity(arr.len());
    for (i, it) in arr.iter().enumerate() {
        let o = it
            .as_object()
            .ok_or_else(|| format!("entries[{i}] 不是对象"))?;
        let name = o
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| format!("entries[{i}] 缺 name"))?;
        let kind_s = o
            .get("kind")
            .or_else(|| o.get("type"))
            .and_then(|k| k.as_str())
            .ok_or_else(|| format!("entries[{i}] 缺 kind"))?;
        let kind = match kind_s {
            "dir" => RowKind::Dir,
            "file" => RowKind::File,
            other => return Err(format!("entries[{i}] kind 未知: {other}")),
        };
        out.push(Entry {
            name: name.to_string(),
            kind,
            size: num_u64(o.get("size")),
            mtime: num_u64(o.get("mtime")),
        });
    }
    Ok(out)
}

fn num_u64(v: Option<&serde_json::Value>) -> u64 {
    match v {
        Some(serde_json::Value::Number(n)) => n
            .as_u64()
            .or_else(|| n.as_f64().map(|f| if f > 0.0 { f as u64 } else { 0 }))
            .unwrap_or(0),
        _ => 0,
    }
}

// ── 抽屉 / 光标 / 三角（帧值纯函数，零墙钟）────────────────────────

/// 抽屉动画账（**单槽**：同一时刻只有一个目录在做抽屉——后一笔盖前一笔，
/// 前一笔的行块早已落位，盖掉只丢动画不丢数据）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawerAnim {
    /// 做动画的目录相对路径
    pub path: String,
    /// 展开（子层到位那一刻起步）/ 收起
    pub opening: bool,
    pub start_ms: u64,
}

impl DrawerAnim {
    pub fn dur_ms(&self) -> u64 {
        if self.opening {
            DRAWER_OPEN_MS
        } else {
            DRAWER_CLOSE_MS
        }
    }
}

/// 抽屉刚体位移（≤0；0 = 完全展开到位）：cur_h = full_h × 展开度，
/// `dy = cur_h − full_h`。展开 = ease-out 生长 240ms（起步即快，第一帧就有
/// 位移）；收起 = 镜像旅程 180ms 收向 0（曲线取自 fx_ease 现成两件，本册
/// 不发明曲线）。full_h ≤ 0（无子行）= 无可坠之物，恒 0
pub fn drawer_dy(anim: &DrawerAnim, now_ms: u64, full_h: i64) -> i64 {
    if full_h <= 0 {
        return 0;
    }
    let dur = anim.dur_ms();
    let e = now_ms.saturating_sub(anim.start_ms);
    let t = if e >= dur { 1.0 } else { e as f32 / dur as f32 };
    let open = if anim.opening {
        ease_out_cubic(t)
    } else {
        1.0 - ease_in_out_cubic(t)
    };
    ((full_h as f32 * open).round() as i64) - full_h
}

/// 兄弟行整体平移账（**六调③**）：`from_idx` 起（含）的行**额外**吃 `dy`
/// ——抽屉在飞时 = 子块之后的第一个兄弟行起（下半树被刚体整体推下）；
/// 收起退场时 = 被摘走的子块原位起（下半树连续上滑落位）。
/// 抽屉子块自己的位移（`DrawerFrame::dy`）不在这里，兄弟行照常可见
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapShift {
    pub from_idx: usize,
    pub dy: i64,
}

/// 抽屉退场账（**六调③ 收起路线 b，2026-09-27 用户裁决**）：收起**仍即时
/// drain 子行**（不变量「展开 ⟺ 子行在树」一行不动），但把被摘走的行高总和
/// 与位置记下来，帧内让下半树从「原位」连续上滑落位——不记这笔，兄弟行就是
/// 瞬移（旧观感）。时长与抽屉收起同档（DRAWER_CLOSE_MS），曲线取 fx_ease 现成件
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetractAnim {
    /// 被摘走子块之后的第一行（= 父行 idx + 1，行表已 drain 完的当下位）
    pub from_idx: usize,
    /// 被摘走行的行高总和（起点位移量）
    pub removed_h: i64,
    pub start_ms: u64,
}

/// 收起退场位移（≥0；0 = 落位完毕）：removed_h 按 ease-out cubic 归 0——
/// 起点整量（下半树还在原位）、收尽 0（接上缩小后的行表）
pub fn retract_dy(rt: &RetractAnim, now_ms: u64) -> i64 {
    let e = now_ms.saturating_sub(rt.start_ms);
    if e >= DRAWER_CLOSE_MS {
        return 0;
    }
    let t = e as f32 / DRAWER_CLOSE_MS as f32;
    (rt.removed_h as f64 * (1.0 - ease_out_cubic(t) as f64)).round() as i64
}

/// 三角角度（0..π/2，180ms ease-out）：展开 0 → 90°，收起 90° → 0°。
/// `at_ms` = 该目录末次翻转时刻（`FileTreeState::tri_at`），0 = 无账按收尽
pub fn tri_angle(expanded: bool, at_ms: u64, now_ms: u64) -> f32 {
    let e = now_ms.saturating_sub(at_ms);
    let t = if e >= TRI_ROT_MS {
        1.0
    } else {
        e as f32 / TRI_ROT_MS as f32
    };
    let p = ease_out_cubic(t);
    let half = std::f32::consts::FRAC_PI_2;
    if expanded { half * p } else { half * (1.0 - p) }
}

/// 光标当前 y（**内容坐标框心**）：从 `cursor_from` 到选中行行心，180ms
/// ease-out cubic；到点贴死目标。无选中 = 0
pub fn cursor_y(state: &FileTreeState, now_ms: u64) -> i64 {
    let Some(sel) = state.sel else {
        return 0;
    };
    let target = state.cursor_target(sel);
    let Some(from) = state.cursor_from else {
        return target;
    };
    let e = now_ms.saturating_sub(state.cursor_at_ms);
    if e >= CURSOR_MOVE_MS {
        return target;
    }
    let t = e as f32 / CURSOR_MOVE_MS as f32;
    from + ((target - from) as f32 * ease_out_cubic(t)).round() as i64
}

/// 活性探针（帧泵闸，涂装接 `fx_spring::fx_frame_due` 的枚举表）：
/// 抽屉在飞 / **退场在滑（六调③）** / 光标在路上 / 三角在转 = true。锁窗不算
/// 活性（锁只是输入门，不动像素）。**活性必须能被外部问出来**——产帧与否是
/// 壳的责任，核不自泵
pub fn anim_active(state: &FileTreeState, now_ms: u64) -> bool {
    if let Some(a) = &state.anim
        && now_ms < a.start_ms.saturating_add(a.dur_ms())
    {
        return true;
    }
    if let Some(rt) = &state.retract
        && now_ms < rt.start_ms.saturating_add(DRAWER_CLOSE_MS)
    {
        return true; // 退场零帧 = 兄弟行卡在半路不落位（漏这笔必鬼影）
    }
    for t in state.tri_at.values() {
        if now_ms < t.saturating_add(TRI_ROT_MS) {
            return true;
        }
    }
    if let Some(sel) = state.sel
        && now_ms < state.cursor_at_ms.saturating_add(CURSOR_MOVE_MS)
        && cursor_y(state, now_ms) != state.cursor_target(sel)
    {
        return true;
    }
    false
}

// ── 命中 / 动作 ────────────────────────────────────────────────────

/// 命中结果
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// 三角（可展开的目录行专属）
    Toggle(usize),
    /// 行其余（选中）
    Row(usize),
}

/// toggle 的动作分发（壳照它动作：要列表就发请求，要预览就开浮层）
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToggleAction {
    /// 目录未取子层：发 `GET /api/fs/list?dir=path`，回来喂 `apply_list`
    NeedList { path: String },
    /// 文件：开预览浮层
    Preview { path: String, name: String },
    /// 已收起（行表已更新）
    Collapsed(usize),
    /// 动画窗口内（同一目录 240/180ms 未满）或越界行——不动作
    Locked,
}

/// 涂装快照（涂装**只吃快照不吃活体**——眼手同尺：屏上画的那一帧与命中
/// 判的那一帧是同一份账）
#[derive(Clone, Debug)]
pub struct FileTreeSnap {
    pub rows: Vec<Row>,
    pub scroll: i64,
    pub sel: Option<usize>,
    /// 光标当前帧 y（内容坐标框心；`snap()` = 收尽值）
    pub cursor_y: i64,
    /// 光标到位目标（内容坐标；判卷/调试对表）
    pub cursor_target: i64,
    /// 抽屉刚体账（None = 无抽屉动画）
    pub drawer: Option<DrawerFrame>,
    /// 兄弟行整体平移账（③；None = 无）
    pub sib_shift: Option<SnapShift>,
    pub root: String,
    pub root_label: String,
    pub expanded: BTreeSet<String>,
    pub loading: BTreeSet<String>,
    /// 三角时钟戳（path → 那次翻转的起算 ms）——涂装据此播 180ms 旋转；
    /// 没有戳的目录（从没被点过）取 0 = 直接收尽值
    pub tri: BTreeMap<String, u64>,
    /// 树总高（滚动上界的快照侧读数）
    pub total_h: i64,
    pub epoch: u64,
}

/// 抽屉刚体当前帧（涂装按 full_h 裁顶缘、按 dy 平移子块）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawerFrame {
    pub path: String,
    pub opening: bool,
    /// 子块全高（无子行 = 0 → 无可画）
    pub full_h: i64,
    /// 当前帧位移（≤0）
    pub dy: i64,
    pub start_ms: u64,
}

// ── 状态核 ──────────────────────────────────────────────────────────

/// 文件树页状态核（纯逻辑：不碰网络、不碰像素）
pub struct FileTreeState {
    /// 平面树（DFS 行序）
    pub rows: Vec<Row>,
    /// 视觉展开的目录（与行的 `expanded` 位同值——行是事实源，这是查询面）
    pub expanded: BTreeSet<String>,
    /// 子层已取过的目录（= 子行在树；收起即清，见模块头注）
    pub loaded: BTreeSet<String>,
    /// 在途请求的目录（涂装转圈用）
    pub loading: BTreeSet<String>,
    /// 根相对路径（空串 = 根）
    pub root: String,
    pub root_label: String,
    /// 选中行下标
    pub sel: Option<usize>,
    /// 滚动位移（像素 ≥ 0；上限 = max(0, total_h − view_h)）
    pub scroll: i64,
    /// 代际（一切变更 +1——涂装 sig 的唯一源）
    pub epoch: u64,
    /// 单槽抽屉动画账
    pub anim: Option<DrawerAnim>,
    /// 单槽收起退场账（③ 路线 b：行即走、下半树连续上滑落位）
    pub retract: Option<RetractAnim>,
    /// 曾展开账（**六调④**，2026-09-27 研究线裁定三档）：用户**显式展开**时
    /// 入账（`remember_expanded`）；用户**显式收起**时在 `collapse` 里摘掉
    /// **自己那一条**（子孙条目保留——重开自己时内部历史照样长回来）；祖先
    /// 收起造成的级联摘行走 `drop_under`，**不动本账**。语义 = 关根再开恢复
    /// 「用户离开时的样子」（显式收起过的保持收起，不被级联复活）
    /// 超帽丢字典序最小者（确定性）
    pub expanded_mem: BTreeSet<String>,
    /// 光标动画起点（内容坐标；None = 直落目标）
    pub cursor_from: Option<i64>,
    pub cursor_at_ms: u64,
    /// 各目录末次三角翻转时刻（`tri_at` 的源）
    tri_at: BTreeMap<String, u64>,
    /// 各目录的锁窗截止时刻（同一目录动画期二次 toggle 忽略）
    lock_until: BTreeMap<String, u64>,
}

impl FileTreeState {
    pub fn new(root_label: &str) -> Self {
        Self {
            rows: Vec::new(),
            expanded: BTreeSet::new(),
            loaded: BTreeSet::new(),
            loading: BTreeSet::new(),
            root: String::new(),
            root_label: root_label.to_string(),
            sel: None,
            scroll: 0,
            epoch: 0,
            anim: None,
            retract: None,
            expanded_mem: BTreeSet::new(),
            cursor_from: None,
            cursor_at_ms: 0,
            tri_at: BTreeMap::new(),
            lock_until: BTreeMap::new(),
        }
    }

    /// 关联版出参解析（调用风格二选一，同一份实现——不许各写一份）
    pub fn entries_of(body: &str) -> Result<Vec<Entry>, String> {
        entries_of(body)
    }

    pub fn row(&self, idx: usize) -> Option<&Row> {
        self.rows.get(idx)
    }

    pub fn is_loading(&self, path: &str) -> bool {
        self.loading.contains(path)
    }

    /// 该目录末次三角翻转时刻（0 = 无账）
    pub fn tri_at(&self, path: &str) -> u64 {
        self.tri_at.get(path).copied().unwrap_or(0)
    }

    /// 选中行行心（内容坐标；越界/无行 = 0）
    pub fn cursor_target(&self, idx: usize) -> i64 {
        row_center(&self.rows, idx)
    }

    /// 根层装载（首次进页/整树刷新）：新根表 + **同路径目录的已展开子树原样
    /// 保留**（块级重挂——重取根层不该把用户展开的树拍平），消失的目录连子孙
    /// 一起走。选中按路径重找（找不到 = 没了），光标随之贴行。
    /// **返回值**（六调④）= 本层级联恢复出来的待取层路径（调用方逐层发
    /// `request_list_quiet`；空 = 无需补取）
    pub fn apply_root_list(&mut self, entries: Vec<Entry>, now_ms: u64) -> Vec<String> {
        let sel_path = self
            .sel
            .and_then(|i| self.rows.get(i))
            .map(|r| r.path.clone());
        let keep = subtree_map(&self.rows, 0);
        self.rows = graft(entries, 0, "", &keep);
        // graft 后「行位即账」：展开 ⟺ 子树被原样挂回 = 已取
        self.expanded = self
            .rows
            .iter()
            .filter(|r| r.kind.is_dir() && r.expanded)
            .map(|r| r.path.clone())
            .collect();
        self.loaded = self.expanded.clone();
        self.loading
            .retain(|p| self.rows.iter().any(|r| r.kind.is_dir() && &r.path == p));
        // ④ 曾展开账级联恢复（本层 = 深度 0 的全部新行）
        let cascade = self.cascade_expanded_mem(0, 0, self.rows.len());
        self.anim = None;
        self.retract = None;
        self.sel = sel_path.and_then(|p| self.rows.iter().position(|r| r.path == p));
        self.reglue_cursor(now_ms);
        self.prune(now_ms);
        self.epoch += 1;
        cascade
    }

    /// 子层回执（`NeedList` 的应答，**用户亲手点的那级**）：插到父行之后（保持
    /// 树序）、父行置展开、抽屉刚体从那一刻起步。**重复回执幂等**（先旧子块整体
    /// 换新）；父行已不在树、或父行已被收起（迟到应答）= 丢弃——应答作废，
    /// 账面自愈。返回值同 `apply_root_list`（六调④ 级联）
    pub fn apply_list(&mut self, parent: &str, entries: Vec<Entry>, now_ms: u64) -> Vec<String> {
        self.apply_list_impl(parent, entries, now_ms, true)
    }

    /// 级联到位回执（六调④）：与 `apply_list` 同一份实现，**只差不起抽屉动画**
    /// （曾展开账里长回来的层不是用户此刻点的，不该再演一遍抽屉）——直接落位
    pub fn apply_list_quiet(
        &mut self,
        parent: &str,
        entries: Vec<Entry>,
        now_ms: u64,
    ) -> Vec<String> {
        self.apply_list_impl(parent, entries, now_ms, false)
    }

    fn apply_list_impl(
        &mut self,
        parent: &str,
        entries: Vec<Entry>,
        now_ms: u64,
        animate: bool,
    ) -> Vec<String> {
        self.loading.remove(parent);
        let Some(pi) = self
            .rows
            .iter()
            .position(|r| r.path == parent && r.kind.is_dir())
        else {
            return Vec::new();
        };
        if !self.rows[pi].expanded {
            return Vec::new(); // 请求在途时用户已收起：子行不许自己长回来
        }
        let depth = self.rows[pi].depth + 1;
        let end = self.subtree_end(pi);
        let removed = end - (pi + 1);
        let keep = subtree_map(&self.rows, depth);
        let block = graft(entries, depth, parent, &keep);
        let n = block.len();
        self.rows.splice(pi + 1..end, block);
        self.rows[pi].expanded = true;
        self.expanded.insert(parent.to_string());
        self.loaded.insert(parent.to_string());
        // ④ 级联恢复（本层 = 刚落位的新块 [pi+1, pi+1+n)）
        let cascade = self.cascade_expanded_mem(depth, pi + 1, pi + 1 + n);
        if animate {
            // 抽屉记账：刚体尺度以子块全高计，起点 = 子层到位这一刻（锁窗同步续）
            self.anim = Some(DrawerAnim {
                path: parent.to_string(),
                opening: true,
                start_ms: now_ms,
            });
            self.retract = None;
            self.lock_until
                .insert(parent.to_string(), now_ms.saturating_add(DRAWER_OPEN_MS));
        }
        self.map_sel(pi, removed, n);
        self.reglue_cursor(now_ms);
        self.prune(now_ms);
        self.epoch += 1;
        cascade
    }

    /// 取层失败回执（壳网络错时调）：摘 loading、父行三角回转、展开账回退
    /// （回退 = 行还在但没子行，与不变量一致：展开 ⟺ 子行在树）
    pub fn list_failed(&mut self, parent: &str, now_ms: u64) {
        self.loading.remove(parent);
        self.loaded.remove(parent);
        self.expanded.remove(parent);
        if let Some(r) = self.rows.iter_mut().find(|r| r.path == parent) {
            r.expanded = false;
        }
        self.anim = None;
        self.tri_at.insert(parent.to_string(), now_ms);
        self.lock_until
            .insert(parent.to_string(), now_ms.saturating_add(TRI_ROT_MS));
        self.prune(now_ms);
        self.epoch += 1;
    }

    /// 点行（涂装把 `Hit::Toggle`/`Hit::Row` 都接到这里，语义统一）：
    /// 目录 → 展开（未取子层则要列表）/ 收起；文件 → 预览。同一目录动画期
    /// （展开 240ms / 收起 180ms）内二次点击 = Locked（nz §3.2 动画锁直译）。
    /// 越界行（行表在点击与派发之间被刷掉）= 空操作，借 Locked 表达「不动作」
    pub fn toggle(&mut self, idx: usize, now_ms: u64) -> ToggleAction {
        let Some(row) = self.rows.get(idx) else {
            return ToggleAction::Locked;
        };
        let path = row.path.clone();
        if self
            .lock_until
            .get(&path)
            .is_some_and(|until| now_ms < *until)
        {
            return ToggleAction::Locked;
        }
        if row.kind == RowKind::File {
            return ToggleAction::Preview {
                path,
                name: row.name.clone(),
            };
        }
        if row.expanded {
            self.collapse(idx, now_ms);
            return ToggleAction::Collapsed(idx);
        }
        // 展开：三角立刻转（第一帧就有反馈，nz 同款先亮后取），子层没到先要列表
        self.tri_at.insert(path.clone(), now_ms);
        self.lock_until
            .insert(path.clone(), now_ms.saturating_add(DRAWER_OPEN_MS));
        self.rows[idx].expanded = true;
        self.expanded.insert(path.clone());
        self.loaded.remove(&path); // 与「展开 ⟺ 子行在树」对齐：无行 = 未取
        self.loading.insert(path.clone());
        self.anim = None; // 抽屉要等子行到位才有刚体可长
        // ④ 曾展开账：用户亲手展开过就记住（**显式收起时 collapse 摘自己那条**，
        // 2026-09-27 研究线裁定；旧「只进不出——collapse 不清」注释已废）
        self.remember_expanded(&path);
        self.prune(now_ms);
        self.epoch += 1;
        ToggleAction::NeedList { path }
    }

    /// 收起子树（连子孙行一起移除 + 收起动画记账）。
    /// **注**：行即走 ⇒ 抽屉刚体全高归 0（dy 恒 0，见 `drawer_full_h`）。但
    /// **六调③ 路线 b（2026-09-27 用户裁决：保守路线，不变量「展开 ⟺ 子行在
    /// 树」不动）**——drain 的当下把被摘走行高总和与落点记成 `RetractAnim`，
    /// 帧内让下半树从原位连续上滑落位（旧观感 = 兄弟行瞬移）。退场账与抽屉
    /// 账并存：抽屉那槽此刻无子块可画（full_h = 0），兄弟行只吃退场一笔
    pub fn collapse(&mut self, idx: usize, now_ms: u64) {
        let Some(row) = self.rows.get(idx) else {
            return;
        };
        if !row.kind.is_dir() || !row.expanded {
            return;
        }
        let path = row.path.clone();
        let end = self.subtree_end(idx);
        let removed_h: i64 = self.rows[idx + 1..end].iter().map(|r| row_h(r.wrap)).sum();
        self.rows.drain(idx + 1..end);
        self.rows[idx].expanded = false;
        // 子孙账一并清（行没了账不许留——重展开要重取，账留则永不再要列表）
        // **曾展开账语义（2026-09-27 研究线裁定）**：用户**显式收起**自己 →
        // 摘自己的入口（免得关根再开把它级联复活 = 撤销用户的收起）；**子孙
        // 账保留**（重开自己时内部历史照样长回来）；祖先收起导致的级联摘行
        // 走 `drop_under`（不碰 mem）——三档一一对应「用户离开时的样子」
        self.drop_under(&path);
        self.expanded_mem.remove(&path);
        self.expanded.remove(&path);
        self.loaded.remove(&path);
        self.loading.remove(&path);
        self.tri_at.insert(path.clone(), now_ms);
        self.lock_until
            .insert(path.clone(), now_ms.saturating_add(DRAWER_CLOSE_MS));
        self.anim = Some(DrawerAnim {
            path,
            opening: false,
            start_ms: now_ms,
        });
        // ③ 退场账：drain 之后的第一个兄弟行（= idx+1）起，连续上滑
        self.retract = Some(RetractAnim {
            from_idx: idx + 1,
            removed_h,
            start_ms: now_ms,
        });
        // 选中落在被摘走的子行里 = 归到父行（父行还在，选中不许凭空消失）
        if let Some(s) = self.sel {
            if s > idx && s < end {
                self.sel = Some(idx);
            } else if s >= end {
                self.sel = Some(s - (end - idx - 1));
            }
        }
        self.reglue_cursor(now_ms);
        self.prune(now_ms);
        self.epoch += 1;
    }

    /// 选中 + 光标起飞（180ms ease-out cubic，从当前所在续走——连点不跳变）。
    /// 光标**不自动滚**到选中行（要不要把行拉进视口是壳的手势决策，本册不擅动）
    pub fn select(&mut self, idx: usize, now_ms: u64) {
        if idx >= self.rows.len() {
            return;
        }
        self.cursor_from = Some(cursor_y(self, now_ms));
        self.cursor_at_ms = now_ms;
        self.sel = Some(idx);
        self.epoch += 1;
    }

    /// 量宽回填换行位（涂装量完名字实宽后喂回——行高 108 的那档；几何唯一
    /// 源，涂装不许自己算行高）
    pub fn set_wrap(&mut self, idx: usize, wrap: bool, now_ms: u64) {
        let Some(r) = self.rows.get_mut(idx) else {
            return;
        };
        if r.wrap == wrap {
            return;
        }
        r.wrap = wrap;
        self.reglue_cursor(now_ms);
        self.epoch += 1;
    }

    /// 滚动（钳 [0, max(0, total_h − view_h)]——超界不越，眼手同尺的上界）
    pub fn scroll_by(&mut self, dy: i64, view_h: i64) {
        let max = (total_h(&self.rows) - view_h).max(0);
        let ns = (self.scroll + dy).clamp(0, max);
        if ns != self.scroll {
            self.scroll = ns;
            self.epoch += 1;
        }
    }

    /// 内容高变了（层增删/换行回填）后钳回上界——壳在每次内容变更后调一次
    /// （手势喂增量时 `scroll_by` 已自钳，这条管「玩家没动但内容缩水」）
    pub fn clamp_scroll(&mut self, view_h: i64) {
        self.scroll_by(0, view_h);
    }

    /// 命中（x/y **页面坐标**，未减 scroll；view_h = 列表视口高）。
    /// 行矩形吃 `row_rects`（内容坐标，天然带滚动与裁剪）——同一份尺，
    /// 「点了没反应/点错行」这类漂移在几何上不可能。
    ///
    /// **目录行整行 = 开合、文件行 = 选中/预览**（2026-09-27 打回改约，
    /// 用户终验症⑤）：三角只是**状态指示**，不是命中区；旧实现把行左
    /// 三角带判成 Toggle、名字区判成 Row，于是「点目录行没反应」（真机
    /// logcat 20+ 次点按零 toggle）。三角盒与名字区之间的死区随之消灭
    /// ——行内任意 x 同义。（旧口径的「证据」是误读：原版截屏里那行是
    /// **选中态的目录**，正常态，不是「名字区只选中」的证明。）
    pub fn hit(&self, x: i64, y: i64, view_h: i64) -> Option<Hit> {
        if x < 0 || !(0..view_h).contains(&y) {
            return None; // 页面之外（视口上下/左侧）不接
        }
        for (idx, top, h) in row_rects(&self.rows, self.scroll, view_h, None) {
            if !(top..top + h).contains(&y) {
                continue;
            }
            let row = &self.rows[idx];
            return Some(if row.kind.is_dir() {
                Hit::Toggle(idx)
            } else {
                Hit::Row(idx)
            });
        }
        None
    }

    /// 按**给定快照**命中（屏代快照的命中入口，BAR-145 判例：命中唯一
    /// 合法源 = 屏上正显示的那一代）。`x/y` = 页面坐标、`view_h` = 行表窗高
    /// ——语义与 `hit` 逐字对齐（目录行整行开合、文件行选中），差别只是
    /// 吃哪份行表。**动画位移一并吃**（六调③：兄弟行被推走时框也要跟着走，
    /// 命中与涂装同吃快照里的 `sib_shift`——否则抽屉在飞的 240ms 里点谁中谁）
    pub fn hit_snap(snap: &FileTreeSnap, x: i64, y: i64, view_h: i64) -> Option<Hit> {
        if x < 0 || !(0..view_h).contains(&y) {
            return None;
        }
        for (idx, top, h) in row_rects(&snap.rows, snap.scroll, view_h, snap.sib_shift.as_ref()) {
            if !(top..top + h).contains(&y) {
                continue;
            }
            let row = &snap.rows[idx];
            return Some(if row.kind.is_dir() {
                Hit::Toggle(idx)
            } else {
                Hit::Row(idx)
            });
        }
        None
    }

    /// 静态快照（无钟投影 = 动画收尽那一帧：光标贴目标、抽屉贴端点、兄弟行
    /// 归位）。命中/判卷/无动画路径用；**产帧路径唯一入口是 `snap_at`**
    pub fn snap(&self) -> FileTreeSnap {
        let drawer_dy = self.anim.as_ref().map(|a| {
            if a.opening {
                0
            } else {
                -self.drawer_full_h_of(&a.path)
            }
        });
        let cy = self.sel.map_or(0, |i| self.cursor_target(i));
        self.snap_frame(cy, drawer_dy, self.sib_shift_frame(0, true))
    }

    /// 帧快照（涂装唯一入口）：光标/抽屉/兄弟行平移按 now_ms 解出当前帧值
    pub fn snap_at(&self, now_ms: u64) -> FileTreeSnap {
        let drawer_dy = self
            .anim
            .as_ref()
            .map(|a| drawer_dy(a, now_ms, self.drawer_full_h_of(&a.path)));
        self.snap_frame(
            cursor_y(self, now_ms),
            drawer_dy,
            self.sib_shift_frame(now_ms, false),
        )
    }

    // ── 内部 ────────────────────────────────────────────────────────

    /// ③ 兄弟行整体平移账（None = 不动）：抽屉在飞 = 子块之后的第一个兄弟行
    /// 起吃抽屉同款 dy（展开时下半树被刚体整体推下；几何自证——子块占
    /// [父行底, 父行底+cur)，兄弟行从 父行底+full_h 滑到终位，起点恰在子块
    /// 顶位、终点接子块底，无重叠）；退场期 = 被摘走的原位起，removed_h 按
    /// ease-out 归 0（连续上滑，不瞬移）。`settled` = 无钟投影（收尽那一帧，
    /// 无位移可言 → None）
    fn sib_shift_frame(&self, now_ms: u64, settled: bool) -> Option<SnapShift> {
        if settled {
            return None;
        }
        if let Some(rt) = &self.retract {
            if now_ms >= rt.start_ms.saturating_add(DRAWER_CLOSE_MS) {
                return None; // 退场已收尽
            }
            return Some(SnapShift {
                from_idx: rt.from_idx,
                dy: retract_dy(rt, now_ms),
            });
        }
        let a = self.anim.as_ref()?;
        if !a.opening {
            return None; // 收起期没有子块在树（行即走）——退场账管那一路
        }
        let pi = self.rows.iter().position(|r| r.path == a.path)?;
        let pd = self.rows[pi].depth;
        let mut b1 = pi + 1;
        while b1 < self.rows.len() && self.rows[b1].depth > pd {
            b1 += 1;
        }
        if b1 == pi + 1 {
            return None; // 无子行 = 无刚体可长
        }
        Some(SnapShift {
            from_idx: b1,
            dy: drawer_dy(a, now_ms, self.drawer_full_h_of(&a.path)),
        })
    }

    fn snap_frame(
        &self,
        cursor_y: i64,
        drawer_dy: Option<i64>,
        sib_shift: Option<SnapShift>,
    ) -> FileTreeSnap {
        let drawer = self.anim.as_ref().map(|a| DrawerFrame {
            path: a.path.clone(),
            opening: a.opening,
            full_h: self.drawer_full_h_of(&a.path),
            dy: drawer_dy.unwrap_or(0),
            start_ms: a.start_ms,
        });
        FileTreeSnap {
            rows: self.rows.clone(),
            scroll: self.scroll,
            sel: self.sel,
            cursor_y,
            cursor_target: self.sel.map_or(0, |i| self.cursor_target(i)),
            drawer,
            sib_shift,
            root: self.root.clone(),
            root_label: self.root_label.clone(),
            expanded: self.expanded.clone(),
            loading: self.loading.clone(),
            tri: self.tri_at.clone(),
            total_h: total_h(&self.rows),
            epoch: self.epoch,
        }
    }

    /// ④ 曾展开账级联恢复：`[start,end)` 里深度 = `depth` 的目录行，若路径在
    /// 曾展开账且**子层不在树**（graft 后没被挂回）= 标记展开（▼）+ 在途账，
    /// 路径收集返回给取数侧逐层发 quiet 请求。**只扫本层**：扫全树会把别支
    /// 深处「用户刚收起」的目录也拉起来
    fn cascade_expanded_mem(&mut self, depth: usize, start: usize, end: usize) -> Vec<String> {
        let mut out = Vec::new();
        for i in start..end.min(self.rows.len()) {
            if self.rows[i].depth != depth || !self.rows[i].kind.is_dir() || self.rows[i].expanded {
                continue;
            }
            let p = self.rows[i].path.clone();
            if !self.expanded_mem.contains(&p) {
                continue;
            }
            self.rows[i].expanded = true;
            self.expanded.insert(p.clone());
            self.loading.insert(p.clone());
            out.push(p);
        }
        out
    }

    /// ④ 曾展开账记账：**用户显式展开**才入账（toggle 的 NeedList 臂调本函数）；
    /// 用户显式收起时在 `collapse` 里摘自己那一条（子孙条目保留）；祖先收起
    /// 造成的级联摘行不动本账。效果 = 关根再开恢复「用户离开时的样子」。
    /// 超帽丢字典序最小者（BTreeSet 有序 = 确定性，不引随机源）
    fn remember_expanded(&mut self, path: &str) {
        self.expanded_mem.insert(path.to_string());
        while self.expanded_mem.len() > MEM_CAP {
            self.expanded_mem.pop_first();
        }
    }

    fn drawer_full_h_of(&self, path: &str) -> i64 {
        self.rows
            .iter()
            .position(|r| r.path == path)
            .map_or(0, |i| drawer_full_h(&self.rows, i))
    }

    /// 父行 idx 的子树末尾（不含 = 下一个 depth ≤ 父 depth 的行）
    fn subtree_end(&self, idx: usize) -> usize {
        let d = self.rows[idx].depth;
        let mut e = idx + 1;
        while e < self.rows.len() && self.rows[e].depth > d {
            e += 1;
        }
        e
    }

    /// 摘掉某目录的**子孙账**（展开/已取/在途/三角/锁窗）——行被摘走时账跟
    /// 着走，否则子孙目录会留着「已取」印子，重展开时永远不再要列表
    fn drop_under(&mut self, dir: &str) {
        let under =
            |p: &str| p.len() > dir.len() && p.as_bytes()[dir.len()] == b'/' && p.starts_with(dir);
        self.expanded.retain(|p| !under(p));
        self.loaded.retain(|p| !under(p));
        self.loading.retain(|p| !under(p));
        self.tri_at.retain(|p, _| !under(p));
        self.lock_until.retain(|p, _| !under(p));
    }

    /// 旧子块 [pi+1, end) 被 n 行新块替换后的选中下标映射
    fn map_sel(&mut self, pi: usize, removed: usize, n: usize) {
        let Some(s) = self.sel else {
            return;
        };
        let end = pi + 1 + removed;
        let ns = if s <= pi {
            s
        } else if s < end {
            if n > 0 { pi + 1 } else { pi }
        } else {
            s + n - removed
        };
        self.sel = Some(ns.min(self.rows.len().saturating_sub(1)));
    }

    /// 行表动过之后把光标贴回选中行（不滑——框属于行，行移框移；行没了清账）
    fn reglue_cursor(&mut self, now_ms: u64) {
        match self.sel {
            Some(i) if i < self.rows.len() => {
                self.cursor_from = Some(self.cursor_target(i));
                self.cursor_at_ms = now_ms;
            }
            _ => {
                self.sel = None;
                self.cursor_from = None;
            }
        }
    }

    /// 短窗口账老化（三角/锁窗/退场都是百毫秒级账，压着不清会变化石表）
    fn prune(&mut self, now_ms: u64) {
        const KEEP_MS: u64 = 2_000;
        self.tri_at
            .retain(|_, t| now_ms < t.saturating_add(KEEP_MS));
        self.lock_until.retain(|_, t| now_ms < *t);
        if let Some(rt) = &self.retract
            && now_ms >= rt.start_ms.saturating_add(DRAWER_CLOSE_MS)
        {
            self.retract = None;
        }
    }
}

/// 路径拼接（根的子项 = 名字本身，nz `dir === '' ? name : dir/name` 同款）
fn join_path(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}/{name}")
    }
}

/// 收集旧行表里深度 = `depth` 的**块**（块 = 该行 + 其后所有更深的行）。
/// 重取某层时按路径把已展开子树的块原样挂回（行在账在，用户展开的树不被
/// 刷新拍平）
fn subtree_map(rows: &[Row], depth: usize) -> BTreeMap<String, Vec<Row>> {
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < rows.len() {
        if rows[i].depth != depth {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < rows.len() && rows[j].depth > depth {
            j += 1;
        }
        if j > i + 1 {
            out.insert(rows[i].path.clone(), rows[i + 1..j].to_vec());
        }
        i = j;
    }
    out
}

/// entries → 行块（父层第 `depth` 层，路径前缀 `prefix`）；同路径有保留块
/// （子层已取）的目录按展开态出（行位即账，不给半截）
fn graft(
    entries: Vec<Entry>,
    depth: usize,
    prefix: &str,
    keep: &BTreeMap<String, Vec<Row>>,
) -> Vec<Row> {
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        let path = join_path(prefix, &e.name);
        let sub = if e.kind.is_dir() {
            keep.get(&path).cloned()
        } else {
            None
        };
        out.push(Row {
            path,
            name: e.name,
            depth,
            kind: e.kind,
            expanded: sub.is_some(),
            wrap: false,
            size: e.size,
        });
        if let Some(s) = sub {
            out.extend(s);
        }
    }
    out
}

// ---- 壳接线面（2026-09-26 施工时补）：共享状态句柄 + 屏代快照账 ----
//
// 为什么走全局而不是 App 字段：文件树页有三个涂装入口（GLES 烘焙、值守
// 倒帧 gate.rs、softbuffer 兜底）与一条手势路，倒帧那两条**手上没有 App**，
// 只有全局够得着。与解析页 `parser_page_handle`/`baked_snap` 同一形态。
//
// 屏代快照账（眼手同尺的真义，照 BAR-145 的判例）：**命中唯一合法源 =
// 屏上正显示的那一代**——网络回执可随时改行表，若命中吃活体，用户点的
// 就是「下一条刚到的行」。烘焙落账、命中吃账，两账同代。

static FILETREE: std::sync::Mutex<Option<std::sync::Arc<std::sync::Mutex<FileTreeState>>>> =
    std::sync::Mutex::new(None);

/// 注册共享状态核（壳启动时一次；host 不注册 = 页面空转合法）
pub fn register_filetree(s: std::sync::Arc<std::sync::Mutex<FileTreeState>>) {
    *FILETREE.lock().unwrap() = Some(s);
}

/// 取共享状态核（涂装/手势/取数三处同源）
pub fn filetree_handle() -> Option<std::sync::Arc<std::sync::Mutex<FileTreeState>>> {
    FILETREE.lock().unwrap().clone()
}

static BAKED: std::sync::Mutex<Option<FileTreeSnap>> = std::sync::Mutex::new(None);

/// 烘焙落屏代快照（每个涂装入口画完自己的那一帧后调一次）
pub fn note_baked_snap(s: FileTreeSnap) {
    *BAKED.lock().unwrap() = Some(s);
}

/// 屏代快照（None = 还没上过屏——命中回落活体，与无烘焙记录的解析页同规）
pub fn baked_snap() -> Option<FileTreeSnap> {
    BAKED.lock().unwrap().clone()
}

/// 重烘 sig 四元组：代 / 滚动 / 选中 / 动画簇。**漏一维 = 鬼影**
/// （stage.rs 头注：脏帧 sig 必须是全部输入）——行表变、滚动、选中换行、
/// 抽屉/光标/三角任一在动，都必须触发重烘
pub type FtSig = (u64, i64, i64, u64);

/// 涂装与重烘共用的帧账：快照 + sig。动画簇在动画期喂 `now_ms`
/// （每帧都变 → 条件重烘接住），静止期恒 0（稳态零重烘）
pub fn frame(now_ms: u64) -> Option<(FileTreeSnap, FtSig)> {
    let h = filetree_handle()?;
    let st = h.lock().unwrap();
    let anim_tick = if anim_active(&st, now_ms) { now_ms } else { 0 };
    let sig = (
        st.epoch,
        st.scroll,
        st.sel.map_or(-1, |i| i as i64),
        anim_tick,
    );
    Some((st.snap_at(now_ms), sig))
}
