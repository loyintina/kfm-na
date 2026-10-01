//! md_layout.rs A 档考题（BAR-169 md 渲染器一期·排版层；BAR-204 换芯
//! 网格文字引擎）：尺子 = char_cells × 实例格步进（排版与涂装同源唯一
//! 纯函数），唯一可调维 = 实例格（cell_w, cell_h)（pinch 双指缩放一统，
//! 字号档体系废除）。行高 = cell_h × 档 scale × 宪法行距 上取整咬实例
//! 半格网；块隙/缩进档全读 demo_page 宪法常量表不动。
//! 变异抽检记录见 bugs.md BAR-204 行。

use kfm_na::ui::demo_page as dp;
use kfm_na::ui::demo_page::{BlockKind, SegStyle};
use kfm_na::ui::md_layout::{grid_stepped_w, layout_md, line_h_grid};

/// 1× 缺省锚（基准格 = termview CELL_W/CELL_H）
const X1: (u32, u32) = (18, 36);

#[test]
fn spec_bar169_20_行高式与demo_page逐值咬合() {
    // 1× 缺省等价承诺（BAR-204 新约编译期钉的运行期姊妹）：cell_h=36 时
    // line_h_grid ≡ 旧 line_h_styled ≡ demo_page::line_h（全档位逐值）
    for scale in [
        dp::CODE_PX / dp::BODY_PX,
        1.0,
        dp::H3_SCALE,
        dp::H2_SCALE,
        dp::H1_SCALE,
    ] {
        assert_eq!(
            line_h_grid(36, scale),
            dp::line_h(dp::BODY_PX * scale),
            "scale={scale}"
        );
    }
}

#[test]
fn spec_bar169_21_块几何全咬半格网且严格累进() {
    let md = "# 题\n\n正文段\n\n```\nx\n```\n\n> 引\n\n- 项\n\n---\n\n## 二";
    let lay = layout_md(md, 500, X1);
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
    // 格口径：1× 正文步进 18px/格；CJK 2 格 = 36px/字。内容 72px =
    // 恰好 2 字一行不断；3 字 = 两行（满即断、刚好放下不断——同
    // modal::wrap_text 律，尺子从像素实量换格步进）
    let lay = layout_md("甲乙", 72, X1);
    assert_eq!(lay.blocks[0].lines.len(), 1, "恰好放下不断");
    assert_eq!(lay.blocks[0].lines[0].w, 72);
    assert_eq!(lay.blocks[0].h, dp::line_h(36.0));

    let lay = layout_md("甲乙丙", 72, X1);
    assert_eq!(lay.blocks[0].lines.len(), 2, "满即断");
    assert_eq!(lay.blocks[0].lines[0].w, 72);
    assert_eq!(lay.blocks[0].lines[1].w, 36);
    assert_eq!(lay.blocks[0].h, dp::line_h(36.0) * 2);
}

#[test]
fn spec_bar169_23_折行保样式段跨行不丢() {
    // 粗体段跨折行：各行都保住 Bold 样式（行内段排归涂装，样式随字走）
    let lay = layout_md("甲乙**丙丁戊己**", 72, X1);
    let lines = &lay.blocks[0].lines;
    assert_eq!(lines.len(), 3, "2 字/行 ×6 = 3 行");
    assert_eq!(lines[0].spans, vec![(SegStyle::Normal, "甲乙".to_string())]);
    assert_eq!(lines[1].spans, vec![(SegStyle::Bold, "丙丁".to_string())]);
    assert_eq!(lines[2].spans, vec![(SegStyle::Bold, "戊己".to_string())]);
}

#[test]
fn spec_bar169_24_h1折行宽扣框量且行宽随字() {
    // H1 折行宽 = 内容宽 − HEAD_TEXT_INSET(18) − HEAD_TOP_TAIL(9)（横带
    // 随字宽不吃满）；块高 = 上垫 + 行带×n + 下垫
    let lh = dp::line_h(dp::BODY_PX * dp::H1_SCALE); // 90
    // H1 步进 = 18 × 1.7 = 30.6/格，CJK 61.2px/字。内容宽 90：扣框量后
    // 63px = 1 字/行（61.2 恰好放下）；不扣 = 90px 仍 1 字/行（122.4 > 90）
    // ——换 150 内容宽拉开分叉：扣框量后 123 = 2 字/行，不扣 150 也是 2 字
    // /行？不：150 不扣 = 2 字恰好（122.4 ≤ 150）……敏感区取 123~150 间
    // 的夹具：内容 130 → 扣框量 103 = 1 字/行；不扣 130 = 2 字/行
    let cw = 130;
    let lay = layout_md("# 甲乙丙", cw, X1);
    let b = &lay.blocks[0];
    assert_eq!(b.kind, BlockKind::H1);
    assert_eq!(b.lines.len(), 3, "扣框量后 1 字/行，3 字折 3 行");
    assert_eq!(b.h, dp::HU + lh * 3 + dp::HU);
    assert_eq!(b.line_h, lh);
    assert_eq!(b.scale, dp::H1_SCALE);
    // 行宽随字：每行 1 字 = 2 格 × 30.6 = 61.2 → 61px（涂装横带读此不重算）
    assert_eq!(b.lines[0].w, 61);
}

#[test]
fn spec_bar169_25_代码围栏不折行() {
    // 长行原样一行（涂装右缘断墨取舍），行宽如实记（>内容宽不钳）
    let lay = layout_md("```\nabcdefghij甲乙丙丁戊己\n```", 90, X1);
    let b = &lay.blocks[0];
    assert_eq!(b.kind, BlockKind::Code);
    assert_eq!(b.lines.len(), 1);
    // 格口径：22 格（10 ASCII + 6 CJK×2）× 代码档步进 15px（18×30/36）
    let want_w = 22 * 15;
    assert_eq!(b.lines[0].w, want_w);
    assert!(want_w > 90, "夹具成立前提：真超宽");
    // 块高 = 上垫 + 行带 + 下垫；行带吃代码档 scale（30/36）不吃正文
    let lh = dp::line_h(dp::CODE_PX);
    assert_eq!(b.h, dp::HU + lh + dp::HU);
    assert_eq!(b.scale, dp::CODE_PX / dp::BODY_PX);
}

#[test]
fn spec_bar169_26_引用列表折行宽扣缩进() {
    // 引用折行宽 = 内容宽 − INDENT_W(18)；列表 = − LIST_TEXT_INSET(36)
    // 格口径 36px/字：内容 90 − 18 = 72 = 恰 2 字；列表 90 − 36 = 54 = 恰 1 字
    let lay = layout_md("> 甲乙丙丁戊", 90, X1);
    assert_eq!(lay.blocks[0].kind, BlockKind::Quote);
    assert_eq!(lay.blocks[0].lines.len(), 3, "扣 1 格缩进后 2 字/行");
    assert_eq!(lay.blocks[0].lines[0].w, 72);

    let lay = layout_md("- 甲乙丙丁", 90, X1);
    assert_eq!(lay.blocks[0].kind, BlockKind::List);
    assert_eq!(lay.blocks[0].lines.len(), 4, "扣 2 格符号位后 1 字/行");
    assert_eq!(lay.blocks[0].lines[0].w, 36);
}

#[test]
fn spec_bar169_27_标题阶梯与文字档() {
    let lay = layout_md("## 二\n### 三\n#### 四\n##### 五\n###### 六", 500, X1);
    assert_eq!(lay.blocks[0].scale, dp::H2_SCALE);
    assert_eq!(lay.blocks[1].scale, dp::H3_SCALE);
    assert_eq!(lay.blocks[2].scale, 1.0);
    assert_eq!(lay.blocks[2].kind, BlockKind::H4);
    assert_eq!(lay.blocks[3].kind, BlockKind::H5);
    assert_eq!(lay.blocks[4].kind, BlockKind::H6);
    // H2/H3 无框块高 = 行带×n（摘框后无上下垫，宪法 2026-09-27 修宪）
    assert_eq!(lay.blocks[0].h, lay.blocks[0].line_h);
}

#[test]
fn spec_bar169_28_空文档一块空正文占位不塌() {
    let lay = layout_md("", 500, X1);
    assert_eq!(lay.blocks.len(), 1);
    assert_eq!(lay.blocks[0].kind, BlockKind::Body);
    assert_eq!(lay.blocks[0].lines.len(), 1);
    assert_eq!(lay.blocks[0].lines[0].w, 0);
    assert_eq!(lay.total_h, dp::line_h(dp::BODY_PX));
}

#[test]
fn spec_bar169_30_分隔线块几何() {
    let lay = layout_md("---", 500, X1);
    let b = &lay.blocks[0];
    assert_eq!(b.kind, BlockKind::Hr);
    assert_eq!(b.h, dp::HU * 2, "上下各 0.5 格线体居中");
    assert!(b.lines.is_empty());
}

// ---- BAR-208 排版缓存钉（滚动卡帧根修的考题面；BAR-204 键换实例格两维）----

#[test]
fn spec_bar208_缓存_peek未中_cached后中_同键同arc() {
    use kfm_na::ui::md_layout::{layout_md_cached, layout_md_peek};
    // 全新代：peek 未中（只查不排——帧内高频路径不许偷偷排版）
    assert!(
        layout_md_peek(9001, 400, X1).is_none(),
        "新代 peek 必须未中（peek 排版 = 高频路径偷偷全量重排）"
    );
    // cached 全路：排版入柜
    let a = layout_md_cached(9001, "# 标题\n\n正文", 400, X1);
    // 同键 peek 命中且同一份 Arc（零克隆零重排的兑现）
    let b = layout_md_peek(9001, 400, X1).expect("cached 后 peek 必须命中");
    assert!(
        std::sync::Arc::ptr_eq(&a, &b),
        "同键必须共读同一份 Arc（重排 = 病灶回潮）"
    );
    // 同键 cached 也不再排（直接回柜）
    let c = layout_md_cached(9001, "# 标题\n\n正文", 400, X1);
    assert!(std::sync::Arc::ptr_eq(&a, &c), "同键 cached 必须回柜不重排");
}

#[test]
fn spec_bar208_缓存_代宽格三位各管失效() {
    use kfm_na::ui::md_layout::{layout_md_cached, layout_md_peek};
    let base = layout_md_cached(9101, "正文内容", 400, X1);
    // 换代（新块回执）→ 未中 → 重排入新柜
    assert!(layout_md_peek(9102, 400, X1).is_none(), "换代必须失效");
    let new_gen = layout_md_cached(9102, "正文内容更多", 400, X1);
    assert!(!std::sync::Arc::ptr_eq(&base, &new_gen), "换代必须重排");
    // 改宽（横竖屏联动）→ 未中
    assert!(layout_md_peek(9102, 500, X1).is_none(), "改宽必须失效");
    let new_w = layout_md_cached(9102, "正文内容更多", 500, X1);
    assert!(!std::sync::Arc::ptr_eq(&new_gen, &new_w), "改宽必须重排");
    // 改实例格（pinch 双指缩放，BAR-204 换约：原样式维的接任者）→ 未中
    assert!(
        layout_md_peek(9102, 500, (36, 72)).is_none(),
        "改实例格必须失效"
    );
}

// ---- BAR-204 字号档废除 · 迁网格文字引擎（a 案：pinch 实例格一统） ----

#[test]
fn spec_bar204_01_缺省1x版面逐值等旧() {
    // 行为零变化承诺：1× 缺省格（18×36）下无折行短文档的版面几何
    // （行带/块高/y 累进/total_h）逐值等于旧 36px/1.4 版面
    let md = "# 题\n\n正文\n\n```\nx\n```\n\n> 引\n\n- 项\n\n---\n\n## 二\n\n### 三";
    let lay = layout_md(md, 500, X1);
    let body_lh = dp::line_h(dp::BODY_PX); // 54
    let want: [(u32, u32); 8] = [
        (
            dp::HU + dp::line_h(dp::BODY_PX * dp::H1_SCALE) + dp::HU,
            dp::line_h(dp::BODY_PX * dp::H1_SCALE),
        ),
        (body_lh, body_lh),
        (
            dp::HU + dp::line_h(dp::CODE_PX) + dp::HU,
            dp::line_h(dp::CODE_PX),
        ),
        (body_lh, body_lh),
        (body_lh, body_lh),
        (dp::HU * 2, dp::HU * 2),
        (
            dp::line_h(dp::BODY_PX * dp::H2_SCALE),
            dp::line_h(dp::BODY_PX * dp::H2_SCALE),
        ),
        (
            dp::line_h(dp::BODY_PX * dp::H3_SCALE),
            dp::line_h(dp::BODY_PX * dp::H3_SCALE),
        ),
    ];
    let mut y = 0;
    for (i, (b, &(wh, wlh))) in lay.blocks.iter().zip(want.iter()).enumerate() {
        assert_eq!(b.y, y, "块{i} y");
        assert_eq!(b.h, wh, "块{i} h 等旧值");
        assert_eq!(b.line_h, wlh, "块{i} line_h 等旧值");
        y += b.h + dp::BLOCK_GAP;
    }
    assert_eq!(lay.total_h, y - dp::BLOCK_GAP);
}

#[test]
fn spec_bar204_02_正文步进吃实例格宽() {
    // 步进 = char_cells × cell_w：全角 2 格、半角 1 格（与终端同宗）
    assert_eq!(grid_stepped_w("甲", 18.0), 36, "CJK 2 格");
    assert_eq!(grid_stepped_w("ab", 18.0), 36, "ASCII 2×1 格");
    assert_eq!(grid_stepped_w("a甲", 18.0), 54, "混排 3 格");
    // pinch 2×（cell 36×72）：步进翻倍——步进不随 cell_w 的变异在这里红
    let lay = layout_md("甲", 500, (36, 72));
    assert_eq!(lay.blocks[0].lines[0].w, 72, "步进必须随实例格宽");
    // 折行点随格尺：同一内容宽 90，1× 下 2 字/行（72 ≤ 90 < 108），
    // 2× 下 1 字/行（72 ≤ 90 < 144）
    let lay1 = layout_md("甲乙丙丁", 90, X1);
    assert_eq!(lay1.blocks[0].lines.len(), 2, "1× 下 2 字/行");
    let lay2 = layout_md("甲乙丙丁", 90, (36, 72));
    assert_eq!(lay2.blocks[0].lines.len(), 4, "2× 下 1 字/行");
}

#[test]
fn spec_bar204_03_标题阶梯步进与字形同缩() {
    // 标题档步进 = cell_w × scale（步进与字形一起缩放——只缩字形不缩
    // 步进的变异在这里红：w 会退回 36/格档）
    let lay = layout_md("# 甲\n\n## 甲\n\n### 甲", 500, X1);
    let w = |scale: f32| (2.0 * 18.0 * scale).round() as u32;
    assert_eq!(lay.blocks[0].lines[0].w, w(dp::H1_SCALE), "H1 步进同缩");
    assert_eq!(lay.blocks[1].lines[0].w, w(dp::H2_SCALE), "H2 步进同缩");
    assert_eq!(lay.blocks[2].lines[0].w, w(dp::H3_SCALE), "H3 步进同缩");
    assert!(w(dp::H1_SCALE) > 36, "夹具前提：阶梯宽 ≠ 正文宽");
}

#[test]
fn spec_bar204_04_行高咬实例半格网() {
    // 实例 HU = cell_h/2：行高 = ceil(cell_h × scale × 1.4 / hu) × hu。
    // cell_h=50（hu=25）：正文 ceil(70/25)=3 → 75；H1 ceil(119/25)=5 → 125
    // ——不咬格（裸 round 70/119）的变异在这里红
    let lay = layout_md("正文\n\n# 题", 500, (20, 50));
    assert_eq!(lay.blocks[0].line_h, 75, "正文行带咬 25 半格网");
    assert_eq!(lay.blocks[1].line_h, 125, "H1 行带咬 25 半格网");
    assert_eq!(lay.blocks[0].line_h % 25, 0);
    assert_eq!(lay.blocks[1].line_h % 25, 0);
    // 1× 锚：hu=18 = 宪法 HU（逐值等旧钉 spec_bar204_01 已锁版面）
    assert_eq!(line_h_grid(36, 1.0), dp::HU * 3);
}

#[test]
fn spec_bar204_05_代码围栏三十比三十六档() {
    // 代码档 scale = CODE_PX/BODY_PX = 30/36：步进 = 18 × 30/36 = 15px/格
    // （代码档吃掉 = 正文档步进 18 的变异在这里红）
    let lay = layout_md("```\naa\n```", 500, X1);
    let b = &lay.blocks[0];
    assert_eq!(b.scale, dp::CODE_PX / dp::BODY_PX);
    assert_eq!(b.lines[0].w, 30, "2 格 × 15px");
    assert_eq!(b.line_h, dp::line_h(dp::CODE_PX), "行带同 30/36 档");
    // 2× 格下同缩：步进 30px/格
    let lay = layout_md("```\naa\n```", 500, (36, 72));
    assert_eq!(lay.blocks[0].lines[0].w, 60);
}

#[test]
fn spec_bar204_06_排版结果由实例格唯一决定() {
    // 缓存 sig 换 (cell_w, cell_h) 两维的纯逻辑面：同文同格 = 逐值同版面；
    // 任一格维变 = 版面变（pinch 必触发重排版——sig 换维的正确性前提）
    let md = "# 题\n\n正文甲乙丙丁戊己\n\n```\nx\n```";
    let a = layout_md(md, 300, X1);
    let b = layout_md(md, 300, X1);
    assert_eq!(a, b, "纯函数：同入参同版面");
    assert_ne!(a, layout_md(md, 300, (36, 36)), "cell_w 变 = 版面变");
    assert_ne!(a, layout_md(md, 300, (18, 72)), "cell_h 变 = 版面变");
}

// ---- BAR-218 表格三档降级（用户 2026-10-01 拍板细则）----

use kfm_na::ui::md_layout::TableTier;

#[test]
fn spec_bar218_07_fit档自然列宽与列隙() {
    // 窄表放得下：Fit 档，列几何 = 自然宽 + 2 格列隙
    let lay = layout_md("| 名 | 值 |\n|---|---|\n| a | b |", 500, X1);
    let b = &lay.blocks[0];
    assert_eq!(b.kind, BlockKind::Table);
    let t = b.table.as_ref().expect("表格块必带载荷");
    assert_eq!(t.tier, TableTier::Fit);
    // 自然宽：col0 = max("名"=2格, "a"=1格)×18 = 36
    assert_eq!(t.col_w[0], 36, "col0 自然宽 = 2 格 × 18px");
    assert_eq!(t.col_x[0], 0);
    assert_eq!(
        t.col_x[1],
        t.col_w[0] + dp::TABLE_COL_GAP_CELLS * 18,
        "列隙 = 2 字符格"
    );
}

#[test]
fn spec_bar218_08_shrink档注水压缩含下限与格内折行() {
    // 自然宽 [20格, 30格]×18 = [360, 540]，合计 + 隙 > 500 → Shrink；
    // 下限 10 格×2 + 隙 = 396 ≤ 500 不入降级
    let md = "| 名 | 值 |\n|---|---|\n| aaaaaaaaaaaaaaaaaaaa | bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb |";
    let lay = layout_md(md, 500, X1);
    let t = lay.blocks[0].table.as_ref().unwrap();
    assert_eq!(t.tier, TableTier::Shrink);
    let min_px = dp::TABLE_COL_MIN_CELLS * 18;
    assert!(t.col_w[0] >= min_px && t.col_w[1] >= min_px, "列宽不破下限");
    assert!(
        t.col_w[0] + t.col_w[1] + dp::TABLE_COL_GAP_CELLS * 18 <= 500,
        "压缩后合计 + 隙 ≤ 内容宽"
    );
    // 长值格内折行：值格折出多行且每行宽 ≤ 列宽
    let cell = &t.rows[0].cells[1];
    assert!(cell.len() >= 2, "30 格值在压缩列内必折行");
    for line in cell {
        assert!(line.w <= t.col_w[1], "折行行宽 ≤ 列宽");
    }
}

#[test]
fn spec_bar218_09_极端三列入cards且表头溶成字段名() {
    // 3 列长文本：自然宽合计远超 200，且全按下限 3×180+2×36=612 > 200
    // → Cards；表头 h=0（溶了）
    let md = "| h1 | h2 | h3 |\n|---|---|---|\n| tttttttttttttttttttt | vaaaaaaaaaaaaaaaaaaa | vbbbbbbbbbbbbbbbbbbbb |";
    let lay = layout_md(md, 200, X1);
    let t = lay.blocks[0].table.as_ref().unwrap();
    assert_eq!(t.tier, TableTier::Cards);
    assert_eq!(t.header.h, 0, "表头溶解不单独成行（永不成卡）");
    assert_eq!(t.labels, vec!["h1", "h2", "h3"]);
    assert_eq!(t.rows.len(), 1, "一内容行一卡");
    let card = &t.rows[0];
    let title_txt: String = card.cells[0]
        .iter()
        .flat_map(|l| l.spans.iter().map(|(_, s)| s.as_str()))
        .collect();
    assert_eq!(title_txt, "tttttttttttttttttttt", "卡标题 = 首列值折行拼接");
    // 字段行首段 = Bold「字段名：」
    assert_eq!(
        card.cells[1][0].spans[0],
        (SegStyle::Bold, "h2：".to_string())
    );
}

#[test]
fn spec_bar218_10_两列极端入deflist() {
    // 2 列全按下限 = 396 > 200 → DefList；cells[0]=名 cells[1]=值
    let md = "| 属性 | 值 |\n|---|---|\n| 名称xxxxxxxxxx | na客户端yyyyyyyyyy |";
    let lay = layout_md(md, 200, X1);
    let t = lay.blocks[0].table.as_ref().unwrap();
    assert_eq!(t.tier, TableTier::DefList);
    assert_eq!(t.rows.len(), 1);
    assert_eq!(t.rows[0].cells.len(), 2, "条目 = 名 + 值 两格");
    assert_eq!(t.header.h, 0, "题头溶解");
}

#[test]
fn spec_bar218_11_块高与行y自洽累进() {
    // Fit 档块高 = 表头带 + 下划带 + 行高累加 + 行隙；后块 y = 表块 y+h+块隙
    let md = "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\n后文";
    let lay = layout_md(md, 500, X1);
    let b = &lay.blocks[0];
    let t = b.table.as_ref().unwrap();
    let lh = b.line_h;
    let expect = t.header.h + dp::TABLE_ROW_PAD + t.rows[0].h + dp::TABLE_ROW_PAD + t.rows[1].h;
    assert_eq!(b.h, expect, "块高 = 表头+下划带+行带累加+行隙");
    assert_eq!(
        t.rows[1].y,
        t.header.h + dp::TABLE_ROW_PAD + t.rows[0].h + dp::TABLE_ROW_PAD
    );
    assert_eq!(lay.blocks[1].y, b.y + b.h + dp::BLOCK_GAP, "后块咬块隙");
    let _ = lh;
}

#[test]
fn spec_bar218_08b_shrink注水法窄列触下限() {
    // 鉴别夹具：自然宽 [2格, 30格, 30格]，纯比例分配 col0 ≈ 20px < 下限
    // 180px → 注水法必须把它固定在下限（摘下限的变异在这里红）
    let md = "| a | bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb | cccccccccccccccccccccccccccccc |\n|---|---|---|\n| 1 | 2 | 3 |";
    let lay = layout_md(md, 700, X1);
    let t = lay.blocks[0].table.as_ref().unwrap();
    assert_eq!(t.tier, TableTier::Shrink);
    let min_px = dp::TABLE_COL_MIN_CELLS * 18;
    assert_eq!(t.col_w[0], min_px, "窄列必须触下限固定（注水法）");
    assert!(
        t.col_w[0] + t.col_w[1] + t.col_w[2] + dp::TABLE_COL_GAP_CELLS * 18 * 2 <= 700,
        "合计 + 隙 ≤ 内容宽"
    );
}
