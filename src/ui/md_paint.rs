//! ui/md_paint.rs — md 绘制层（BAR-169 md 渲染器一期，壳层）：MdLayout
//! → 像素。涂装规格 = theme.md §2.5 淡彩六色家族 + §三 md 条款——**demo
//! 页涂装即打样规格**，本册是同配方的数据驱动版（demo 页硬编码样品
//! 二期才换芯，本单不动）。
//!
//! 消费关系：md_parse（事件流）→ md_layout（折行几何，行宽实量存档）
//! → 本册（照几何涂装，量宽与排版同一把尺 = MdMeasure 对 TermView 的
//! 落地）。查看器正文视口纵裁剪由调用方喂 clip（BAR-167 ①滚动双裁
//! 纪律：上裁防污染标题/分隔线带，下裁守关闭钮前隙）。

use crate::termview::{
    CELL_W, Frame, TermView, paint_demo_chip, paint_thin_frame, ring_gradient_rgb,
};
use crate::ui::accent::{self, AccentPair};
use crate::ui::demo_page as dp;
use crate::ui::demo_page::{BlockKind, SegStyle};
use crate::ui::md_layout::{MdLayout, MdMeasure};

/// 量宽通路落地：排版与涂装同一把尺（TermView::text_width 真字尺）
impl MdMeasure for TermView {
    fn md_text_w(&self, text: &str, px: f32) -> u32 {
        self.text_width(text, px)
    }
}

/// dyn 转发（android 壳的 term 句柄是 Box<dyn TermEmu>——cfg 盲区件，
/// android check 唯一兜底）；量宽仍走同一条 text_width 通路
impl MdMeasure for Box<dyn crate::termview::TermEmu> {
    fn md_text_w(&self, text: &str, px: f32) -> u32 {
        self.text_width(text, px)
    }
}

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
                                b.px,
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
                            frame, line, x0, content_r, ly, b.line_h, b.px, fg, 0.0, yclip,
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
                            frame, line, x0, content_r, ly, b.line_h, b.px, fg, 0.0, yclip,
                        );
                        if double {
                            self.md_text_line(
                                frame, line, x0, content_r, ly, b.line_h, b.px, fg, 1.0, yclip,
                            );
                        }
                    }
                }
                BlockKind::Body => {
                    for (i, line) in b.lines.iter().enumerate() {
                        let ly = by + i as i64 * i64::from(b.line_h);
                        self.md_spans_line(
                            frame, line, x0, content_r, ly, b.line_h, b.px, body_fg, &pastel,
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
                            b.px,
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
                            b.px,
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
                            b.px,
                            body_fg,
                            0.0,
                            yclip,
                        );
                    }
                }
                BlockKind::Hr => {
                    // 3px 横向 accent 渐变（块带 HU×2 内居中，宪法 2026-09-27
                    // 拍板加粗与框厚同尺）
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
            }
        }
    }

    /// 单行文字涂装（无行内段）：右裁 content_r + 纵裁 yclip
    #[allow(clippy::too_many_arguments)]
    fn md_text_line(
        &self,
        frame: &mut Frame<'_>,
        line: &crate::ui::md_layout::MdLine,
        x: i64,
        content_r: i64,
        y: i64,
        rh: u32,
        px: f32,
        fg: u32,
        inset: f32,
        yclip: Option<(i32, i32)>,
    ) {
        // 段列拼串单行画（H1-H6/引用/列表/代码行内无样式段；正文段排走
        // md_spans_line）—— spans 为一行的完整样式列，这里顺序段排
        let mut pen = x;
        for (style, text) in &line.spans {
            let _ = style; // 标题/引用等单色行：样式段文字同色（行内强调档归正文路）
            if pen >= content_r {
                break;
            }
            let cx = pen.max(0);
            let cr = content_r.min(i64::from(frame.w));
            if cx >= cr || y + i64::from(rh) <= 0 || y >= i64::from(frame.h) {
                pen += i64::from(self.text_width(text, px));
                continue;
            }
            self.draw_text_left_ex(
                frame,
                text,
                cx as u32,
                (cr - cx) as u32,
                y.max(0) as u32,
                rh,
                px,
                fg,
                inset,
                yclip,
            );
            pen += i64::from(self.text_width(text, px));
        }
    }

    /// 正文行段排（粗体 = 淡彩 slot0 双绘；行内码 = 淡彩 slot3 + 渐变
    /// 暗底小块——demo 页打样配方数据驱动版，pen 累进与排版行宽同尺）
    #[allow(clippy::too_many_arguments)]
    fn md_spans_line(
        &self,
        frame: &mut Frame<'_>,
        line: &crate::ui::md_layout::MdLine,
        x0: i64,
        content_r: i64,
        y: i64,
        rh: u32,
        px: f32,
        body_fg: u32,
        pastel: &[u32; 6],
        accent: AccentPair,
        denom: i64,
        yclip: Option<(i32, i32)>,
        clip: (i64, i64),
    ) {
        let mut pen = x0;
        for (style, text) in &line.spans {
            let tw = i64::from(self.text_width(text, px));
            match style {
                SegStyle::Normal => {
                    self.md_text_line(
                        frame,
                        &seg_line(style, text),
                        pen,
                        content_r,
                        y,
                        rh,
                        px,
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
                            px,
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
                        px,
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
    /// parse→layout→paint 零平行实现，mini 字号档（24px/1.4，宪法同
    /// 行距）。x0/y0 = 展台内区原点，cw = 内区宽，clip = 展台纵裁剪带
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
        let style = crate::ui::md_layout::MdStyle {
            body_px: 24.0,
            line_ratio: dp::LINE_RATIO,
        };
        let lay = crate::ui::md_layout::layout_md(sample, cw, &style, self);
        self.paint_md_body(frame, &lay, x0, y0, cw, clip, denom, accent);
    }
}

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
    use crate::ui::md_layout::{MdLayout, MdStyle, layout_md};

    fn tv() -> TermView {
        crate::termview::build_vendored().expect("内嵌字体必成").0
    }

    fn paint(md: &str, w: u32, h: u32, y0: i64, clip: (i64, i64)) -> (Vec<u32>, MdLayout) {
        let t = tv();
        let lay = layout_md(md, w, &MdStyle::default(), &t);
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
        // 分隔线：3px 横带中线出墨
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
        // 零平行实现 + mini 字号（24px）。钉两件事：①出墨与 paint_md_body
        // 同帧等价（同一样品同一缓冲区两路画，逐像素相等）；②布局行高
        // 是 mini 档不是默认档（防有人改回正文字号，展品变巨人）。
        let t = tv();
        let sample = "# T\n\nbody";
        let w = 200u32;
        let h = 400u32;
        let clip = (0i64, i64::from(h));
        let denom = ((w - 1) + (h - 1)) as i64;
        let acc = crate::ui::accent::FALLBACK;
        let mini = MdStyle {
            body_px: 24.0,
            line_ratio: dp::LINE_RATIO,
        };
        let lay = layout_md(sample, w, &mini, &t);
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
        // mini 档钉：同一样品默认档行高必须更高（不等式判卷，不钉死像素值）
        let lay_default = layout_md(sample, w, &MdStyle::default(), &t);
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
}
