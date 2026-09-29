//! filetree_spec.rs — 文件树页内容核考卷（A 档）。
//! 判卷对象：src/ui/filetree.rs（纯逻辑：几何 / 出参解析 / 状态机 / 帧值）。
//!
//! 参数锚点：行高 90/108 物理 px（BAR-178 半格网标定 2.5/3 格，原表 86/118
//! 真机截屏实测留档 docs/active/文件树.md §三）、缩进表 CSS
//! 18/16/14/… 递减增量、dpr 3.06、深度 0..4 缩进 0/55/104/147/184。
//!
//! 变异抽检预期（先改坏、看对应考题变红，cp 备份恢复——禁用 git checkout）：
//! - `indent_px` 去掉 `.min(SHIFT_CLAMP_PX)` → 「深层钳 489」红；
//! - `density` 反号（`shift/18 - 1`，深层更淡）→ 「密度单调」红；
//! - `DrawerAnim::dur_ms` 两档对调（收起 240）→ 「收起 180 收尽」红；
//! - `hit` 不吃 scroll（行矩形按 scroll=0 判）→ 「滚动后命中平移」红；
//! - `toggle` 对文件也走 NeedList 分支 → 「文件 = Preview」红。

use kfm_na::termview::{self, FT_BAR_H};
use kfm_na::ui::accent::AccentPair;
use kfm_na::ui::filetree::*;

// ── 造树小工具 ──────────────────────────────────────────────────────

fn ent(name: &str, kind: RowKind) -> Entry {
    Entry {
        name: name.into(),
        kind,
        size: 0,
        mtime: 0,
    }
}

fn dir(name: &str) -> Entry {
    ent(name, RowKind::Dir)
}

fn file(name: &str) -> Entry {
    ent(name, RowKind::File)
}

/// 根 = [a(dir), b.txt(file)]（两行各 90 → 总高 180）
fn tree() -> FileTreeState {
    let mut s = FileTreeState::new("根");
    s.apply_root_list(vec![dir("a"), file("b.txt")], 0);
    s
}

/// 展开 a 并取回 [sub(dir), x.txt(file)]：行表 = a, a/sub, a/x.txt, b.txt
fn tree_a_open() -> FileTreeState {
    let mut s = tree();
    assert_eq!(s.toggle(0, 0), ToggleAction::NeedList { path: "a".into() });
    s.apply_list("a", vec![dir("sub"), file("x.txt")], 10);
    s
}

// ── 几何：缩进 ──────────────────────────────────────────────────────

#[test]
fn spec_缩进_逐层锚点表() {
    // 实测锚点（真机截屏）保持不变：深度 0..4 的**层间距** = 0 / 55 / 104 /
    // 147 / 184（三角左缘 − 基线 18）。六调⑤⑥ 重排后三角独立成轴（`tri_x`
    // 逐层累进），前段表值换算与旧累加表同值 → 锚点逐字保住
    assert_eq!(
        (0..=4).map(tri_x).collect::<Vec<_>>(),
        vec![18, 73, 122, 165, 202]
    );
    assert_eq!(
        (0..=4).map(|d| tri_x(d) - tri_x(0)).collect::<Vec<_>>(),
        vec![0, 55, 104, 147, 184]
    );
    // 表值本身（CSS px）= 步进来源
    assert_eq!(shift_css(0), 18);
    assert_eq!(shift_css(1), 16);
    assert_eq!(shift_css(4), 10);
    assert_eq!(step_px(0), 0, "0 层无步进");
    assert_eq!(step_px(1), 55);
    assert_eq!(step_px(2), 49);
    assert_eq!(step_px(4), 37);
}

#[test]
fn spec_缩进_深层钳489() {
    // 三角左缘总量封顶 489（实测锚点取整 = 160 CSS × 3.06 截断）
    assert!(tri_x(18) < SHIFT_CLAMP_PX, "{}", tri_x(18));
    assert_eq!(tri_x(19), SHIFT_CLAMP_PX);
    assert_eq!(tri_x(80), SHIFT_CLAMP_PX);
    // 全程非递减（钳制不许出现回缩）——竖条同样
    let (mut prev_t, mut prev_b) = (0i64, 0i64);
    for d in 0..=80 {
        let (t, b) = (tri_x(d), bar_x(d));
        assert!(
            t >= prev_t && b >= prev_b,
            "深度 {d} 回缩: {prev_t}/{prev_b} → {t}/{b}"
        );
        prev_t = t;
        prev_b = b;
    }
    // 钳制档：三角不再右移，竖条随之改由 ⑥ 的缝兜（不再吊父三角）
    assert_eq!(bar_x(80), SHIFT_CLAMP_PX - (ROW_BAR_W + GAP_MIN_PX));
    assert!(bar_x(80) < tri_x(80));
}

#[test]
fn spec_缩进_超表长末档兜() {
    // nz `SHIFT_TABLE[Math.min(d, len-1)] ?? 2` 同款：超表长按末档 2
    assert_eq!(shift_css(19), 2);
    assert_eq!(shift_css(20), 2);
    assert_eq!(shift_css(64), 2);
    // 末档 2 CSS 换算 ≈ 6px 不够让开「竖条 + 缝」，一律吃到步进下限
    assert_eq!(step_px(20), BAR_ALIGN_DX + ROW_BAR_W + GAP_MIN_PX);
}

#[test]
fn spec_六调1_名字与三角留间隔() {
    // ①：名字左缘 = 三角盒右缘 + TRI_NAME_GAP_PX（旧版紧贴 = 差 0）
    assert_eq!(TRI_NAME_GAP_PX, 12);
    for d in 0..=8 {
        assert_eq!(
            name_x(d) - (tri_x(d) + TRI_W),
            TRI_NAME_GAP_PX,
            "深度 {d} 名字与三角盒的间隔不对"
        );
    }
    assert!(name_x(0) > tri_x(0) + TRI_W, "紧贴（无间隔）即打回重演");
}

#[test]
fn spec_六调5_竖条吊在父三角正下方() {
    // ⑤：竖条左缘 = 父行三角盒左缘 + BAR_ALIGN_DX。**非钳制档 = 1..=18**
    // （三角左缘到 19 层才封顶 489，那里改由 ⑥ 的缝钳住，见下一条）
    assert_eq!(BAR_ALIGN_DX, 5);
    assert_eq!(bar_x(0), 0, "根层没有竖条，行内容基准 = 0");
    for d in 1..=18 {
        assert_eq!(
            bar_x(d),
            tri_x(d - 1) + BAR_ALIGN_DX,
            "深度 {d} 竖条没吊父三角"
        );
    }
    // 钳制档例外（报告已写明）：三角封顶后竖条改吃缝钳
    assert!(bar_x(19) < tri_x(18) + BAR_ALIGN_DX);
    assert_eq!(bar_x(19), tri_x(19) - (ROW_BAR_W + GAP_MIN_PX));
    // indent_px 语义改为 bar_x（本册单源；涂装/探针同吃）
    for d in 0..=8 {
        assert_eq!(indent_px(d), bar_x(d));
    }
}

#[test]
fn spec_六调6_深层保缝与步进下限() {
    // ⑥：∀深度 本行三角盒左缘 − 竖条右缘 ≥ GAP_MIN_PX（构造保证）
    assert_eq!(GAP_MIN_PX, 6);
    for d in 0..=40 {
        let gap = tri_x(d) - (bar_x(d) + ROW_BAR_W);
        assert!(gap >= GAP_MIN_PX, "深度 {d} 缝不足: {gap}");
    }
    // 深层步进下限 = 由 ⑥ 反解（STEP_MIN_PX = 5 + 6 + 6 = 17）：表尾递减档
    // （≤5 CSS ≈ 15px）一律兜住，等价于「深层改常量步进」
    let step_min = BAR_ALIGN_DX + ROW_BAR_W + GAP_MIN_PX;
    assert_eq!(step_min, 17);
    for d in 10..=40 {
        assert_eq!(step_px(d), step_min, "深度 {d} 步进该吃下限");
    }
    // 非钳制档（三角还没封顶）：累进步进逐字 = 下限
    for d in 10..=18 {
        assert_eq!(tri_x(d) - tri_x(d - 1), step_min, "深度 {d} 三角步进");
    }
}

// ── 几何：密度 / α ──────────────────────────────────────────────────

#[test]
fn spec_密度_深层更浓单调() {
    assert_eq!(density(0), 0.0);
    // 表值严格递减段（0..=11）：密度严格递增
    for d in 0..11 {
        assert!(
            density(d) < density(d + 1),
            "深度 {d}→{} 没变浓: {} vs {}",
            d + 1,
            density(d),
            density(d + 1)
        );
    }
    // 全档非递减（表里 3/2 有重复档，只许平不许回）
    for d in 0..19 {
        assert!(density(d) <= density(d + 1));
    }
    // 深层：0.89 档（shift 2 / 18）
    assert!((density(19) - 0.888_888_9).abs() < 1e-5);
    assert!(density(19) > 0.88);
    // 超表长仍是末档
    assert_eq!(density(40), density(19));
}

#[test]
fn spec_行底与边框alpha域与单调() {
    // 六调②：根层不画行带（露页底 CARD_PAGE_BG）——α 恒 0
    assert_eq!(band_alpha(0), 0.0);
    assert_eq!(border_op(0), 0.3);
    // 深度 ≥1：域 [0.05, 0.31) 与非递减照旧（公式一行未动）
    let mut prev = band_alpha(1);
    for d in 1..=40 {
        let a = band_alpha(d);
        assert!((0.05..0.31).contains(&a), "深度 {d} α 出域: {a}");
        assert!(a >= prev);
        prev = a;
    }
    assert!(band_alpha(1) > band_alpha(0), "深一层必需比根层浓");
    assert!(band_alpha(19) > 0.28, "{}", band_alpha(19));
    assert!(border_op(19) > 0.7 && border_op(19) < 0.75);
    // 深度 0 不画左强调边（nz：row.depth > 0 才挂 span）
    assert!(!left_bar_on(0));
    assert!(left_bar_on(1));
}

// ── 几何：行高 / 行矩形 ─────────────────────────────────────────────

#[test]
fn spec_行高_单行90换行108() {
    // BAR-178 半格网标定：90 = 2.5 格 / 108 = 3 格（基准格 CELL_H 36）
    assert_eq!(row_h(false), 90);
    assert_eq!(row_h(true), 108);
    assert_eq!(ROW_H, 90);
    assert_eq!(ROW_H_WRAP, 108);
}

#[test]
fn spec_total_h与row_rects一致() {
    let mut s = tree();
    s.set_wrap(1, true, 0); // b.txt 长名换行
    assert_eq!(total_h(&s.rows), 90 + 108);
    let rects = row_rects(&s.rows, 0, 10_000, None);
    assert_eq!(rects, vec![(0, 0, 90), (1, 90, 108)]);
    // 相邻行首尾相接（不重叠不留缝），末行底 = 总高
    for w in rects.windows(2) {
        assert_eq!(w[0].1 + w[0].2, w[1].1);
    }
    let last = rects.last().unwrap();
    assert_eq!(last.1 + last.2, total_h(&s.rows));
}

#[test]
fn spec_row_rects_相交才出() {
    let mut s = tree();
    s.set_wrap(1, true, 0);
    // 视口 90 高、滚 100：行 0 整体在上方（y+h = -10）→ 不出
    let rects = row_rects(&s.rows, 100, 90, None);
    assert_eq!(rects, vec![(1, -10, 108)]);
    // 视口高 0 = 没有可画的行
    assert!(row_rects(&s.rows, 0, 0, None).is_empty());
}

#[test]
fn spec_row_rects_滚动平移() {
    let s = tree();
    let a = row_rects(&s.rows, 0, 10_000, None);
    let b = row_rects(&s.rows, 30, 10_000, None);
    assert_eq!(a[1].1 - b[1].1, 30, "同一行随 scroll 平移同一位移");
    assert_eq!(a[1].2, b[1].2, "行高不随滚动变");
}

/// ③ 兄弟行整体平移进命中尺（眼手同尺）：`SnapShift` 一起吃，屏上画哪就是哪
#[test]
fn spec_row_rects_兄弟行平移() {
    let s = tree();
    let ss = SnapShift {
        from_idx: 1,
        dy: -40,
    };
    let a = row_rects(&s.rows, 0, 10_000, None);
    let b = row_rects(&s.rows, 0, 10_000, Some(&ss));
    assert_eq!(a[0], b[0], "from_idx 之前的行不动");
    assert_eq!((b[1].0, b[1].1), (1, a[1].1 - 40), "行 1 起吃 dy");
}

#[test]
fn spec_兄弟首尾判定() {
    let s = tree_a_open();
    // 行表：a(0) / a-sub(1) / a-x(1) / b.txt(0)
    assert_eq!(sibling_ends(&s.rows, 0), (true, false)); // a：根块首（后面还有 b.txt）
    assert_eq!(sibling_ends(&s.rows, 1), (true, false)); // sub：子块首
    assert_eq!(sibling_ends(&s.rows, 2), (false, true)); // x.txt：子块尾
    assert_eq!(sibling_ends(&s.rows, 3), (false, true)); // b.txt：根块尾
    assert_eq!(sibling_ends(&s.rows, 9), (false, false)); // 越界不 panic
}

#[test]
fn spec_抽屉刚体全高() {
    let s = tree_a_open();
    assert_eq!(drawer_full_h(&s.rows, 0), 180); // a 的子块 = 两行 × 90
    assert_eq!(drawer_full_h(&s.rows, 1), 0); // sub 无子行
    assert_eq!(drawer_full_h(&s.rows, 3), 0); // 文件行
    assert_eq!(drawer_full_h(&s.rows, 99), 0); // 越界
}

#[test]
fn spec_光标上线长钳制() {
    assert_eq!(cursor_line_w(300, 600), 300); // 名字比行窄：跟名字
    assert_eq!(cursor_line_w(1, 600), CURSOR_NAME_MIN); // 太短抬到下限
    assert_eq!(cursor_line_w(9_999, 600), 590); // 太长钳到 行宽 − 10
    assert_eq!(cursor_line_w(9_999, 4), CURSOR_NAME_MIN); // 病态窄行不塌成负
}

#[test]
fn spec_常量_颜色rrggbbaa口径() {
    // 规格原文口径：末位 FF 是 alpha（与 termview 的 AARRGGBB 相反）。
    // **打回改约（症③）**：CHEVRON_RGB 降级为「原版那次召唤的实测色留档」
    // ——现役三角取页 accent 渐变同源采样（涂装侧），常量本身仍按原值钉。
    assert_eq!(rrggbbaa(CURSOR_LINE_RGB), (0, 212, 255, 255));
    assert_eq!(rrggbbaa(CHEVRON_RGB), (0, 148, 178, 255));
    assert_eq!(CURSOR_LINE_ALPHA, 0.7);
    assert_eq!(CURSOR_FILL_ALPHA, 0.15);
    assert_eq!(CURSOR_INSET, 4);
    assert_eq!(ROW_RADIUS, 12);
    assert_eq!((DRAWER_OPEN_MS, DRAWER_CLOSE_MS), (240, 180));
    assert_eq!((TRI_ROT_MS, CURSOR_MOVE_MS), (180, 180));
    assert_eq!(FT_CSS, 3.06);
}

#[test]
fn spec_bar165打回_字号与三角等比放大() {
    // 症①（2026-09-27 二修）：34px 字配 20×22 比例失调 → 字 44；
    // **三修**（同日用户终验）：26×28 仍太小 → 盒 **32×34**、基形 26×32。
    // 比例取证：nz 判据稿 §3.1 = 行高 26 / 字 11 / 三角 9 CSS（字:三角 ≈
    // 1.22）；真机实测字高 32~33 配三角 24 → 字:三角墨高 ≈ 1.37；本实现
    // 44 : 32 = 1.375 ✓ 对齐。尺子落在常量上（同文件冒烟还逐像素验落墨）。
    // 精确值钉已含「不许回退旧尺寸」（任何回退即红），不再叠阈值断言——
    // 常量阈值断言触发 clippy assertions_on_constants（2026-09-27 chain 红实录）
    assert_eq!((TRI_W, TRI_H), (32, 34), "三角盒三修放大（症①）");
    let src = include_str!("../src/termview.rs");
    // **BAR-178 改约（2026-09-29）**：FT_TEXT_PX 常量删除——文件树文字
    // 全走网格文字引擎（布局唯一源），字号自此吃格子变量（grid_fit 读
    // TermView 实例格 = pinch 联动）。症①「字太小/比例失调」的量级诉求
    // 由引擎的格字号承接（基准格下主字体 ≈ 30px 档 + 行高 2.5 格）；回退
    // = 名字重新硬编码 px 或吃 fontdue 自然步进，本钉咬这两条路
    assert!(
        !src.contains("FT_TEXT_PX"),
        "行名字号不许回硬编码 px 常量（BAR-178：字号吃格子变量，FT_TEXT_PX 已删）"
    );
    assert!(
        src.contains("self.measure_items_grid(&row.name)") && src.contains("draw_grid_text_left("),
        "行名落笔必须走网格文字引擎（BAR-178 布局唯一源）"
    );
    assert!(
        src.contains("let (hwt, hht) = (13.0f32, 16.0f32);"),
        "三角基形随盒等比（32×34 内 26×32，墨高 32 = 症①放大档）"
    );
    assert!(
        src.contains("let bx1 = g.x1;"),
        "光标右缘扩到行表窗全宽（症②；旧版到名字实量宽 + 内缩就收）"
    );
    assert!(
        src.contains("paint_open_cursor(") && src.contains("封存件复活（症④"),
        "光标必须吃封存件 paint_open_cursor（症④）——自绘无圆角版已废弃"
    );
    // 三修症③：左竖条纯色（从 accent 双色里选 c1），不再 ring_gradient 采样
    assert!(
        src.contains("竖条改**纯色** accent.c1"),
        "左竖条必须是纯色 accent.c1（症③；回渐变即红）"
    );
    assert!(
        !src.contains("let c = ring_gradient_rgb(accent.c1, accent.c2, 0, off + ly, denom);"),
        "竖条不许回渐变采样（症③）"
    );
}

// ── 出参解析 ────────────────────────────────────────────────────────

#[test]
fn spec_出参解析_正常形状() {
    let body = r#"{"ok":true,"dir":"","entries":[
        {"name":"a","kind":"dir","size":0,"mtime":11},
        {"name":"b.txt","kind":"file","size":12,"mtime":22}]}"#;
    let es = entries_of(body).unwrap();
    assert_eq!(es.len(), 2);
    assert_eq!(
        es[0],
        Entry {
            name: "a".into(),
            kind: RowKind::Dir,
            size: 0,
            mtime: 11
        }
    );
    assert_eq!(es[1].kind, RowKind::File);
    assert_eq!((es[1].size, es[1].mtime), (12, 22));
    assert_eq!(RowKind::Dir.as_str(), "dir");
    // 空层是合法应答（空目录）
    assert!(
        entries_of(r#"{"ok":true,"entries":[]}"#)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn spec_出参解析_容错两处() {
    // ①kfmv4/nz 老形状：`type` 词、无 `ok`
    let old = r#"{"dir":"","entries":[{"name":"x","type":"dir","size":3,"mtime":4}]}"#;
    let es = entries_of(old).unwrap();
    assert_eq!(es[0].kind, RowKind::Dir);
    assert_eq!(es[0].size, 3);
    // ②size/mtime 缺位按 0（显示元数据不挡树）
    let lean = r#"{"ok":true,"entries":[{"name":"y","kind":"file"}]}"#;
    let es = entries_of(lean).unwrap();
    assert_eq!((es[0].size, es[0].mtime), (0, 0));
    // 关联版与自由函数同一份实现
    assert_eq!(FileTreeState::entries_of(lean).unwrap(), es);
}

#[test]
fn spec_出参解析_坏输入全拒() {
    assert!(entries_of("{").is_err()); // 坏 JSON
    assert!(entries_of("").is_err());
    assert!(entries_of("[]").is_err()); // 不是对象
    assert!(entries_of(r#"{"ok":true}"#).is_err()); // 缺 entries
    assert!(entries_of(r#"{"ok":false,"entries":[]}"#).is_err()); // 服务端自报失败
    assert!(entries_of(r#"{"ok":true,"entries":[{"kind":"dir"}]}"#).is_err()); // 缺 name
    assert!(entries_of(r#"{"ok":true,"entries":[{"name":"a"}]}"#).is_err()); // 缺 kind
    assert!(entries_of(r#"{"ok":true,"entries":[{"name":"a","kind":"link"}]}"#).is_err());
    assert!(entries_of(r#"{"ok":true,"entries":["a"]}"#).is_err()); // 条目不是对象
}

#[test]
fn spec_出参解析_直连装树() {
    let body = r#"{"ok":true,"dir":"","entries":[{"name":"src","kind":"dir"},{"name":"README.md","kind":"file"}]}"#;
    let mut s = FileTreeState::new("kfm-na");
    s.apply_root_list(entries_of(body).unwrap(), 0);
    assert_eq!(s.rows.len(), 2);
    assert_eq!(s.rows[0].path, "src");
    assert_eq!(s.rows[0].depth, 0);
    assert!(s.rows[0].kind.is_dir());
    assert_eq!(s.rows[1].name, "README.md");
    assert!(s.rows.iter().all(|r| !r.expanded));
}

// ── 状态机：装树 / 展开 / 收起 ──────────────────────────────────────

#[test]
fn spec_apply_root_list_深度0装行() {
    let s = tree();
    assert_eq!(s.rows.len(), 2);
    assert!(s.rows.iter().all(|r| r.depth == 0));
    assert_eq!(s.rows[0].path, "a", "根的子项路径 = 名字本身");
    assert!(s.expanded.is_empty() && s.loaded.is_empty() && s.loading.is_empty());
    assert_eq!(s.root, "", "根相对路径 = 空串");
    assert_eq!(s.root_label, "根");
    assert_eq!(s.scroll, 0);
}

#[test]
fn spec_toggle_未取目录_要列表() {
    let mut s = tree();
    let a = s.toggle(0, 1_000);
    assert_eq!(a, ToggleAction::NeedList { path: "a".into() });
    // 第一帧就有反馈：三角立刻亮（展开位 = true），请求在途挂 loading
    assert!(s.rows[0].expanded);
    assert!(s.expanded.contains("a"));
    assert!(s.is_loading("a"));
    assert!(!s.loaded.contains("a"), "行没到 = 未取，loaded 不许先记");
    assert_eq!(s.tri_at("a"), 1_000, "三角旋转起点 = 点击时刻");
    // 子层没到 → 没有刚体可长
    assert!(s.anim.is_none());
}

#[test]
fn spec_apply_list_子行紧跟父行_深度加一() {
    let s = tree_a_open();
    assert_eq!(s.rows.len(), 4);
    assert_eq!(
        s.rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
        vec!["a", "a/sub", "a/x.txt", "b.txt"]
    );
    assert_eq!(s.rows[1].depth, 1);
    assert_eq!(s.rows[2].depth, 1);
    assert_eq!(s.rows[3].depth, 0, "下一棵兄弟树回到根层");
    assert!(s.rows[0].expanded);
    assert!(!s.rows[1].expanded);
    assert!(!s.is_loading("a"));
    // 不变量：展开 ⟺ 子行在树；loaded = 已取
    assert_eq!(s.expanded, s.loaded);
    assert_eq!(s.expanded.iter().cloned().collect::<Vec<_>>(), vec!["a"]);
    // 抽屉刚体在子层到位那一刻起步（展开档）
    let anim = s.anim.clone().unwrap();
    assert_eq!(
        (anim.path.as_str(), anim.opening, anim.start_ms),
        ("a", true, 10)
    );
    assert_eq!(drawer_dy(&anim, 10, 180), -180);
    assert_eq!(drawer_dy(&anim, 250, 180), 0);
}

#[test]
fn spec_apply_list_重复回执幂等() {
    let mut s = tree_a_open();
    let before = s.rows.clone();
    s.apply_list("a", vec![dir("sub"), file("x.txt")], 5_000);
    assert_eq!(s.rows, before, "同层重取不许插两份");
    assert_eq!(s.rows.len(), 4);
}

#[test]
fn spec_apply_list_迟到回执丢弃() {
    let mut s = tree();
    s.toggle(0, 0); // 请求 a 的子层
    s.collapse(0, 10); // 用户反悔收起（行即走）
    s.apply_list("a", vec![file("x.txt")], 20); // 迟到的应答
    assert_eq!(s.rows.len(), 2, "父行已收起的应答不许把子行插回来");
    assert!(!s.is_loading("a"));
    // 无主回执（父行从没存在过）同样丢弃
    let mut s2 = tree();
    s2.apply_list("ghost", vec![file("z")], 0);
    assert_eq!(s2.rows.len(), 2);
}

#[test]
fn spec_apply_list_保留已展开的孙层() {
    let mut s = tree_a_open();
    s.toggle(1, 100); // 展开 a/sub
    s.apply_list("a/sub", vec![file("deep.txt")], 110);
    assert_eq!(s.rows.len(), 5);
    // 重取 a 的层：a/sub 的子树按路径保留（行在账在）
    s.apply_list("a", vec![dir("sub"), file("x.txt")], 200);
    assert_eq!(s.rows.len(), 5);
    assert_eq!(s.rows[2].path, "a/sub/deep.txt");
    assert!(s.rows[1].expanded);
    assert!(s.loaded.contains("a/sub"));
}

#[test]
fn spec_collapse_连子孙一起移除() {
    let mut s = tree_a_open();
    s.toggle(1, 100);
    s.apply_list("a/sub", vec![file("deep.txt")], 110);
    assert_eq!(s.rows.len(), 5);
    s.collapse(0, 200);
    assert_eq!(
        s.rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
        vec!["a", "b.txt"],
        "收起 = 父行留、子孙全走"
    );
    assert!(!s.rows[0].expanded);
    assert!(s.expanded.is_empty(), "子孙账一并清（重展开要重取）");
    assert!(s.loaded.is_empty());
    assert!(!s.is_loading("a"));
    let anim = s.anim.clone().unwrap();
    assert_eq!(
        (anim.path.as_str(), anim.opening, anim.start_ms),
        ("a", false, 200)
    );
}

#[test]
fn spec_collapse_幂等与非目录() {
    let mut s = tree();
    s.collapse(1, 0); // 文件行
    assert_eq!(s.rows.len(), 2);
    s.collapse(9, 0); // 越界
    assert_eq!(s.rows.len(), 2);
    s.collapse(0, 0); // 未展开的目录
    assert_eq!(s.rows.len(), 2);
    assert!(s.anim.is_none(), "空操作不许记动画账");
}

#[test]
fn spec_toggle_文件_预览() {
    let mut s = tree();
    assert_eq!(
        s.toggle(1, 1_000),
        ToggleAction::Preview {
            path: "b.txt".into(),
            name: "b.txt".into()
        }
    );
    assert_eq!(s.rows.len(), 2, "文件不展开、不请求");
    assert!(!s.is_loading("b.txt"));
    assert!(s.anim.is_none());
}

#[test]
fn spec_toggle_动画窗口内二次_锁住() {
    let mut s = tree();
    assert_eq!(
        s.toggle(0, 1_000),
        ToggleAction::NeedList { path: "a".into() }
    );
    // 展开锁窗 240ms 未满
    assert_eq!(s.toggle(0, 1_100), ToggleAction::Locked);
    assert_eq!(s.toggle(0, 1_239), ToggleAction::Locked);
    // 子层到位把锁续到「到位 + 240」（抽屉在长，不许被点击打断）
    s.apply_list("a", vec![file("x.txt")], 1_250);
    assert_eq!(s.toggle(0, 1_300), ToggleAction::Locked);
    // 锁窗过后：已展开 → 收起
    assert_eq!(s.toggle(0, 1_500), ToggleAction::Collapsed(0));
    // 收起锁窗 180ms 内再点 = Locked，过后重开 = 再要列表
    assert_eq!(s.toggle(0, 1_600), ToggleAction::Locked);
    assert_eq!(
        s.toggle(0, 1_700),
        ToggleAction::NeedList { path: "a".into() }
    );
}

#[test]
fn spec_toggle_越界_锁住() {
    let mut s = tree();
    assert_eq!(s.toggle(99, 0), ToggleAction::Locked);
    assert_eq!(s.epoch, 1, "空操作不许改账面（首装 = 1 代）");
}

#[test]
fn spec_list_failed_回退展开账() {
    let mut s = tree();
    s.toggle(0, 1_000);
    s.list_failed("a", 1_100);
    assert!(!s.rows[0].expanded);
    assert!(s.expanded.is_empty());
    assert!(s.loaded.is_empty());
    assert!(!s.is_loading("a"), "失败要摘 loading（否则转圈永不灭）");
    // 三角回转窗口内算活性，过后不产帧
    assert!(anim_active(&s, 1_150));
    assert!(!anim_active(&s, 1_400));
    // 回退后立刻可重试（窗已过）
    assert_eq!(
        s.toggle(0, 1_500),
        ToggleAction::NeedList { path: "a".into() }
    );
}

/// 硬指标（六调④）：内部展开 N 层 → 关根 → 再开根 → N 层原样恢复
#[test]
fn spec_六调4_曾展开账级联恢复() {
    let mut s = FileTreeState::new("根");
    s.apply_root_list(vec![dir("a"), file("b.txt")], 0);
    // 三级嵌套逐层展开（每一级都是用户亲手点的）
    assert_eq!(s.toggle(0, 10), ToggleAction::NeedList { path: "a".into() });
    assert_eq!(
        s.apply_list("a", vec![dir("sub"), file("x.txt")], 20),
        Vec::<String>::new(),
        "子层里没有曾展开过的目录 → 无级联"
    );
    assert_eq!(
        s.toggle(1, 30),
        ToggleAction::NeedList {
            path: "a/sub".into()
        }
    );
    assert_eq!(
        s.apply_list("a/sub", vec![dir("deep"), file("y.txt")], 40),
        Vec::<String>::new()
    );
    assert_eq!(
        s.toggle(2, 50),
        ToggleAction::NeedList {
            path: "a/sub/deep".into()
        }
    );
    assert_eq!(
        s.apply_list("a/sub/deep", vec![file("z.txt")], 60),
        Vec::<String>::new()
    );
    let before = s.rows.clone();
    assert_eq!(before.len(), 7);
    let depths: Vec<usize> = before.iter().map(|r| r.depth).collect();
    assert_eq!(depths, vec![0, 1, 2, 3, 2, 1, 0]);
    let expanded_before: Vec<String> = s.expanded.iter().cloned().collect();
    assert_eq!(expanded_before, vec!["a", "a/sub", "a/sub/deep"]);
    // 关根（根级收起 = 整支走）：行/账照清；**曾展开账语义改约（2026-09-27
    // 研究线裁定）** = 显式收起**摘自己那一条**、**子孙账保留**（所以这里
    // 只剩 a/sub 与 a/sub/deep；根 a 的那条被这次显式收起摘掉）
    s.collapse(0, 100);
    assert_eq!(s.rows.len(), 2);
    assert!(s.expanded.is_empty() && s.loaded.is_empty() && s.loading.is_empty());
    assert_eq!(
        s.expanded_mem.iter().cloned().collect::<Vec<_>>(),
        vec!["a/sub", "a/sub/deep"],
        "显式收起摘自己那条；级联摘行不动子孙账"
    );
    // 再开根：**a 自己是被显式收起过的 → mem 无它 → 不被级联拉起**（新语义，
    // 2026-09-27 研究线裁定：显式收起过的保持收起）；用户再点 a 才长回来
    let cascade = s.apply_root_list(vec![dir("a"), file("b.txt")], 1_000);
    assert!(
        cascade.is_empty(),
        "显式收起过的目录不许被关根再开级联复活：{cascade:?}"
    );
    assert!(!s.rows[0].expanded, "a 保持收起态");
    // 用户再点 a（显式展开）→ 它的**子孙账仍在** → 内部历史立刻级联回取
    assert_eq!(
        s.toggle(0, 1_100),
        ToggleAction::NeedList { path: "a".into() }
    );
    assert!(s.rows[0].expanded, "▼ 立刻显（不等数据）");
    assert!(s.is_loading("a"));
    assert!(!s.loaded.contains("a"), "行没到 = 未取");
    // 逐层 quiet 落位（dfs 三层：到没有命中为止）
    assert_eq!(
        s.apply_list_quiet("a", vec![dir("sub"), file("x.txt")], 1_010),
        vec!["a/sub"]
    );
    assert!(s.anim.is_none(), "级联恢复不许打抽屉动画");
    assert_eq!(
        s.apply_list_quiet("a/sub", vec![dir("deep"), file("y.txt")], 1_020),
        vec!["a/sub/deep"]
    );
    assert_eq!(
        s.apply_list_quiet("a/sub/deep", vec![file("z.txt")], 1_030),
        Vec::<String>::new()
    );
    // 原样恢复：行表逐字相同（路径/深度/展开位/顺序）
    assert_eq!(s.rows, before, "N 层原样恢复");
    assert_eq!(
        s.expanded.iter().cloned().collect::<Vec<_>>(),
        expanded_before
    );
    assert!(s.loading.is_empty());
    assert!(s.anim.is_none() && s.retract.is_none());
}

/// ④ 上限策略：MEM_CAP 封顶，超帽丢**字典序最小者**（确定性，不引随机源）
#[test]
fn spec_六调4_曾展开账上限丢字典序最小() {
    let mut s = FileTreeState::new("根");
    let entries: Vec<Entry> = (0..=MEM_CAP).map(|i| dir(&format!("d{i:04}"))).collect();
    s.apply_root_list(entries, 0);
    assert_eq!(s.rows.len(), MEM_CAP + 1);
    for i in 0..=MEM_CAP {
        let idx = s
            .rows
            .iter()
            .position(|r| r.path == format!("d{i:04}"))
            .unwrap();
        assert!(matches!(
            s.toggle(idx, i as u64),
            ToggleAction::NeedList { .. }
        ));
    }
    assert_eq!(s.expanded_mem.len(), MEM_CAP, "超帽即丢到帽内");
    assert!(!s.expanded_mem.contains("d0000"), "丢的是字典序最小者");
    assert!(s.expanded_mem.contains(&format!("d{MEM_CAP:04}")));
}

/// ④ 曾展开账三档语义（2026-09-27 研究线裁定）：**显式展开入账 / 显式收起
/// 摘自己那条 / 祖先收起造成的级联摘行不动 mem**。效果 = 关根再开恢复用户
/// 离开时的样子（显式收起过的保持收起，不被级联复活）
#[test]
fn spec_六调4_显式收起摘自己_子孙账保留_级联不动mem() {
    let mut s = FileTreeState::new("根");
    s.apply_root_list(vec![dir("a")], 0);
    // a → a/sub → a/sub/deep 三层逐级显式展开
    assert_eq!(s.toggle(0, 10), ToggleAction::NeedList { path: "a".into() });
    s.apply_list("a", vec![dir("sub")], 20);
    assert_eq!(
        s.toggle(1, 30),
        ToggleAction::NeedList {
            path: "a/sub".into()
        }
    );
    s.apply_list("a/sub", vec![dir("deep")], 40);
    assert_eq!(
        s.toggle(2, 50),
        ToggleAction::NeedList {
            path: "a/sub/deep".into()
        }
    );
    s.apply_list("a/sub/deep", vec![file("z.txt")], 60);
    assert_eq!(s.expanded_mem.len(), 3, "三层都在账里");
    // ① 祖先收起（a）= 级联摘行：子孙账**一律不动**（行没了账留着）
    s.collapse(0, 70);
    assert!(
        !s.expanded_mem.contains("a"),
        "祖先自己是被显式收起的 → 摘自己那条"
    );
    assert!(
        s.expanded_mem.contains("a/sub") && s.expanded_mem.contains("a/sub/deep"),
        "级联摘行不许动子孙账：{:#?}",
        s.expanded_mem
    );
    // ② 关根再开：a 不复活（用户显式收起过），但重开 a 之后其内部历史长回来
    let _ = s.apply_root_list(vec![dir("a")], 80);
    assert!(
        !s.rows.iter().any(|r| r.path == "a/sub"),
        "a 不复活（未被级联拉起）"
    );
    assert_eq!(
        s.toggle(0, 300),
        ToggleAction::NeedList { path: "a".into() }
    );
    let cascade = s.apply_list("a", vec![dir("sub")], 100);
    assert_eq!(
        cascade,
        vec!["a/sub".to_string()],
        "重开 a → 内部曾展开的 sub 立刻级联回取（子孙账保留的兑现）"
    );
}

/// ④ 级联只扫**本次 graft 的那一层**：别支（同深度、不在这块里）的曾展开账
/// 不许被一次无关的取层拉起来——扫全树会把「用户刚收起的别支目录」也重新
/// 长出来（本册实现口径：range 夹在 graft 出的新块上）
#[test]
fn spec_六调4_级联只扫本层() {
    let mut s = FileTreeState::new("根");
    s.apply_root_list(vec![dir("a"), dir("b")], 0);
    // b 下展开 sub 再收起：曾展开账留着 b/sub，行没了
    assert_eq!(s.toggle(1, 10), ToggleAction::NeedList { path: "b".into() });
    assert_eq!(
        s.apply_list("b", vec![dir("sub")], 20),
        Vec::<String>::new()
    );
    assert_eq!(
        s.toggle(2, 30),
        ToggleAction::NeedList {
            path: "b/sub".into()
        }
    );
    assert_eq!(
        s.apply_list("b/sub", vec![file("z.txt")], 40),
        Vec::<String>::new()
    );
    s.collapse(2, 50);
    // **语义改约（2026-09-27 研究线裁定）**：用户**显式收起** b/sub → 摘它
    // **自己那一条**（免得关根再开把它级联复活 = 撤销用户的收起）
    assert!(
        !s.expanded_mem.contains("b/sub"),
        "显式收起必摘自己那一条（旧「只进不出」语义已废）"
    );
    // 展 a 后重取 a 的层：b/sub 与 a 同深度但不在本块里，不许被拉起
    assert_eq!(
        s.toggle(0, 100),
        ToggleAction::NeedList { path: "a".into() }
    );
    let cascade = s.apply_list("a", vec![file("x.txt")], 110);
    assert_eq!(
        cascade,
        Vec::<String>::new(),
        "别支的曾展开账不许被本层取法拉起"
    );
    let sub = s.rows.iter().find(|r| r.path == "b/sub").unwrap();
    assert!(!sub.expanded, "别支收起态必须保持（级联只扫本层）");
    assert!(!s.is_loading("b/sub"));
}

#[test]
fn spec_apply_root_list_保留已展开子树() {
    let mut s = tree_a_open();
    // 同表重取根层：树不拍平
    s.apply_root_list(vec![dir("a"), file("b.txt")], 1_000);
    assert_eq!(
        s.rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
        vec!["a", "a/sub", "a/x.txt", "b.txt"]
    );
    assert!(s.rows[0].expanded);
    assert_eq!(s.expanded, s.loaded);
    // 目录消失：连子孙一起走，账同清
    s.apply_root_list(vec![file("b.txt")], 2_000);
    assert_eq!(s.rows.len(), 1);
    assert_eq!(s.rows[0].path, "b.txt");
    assert!(s.expanded.is_empty() && s.loaded.is_empty());
    assert!(s.anim.is_none());
}

#[test]
fn spec_行表增删_选中不悬空() {
    let mut s = tree();
    s.select(1, 0); // 选中 b.txt
    s.toggle(0, 0);
    s.apply_list("a", vec![file("x.txt")], 10); // 前插一行 → 选中行后移
    assert_eq!(s.sel, Some(2));
    assert_eq!(s.rows[2].path, "b.txt");
    // 选中落在被收走的子行里 = 归父行
    s.select(1, 20); // a/x.txt
    s.collapse(0, 300);
    assert_eq!(s.sel, Some(0));
    assert_eq!(s.rows[0].path, "a");
    // 选中的行被整树刷掉 = 清账（下标不许悬空）
    s.select(0, 400);
    s.apply_root_list(vec![file("other")], 500);
    assert_eq!(s.sel, None);
    assert_eq!(cursor_y(&s, 500), 0);
}

// ── 光标 ────────────────────────────────────────────────────────────

#[test]
fn spec_select_光标目标行心_180ms单调逼近() {
    let mut s = tree();
    assert_eq!(cursor_y(&s, 0), 0, "没选中 = 0");
    s.select(0, 1_000);
    let t0 = s.cursor_target(0);
    assert_eq!(t0, ROW_H / 2, "目标 = 行心");
    assert_eq!(cursor_y(&s, 1_180), t0, "180ms 到点");
    assert_eq!(cursor_y(&s, 9_999), t0, "过点贴死");
    // 换行：从当前所在滑向新行心
    s.select(1, 2_000);
    let t1 = s.cursor_target(1);
    assert_eq!(t1, ROW_H + ROW_H / 2);
    let mut prev = cursor_y(&s, 2_000);
    assert_eq!(prev, t0, "起点 = 上一选中行心（连点不跳变）");
    for ms in (2_010..2_181).step_by(10) {
        let y = cursor_y(&s, ms);
        assert!(y >= prev, "单调逼近: {prev} → {y} @{ms}");
        assert!(y <= t1);
        prev = y;
    }
    assert_eq!(cursor_y(&s, 2_180), t1);
    // 半程已走大半（ease-out：起步即快，不是线性也不是 ease-in）
    assert!(cursor_y(&s, 2_090) - t0 > (t1 - t0) / 2);
}

#[test]
fn spec_select_首选中从内容原点起飞() {
    let mut s = tree();
    s.select(1, 0);
    assert_eq!(s.cursor_from, Some(0), "首个选中：起点 = 内容原点");
    assert_eq!(cursor_y(&s, 0), 0);
    assert!(cursor_y(&s, 90) > 0);
    assert!(cursor_y(&s, 90) < s.cursor_target(1));
    assert_eq!(cursor_y(&s, 180), s.cursor_target(1));
}

#[test]
fn spec_set_wrap_光标贴行不滑() {
    let mut s = tree();
    s.select(1, 0);
    let target0 = s.cursor_target(1);
    s.set_wrap(0, true, 50); // 量宽回填：a 换行 → 行高 90 → 108
    assert!(s.rows[0].wrap);
    assert_eq!(s.cursor_target(1), target0 + (ROW_H_WRAP - ROW_H));
    assert_eq!(
        cursor_y(&s, 50),
        s.cursor_target(1),
        "框属于行：行移框移（不滑）"
    );
    // 幂等：同值回填不动账
    let epoch = s.epoch;
    s.set_wrap(0, true, 60);
    assert_eq!(s.epoch, epoch);
    assert_eq!(total_h(&s.rows), ROW_H_WRAP + ROW_H);
}

// ── 滚动 ────────────────────────────────────────────────────────────

#[test]
fn spec_滚动_钳制上下界() {
    let mut s = tree(); // 总高 180
    s.scroll_by(50, 100); // 上界 = 180 − 100 = 80，50 在界内
    assert_eq!(s.scroll, 50);
    s.scroll_by(100, 100);
    assert_eq!(s.scroll, 80, "超界不越");
    s.scroll_by(-1_000, 100);
    assert_eq!(s.scroll, 0, "上滑不许变负");
    s.scroll_by(30, 180); // 一屏装得下 → 上界 0
    assert_eq!(s.scroll, 0);
    s.scroll_by(0, 0); // 病态视口高（未布局）：上界 = 总高，仍钳
    assert_eq!(s.scroll, 0);
}

#[test]
fn spec_滚动_内容缩水钳回() {
    let mut s = tree_a_open(); // 总高 360（四行）
    s.scroll_by(1_000, 100);
    assert_eq!(s.scroll, 260);
    s.collapse(0, 100); // 内容缩回 180
    s.clamp_scroll(100);
    assert_eq!(s.scroll, 80, "内容缩水后视口不许悬在空白上");
}

#[test]
fn spec_滚动_只动账面时才换代() {
    let mut s = tree();
    let e0 = s.epoch;
    s.scroll_by(10, 100);
    assert!(s.epoch > e0, "滚动换代（涂装 sig 要吃）");
    let e1 = s.epoch;
    s.scroll_by(0, 100);
    assert_eq!(s.epoch, e1, "没动就不换代（省重烘）");
}

// ── 命中 ────────────────────────────────────────────────────────────

#[test]
fn spec_命中_目录整行开合_死区消灭() {
    // **打回改约（症⑤，2026-09-27 用户终裁）**：目录行**整行**点按 = 开合，
    // 三角只是状态指示；旧实现把行左三角带判 Toggle、名字区判 Row → 真机
    // 20+ 次点按零 toggle（用户点目录行没反应）。行内任意 x（含三角盒与
    // 名字区之间的死区）同义 = Toggle
    let s = tree();
    for x in [0, 1, TRI_W - 1, TRI_W, TRI_W + 1, 120, 400, 1_200] {
        assert_eq!(
            s.hit(x, 10, 180),
            Some(Hit::Toggle(0)),
            "目录行 x={x} 都该开合（死区不许存在）"
        );
    }
    // 行底边界：y = 行高 落在下一行（文件行 → 选中）
    assert_eq!(s.hit(400, ROW_H - 1, 180), Some(Hit::Toggle(0)));
    assert_eq!(s.hit(400, ROW_H, 180), Some(Hit::Row(1)));
}

#[test]
fn spec_命中_文件行整行选中() {
    let s = tree();
    for x in [0, TRI_W, 400, 1_200] {
        assert_eq!(
            s.hit(x, ROW_H + 10, 180),
            Some(Hit::Row(1)),
            "文件行没有三角，行内任意 x = 选中/预览"
        );
    }
}

#[test]
fn spec_命中_深行目录整行开合() {
    let s = tree_a_open(); // a/sub 在行 1，深度 1
    let y = ROW_H + 10;
    // 缩进留白 / 三角盒位 / 名字区——三种位置同义（旧口径在此分三档）
    for x in [0, 30, 60, 74, 75, 200, 900] {
        assert_eq!(
            s.hit(x, y, 360),
            Some(Hit::Toggle(1)),
            "深缩进目录行 x={x} 都该开合"
        );
    }
    // 文件行（a/x.txt，深度 1）全行选中
    let y2 = ROW_H * 2 + 10;
    for x in [0, 60, 200] {
        assert_eq!(s.hit(x, y2, 360), Some(Hit::Row(2)));
    }
}

#[test]
fn spec_命中_滚动后平移() {
    let mut s = tree();
    assert_eq!(s.hit(120, 90, 180), Some(Hit::Row(1)));
    s.scroll_by(80, 100);
    assert_eq!(s.scroll, 80);
    // 同一行：屏上位置 = 内容位置 − scroll
    assert_eq!(s.hit(120, 10, 100), Some(Hit::Row(1)));
    assert_eq!(s.hit(120, 9, 100), Some(Hit::Toggle(0)), "上边界随滚动平移");
    assert_eq!(s.hit(120, 100, 100), None, "视口外不点");
    assert_eq!(s.hit(120, -1, 100), None);
}

#[test]
fn spec_命中_行外与负x() {
    let s = tree();
    assert_eq!(s.hit(120, 500, 180), None, "行表之下");
    assert_eq!(s.hit(-1, 10, 180), None, "页左之外");
    assert_eq!(s.hit(120, 10, 0), None, "视口高 0 无可点");
    let empty = FileTreeState::new("空");
    assert_eq!(empty.hit(10, 10, 180), None, "空树");
}

// ── 抽屉 / 三角 / 活性 ──────────────────────────────────────────────

#[test]
fn spec_抽屉_展开240端点与单调() {
    let a = DrawerAnim {
        path: "a".into(),
        opening: true,
        start_ms: 1_000,
    };
    assert_eq!(a.dur_ms(), DRAWER_OPEN_MS);
    assert_eq!(drawer_dy(&a, 1_000, 118), -118, "起点：整块压在顶缘之上");
    assert_eq!(drawer_dy(&a, 1_240, 118), 0, "240ms 到位");
    assert_eq!(drawer_dy(&a, 9_999, 118), 0);
    assert_ne!(
        drawer_dy(&a, 1_200, 118),
        0,
        "200ms 还没到（展开是 240 档）"
    );
    let mut prev = drawer_dy(&a, 1_000, 118);
    for ms in (1_010..1_240).step_by(10) {
        let d = drawer_dy(&a, ms, 118);
        assert!(d >= prev && d <= 0, "刚体只许往下浮: {prev} → {d} @{ms}");
        prev = d;
    }
    // 半程已走大半（ease-out）
    assert!(drawer_dy(&a, 1_120, 118) > -118 / 2);
}

#[test]
fn spec_抽屉_收起180不许与展开串台() {
    let c = DrawerAnim {
        path: "a".into(),
        opening: false,
        start_ms: 1_000,
    };
    assert_eq!(c.dur_ms(), DRAWER_CLOSE_MS);
    assert_eq!(drawer_dy(&c, 1_000, 118), 0);
    assert_ne!(drawer_dy(&c, 1_150, 118), -118, "150ms 未收尽");
    assert_eq!(drawer_dy(&c, 1_180, 118), -118, "180ms 收尽");
    assert_eq!(drawer_dy(&c, 1_200, 118), -118);
    let mut prev = drawer_dy(&c, 1_000, 118);
    for ms in (1_010..1_180).step_by(10) {
        let d = drawer_dy(&c, ms, 118);
        assert!(
            d <= prev && d >= -118,
            "收起只许往上没入: {prev} → {d} @{ms}"
        );
        prev = d;
    }
}

#[test]
fn spec_抽屉_无子行dy恒零() {
    let a = DrawerAnim {
        path: "a".into(),
        opening: true,
        start_ms: 0,
    };
    assert_eq!(drawer_dy(&a, 100, 0), 0);
    assert_eq!(drawer_dy(&a, 100, -5), 0);
}

/// ③ **帧账**（六调核心）：展开期兄弟行 y = 内容位 + dy，dy 单调
/// （−full_h → 0）且相邻帧差不超单帧最大位移（无跳变、无瞬移）
#[test]
fn spec_六调3_展开期兄弟行整体平移帧账() {
    let s = tree_a_open(); // 行表 = a / a-sub / a-x / b.txt，抽屉 10ms 起步 240ms
    let full = drawer_full_h(&s.rows, 0);
    assert_eq!(full, 180);
    // 兄弟行 = 子块之后的第一个（b.txt，idx 3）；子块里的行不吃这笔
    let mut prev_dy: Option<i64> = None;
    let mut prev_y: Option<i64> = None;
    let mut seen_zero = false;
    for ms in (10..=250).step_by(10) {
        let snap = s.snap_at(ms);
        let ss = snap.sib_shift.expect("展开期必须记兄弟行平移");
        assert_eq!(ss.from_idx, 3, "起点 = 子块之后的第一个兄弟行");
        let dy = ss.dy;
        assert!(dy <= 0 && dy >= -full, "dy 出域: {dy}");
        if let Some(p) = prev_dy {
            if dy == 0 {
                seen_zero = true;
            }
            assert!(dy >= p, "dy 只许往上浮（单调）: {p} → {dy} @{ms}");
            // 单帧最大位移：240ms 走 3×full（ease-out 起步斜率 3）→ 10ms ≈ 21.5，
            // 取 full/4 = 43 为界（跳变/瞬移必超）
            assert!(dy - p <= full / 4, "相邻帧跳变: {p} → {dy} @{ms}");
        }
        // 兄弟行屏上位置 = 内容位 + dy（与涂装同一把尺）
        let content_top = row_top(&snap.rows, 3);
        let rect = row_rects(&snap.rows, snap.scroll, 10_000, Some(&ss))
            .into_iter()
            .find(|r| r.0 == 3)
            .expect("兄弟行必在可见表");
        assert_eq!(rect.1, content_top + dy);
        if let Some(py) = prev_y {
            assert!(
                (rect.1 - py).abs() <= full / 4,
                "兄弟行跳变: {py} → {}",
                rect.1
            );
        }
        prev_y = Some(rect.1);
        prev_dy = Some(dy);
    }
    assert!(seen_zero, "240ms 内必须走到 0（落位）");
    // 收尽帧：兄弟行回到内容位（dy = 0；无钟投影 = 无位移 → None）
    assert_eq!(s.snap_at(250).sib_shift.unwrap().dy, 0);
    assert_eq!(s.snap_at(10_000).sib_shift.unwrap().dy, 0);
    assert!(s.snap().sib_shift.is_none(), "静态投影 = 收尽无位移");
}

/// ③ **帧账**（收起，路线 b）：行即刻走，但兄弟行从「被摘走行的原位」连续上滑
#[test]
fn spec_六调3_收起期兄弟行连续上滑() {
    let mut s = tree_a_open();
    let removed_h = drawer_full_h(&s.rows, 0);
    assert_eq!(removed_h, 180);
    s.collapse(0, 300);
    assert_eq!(s.rows.len(), 2, "行即刻走（不变量不动）");
    let mut prev = i64::MAX;
    let mut steps = 0;
    for ms in (300..300 + DRAWER_CLOSE_MS).step_by(10) {
        let snap = s.snap_at(ms);
        let ss = snap.sib_shift.expect("退场期必须记兄弟行平移");
        assert_eq!(ss.from_idx, 1, "起点 = 父行之后（被摘走行的原位）");
        assert!(ss.dy >= 0 && ss.dy <= removed_h, "dy 出域: {}", ss.dy);
        assert!(
            ss.dy <= prev,
            "只许连续上滑（单调递减）: {prev} → {} @{ms}",
            ss.dy
        );
        if prev != i64::MAX {
            assert!(prev - ss.dy <= removed_h / 4, "相邻帧跳变（瞬移）@{ms}");
        }
        prev = ss.dy;
        steps += 1;
    }
    assert!(
        s.snap_at(300).sib_shift.unwrap().dy == removed_h,
        "起点 = 整量（下半树还在被摘走行的原位）"
    );
    assert!(steps > 10, "180ms 该有十几帧");
    assert!(prev < removed_h / 4, "收尾已接近落位: {prev}");
    // 收尽即无账（兄弟行贴死在缩小后的行表上）
    assert!(s.snap_at(300 + DRAWER_CLOSE_MS).sib_shift.is_none());
    assert!(s.snap().sib_shift.is_none());
    assert_eq!(retract_dy(&s.retract.clone().unwrap(), 9_999), 0);
}

/// ③ 退场账的活性（漏这笔 = 兄弟行卡在半路不落位）
/// ③ 退场账的活性（收起后 180ms 内必须继续产帧，兄弟行才滑得到位）。
/// **诚实注**：工单要求 `anim_active` 把 retract 算进去，现役里收起那一拍同时
/// 记了 closing 抽屉账（同档 180ms），两笔账窗口重合——所以这条钉咬的是
/// 「退场期产帧」这个行为，分不出是哪笔账供的帧；retract 那一路是防御性的
/// （任一路径若只留 retract 不留 anim，它就得顶上）
#[test]
fn spec_六调3_退场账算活性() {
    let mut s = tree_a_open();
    s.collapse(0, 1_000);
    assert!(anim_active(&s, 1_000));
    assert!(anim_active(&s, 1_179));
    assert!(!anim_active(&s, 1_180), "180ms 落位即哑（退场账同档）");
    // 抽屉账在飞时照旧算活性（两笔账互不遮蔽）
    let s2 = tree_a_open();
    assert!(anim_active(&s2, 10 + DRAWER_OPEN_MS - 1));
    assert!(!anim_active(&s2, 10 + DRAWER_OPEN_MS));
}

#[test]
fn spec_三角_端点与半程() {
    let half = std::f32::consts::FRAC_PI_2;
    assert_eq!(tri_angle(true, 0, 0), 0.0, "展开起点：▶");
    assert!((tri_angle(true, 0, 180) - half).abs() < 1e-6, "展开到位：▼");
    assert!((tri_angle(false, 0, 0) - half).abs() < 1e-6, "收起起点：▼");
    assert_eq!(tri_angle(false, 0, 180), 0.0, "收起到位：▶");
    let mid = tri_angle(true, 0, 90);
    assert!(
        mid > half / 2.0 && mid < half,
        "半程过了大半（ease-out）: {mid}"
    );
    assert!(tri_angle(true, 0, 30) < mid);
    // 无账（tri_at = 0）按收尽：外问角度直接得终值
    assert!((tri_angle(true, 0, 100_000) - half).abs() < 1e-6);
    assert_eq!(tri_angle(false, 0, 100_000), 0.0);
}

#[test]
fn spec_活性_探针三类() {
    let mut s = tree();
    assert!(!anim_active(&s, 0), "无动画不产帧");
    // ①光标在路上
    s.select(0, 1_000);
    assert!(anim_active(&s, 1_000));
    assert!(anim_active(&s, 1_100));
    assert!(!anim_active(&s, 1_180), "180ms 到点即哑");
    // ②三角在转（要列表期间没有抽屉刚体，只有三角）
    assert_eq!(
        s.toggle(0, 5_000),
        ToggleAction::NeedList { path: "a".into() }
    );
    assert!(anim_active(&s, 5_000));
    assert!(anim_active(&s, 5_170));
    assert!(!anim_active(&s, 5_181));
    // ③抽屉在飞
    s.apply_list("a", vec![file("x.txt")], 6_000);
    assert!(anim_active(&s, 6_000));
    assert!(anim_active(&s, 6_239));
    assert!(!anim_active(&s, 6_240));
    // ④收起：三角回转窗（行已走，刚体归零）
    s.collapse(0, 7_000);
    assert!(anim_active(&s, 7_000));
    assert!(anim_active(&s, 7_179));
    assert!(!anim_active(&s, 7_400));
}

#[test]
fn spec_快照_帧值与静态投影() {
    let mut s = tree_a_open(); // apply_list 记在 ms=10（抽屉刚体起步）
    // 起步那一帧：刚体全高 = a 的子块（两行 × 90），整块压在父行下缘之上
    let frame = s.snap_at(10);
    assert_eq!(frame.rows.len(), 4);
    assert_eq!(frame.total_h, 360);
    assert_eq!(frame.epoch, s.epoch);
    let d = frame.drawer.clone().unwrap();
    assert_eq!(d.path, "a");
    assert!(d.opening);
    assert_eq!((d.full_h, d.dy), (180, -180));
    // 中途帧（半程已过大半——ease-out）与收尽帧
    let mid = s.snap_at(130).drawer.unwrap().dy;
    assert!(mid > -90 && mid < 0, "半程已过大半: {mid}");
    assert!(s.snap_at(60).drawer.unwrap().dy > -180);
    assert_eq!(s.snap_at(250).drawer.unwrap().dy, 0);
    assert_eq!(s.snap_at(10_000).drawer.unwrap().dy, 0);
    // 选中：帧值在 180ms 内单调逼近，收尽帧 = 目标
    s.select(3, 1_000); // b.txt
    let target = s.cursor_target(3);
    assert_eq!(target, 3 * ROW_H + ROW_H / 2);
    assert_eq!(s.snap_at(1_000).cursor_y, 0);
    assert!(s.snap_at(1_090).cursor_y < target);
    assert_eq!(s.snap_at(1_180).cursor_y, target);
    assert_eq!(s.snap_at(1_180).sel, Some(3));
    // 静态投影 = 收尽那一帧（无钟路径不吃动画）
    let still = s.snap();
    assert_eq!(still.drawer.unwrap().dy, 0);
    assert_eq!(still.cursor_y, target);
    assert_eq!(still.sel, Some(3));
    assert_eq!(
        still.expanded.iter().cloned().collect::<Vec<_>>(),
        vec!["a"]
    );
    // 无选中：光标归零
    let s2 = tree();
    assert_eq!(s2.snap().cursor_y, 0);
    assert_eq!(s2.snap_at(0).cursor_target, 0);
    assert_eq!(s2.snap().total_h, 180);
    assert!(s2.snap().drawer.is_none());
    assert!(s2.snap().loading.is_empty());
}

#[test]
fn spec_端到端_一段真实会话() {
    // 开页 → 取根层 → 点三角 → 取子层 → 抽屉落下 → 深一层 → 点文件 → 选中
    // → 滚动 → 命中随行 → 收起。全程只吃公开面（出参解析 → 状态机 → 帧值 → 命中）
    let mut st = FileTreeState::new("kfm-na");
    st.apply_root_list(
        entries_of(
            r#"{"ok":true,"dir":"","entries":[
                {"name":"src","kind":"dir","size":0,"mtime":1},
                {"name":"docs","kind":"dir","size":0,"mtime":2},
                {"name":"README.md","kind":"file","size":10,"mtime":3}]}"#,
        )
        .unwrap(),
        0,
    );
    assert_eq!(st.rows.len(), 3);
    let view_h = 6 * ROW_H;
    // src 的三角（深度 0 → 行左带）
    assert_eq!(st.hit(5, 10, view_h), Some(Hit::Toggle(0)));
    assert_eq!(
        st.toggle(0, 100),
        ToggleAction::NeedList { path: "src".into() }
    );
    let body = r#"{"ok":true,"dir":"src","entries":[
        {"name":"ui","kind":"dir"},{"name":"main.rs","kind":"file"}]}"#;
    st.apply_list("src", entries_of(body).unwrap(), 200);
    assert_eq!(
        st.rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
        vec!["src", "src/ui", "src/main.rs", "docs", "README.md"]
    );
    // 抽屉刚体：200ms 起步 180 高，440ms 到位
    let d = st.snap_at(200).drawer.unwrap();
    assert_eq!((d.full_h, d.dy), (180, -180));
    assert_eq!(st.snap_at(440).drawer.unwrap().dy, 0);
    assert!(anim_active(&st, 300));
    assert!(!anim_active(&st, 500));
    // 深一层：src/ui 的三角盒在 [55, 75)（缩进之后）
    let y_ui = ROW_H + 10;
    assert_eq!(st.hit(60, y_ui, view_h), Some(Hit::Toggle(1)));
    assert_eq!(
        st.toggle(1, 1_000),
        ToggleAction::NeedList {
            path: "src/ui".into()
        }
    );
    st.apply_list("src/ui", vec![file("filetree.rs")], 1_100);
    assert_eq!(st.rows.len(), 6);
    assert_eq!(st.rows[2].path, "src/ui/filetree.rs");
    // 点 README.md 行（第 6 行）→ 文件预览（目录行才会要列表）
    assert_eq!(st.hit(200, 5 * ROW_H + 10, view_h), Some(Hit::Row(5)));
    assert_eq!(
        st.toggle(5, 2_000),
        ToggleAction::Preview {
            path: "README.md".into(),
            name: "README.md".into()
        }
    );
    assert_eq!(
        st.hit(5, 4 * ROW_H + 10, view_h),
        Some(Hit::Toggle(4)),
        "docs 也是目录"
    );
    // 选中 README.md → 光标 180ms 落到行心
    st.select(5, 2_000);
    let tgt = st.cursor_target(5);
    assert_eq!(st.snap_at(2_180).cursor_y, tgt);
    assert_eq!(tgt, 5 * ROW_H + ROW_H / 2);
    // 滚动：命中随行平移（同一行 y −100）
    st.scroll_by(100, 300);
    assert_eq!(st.scroll, 100);
    assert_eq!(st.hit(200, 5 * ROW_H + 10 - 100, view_h), Some(Hit::Row(5)));
    // 收起 src：子孙全走，选中行随之上移到新位
    st.collapse(0, 3_000);
    assert_eq!(
        st.rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
        vec!["src", "docs", "README.md"]
    );
    assert_eq!(st.sel, Some(2));
    assert!(st.expanded.is_empty() && st.loaded.is_empty() && st.loading.is_empty());
    // 内容缩回一屏内 → 滚动自净；收起后照样能再展开
    st.scroll_by(0, view_h);
    assert_eq!(st.scroll, 0, "内容缩水后视口自净到顶");
    assert_eq!(
        st.hit(5, 10, view_h),
        Some(Hit::Toggle(0)),
        "收起后还能再展开"
    );
}

// ── 涂装侧纯件（BAR-165，termview 新增 pub 辅助）────────────────────
// 判卷对象：src/termview.rs 的 ft_geom / ft_band_rgb / ft_wrap_split——
// 涂装与手势/命中同吃的那把尺（paint_ft_content_impl 本体是 C 档感官，
// 判卷人 = 实拍）。

#[test]
fn spec_文件树几何窗_涂装与命中同一把尺() {
    // 真机 1260×2800：内容左缘 = 环细缘内 + 1 格（16+9+18），右缘镜像
    let g = termview::ft_geom(1260, 2800, 0);
    assert_eq!((g.x0, g.x1), (43, 1223));
    assert_eq!(g.list_y0, 55, "行表窗顶 = 页环上内缘");
    assert_eq!(g.bar_y1, 2781, "栏底 = 页环底内缘（BAR-121：内容不压环）");
    assert_eq!(g.bar_y0, 2781 - FT_BAR_H);
    assert_eq!(g.list_y1, g.bar_y0, "行表窗底 = 底栏顶");
    // 键盘在场：底栏与窗底一起上浮（行表窗高就是喂 hit 的 view_h）
    let k = termview::ft_geom(1260, 2800, 600);
    assert_eq!(k.bar_y1, 2781 - 600);
    assert_eq!(k.list_y1 - k.list_y0, g.list_y1 - g.list_y0 - 600);
    assert!(k.bar_y0 >= k.list_y0 && k.bar_y1 >= k.bar_y0);
    // 病态小屏自钳不自伤
    let s = termview::ft_geom(200, 120, 0);
    assert!(s.list_y1 >= s.list_y0 && s.bar_y1 >= s.bar_y0 && s.x1 <= 200);
    // 行带取色：同深度同色，深度奇偶分取 accent 双色（不引字面色）
    let acc = AccentPair {
        c1: 0x0011_2233,
        c2: 0x0044_5566,
    };
    assert_eq!(termview::ft_band_rgb(0, acc), acc.c1);
    assert_eq!(termview::ft_band_rgb(1, acc), acc.c2);
    assert_eq!(termview::ft_band_rgb(2, acc), acc.c1);
    assert_eq!(termview::ft_band_rgb(7, acc), acc.c2);
}

#[test]
fn spec_文件树换行切分() {
    // 装得下 = 全在首行
    assert_eq!(termview::ft_wrap_split(&[10.0, 10.0], 100.0), 2);
    // 满即断、刚好放下不断
    assert_eq!(termview::ft_wrap_split(&[10.0, 10.0], 20.0), 2);
    assert_eq!(termview::ft_wrap_split(&[10.0, 10.0, 10.0], 20.0), 2);
    assert_eq!(termview::ft_wrap_split(&[10.0, 9.0, 1.0], 19.0), 2);
    // 首字超宽：独占首行不吞字（≥1，病态窄行不死循环）
    assert_eq!(termview::ft_wrap_split(&[50.0, 5.0], 20.0), 1);
    assert_eq!(termview::ft_wrap_split(&[50.0], 1.0), 1);
    // 空表不炸
    assert_eq!(termview::ft_wrap_split(&[], 20.0), 0);
}
