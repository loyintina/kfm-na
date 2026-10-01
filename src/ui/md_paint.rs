//! ui/md_paint.rs — md 绘制层（BAR-169 md 渲染器一期，壳层；BAR-204
//! 换芯网格文字引擎）：MdLayout → 像素。涂装规格 = theme.md §2.5 淡彩
//! 六色家族 + §三 md 条款——**demo 页涂装即打样规格**，本册是同配方的
//! 数据驱动版（demo 页硬编码样品二期才换芯，本单不动）。
//!
//! 消费关系：md_parse（事件流）→ md_layout（折行几何，行宽存档）
//! → 本册（照几何涂装，落笔走网格引擎 `measure_items_grid_stepped` +
//! `draw_grid_text_stepped`：步进 = char_cells × 实例格宽 × 档 scale，
//! 字形 px = grid_fit 实例格 × scale——pen 累进与排版行宽同一把尺
//! = `grid_stepped_w` 唯一源）。查看器正文视口纵裁剪由调用方喂 clip
//! （BAR-167 ①滚动双裁纪律：上裁防污染标题/分隔线带，下裁守关闭钮
//! 前隙）。

use crate::termview::{
    CELL_W, Frame, TermView, paint_demo_chip, paint_thin_frame, ring_gradient_rgb,
};
use crate::ui::accent::{self, AccentPair};
use crate::ui::demo_page as dp;
use crate::ui::demo_page::{BlockKind, SegStyle};
use crate::ui::md_layout::{MdLayout, grid_stepped_w};

impl TermView {
    /// md 正文涂装（数据驱动版 paint_demo_content_impl）：x0/y0 = 文档
    /// 原点屏坐标（y0 = 视口顶 − scroll，滚动由调用方平移）；clip =
    /// 视口纵裁剪 [top, bottom)；denom = 页渐变尺分母（与页环同一把
    /// 135° 尺）。逐块跳过视口外块（滚动性能与下裁纪律同源）
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_md_body(
        &self,
        frame: &mut Frame<'_>,
        lay: &MdLayout,
        x0: i64,
        y0: i64,
        cw: u32,
        clip: (i64, i64),
        denom: i64,
        accent: AccentPair,
    ) {
        let body_fg = 0x00BF_BFBF; // 0.75 白（正文档）
        let meta_fg = 0x0080_8080; // 0.5 白（次级档）
        let pastel = accent::pastel_family(accent.c1);
        let (fw, fh) = (i64::from(frame.w), i64::from(frame.h));
        let content_r = x0 + i64::from(cw); // 文字右裁（未钳屏）
        let yclip = Some((clip.0 as i32, clip.1 as i32));

        for b in &lay.blocks {
            let by = y0 + i64::from(b.y);
            let bh = i64::from(b.h);
            if by + bh <= clip.0 || by >= clip.1 {
                continue; // 整块出视口
            }
            match b.kind {
                BlockKind::H1 => {
                    // [ 半包框逐行版（折行条款：横带随每行字宽）——左竖带
                    // 整块连续，顶横带逐行各画（宽 = 缩进 + 该行实量 + 收尾），
                    // 上下圆角只归首行顶/末行底；框内局部渐变尺同 demo
                    let t = i64::from(dp::HEAD_FRAME_T);
                    let r = i64::from(dp::HEAD_CORNER_R);
                    let n = b.lines.len() as i64;
                    for (i, line) in b.lines.iter().enumerate() {
                        let ly = by + i64::from(dp::HU) + i as i64 * i64::from(b.line_h);
                        let top_w = i64::from(dp::HEAD_TEXT_INSET)
                            + i64::from(line.w)
                            + i64::from(dp::HEAD_TOP_TAIL);
                        let denom_l = (top_w - 1).max(0) + (bh - 1).max(0);
                        let mut ink = |ax: i64, ay: i64| {
                            if ax < 0
                                || ax >= fw
                                || ay < 0
                                || ay >= fh
                                || ay < clip.0
                                || ay >= clip.1
                            {
                                return;
                            }
                            let c =
                                ring_gradient_rgb(accent.c1, accent.c2, ax - x0, ay - by, denom_l);
                            frame.blend_px(ax as u32, ay as u32, c, 255);
                        };
                        // 左竖带：首行从块顶圆心起，末行到块底圆心止，中行满带
                        let (v0, v1) = (
                            if i == 0 { by + r } else { ly },
                            if i as i64 == n - 1 {
                                by + bh - r
                            } else {
                                ly + i64::from(b.line_h)
                            },
                        );
                        for ay in v0..v1 {
                            for ax in x0..x0 + t {
                                ink(ax, ay);
                            }
                        }
                        // 顶横带（本行字宽收尾）：首行贴块顶（上垫 HU 是
                        // 横带与文字的间距——宪法「文字距框缘上 ≥0.5 格」），
                        // 折行续行贴本行带顶（行距即间距）
                        let band_y = if i == 0 { by } else { ly };
                        for ay in band_y..band_y + t {
                            for ax in x0 + r..x0 + top_w {
                                ink(ax, ay);
                            }
                        }
                        // 圆角：首行顶 / 末行底各一枚四分之一环带
                        if i == 0 || i as i64 == n - 1 {
                            let (cy, ya, yb) = if i == 0 {
                                (by + r, by, by + r)
                            } else {
                                (by + bh - r, by + bh - r, by + bh)
                            };
                            for ay in ya..yb {
                                for ax in x0..x0 + r {
                                    let dx = (ax - (x0 + r)) as f64 + 0.5;
                                    let dy = (ay - cy) as f64 + 0.5;
                                    let d = (dx * dx + dy * dy).sqrt();
                                    if d >= (r - t) as f64 && d <= r as f64 {
                                        ink(ax, ay);
                                    }
                                }
                            }
                        }
                        // 文字：距框缘上 0.5 格、左 1 格；淡彩 slot0 双绘
                        for inset in [0.0, 1.0] {
                            self.md_text_line(
                                frame,
                                line,
                                x0 + i64::from(dp::HEAD_TEXT_INSET),
                                content_r,
                                ly,
                                b.line_h,
                                b.scale,
                                pastel[dp::pastel_role::BOLD],
                                inset,
                                yclip,
                            );
                        }
                    }
                }
                BlockKind::H2 | BlockKind::H3 => {
                    let fg = pastel[if b.kind == BlockKind::H2 {
                        dp::pastel_role::H2
                    } else {
                        dp::pastel_role::H3
                    }];
                    for (i, line) in b.lines.iter().enumerate() {
                        let ly = by + i as i64 * i64::from(b.line_h);
                        self.md_text_line(
                            frame, line, x0, content_r, ly, b.line_h, b.scale, fg, 0.0, yclip,
                        );
                    }
                }
                BlockKind::H4 | BlockKind::H5 | BlockKind::H6 => {
                    let (fg, double) = match b.kind {
                        BlockKind::H4 => (pastel[dp::pastel_role::H4], true),
                        BlockKind::H5 => (body_fg, false),
                        _ => (meta_fg, false),
                    };
                    for (i, line) in b.lines.iter().enumerate() {
                        let ly = by + i as i64 * i64::from(b.line_h);
                        self.md_text_line(
                            frame, line, x0, content_r, ly, b.line_h, b.scale, fg, 0.0, yclip,
                        );
                        if double {
                            self.md_text_line(
                                frame, line, x0, content_r, ly, b.line_h, b.scale, fg, 1.0, yclip,
                            );
                        }
                    }
                }
                BlockKind::Body => {
                    for (i, line) in b.lines.iter().enumerate() {
                        let ly = by + i as i64 * i64::from(b.line_h);
                        self.md_spans_line(
                            frame, line, x0, content_r, ly, b.line_h, b.scale, body_fg, &pastel,
                            accent, denom, yclip, clip,
                        );
                    }
                }
                BlockKind::Code => {
                    // 展示型值框配方（四边均匀细框 + 渐变暗底内芯，
                    // paint_thin_frame 共享件页尺采样；自带纵裁剪）
                    paint_thin_frame(frame, x0, by, cw, b.h, accent, denom, clip);
                    for (i, line) in b.lines.iter().enumerate() {
                        let ly = by + i64::from(dp::HU) + i as i64 * i64::from(b.line_h);
                        self.md_text_line(
                            frame,
                            line,
                            x0 + i64::from(CELL_W),
                            content_r,
                            ly,
                            b.line_h,
                            b.scale,
                            body_fg,
                            0.0,
                            yclip,
                        );
                    }
                }
                BlockKind::Quote => {
                    // 左竖线 2px（块内纵向渐变尺）+ 缩进 1 格白 0.5
                    for ay in by.max(clip.0)..(by + bh).min(clip.1) {
                        if ay < 0 || ay >= fh {
                            continue;
                        }
                        for ax in x0..x0 + i64::from(dp::QUOTE_BAR_W) {
                            if ax < 0 || ax >= fw {
                                continue;
                            }
                            let c = ring_gradient_rgb(accent.c1, accent.c2, 0, ay - by, bh - 1);
                            frame.blend_px(ax as u32, ay as u32, c, 255);
                        }
                    }
                    for (i, line) in b.lines.iter().enumerate() {
                        let ly = by + i as i64 * i64::from(b.line_h);
                        self.md_text_line(
                            frame,
                            line,
                            x0 + i64::from(dp::INDENT_W),
                            content_r,
                            ly,
                            b.line_h,
                            b.scale,
                            meta_fg,
                            0.0,
                            yclip,
                        );
                    }
                }
                BlockKind::List => {
                    // ▪ = 程序化 8px accent 方块（不赌字体字形）只在项首行；
                    // 折行续行与文字同缩进（2 格）不挂符
                    let mark = i64::from(dp::LIST_MARK_PX);
                    for (i, line) in b.lines.iter().enumerate() {
                        let ly = by + i as i64 * i64::from(b.line_h);
                        if line.item_start {
                            let my = ly + (i64::from(b.line_h) - mark) / 2;
                            for ay in my.max(clip.0)..(my + mark).min(clip.1) {
                                if ay < 0 || ay >= fh {
                                    continue;
                                }
                                for ax in x0..x0 + mark {
                                    if ax < 0 || ax >= fw {
                                        continue;
                                    }
                                    let c =
                                        ring_gradient_rgb(accent.c1, accent.c2, 0, ay - by, bh - 1);
                                    frame.blend_px(ax as u32, ay as u32, c, 255);
                                }
                            }
                        }
                        self.md_text_line(
                            frame,
                            line,
                            x0 + i64::from(dp::LIST_TEXT_INSET),
                            content_r,
                            ly,
                            b.line_h,
                            b.scale,
                            body_fg,
                            0.0,
                            yclip,
                        );
                    }
                }
                BlockKind::Hr => {
                    // HR_THICK 横向 accent 渐变（块带 HU×2 内居中；
                    // 宪法 2026-09-27 加粗 1→3，BAR-218 再加粗 3→5）
                    let ly = by + (bh - i64::from(dp::HR_THICK)) / 2;
                    for ay in ly.max(clip.0)..(ly + i64::from(dp::HR_THICK)).min(clip.1) {
                        if ay < 0 || ay >= fh {
                            continue;
                        }
                        for ax in x0..content_r {
                            if ax < 0 || ax >= fw {
                                continue;
                            }
                            let c = ring_gradient_rgb(accent.c2, accent.c1, ax, ay, denom);
                            frame.blend_px(ax as u32, ay as u32, c, 180);
                        }
                    }
                }
                BlockKind::Sign => {} // demo 页专属，解析器永不产出
                BlockKind::Table => {
                    // BAR-218 三档：Fit/Shrink = 表头淡彩 slot4 + 3px 下划
                    // accent 渐变 + 内容行白 0.75（列几何排版层存档，格内已
                    // 折行）；Cards = 每行一卡（值框同配方）标题淡彩 slot0
                    // 双绘 + 字段行（Bold 字段名前缀走段排）；DefList =
                    // 名（slot0 双绘）+ 值（缩进 1 格白 0.75）无框
                    if let Some(t) = &b.table {
                        match t.tier {
                            crate::ui::md_layout::TableTier::Fit
                            | crate::ui::md_layout::TableTier::Shrink => {
                                // 表头各格（淡彩 slot4，段排保行内样式）
                                for (j, cell) in t.header.cells.iter().enumerate() {
                                    let cx = x0 + i64::from(t.col_x[j]);
                                    let cr = cx + i64::from(t.col_w[j]);
                                    for (i, line) in cell.iter().enumerate() {
                                        let ly = by + i as i64 * i64::from(b.line_h);
                                        self.md_spans_line(
                                            frame,
                                            line,
                                            cx,
                                            cr,
                                            ly,
                                            b.line_h,
                                            b.scale,
                                            pastel[dp::pastel_role::H2],
                                            &pastel,
                                            accent,
                                            denom,
                                            yclip,
                                            clip,
                                        );
                                    }
                                }
                                // 表头下划带：3px accent 渐变（带内居中）
                                let uy = by
                                    + i64::from(t.header.h)
                                    + (i64::from(dp::TABLE_ROW_PAD)
                                        - i64::from(dp::TABLE_HEAD_UNDER))
                                        / 2;
                                for ay in uy.max(clip.0)
                                    ..(uy + i64::from(dp::TABLE_HEAD_UNDER)).min(clip.1)
                                {
                                    if ay < 0 || ay >= fh {
                                        continue;
                                    }
                                    for ax in x0..content_r {
                                        if ax < 0 || ax >= fw {
                                            continue;
                                        }
                                        let c =
                                            ring_gradient_rgb(accent.c2, accent.c1, ax, ay, denom);
                                        frame.blend_px(ax as u32, ay as u32, c, 200);
                                    }
                                }
                                // 内容行各格
                                for row in &t.rows {
                                    let ry = by + i64::from(row.y);
                                    for (j, cell) in row.cells.iter().enumerate() {
                                        let cx = x0 + i64::from(t.col_x[j]);
                                        let cr = cx + i64::from(t.col_w[j]);
                                        for (i, line) in cell.iter().enumerate() {
                                            let ly = ry + i as i64 * i64::from(b.line_h);
                                            self.md_spans_line(
                                                frame, line, cx, cr, ly, b.line_h, b.scale,
                                                body_fg, &pastel, accent, denom, yclip, clip,
                                            );
                                        }
                                    }
                                }
                            }
                            crate::ui::md_layout::TableTier::Cards => {
                                let inner_x = x0 + i64::from(dp::INDENT_W);
                                let inner_r = content_r - i64::from(dp::INDENT_W);
                                for row in &t.rows {
                                    let ry = by + i64::from(row.y);
                                    // 卡框 = 展示型值框同配方（四边细框+渐变暗底）
                                    paint_thin_frame(frame, x0, ry, cw, row.h, accent, denom, clip);
                                    let mut ly = ry + i64::from(dp::TABLE_ROW_PAD);
                                    for (ci, cell) in row.cells.iter().enumerate() {
                                        for line in cell {
                                            if ci == 0 {
                                                // 卡标题：淡彩 slot0 双绘
                                                for inset in [0.0, 1.0] {
                                                    self.md_text_line(
                                                        frame,
                                                        line,
                                                        inner_x,
                                                        inner_r,
                                                        ly,
                                                        b.line_h,
                                                        b.scale,
                                                        pastel[dp::pastel_role::BOLD],
                                                        inset,
                                                        yclip,
                                                    );
                                                }
                                            } else {
                                                // 字段行：Bold 字段名前缀走段排
                                                self.md_spans_line(
                                                    frame, line, inner_x, inner_r, ly, b.line_h,
                                                    b.scale, body_fg, &pastel, accent, denom,
                                                    yclip, clip,
                                                );
                                            }
                                            ly += i64::from(b.line_h);
                                        }
                                    }
                                }
                            }
                            crate::ui::md_layout::TableTier::DefList => {
                                for row in &t.rows {
                                    let ry = by + i64::from(row.y);
                                    let mut ly = ry;
                                    for (ci, cell) in row.cells.iter().enumerate() {
                                        for line in cell {
                                            if ci == 0 {
                                                // 名：淡彩 slot0 双绘
                                                for inset in [0.0, 1.0] {
                                                    self.md_text_line(
                                                        frame,
                                                        line,
                                                        x0,
                                                        content_r,
                                                        ly,
                                                        b.line_h,
                                                        b.scale,
                                                        pastel[dp::pastel_role::BOLD],
                                                        inset,
                                                        yclip,
                                                    );
                                                }
                                            } else {
                                                // 值：缩进 1 格白 0.75
                                                self.md_spans_line(
                                                    frame,
                                                    line,
                                                    x0 + i64::from(dp::INDENT_W),
                                                    content_r,
                                                    ly,
                                                    b.line_h,
                                                    b.scale,
                                                    body_fg,
                                                    &pastel,
                                                    accent,
                                                    denom,
                                                    yclip,
                                                    clip,
                                                );
                                            }
                                            ly += i64::from(b.line_h);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// 单行文字涂装（无行内段）·网格引擎版：步进 = char_cells × 实例
    /// 格宽 × 档 scale（与排版尺同源唯一源 grid_stepped_w），字形 px =
    /// grid_fit 实例格 × scale；右裁 content_r + 纵裁 yclip
    #[allow(clippy::too_many_arguments)]
    fn md_text_line(
        &self,
        frame: &mut Frame<'_>,
        line: &crate::ui::md_layout::MdLine,
        x: i64,
        content_r: i64,
        y: i64,
        rh: u32,
        scale: f32,
        fg: u32,
        inset: f32,
        yclip: Option<(i32, i32)>,
    ) {
        // 段列逐段画（H1-H6/引用/列表/代码行内无样式段；正文段排走
        // md_spans_line）；字形线盒（cell_h × scale）在行带内纵向居中
        let (cell_w, cell_h) = self.cell_size();
        let step = cell_w as f32 * scale;
        let glyph_h = cell_h as f32 * scale;
        let y_top = y + ((i64::from(rh) as f32 - glyph_h).max(0.0) / 2.0) as i64;
        let mut pen = x;
        for (style, text) in &line.spans {
            let _ = style; // 标题/引用等单色行：样式段文字同色（行内强调档归正文路）
            let tw = i64::from(grid_stepped_w(text, step));
            if pen >= content_r {
                break;
            }
            let cx = pen + inset as i64;
            let cr = content_r.min(i64::from(frame.w));
            if cx < cr && y + i64::from(rh) > 0 && y < i64::from(frame.h) {
                let (items, _) = self.measure_items_grid_stepped(text, step, scale);
                self.draw_grid_text_stepped(
                    frame,
                    &items,
                    cx,
                    y_top,
                    step,
                    (cr - cx).max(0) as u32,
                    fg,
                    0,
                    yclip,
                );
            }
            pen += tw;
        }
    }

    /// 正文行段排（粗体 = 淡彩 slot0 双绘；行内码 = 淡彩 slot3 + 渐变
    /// 暗底小块——demo 页打样配方数据驱动版，pen 累进与排版行宽同尺
    /// = grid_stepped_w 同一尺）
    #[allow(clippy::too_many_arguments)]
    fn md_spans_line(
        &self,
        frame: &mut Frame<'_>,
        line: &crate::ui::md_layout::MdLine,
        x0: i64,
        content_r: i64,
        y: i64,
        rh: u32,
        scale: f32,
        body_fg: u32,
        pastel: &[u32; 6],
        accent: AccentPair,
        denom: i64,
        yclip: Option<(i32, i32)>,
        clip: (i64, i64),
    ) {
        let step = self.cell_size().0 as f32 * scale;
        let mut pen = x0;
        for (style, text) in &line.spans {
            let tw = i64::from(grid_stepped_w(text, step));
            match style {
                SegStyle::Normal => {
                    self.md_text_line(
                        frame,
                        &seg_line(style, text),
                        pen,
                        content_r,
                        y,
                        rh,
                        scale,
                        body_fg,
                        0.0,
                        yclip,
                    );
                    pen += tw;
                }
                SegStyle::Bold => {
                    for inset in [0.0, 1.0] {
                        self.md_text_line(
                            frame,
                            &seg_line(style, text),
                            pen,
                            content_r,
                            y,
                            rh,
                            scale,
                            pastel[dp::pastel_role::BOLD],
                            inset,
                            yclip,
                        );
                    }
                    pen += tw + 1;
                }
                SegStyle::Code => {
                    let chip_w = (tw + 12).max(0) as u32; // 文字左右各 6px 内垫
                    let chip_h = 40.min(rh);
                    let chip_y = y + (i64::from(rh) - i64::from(chip_h)) / 2;
                    if chip_y + i64::from(chip_h) > clip.0 && chip_y < clip.1 {
                        paint_demo_chip(frame, pen, chip_y, chip_w, chip_h, accent, denom);
                    }
                    self.md_text_line(
                        frame,
                        &seg_line(style, text),
                        pen + 6,
                        content_r,
                        y,
                        rh,
                        scale,
                        pastel[dp::pastel_role::INLINE_CODE],
                        0.0,
                        yclip,
                    );
                    pen += i64::from(chip_w);
                }
            }
        }
    }

    /// md 引擎展品预览（BAR-207 组件池 md 引擎栏）：真实管线微缩——
    /// parse→layout→paint 零平行实现，mini 实例格（24px 档 = 标准格
    /// 2/3，宪法同行距；BAR-204 换芯后档位废、格缩即字缩）。
    /// x0/y0 = 展台内区原点，cw = 内区宽，clip = 展台纵裁剪带
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_md_preview(
        &self,
        frame: &mut Frame<'_>,
        sample: &str,
        x0: i64,
        y0: i64,
        cw: u32,
        clip: (i64, i64),
        denom: i64,
        accent: AccentPair,
    ) {
        let lay = crate::ui::md_layout::layout_md(sample, cw, PREVIEW_MINI_CELL);
        self.paint_md_body(frame, &lay, x0, y0, cw, clip, denom, accent);
    }
}

/// 组件池 md 预览 mini 实例格（BAR-207）：标准格 (18,36) 的 2/3 =
/// 原 mini 24px 字号档（BAR-204 格缩即字缩条款下的等价物）
pub(crate) const PREVIEW_MINI_CELL: (u32, u32) = (12, 24);

/// 单段拼 MdLine（段排内部复用单行涂装路）
fn seg_line(style: &SegStyle, text: &str) -> crate::ui::md_layout::MdLine {
    crate::ui::md_layout::MdLine {
        spans: vec![(*style, text.to_string())],
        w: 0,
        item_start: false,
    }
}

#[cfg(test)]
mod md_paint_smoke {
    //! md_paint 烟雾钉（BAR-169，ASCII 夹具——宿主字体无 CJK 字形的
    //! 判卷陷阱 BAR-167 已录：要真字形像素用 ASCII 正文）
    use super::*;
    use crate::ui::md_layout::{MdLayout, layout_md};

    fn tv() -> TermView {
        crate::termview::build_vendored().expect("内嵌字体必成").0
    }

    fn paint(md: &str, w: u32, h: u32, y0: i64, clip: (i64, i64)) -> (Vec<u32>, MdLayout) {
        let t = tv();
        let lay = layout_md(md, w, t.cell_size());
        let mut buf = vec![0u32; (w * h) as usize];
        {
            let mut frame = Frame {
                buf: &mut buf,
                w,
                h,
            };
            t.paint_md_body(
                &mut frame,
                &lay,
                0,
                y0,
                w,
                clip,
                ((w.max(1) - 1) + (h.max(1) - 1)).max(1) as i64,
                crate::ui::accent::FALLBACK,
            );
        }
        (buf, lay)
    }

    fn ink(buf: &[u32], w: u32, x: u32, y: u32) -> bool {
        buf[(y * w + x) as usize] & 0x00FF_FFFF != 0
    }

    #[test]
    fn spec_bar169_40_六样结构件出墨到位() {
        let md = "# Title\n\n## Sub\n\nbody **bold** `code` tail\n\n```\nlet x = 1;\n```\n\n> quote line\n\n- item one\n- item two\n\n---";
        let w = 800;
        let h = 2400;
        let (buf, lay) = paint(md, w, h, 0, (0, i64::from(h)));
        let kinds: Vec<BlockKind> = lay.blocks.iter().map(|b| b.kind).collect();
        assert_eq!(
            kinds,
            vec![
                BlockKind::H1,
                BlockKind::H2,
                BlockKind::Body,
                BlockKind::Code,
                BlockKind::Quote,
                BlockKind::List,
                BlockKind::Hr
            ]
        );
        // H1：左竖带 + 顶横带出墨（[ 半包框）
        let b = &lay.blocks[0];
        assert!(ink(&buf, w, 1, b.y + dp::HEAD_CORNER_R + 1), "H1 左竖带");
        assert!(ink(&buf, w, dp::HEAD_CORNER_R + 4, b.y + 1), "H1 顶横带");
        // H2 文字带出墨
        let b = &lay.blocks[1];
        assert!(
            (b.y..b.y + b.line_h).any(|y| (0..200).any(|x| ink(&buf, w, x, y))),
            "H2 文字"
        );
        // 正文行出墨（含粗体/行内码 chip：chip 是不透明直写块，文字带中部必有墨）
        let b = &lay.blocks[2];
        assert!(
            (b.y..b.y + b.line_h).any(|y| (0..400).any(|x| ink(&buf, w, x, y))),
            "Body 文字"
        );
        // 代码围栏：值框左边框出墨（3px 细框）
        let b = &lay.blocks[3];
        assert!(ink(&buf, w, 1, b.y + b.h / 2), "围栏左框");
        // 引用：左竖线出墨
        let b = &lay.blocks[4];
        assert!(ink(&buf, w, 0, b.y + 2), "引用竖线");
        // 列表：▪ 8px 方块在项首行中线出墨
        let b = &lay.blocks[5];
        let my = b.y + (b.line_h - dp::LIST_MARK_PX) / 2 + dp::LIST_MARK_PX / 2;
        assert!(ink(&buf, w, dp::LIST_MARK_PX / 2, my), "列表 ▪ 符");
        // 分隔线：5px 横带中线出墨（BAR-218）
        let b = &lay.blocks[6];
        assert!(ink(&buf, w, w / 2, b.y + dp::HU), "分隔线");
    }

    #[test]
    fn spec_bar169_41_滚动平移与上裁下裁() {
        // 两行正文，视口 [100, 200)：scroll=36 后第二行进视口位 =
        // 不滚时第一行的位（平移账）；clip 上方恒无墨（上裁纪律）
        let md = "line one\n\nline two";
        let w = 800;
        let h = 600;
        let clip = (100i64, 200i64);
        let (buf0, lay) = paint(md, w, h, 100, clip);
        assert_eq!(lay.blocks.len(), 2);
        let gap = lay.blocks[1].y - lay.blocks[0].y; // 行带 + 块隙
        let (buf1, _) = paint(md, w, h, 100 - i64::from(gap), clip);
        // 上裁：clip 上方两版都必须无墨
        for y in 0..clip.0 as u32 {
            assert!((0..w).all(|x| !ink(&buf0, w, x, y)), "上裁区有墨 y={y}");
            assert!(
                (0..w).all(|x| !ink(&buf1, w, x, y)),
                "上裁区有墨(滚后) y={y}"
            );
        }
        // 平移账：滚后 line two 的墨带 = 不滚时 line one 的墨带（同位同形）
        let b0 = lay.blocks[0].y + 100; // 不滚时 line one 屏位
        let has_ink = |buf: &Vec<u32>, y0: u32| {
            (y0..y0 + lay.blocks[0].line_h).any(|y| (0..300).any(|x| ink(buf, w, x, y)))
        };
        assert!(has_ink(&buf0, b0), "不滚时 line one 在视口顶出墨");
        assert!(has_ink(&buf1, b0), "滚一块隙后 line two 落在同一屏位");
    }

    #[test]
    fn spec_bar207_预览走真管线且为mini档() {
        // BAR-207 组件池 md 引擎栏：paint_md_preview = parse→layout→paint
        // 零平行实现 + mini 实例格（24px 档等价，BAR-204 格缩即字缩）。
        // 钉两件事：①出墨与 paint_md_body 同帧等价（同一样品同一缓冲区
        // 两路画，逐像素相等）；②布局行高是 mini 格不是默认格
        // （防有人改回正文格，展品变巨人）。
        let t = tv();
        let sample = "# T\n\nbody";
        let w = 200u32;
        let h = 400u32;
        let clip = (0i64, i64::from(h));
        let denom = ((w - 1) + (h - 1)) as i64;
        let acc = crate::ui::accent::FALLBACK;
        let mini = PREVIEW_MINI_CELL;
        let lay = layout_md(sample, w, mini);
        let mut buf_a = vec![0u32; (w * h) as usize];
        {
            let mut frame = Frame {
                buf: &mut buf_a,
                w,
                h,
            };
            t.paint_md_preview(&mut frame, sample, 0, 0, w, clip, denom, acc);
        }
        let mut buf_b = vec![0u32; (w * h) as usize];
        {
            let mut frame = Frame {
                buf: &mut buf_b,
                w,
                h,
            };
            t.paint_md_body(&mut frame, &lay, 0, 0, w, clip, denom, acc);
        }
        assert_eq!(buf_a, buf_b, "预览路与正文路同帧不等价——出现平行实现");
        assert!(buf_a.iter().any(|&p| p & 0x00FF_FFFF != 0), "预览零墨");
        // mini 档钉：同一样品默认格行高必须更高（不等式判卷，不钉死像素值）
        let lay_default = layout_md(sample, w, t.cell_size());
        assert!(
            lay.blocks[0].line_h < lay_default.blocks[0].line_h,
            "预览行高未落到 mini 档"
        );
    }

    #[test]
    fn spec_bar169_42_空文档与病态尺寸不炸() {
        let (_, lay) = paint("", 800, 600, 0, (0, 600));
        assert_eq!(lay.blocks.len(), 1, "空文档占位一块");
        let (buf, _) = paint("", 800, 600, 0, (0, 600));
        assert!(buf.iter().all(|&p| p == 0), "空行零墨");
        // 零宽/零高/负原点不炸
        let _ = paint("# x", 0, 0, 0, (0, 0));
        let _ = paint("# x", 800, 600, -5000, (0, 600));
    }

    #[test]
    fn spec_bar218_表格三档出墨() {
        // Fit：表头下划带中线出墨（3px accent 渐变）
        let md = "| A | B |\n|---|---|\n| x | y |";
        let (buf, lay) = paint(md, 600, 1200, 0, (0, 1200));
        let b = &lay.blocks[0];
        let t = b.table.as_ref().unwrap();
        let uy = b.y + t.header.h + (dp::TABLE_ROW_PAD - dp::TABLE_HEAD_UNDER) / 2 + 1;
        assert!(ink(&buf, 600, 300, uy), "Fit 表头下划带出墨");
        // 表头文字出墨（淡彩 slot4）
        assert!(
            (b.y..b.y + t.header.h).any(|y| (0..200).any(|x| ink(&buf, 600, x, y))),
            "表头文字带"
        );
        // Cards：窄宽 3 列长文本 → 每行一卡，卡框左边框出墨（值框同配方 3px）
        let md3 = "| h1 | h2 | h3 |\n|---|---|---|\n| tttttttttttttttttttt | vaaaaaaaaaaaaaaaaaaa | vbbbbbbbbbbbbbbbbbbbb |";
        let (buf3, lay3) = paint(md3, 200, 1200, 0, (0, 1200));
        let b3 = &lay3.blocks[0];
        let t3 = b3.table.as_ref().unwrap();
        assert_eq!(t3.tier, crate::ui::md_layout::TableTier::Cards);
        let card = &t3.rows[0];
        assert!(
            ink(&buf3, 200, 1, b3.y + card.y + card.h / 2),
            "Cards 卡框左边框出墨"
        );
        // DefList：窄宽 2 列 → 无框但名带出墨
        let md2 = "| 属性 | 值 |\n|---|---|\n| 名称xxxxxxxxxx | na客户端yyyyyyyyyy |";
        let (buf2, lay2) = paint(md2, 200, 1200, 0, (0, 1200));
        let b2 = &lay2.blocks[0];
        let t2 = b2.table.as_ref().unwrap();
        assert_eq!(t2.tier, crate::ui::md_layout::TableTier::DefList);
        assert!(
            (b2.y..b2.y + b2.h).any(|y| (0..150).any(|x| ink(&buf2, 200, x, y))),
            "DefList 条目出墨"
        );
    }

    #[test]
    fn spec_bar218_分隔线五px厚() {
        // BAR-218 用户再判「太细」：HR_THICK 3→5——分隔线带中线列的
        // 连续出墨行数 = 5（厚度钉，回不薄）
        let (buf, lay) = paint("上文\n\n---\n\n下文", 600, 600, 0, (0, 600));
        let b = lay
            .blocks
            .iter()
            .find(|b| b.kind == BlockKind::Hr)
            .expect("分隔线块");
        let inked: Vec<u32> = (b.y..b.y + b.h)
            .filter(|&y| ink(&buf, 600, 300, y))
            .collect();
        assert_eq!(
            inked.len() as u32,
            dp::HR_THICK,
            "分隔线出墨行数 = HR_THICK（5px）"
        );
    }

    #[test]
    fn spec_bar204_涂装尺与排版尺同源() {
        // 涂装 pen 步进 = 排版行宽同一把格尺：正文「aa」（2 格 × 18px =
        // 36px）墨不许越过排版行宽右缘——涂装尺分叉（步进不吃实例格/
        // 不吃档 scale）的变异在这里红（第二字墨落进 36..72）
        let (buf, lay) = paint("aa", 200, 600, 0, (0, 600));
        let b = &lay.blocks[0];
        assert_eq!(b.lines[0].w, 36, "排版尺：2 格 × 18px 步进");
        let band = b.y..b.y + b.line_h;
        assert!(
            band.clone().any(|y| (0..36).any(|x| ink(&buf, 200, x, y))),
            "行宽内必须有墨"
        );
        for y in band {
            assert!(
                (36..200).all(|x| !ink(&buf, 200, x, y)),
                "墨溢过排版行宽右缘 y={y}（涂装尺分叉）"
            );
        }
    }
}
