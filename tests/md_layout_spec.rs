//! md_layout.rs A 档考题（BAR-169 md 渲染器一期·排版层）：
//! 尺子单源咬合 demo_page / 折行条款 / 块几何累进 / 样式可调项。
//! Mock 定宽尺：char_w = px/2（加性无漂移，钉可算精确值）。
//! 变异抽检记录见 bugs.md BAR-169 行。

use kfm_na::ui::demo_page as dp;
use kfm_na::ui::demo_page::{BlockKind, SegStyle};
use kfm_na::ui::md_layout::{MdMeasure, MdStyle, layout_md, line_h_styled};

/// Mock 定宽尺：每字符 px/2（36px 正文字 = 18px/字）
struct Mock;
impl MdMeasure for Mock {
    fn md_text_w(&self, text: &str, px: f32) -> u32 {
        text.chars().count() as u32 * (px as u32 / 2)
    }
}

fn def() -> MdStyle {
    MdStyle::default()
}

#[test]
fn spec_bar169_20_行高式与demo_page逐值咬合() {
    // 缺省 ratio 下 line_h_styled ≡ demo_page::line_h（尺子单源编译期同式钉）
    for px in [30.0, 36.0, 43.2, 52.2, 61.2] {
        assert_eq!(line_h_styled(px, dp::LINE_RATIO), dp::line_h(px), "px={px}");
    }
}

#[test]
fn spec_bar169_21_块几何全咬半格网且严格累进() {
    let md = "# 题\n\n正文段\n\n```\nx\n```\n\n> 引\n\n- 项\n\n---\n\n## 二";
    let lay = layout_md(md, 500, &def(), &Mock);
    let mut y = 0;
    for (i, b) in lay.blocks.iter().enumerate() {
        assert_eq!(b.y, y, "块{i} y 累进");
        assert_eq!(b.y % dp::HU, 0, "块{i} y 咬半格网");
        assert_eq!(b.h % dp::HU, 0, "块{i} h 咬半格网");
        y += b.h + dp::BLOCK_GAP;
    }
    // total_h = 块高和 + 块隙×(n−1)（尾块后不欠隙）
    assert_eq!(lay.total_h, y - dp::BLOCK_GAP);
    // 块序 = 文档序
    let kinds: Vec<BlockKind> = lay.blocks.iter().map(|b| b.kind).collect();
    assert_eq!(
        kinds,
        vec![
            BlockKind::H1,
            BlockKind::Body,
            BlockKind::Code,
            BlockKind::Quote,
            BlockKind::List,
            BlockKind::Hr,
            BlockKind::H2
        ]
    );
}

#[test]
fn spec_bar169_22_正文折行满即断刚好放下不断() {
    // 36px 字 = 18px/字；content 90px = 恰好 5 字一行不断；6 字 = 两行
    let lay = layout_md("甲乙丙丁戊", 90, &def(), &Mock);
    assert_eq!(lay.blocks[0].lines.len(), 1, "恰好放下不断");
    assert_eq!(lay.blocks[0].lines[0].w, 90);
    assert_eq!(lay.blocks[0].h, dp::line_h(36.0));

    let lay = layout_md("甲乙丙丁戊己", 90, &def(), &Mock);
    assert_eq!(lay.blocks[0].lines.len(), 2, "满即断");
    assert_eq!(lay.blocks[0].lines[0].w, 90);
    assert_eq!(lay.blocks[0].lines[1].w, 18);
    assert_eq!(lay.blocks[0].h, dp::line_h(36.0) * 2);
}

#[test]
fn spec_bar169_23_折行保样式段跨行不丢() {
    // 粗体段跨折行：两行都保住 Bold 样式（行内段排归涂装，样式随字走）
    let lay = layout_md("甲乙**丙丁戊己**", 90, &def(), &Mock);
    let l0 = &lay.blocks[0].lines[0];
    let l1 = &lay.blocks[0].lines[1];
    assert_eq!(
        l0.spans,
        vec![
            (SegStyle::Normal, "甲乙".to_string()),
            (SegStyle::Bold, "丙丁戊".to_string())
        ]
    );
    assert_eq!(l1.spans, vec![(SegStyle::Bold, "己".to_string())]);
}

#[test]
fn spec_bar169_24_h1折行宽扣框量且行宽随字() {
    // H1 折行宽 = 内容宽 − HEAD_TEXT_INSET(18) − HEAD_TOP_TAIL(9)（横带
    // 随字宽不吃满）；块高 = 上垫 + 行带×n + 下垫
    let px = dp::BODY_PX * dp::H1_SCALE; // 61.2 → mock 30px/字
    let lh = dp::line_h(px);
    // 内容宽 90：扣框量后折行宽 63px = 2 字/行（60 恰好放下）；不扣 = 90px
    // 整行 3 字恰好放下——本钉对「摘框量」变异敏感（首版 60px 尺两路同折
    // 两行，变异漏杀实录：钉的敏感区 = 两路分叉的宽度带）
    let cw = 90;
    let lay = layout_md("# 甲乙丙", cw, &def(), &Mock);
    let b = &lay.blocks[0];
    assert_eq!(b.kind, BlockKind::H1);
    assert_eq!(b.lines.len(), 2, "扣框量后 2 字/行，3 字折两行");
    assert_eq!(b.h, dp::HU + lh * 2 + dp::HU);
    assert_eq!(b.line_h, lh);
    assert_eq!(b.px, px);
    // 行宽随字：首行 2 字 60px，次行 1 字 30px（涂装横带读此不重算）
    assert_eq!(b.lines[0].w, 60);
    assert_eq!(b.lines[1].w, 30);
}

#[test]
fn spec_bar169_25_代码围栏不折行() {
    // 长行原样一行（涂装右缘断墨取舍），行宽如实记（>内容宽不钳）
    let lay = layout_md("```\nabcdefghij甲乙丙丁戊己\n```", 90, &def(), &Mock);
    let b = &lay.blocks[0];
    assert_eq!(b.kind, BlockKind::Code);
    assert_eq!(b.lines.len(), 1);
    let want_w = 16 * (dp::CODE_PX as u32 / 2); // 16 字 × 15px
    assert_eq!(b.lines[0].w, want_w);
    assert!(want_w > 90, "夹具成立前提：真超宽");
    // 块高 = 上垫 + 行带 + 下垫；行带吃 CODE_PX 不吃正文字号
    let lh = dp::line_h(dp::CODE_PX);
    assert_eq!(b.h, dp::HU + lh + dp::HU);
    assert_eq!(b.px, dp::CODE_PX);
}

#[test]
fn spec_bar169_26_引用列表折行宽扣缩进() {
    // 引用折行宽 = 内容宽 − INDENT_W(18)；列表 = − LIST_TEXT_INSET(36)
    // 36px 字 18px/字：内容 90 − 18 = 72 = 恰 4 字；列表 90 − 36 = 54 = 恰 3 字
    let lay = layout_md("> 甲乙丙丁戊", 90, &def(), &Mock);
    assert_eq!(lay.blocks[0].kind, BlockKind::Quote);
    assert_eq!(lay.blocks[0].lines.len(), 2, "扣 1 格缩进后 4 字/行");
    assert_eq!(lay.blocks[0].lines[0].w, 72);

    let lay = layout_md("- 甲乙丙丁", 90, &def(), &Mock);
    assert_eq!(lay.blocks[0].kind, BlockKind::List);
    assert_eq!(lay.blocks[0].lines.len(), 2, "扣 2 格符号位后 3 字/行");
    assert_eq!(lay.blocks[0].lines[0].w, 54);
}

#[test]
fn spec_bar169_27_标题字号阶梯与文字档() {
    let lay = layout_md(
        "## 二\n### 三\n#### 四\n##### 五\n###### 六",
        500,
        &def(),
        &Mock,
    );
    assert_eq!(lay.blocks[0].px, dp::BODY_PX * dp::H2_SCALE);
    assert_eq!(lay.blocks[1].px, dp::BODY_PX * dp::H3_SCALE);
    assert_eq!(lay.blocks[2].px, dp::BODY_PX);
    assert_eq!(lay.blocks[2].kind, BlockKind::H4);
    assert_eq!(lay.blocks[3].kind, BlockKind::H5);
    assert_eq!(lay.blocks[4].kind, BlockKind::H6);
    // H2/H3 无框块高 = 行带×n（摘框后无上下垫，宪法 2026-09-27 修宪）
    assert_eq!(lay.blocks[0].h, lay.blocks[0].line_h);
}

#[test]
fn spec_bar169_28_空文档一块空正文占位不塌() {
    let lay = layout_md("", 500, &def(), &Mock);
    assert_eq!(lay.blocks.len(), 1);
    assert_eq!(lay.blocks[0].kind, BlockKind::Body);
    assert_eq!(lay.blocks[0].lines.len(), 1);
    assert_eq!(lay.blocks[0].lines[0].w, 0);
    assert_eq!(lay.total_h, dp::line_h(dp::BODY_PX));
}

#[test]
fn spec_bar169_29_样式可调项真生效() {
    // 字号基准 36→72：行高翻倍、折行变密（字宽 36px/字，90px 内容 2 字/行）
    let big = MdStyle {
        body_px: 72.0,
        line_ratio: dp::LINE_RATIO,
    };
    let lay = layout_md("甲乙丙丁戊", 90, &big, &Mock);
    assert_eq!(lay.blocks[0].line_h, line_h_styled(72.0, dp::LINE_RATIO));
    assert_eq!(lay.blocks[0].lines.len(), 3, "2 字/行 ×5 = 3 行");
    // 行距 1.4→2.0：行高变、折行不变
    let loose = MdStyle {
        body_px: 36.0,
        line_ratio: 2.0,
    };
    let lay = layout_md("甲乙丙丁戊", 90, &loose, &Mock);
    assert_eq!(lay.blocks[0].line_h, line_h_styled(36.0, 2.0));
    assert_ne!(lay.blocks[0].line_h, dp::line_h(36.0), "行距旋钮真生效");
    assert_eq!(lay.blocks[0].lines.len(), 1);
}

#[test]
fn spec_bar169_30_分隔线块几何() {
    let lay = layout_md("---", 500, &def(), &Mock);
    let b = &lay.blocks[0];
    assert_eq!(b.kind, BlockKind::Hr);
    assert_eq!(b.h, dp::HU * 2, "上下各 0.5 格线体居中");
    assert!(b.lines.is_empty());
}

// ---- BAR-208 排版缓存钉（滚动卡帧根修的考题面）----

#[test]
fn spec_bar208_缓存_peek未中_cached后中_同键同arc() {
    use kfm_na::ui::md_layout::{layout_md_cached, layout_md_peek};
    let st = def();
    // 全新代：peek 未中（只查不排——帧内高频路径不许偷偷排版）
    assert!(
        layout_md_peek(9001, 400, &st).is_none(),
        "新代 peek 必须未中（peek 排版 = 高频路径偷偷全量重排）"
    );
    // cached 全路：排版入柜
    let a = layout_md_cached(9001, "# 标题\n\n正文", 400, &st, &Mock);
    // 同键 peek 命中且同一份 Arc（零克隆零重排的兑现）
    let b = layout_md_peek(9001, 400, &st).expect("cached 后 peek 必须命中");
    assert!(
        std::sync::Arc::ptr_eq(&a, &b),
        "同键必须共读同一份 Arc（重排 = 病灶回潮）"
    );
    // 同键 cached 也不再排（直接回柜）
    let c = layout_md_cached(9001, "# 标题\n\n正文", 400, &st, &Mock);
    assert!(std::sync::Arc::ptr_eq(&a, &c), "同键 cached 必须回柜不重排");
}

#[test]
fn spec_bar208_缓存_代宽样式三位各管失效() {
    use kfm_na::ui::md_layout::{layout_md_cached, layout_md_peek};
    let st = def();
    let base = layout_md_cached(9101, "正文内容", 400, &st, &Mock);
    // 换代（新块回执）→ 未中 → 重排入新柜
    assert!(layout_md_peek(9102, 400, &st).is_none(), "换代必须失效");
    let new_gen = layout_md_cached(9102, "正文内容更多", 400, &st, &Mock);
    assert!(!std::sync::Arc::ptr_eq(&base, &new_gen), "换代必须重排");
    // 改宽（横竖屏/字号联动）→ 未中
    assert!(layout_md_peek(9102, 500, &st).is_none(), "改宽必须失效");
    let new_w = layout_md_cached(9102, "正文内容更多", 500, &st, &Mock);
    assert!(!std::sync::Arc::ptr_eq(&new_gen, &new_w), "改宽必须重排");
    // 改样式（渲染设置卡旋钮）→ 未中
    let st2 = MdStyle {
        body_px: st.body_px * 1.2,
        ..st
    };
    assert!(layout_md_peek(9102, 500, &st2).is_none(), "改样式必须失效");
}
