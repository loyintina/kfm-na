//! mail_page_spec.rs — BAR-214 信箱页状态核/几何/信头纯件考题（A 档）。
//!
//! 变异抽检纪律：改坏 src/ui/mail_page.rs 前 cp 备份，验完备份复原
//! （禁 git checkout——工作树有在途活）。

use kfm_na::mail_feed::MailEntry;
use kfm_na::termview::CELL_H;
use kfm_na::ui::dual_pool::PoolRect;
use kfm_na::ui::mail_page::*;

fn entry(name: &str, from: &str, to: &str, title: &str, summary: Option<&str>) -> MailEntry {
    MailEntry {
        name: name.to_string(),
        bytes: 100,
        mtime: 1,
        time: "2026-10-01 08:00 +08:00".to_string(),
        from: from.to_string(),
        to: to.to_string(),
        title: title.to_string(),
        summary: summary.map(|s| s.to_string()),
        floors: Vec::new(),
    }
}

/// 钉①：信头纯件——剥部门/拆收件人/时间去时区
#[test]
fn spec_bar214_信头纯件() {
    // 剥部门：剥完 ≥2 字才剥
    assert_eq!(strip_dept("开发部观澜"), "观澜");
    assert_eq!(strip_dept("研究部清和"), "清和");
    assert_eq!(strip_dept("评审部白露"), "白露");
    assert_eq!(strip_dept("全体"), "全体"); // 无部字原样
    assert_eq!(strip_dept("研究部全体"), "全体");
    assert_eq!(strip_dept("甲乙"), "甲乙"); // 二字名无部原样
    // 拆收件人：主分隔「、」+ 兼容族 + 逐段剥部门
    assert_eq!(split_recipients("全体"), vec!["全体".to_string()]);
    assert_eq!(
        split_recipients("评审部白露、全体"),
        vec!["白露".to_string(), "全体".to_string()]
    );
    assert_eq!(
        split_recipients("研究部清和，开发部观澜"),
        vec!["清和".to_string(), "观澜".to_string()]
    );
    assert!(split_recipients("").is_empty());
    // 时间去时区
    assert_eq!(fmt_time("2026-09-30 08:00 +08:00"), "2026-09-30 08:00");
    assert_eq!(fmt_time("2026-09-30 08:00"), "2026-09-30 08:00");
}

/// 钉②：行高同源——const 值与 md 管线 line_h_grid 运行期对表
/// （const 钉死是 f32 ceil/round 非 const 的妥协，本钉是同源性的牙；
/// BAR-221 新约：const = 默认格值，实例格联动钉见 spec_bar221）
#[test]
fn spec_bar214_行高同源对表() {
    use kfm_na::ui::demo_page as dp;
    use kfm_na::ui::md_layout::line_h_grid;
    assert_eq!(BODY_LH, line_h_grid(CELL_H, 1.0));
    assert_eq!(H1_LH, line_h_grid(CELL_H, dp::H1_SCALE));
    assert_eq!(H1_BLOCK_H, dp::HU + H1_LH + dp::HU);
    let d = default_metrics();
    assert_eq!(d.body_lh, BODY_LH, "默认格 metrics 与 const 零漂移");
    assert_eq!(d.h1_block_h, H1_BLOCK_H);
}

/// 钉③：第一栏高 = max(H1 块, 收件人行数)；两收件人上下并置长行
#[test]
fn spec_bar214_首栏高随收件人() {
    let m = default_metrics();
    let one = lay_item(
        &entry("a", "开发部观澜", "全体", "t", Some("s")),
        0,
        40,
        &m,
        0.0,
    );
    assert_eq!(one.meta_h, H1_BLOCK_H, "单收件人栏高 = H1 块高");
    assert_eq!(one.recipients, vec!["全体".to_string()]);
    let two = lay_item(
        &entry("b", "开发部观澜", "评审部白露、全体", "t", Some("s")),
        0,
        40,
        &m,
        0.0,
    );
    assert_eq!(
        two.meta_h,
        H1_BLOCK_H.max(2 * BODY_LH),
        "双收件人栏高 = max(H1 块, 两行)"
    );
    assert_eq!(two.recipients.len(), 2);
    let three = lay_item(
        &entry("c", "f", "甲、乙、丙", "t", Some("s")),
        0,
        40,
        &m,
        0.0,
    );
    assert_eq!(three.meta_h, H1_BLOCK_H.max(3 * BODY_LH));
}

/// 钉④：渲染器不设最大高度——长标题/长摘要折行往下长，卡高 = 三栏实量和
#[test]
fn spec_bar214_折行长高无上限() {
    let m = default_metrics();
    let short = lay_item(&entry("a", "f", "全体", "短", Some("短")), 0, 40, &m, 0.0);
    assert_eq!(short.title_lines.len(), 1);
    assert_eq!(short.sum_lines.len(), 1);
    let expect_h = ITEM_PAD_V * 2 + short.meta_h + ROW_GAP + BODY_LH + ROW_GAP + BODY_LH;
    assert_eq!(short.h, expect_h);
    // 长标题（40 格宽，每汉字 2 格 → 21 字折两行）
    let long_title = "这是一段足够长的标题用来验证折行机制是否按格正确工作";
    let long = lay_item(
        &entry("b", "f", "全体", long_title, Some("短")),
        0,
        40,
        &m,
        0.0,
    );
    assert!(long.title_lines.len() >= 2, "长标题必须折行");
    assert_eq!(
        long.h,
        ITEM_PAD_V * 2
            + long.meta_h
            + ROW_GAP
            + long.title_lines.len() as u32 * BODY_LH
            + ROW_GAP
            + BODY_LH
    );
    // 超长摘要：100 行也照长（无最大高度条款）
    let huge_sum = "很长的摘要内容".repeat(200);
    let huge = lay_item(
        &entry("c", "f", "全体", "t", Some(&huge_sum)),
        0,
        40,
        &m,
        0.0,
    );
    assert!(huge.sum_lines.len() > 10);
    assert_eq!(
        huge.h,
        huge.sum_y + huge.sum_lines.len() as u32 * BODY_LH + ITEM_PAD_V
    );
    // 折行文本逐字守恒（grid_wrap 区间拼回 = 原文去断点空格）
    let rejoined: String = long
        .title_lines
        .iter()
        .map(|&(a, b)| long_title.chars().skip(a).take(b - a).collect::<String>())
        .collect();
    let orig: String = long_title.chars().filter(|c| *c != ' ').collect();
    assert_eq!(rejoined, orig, "折行不许丢字");
}

/// 钉⑤：摘要三态词面 + 空标题保底一行
#[test]
fn spec_bar214_摘要三态与空保底() {
    let m = default_metrics();
    let pending = lay_item(&entry("a", "f", "全体", "t", None), 0, 40, &m, 0.0);
    assert_eq!(pending.sum_lines.len(), 1);
    let empty = lay_item(&entry("b", "f", "全体", "t", Some("")), 0, 40, &m, 0.0);
    assert_eq!(empty.sum_lines.len(), 1);
    let no_title = lay_item(&entry("c", "f", "全体", "", Some("s")), 0, 40, &m, 0.0);
    assert_eq!(no_title.title_lines.len(), 1, "空标题保底一行不塌高");
    // 三态卡高一致（占位行与实单行同高 = 摘要不跳变）
    assert_eq!(pending.h, empty.h);
}

/// 钉⑥：流水前缀和 + 滚动/可见窗/懒加载窗/命中
#[test]
fn spec_bar214_流水滚动命中账() {
    let entries: Vec<MailEntry> = (0..10)
        .map(|i| entry(&format!("n{i}"), "f", "全体", "t", Some("s")))
        .collect();
    let lays = lay_items(&entries, 40, &default_metrics());
    // 前缀和：top(i+1) = top(i) + h + ITEM_GAP
    for w in lays.windows(2) {
        assert_eq!(w[1].top, w[0].top + i64::from(w[0].h) + i64::from(ITEM_GAP));
    }
    let th = total_h(&lays);
    assert_eq!(th, lays[9].top + i64::from(lays[9].h));
    let vp_h = th / 3;
    // 贴底：最新（末条目）必在可见窗尾
    let vr = visible_range(&lays, vp_h, 0);
    assert_eq!(vr.end, 10);
    assert!(vr.start < 10);
    // 看更旧：offset_bottom = max → 首条目可见
    let max = scroll_max(&lays, vp_h);
    let vr0 = visible_range(&lays, vp_h, max);
    assert_eq!(vr0.start, 0);
    // 懒加载窗：上沿向更旧扩 SUMMARY_LOOKBACK 屏
    let sw = summary_window(&lays, vp_h, 0);
    assert!(sw.start <= vr.start && sw.end >= vr.end);
    // 命中：贴底态点最新卡中心 = Item(9)；点卡间缝 = Page
    let area = PoolRect {
        x: 0,
        y: 0,
        w: 720,
        h: 0,
    };
    let vp = (0i64, vp_h);
    let r9 = item_rect(&area, vp, &lays, 9, 0);
    assert_eq!(
        hit(&area, vp, &lays, 0, r9.x + 5, r9.y + 5, &default_metrics()),
        Some(MailPageHit::Item(9))
    );
    // 卡缝（条目 8 底与 9 顶之间）
    let r8 = item_rect(&area, vp, &lays, 8, 0);
    let gap_y = r8.y + i64::from(r8.h) + i64::from(ITEM_GAP) / 2;
    assert_eq!(
        hit(&area, vp, &lays, 0, r9.x + 5, gap_y, &default_metrics()),
        Some(MailPageHit::Page)
    );
}

/// 钉⑦：状态核追底语义（follow_tail 同款）
#[test]
fn spec_bar214_追底状态机() {
    use kfm_na::mail_feed::MailKey;
    let m = default_metrics();
    let mut v = MailPageView::new(MailKey::NaBook);
    let entries: Vec<MailEntry> = (0..20)
        .map(|i| entry(&format!("n{i}"), "f", "全体", "t", Some("s")))
        .collect();
    v.sync_items(&entries, 40, 300, &m, 0);
    assert!(v.follow() && v.offset_bottom() == 0, "初态追底贴底");
    // 上滑看旧：follow 取消
    assert!(v.scroll_by(500));
    assert!(!v.follow());
    assert_eq!(v.offset_bottom(), 500);
    // 空增量不变不 bump（sig 鬼影纪律）
    let ep = v.epoch();
    assert!(!v.scroll_by(0));
    assert_eq!(v.epoch(), ep);
    // 回底恢复追底
    assert!(v.scroll_by(-500));
    assert!(v.follow() && v.offset_bottom() == 0);
    // 追底态同步新数据：恒贴底
    v.scroll_by(300);
    v.sync_items(&entries, 40, 300, &m, 0);
    assert_eq!(v.offset_bottom(), 300, "非追底同步不动视口");
    // 内容缩水钳回上限
    v.sync_items(&entries[..3], 40, 300, &m, 0);
    assert!(v.offset_bottom() <= scroll_max(v.lays(), 300));
}

/// 钉⑧：页几何/文宽格数/显示标题/摘要占位/全局句柄开收脏位
#[test]
fn spec_bar214_页几何与全局句柄() {
    use kfm_na::mail_feed::MailKey;
    use kfm_na::termview::CELL_W;
    let g = mail_geom(1260, 2560, 0);
    assert_eq!(g.bar_y1 - g.bar_y0, TOP_BAR_H, "顶栏高 = TOP_BAR_H");
    assert_eq!(g.div_y, g.bar_y1, "分隔线贴顶栏底");
    assert!(g.view_y1 > g.view_y0 && g.x1 > g.x0);
    let area = items_area(&g);
    assert_eq!(area.x, g.x0);
    assert_eq!(area.w, (g.x1 - g.x0) as u32);
    assert_eq!(
        text_cells_of(area.w, &default_metrics()),
        (area.w - ITEM_PAD_H * 2) / CELL_W,
        "文宽格数 = (卡宽 - 两侧 pad) / 格宽"
    );
    assert_eq!(TITLE_INDENT_CELLS, 2, "标题栏缩进 2 格（引用竖线+缝同尺）");
    // 显示标题回落 + 摘要占位两态词面
    let e = entry("0138号.md", "开发部观澜", "全体", "", None);
    assert_eq!(display_title(&e), "0138号.md", "缺字头标题回落信名");
    let e2 = entry("0138号.md", "开发部观澜", "全体", "真标题", Some(""));
    assert_eq!(display_title(&e2), "真标题");
    assert!(!SUMMARY_PENDING.is_empty() && !SUMMARY_EMPTY.is_empty());
    assert_ne!(SUMMARY_PENDING, SUMMARY_EMPTY, "两态词面不许同文");
    // 全局句柄：开→读回→脏位立→取走即清→收→脏位再立
    close(); // 防前题残态（关着再关幂等不涨脏）
    assert!(open_key().is_none());
    open(MailKey::MainBook);
    assert_eq!(open_key(), Some(MailKey::MainBook));
    assert!(take_dirty(), "开页必立脏位");
    assert!(!take_dirty(), "脏位取走即清");
    close();
    assert!(open_key().is_none());
    assert!(take_dirty(), "收页必立脏位");
}

/// 钉⑧（BAR-220）：页存活闸状态机——开=活 / 收=死 / 重开=活。
/// 烘焙槽凭此闸退场滑出期的空页重烘（close 后 sig 归零必异，
/// 无闸 = 空页重烘恰撞动画帧 = 半途卡）。变异咬：is_open 恒 true
/// → 收页断言红；恒 false → 开页断言红
#[test]
fn spec_bar220_页存活闸状态机() {
    use kfm_na::mail_feed::MailKey;
    close(); // 防前题残态
    assert!(!is_open(), "收态 = 死");
    open(MailKey::NaBook);
    assert!(is_open(), "开态 = 活");
    close();
    assert!(!is_open(), "收后 = 死（退场滑出期烘焙闸落）");
    open(MailKey::NaBook);
    assert!(is_open(), "重开 = 活（烘焙闸起）");
    close();
}

/// 钉⑨（BAR-221）：实例格几何账——默认格 metrics 与 const 逐值对表
/// 零漂移；实例格变 → 几何全联动（留白/栏距/行高/H1 块高/卡高/文宽
/// 折算）；note_cell 喂格 → 页态几何账与 mail_geom 右缘让位同吃新格；
/// 收页 = 默认格回退。变异咬：metrics_of 某字段回 CELL 常量 → 对表
/// 断言红；note_cell 不写 v.cell → mail_geom/cur_metrics 断言红；
/// mail_geom 右缘回 CELL_W → x1 差值断言红
#[test]
fn spec_bar221_实例格几何联动() {
    use kfm_na::mail_feed::MailKey;
    use kfm_na::termview::CELL_W;
    let d = default_metrics();
    assert_eq!(d.body_lh, BODY_LH);
    assert_eq!(d.h1_lh, H1_LH);
    assert_eq!(d.h1_block_h, H1_BLOCK_H);
    assert_eq!(d.item_pad_v, ITEM_PAD_V);
    assert_eq!(d.item_pad_h, ITEM_PAD_H);
    assert_eq!(d.row_gap, ROW_GAP);
    assert_eq!(d.item_gap, ITEM_GAP);
    // 双倍格：几何全翻（行高走 line_h_grid 同源，留白/栏距吃格直出）
    let big = metrics_of((CELL_W * 2, CELL_H * 2));
    assert_eq!(big.item_pad_v, CELL_H * 2);
    assert_eq!(big.row_gap, CELL_H);
    assert_eq!(
        big.body_lh,
        kfm_na::ui::md_layout::line_h_grid(CELL_H * 2, 1.0)
    );
    assert_ne!(big.body_lh, d.body_lh, "格变 → 行高必变");
    let e = entry("a", "f", "全体", "t", Some("s"));
    let lay_d = lay_item(&e, 0, 40, &d, 0.0);
    let lay_b = lay_item(&e, 0, 40, &big, 0.0);
    assert_eq!(lay_b.meta_y, big.item_pad_v, "卡内留白吃实例格");
    assert!(lay_b.h > lay_d.h, "格变大 → 卡高必长");
    // 文宽折算吃实例格宽
    assert_eq!(
        text_cells_of(720, &big),
        (720 - big.item_pad_h * 2) / big.cell_w
    );
    // note_cell → 页态几何账 + mail_geom 右缘让位跟实例格宽
    close(); // 防前题残态（页关 = 默认格回退）
    assert_eq!(cur_cell(), (CELL_W, CELL_H));
    let g0 = mail_geom(1260, 2560, 0);
    open(MailKey::NaBook);
    note_cell((CELL_W * 2, CELL_H * 2));
    assert_eq!(cur_cell(), (CELL_W * 2, CELL_H * 2));
    assert_eq!(cur_metrics().body_lh, big.body_lh, "页态几何账吃喂入格");
    let g1 = mail_geom(1260, 2560, 0);
    assert_eq!(
        g0.x1 - g1.x1,
        i64::from(CELL_W),
        "格宽翻倍 → 右缘让位多 1 格"
    );
    close();
    assert_eq!(cur_cell(), (CELL_W, CELL_H), "收页 = 默认格回退");
}

/// 带楼层条目构造（BAR-222 钉用）
fn floored_entry(name: &str, floors: Vec<(&str, &str, &str, &str)>) -> MailEntry {
    let mut e = entry(name, "开发部观澜", "全体", "题", Some("摘"));
    e.floors = floors
        .into_iter()
        .enumerate()
        .map(
            |(i, (author, to, time, body))| kfm_na::mail_feed::MailFloor {
                n: (i + 1) as u32,
                author: author.to_string(),
                to: to.to_string(),
                time: time.to_string(),
                summary: None,
                detail: None,
                body: body.to_string(),
            },
        )
        .collect();
    e
}

/// 钉（BAR-222）①：第四栏几何——卡高 = 各栏实量之和（楼层三行全账进高），
/// 楼数/楼文长多少卡就长多少（无帽同律）
#[test]
fn spec_bar222_第四栏几何全账() {
    let m = default_metrics();
    let e = floored_entry(
        "0001号甲致乙的通报.md",
        vec![
            ("观澜", "观澜", "2026-10-01 17:23 +08:00", "一楼正文"),
            ("承影", "1楼观澜", "2026-10-02 14:40 +08:00", "回楼"),
        ],
    );
    let base = lay_item(
        &entry(
            "0001号甲致乙的通报.md",
            "开发部观澜",
            "全体",
            "题",
            Some("摘"),
        ),
        0,
        40,
        &m,
        0.0,
    );
    let lay = lay_item(&e, 0, 40, &m, 0.0);
    assert_eq!(lay.floors.len(), 2);
    // 每楼 = ROW_GAP + BODY_LH(引用行) + H2_LH(层主行) + 正文行×BODY_LH
    let per_floor = m.row_gap + m.body_lh + m.h2_lh + m.body_lh;
    assert_eq!(lay.h, base.h + per_floor * 2, "两楼全账进卡高");
    // 楼内三行纵序：引用 < 层主 < 正文，逐行咬高
    let f0 = &lay.floors[0];
    assert_eq!(f0.who_y, f0.quote_y + m.body_lh);
    assert_eq!(f0.body_y, f0.who_y + m.h2_lh);
    assert_eq!(f0.body_lines.len(), 1, "短文单行");
    // 长楼文折行全账：40 格宽 500 字必多行，行数 = grid_wrap 全量（不截）
    let long = "长".repeat(500);
    let e2 = floored_entry(
        "0002号甲致乙的通报.md",
        vec![("观澜", "观澜", "2026-10-01 17:23 +08:00", &long)],
    );
    let lay2 = lay_item(&e2, 0, 40, &m, 0.0);
    let want = kfm_na::ui::grid_text::grid_wrap(&long, 40).len();
    assert!(want > 1);
    assert_eq!(
        lay2.floors[0].body_lines.len(),
        want,
        "楼正文折行全账不截断"
    );
    assert_eq!(
        lay2.h,
        base.h + m.row_gap + m.body_lh + m.h2_lh + want as u32 * m.body_lh
    );
}

/// 钉（BAR-231 改约：卡内只显摘要）③：几何吃 display_text 单源——
/// 新形楼折行区间只对 summary 算（detail/body 原文不参与），旧形回落 body
#[test]
fn spec_bar222_楼层几何吃渲染单源() {
    let m = default_metrics();
    let mut e = entry(
        "0001号甲致乙的通报.md",
        "开发部观澜",
        "全体",
        "题",
        Some("摘"),
    );
    e.floors = vec![kfm_na::mail_feed::MailFloor {
        n: 1,
        author: "观澜".to_string(),
        to: "观澜".to_string(),
        time: "2026-10-01 17:23 +08:00".to_string(),
        summary: Some("白话摘要".to_string()),
        detail: Some("细节全文".to_string()),
        body: "长".repeat(100),
    }];
    let lay = lay_item(&e, 0, 40, &m, 0.0);
    let want = kfm_na::ui::grid_text::grid_wrap("白话摘要", 40);
    assert_eq!(
        lay.floors[0].body_lines, want,
        "折行区间只对 summary 算（detail 不进卡，body 原文不参与）"
    );
}

/// 钉（BAR-222）②：账务族同尺——带楼层卡进了 total_h/scroll_max/可见窗
/// （§二连带：别只放高不算账）
#[test]
fn spec_bar222_楼层进账务族() {
    let es = vec![
        floored_entry(
            "0001号甲致乙的通报.md",
            vec![("甲", "乙", "2026-10-02 08:00 +08:00", "楼")],
        ),
        entry(
            "0002号丙致丁的通报.md",
            "开发部观澜",
            "全体",
            "题",
            Some("摘"),
        ),
    ];
    let m = default_metrics();
    let lays = lay_items(&es, 40, &m);
    let th = total_h(&lays);
    assert_eq!(
        th,
        i64::from(lays[0].h) + i64::from(m.item_gap) + i64::from(lays[1].h),
        "带楼层卡高进总账"
    );
    // 视口矮于总账 → scroll_max = 差值（含楼层那一截）
    let sm = scroll_max(&lays, 100);
    assert_eq!(sm, th - 100);
    // 可见窗能框住带楼层的卡（半卡在沿也画）
    let vr = visible_range(
        &lays,
        i64::from(lays[0].h) + 10,
        th - (i64::from(lays[0].h) + 10),
    );
    assert!(vr.contains(&0));
    // 行高同源：H2_LH 与 md 管线对表
    use kfm_na::ui::demo_page as dp;
    assert_eq!(
        H2_LH,
        kfm_na::ui::md_layout::line_h_grid(CELL_H, dp::H2_SCALE)
    );
}

// ---- BAR-244（NA0173，MAIN0141）：信卡楼层展开/折叠 ----

/// n 楼条目构造（每楼单行短文，几何可手算）
fn n_floor_entry(name: &str, n: usize) -> MailEntry {
    floored_entry(
        name,
        (0..n)
            .map(|i| {
                (
                    if i == 0 { "观澜" } else { "承影" },
                    "观澜",
                    "2026-10-02 08:00 +08:00",
                    "楼正文",
                )
            })
            .collect(),
    )
}

/// 一楼全账高（row_gap + 引用行 + 层主行 + 正文行数×body_lh）
fn floor_full_h(fl: &FloorLay, m: &Metrics) -> u32 {
    m.row_gap + m.body_lh + m.h2_lh + fl.body_lines.len() as u32 * m.body_lh
}

/// 钉（BAR-244）①：折叠态（n=5, p=0）只排两楼 + 底控件——ctrl_bottom 紧跟
/// floor2 底+row_gap，ctrl_top 消失（None 无间断跳），卡高 = 折叠公式逐值；
/// floors 全量进 Vec（涂装裁剪），extra_band 折叠态空带
#[test]
fn spec_bar244_折叠态只排两楼加控件() {
    let m = default_metrics();
    let e = n_floor_entry("5楼.md", 5);
    let lay = lay_item(&e, 0, 40, &m, 0.0);
    assert!(lay.ctrl_top.is_none(), "p=0 顶控件块消失");
    assert_eq!(lay.floors.len(), 5, "floors 全量进账（露出归涂装裁剪）");
    let f2 = &lay.floors[1];
    let f2_end = f2.body_y + f2.body_lines.len() as u32 * m.body_lh;
    let cb = lay.ctrl_bottom.expect("n>2 必有底控件");
    assert_eq!(cb, f2_end + m.row_gap, "折叠态底控件紧跟 floor2 底+row_gap");
    assert_eq!(
        lay.h,
        cb + m.body_lh + m.item_pad_v,
        "折叠卡高 = 底控件行底 + 下留白（ctrl_h = body_lh）"
    );
    let (by, bh) = lay.extra_band.expect("n>2 必有裁剪带账");
    assert_eq!(by, f2_end + m.row_gap, "裁剪带起 = floor2 底+row_gap");
    assert_eq!(bh, 0, "p=0 裁剪带空（floors[2..] 零露出）");
    // 前两楼位置与无控件旧约同尺（A1=0 不下移）
    let sum_end = lay.sum_y + lay.sum_lines.len() as u32 * m.body_lh;
    assert_eq!(lay.floors[0].quote_y, sum_end + m.row_gap);
}

/// 钉（BAR-244）②：展开态（p=1）全楼 + 双控件——ctrl_top 行在摘要栏底
/// +row_gap（块全量 row_gap+ctrl_h），ctrl_bottom 在 floor5 底+row_gap，
/// 卡高 = 全量旧账 + 2×(row_gap+ctrl_h)
#[test]
fn spec_bar244_展开态全楼双控件() {
    let m = default_metrics();
    let e = n_floor_entry("5楼.md", 5);
    let lay = lay_item(&e, 0, 40, &m, 1.0);
    let sum_end = lay.sum_y + lay.sum_lines.len() as u32 * m.body_lh;
    let (ty, a1) = lay.ctrl_top.expect("p=1 顶控件块全量");
    assert_eq!(
        ty,
        sum_end + m.row_gap,
        "顶控件行 = 摘要栏底+row_gap 最终位"
    );
    assert_eq!(a1, m.row_gap + m.body_lh, "p=1 顶块全量 = row_gap+ctrl_h");
    let f5 = &lay.floors[4];
    let f5_end = f5.body_y + f5.body_lines.len() as u32 * m.body_lh;
    assert_eq!(
        lay.ctrl_bottom.unwrap(),
        f5_end + m.row_gap,
        "展开态底控件在全楼之后"
    );
    let full_old: u32 = sum_end
        + lay
            .floors
            .iter()
            .map(|fl| floor_full_h(fl, &m))
            .sum::<u32>()
        + m.item_pad_v;
    assert_eq!(
        lay.h,
        full_old + 2 * (m.row_gap + m.body_lh),
        "展开卡高 = 全量旧账 + 顶/底两控件块"
    );
    // 裁剪带全量露出 floors[2..]（extra_full 含楼 3 前的引导 row_gap，
    // 带尾恰接底控件行顶）
    let f2 = &lay.floors[1];
    let f2_end = f2.body_y + f2.body_lines.len() as u32 * m.body_lh;
    let (by, bh) = lay.extra_band.unwrap();
    assert_eq!(by, f2_end + m.row_gap);
    assert_eq!(bh, f5_end - f2_end, "p=1 裁剪带 = extra_full 全量");
    assert_eq!(by + bh, lay.ctrl_bottom.unwrap(), "带尾 = 底控件行顶");
}

/// 钉（BAR-244）③：边界——楼 ≤2 任意 p 无控件（卡高与 p 无关）；n=3
/// 折叠态底控件文案数 = n−2 = 1
#[test]
fn spec_bar244_边界两楼无控件() {
    let m = default_metrics();
    let e2 = n_floor_entry("2楼.md", 2);
    let h0 = lay_item(&e2, 0, 40, &m, 0.0).h;
    for p in [0.0, 0.5, 1.0] {
        let lay = lay_item(&e2, 0, 40, &m, p);
        assert!(lay.ctrl_top.is_none(), "n=2 无顶控件（p={p}）");
        assert!(lay.ctrl_bottom.is_none(), "n=2 无底控件（p={p}）");
        assert!(lay.extra_band.is_none(), "n=2 无裁剪带（p={p}）");
        assert_eq!(lay.h, h0, "n=2 卡高与 p 无关");
    }
    let e1 = n_floor_entry("1楼.md", 1);
    let lay1 = lay_item(&e1, 0, 40, &m, 1.0);
    assert!(lay1.ctrl_bottom.is_none(), "n=1 同样无控件");
    let e3 = n_floor_entry("3楼.md", 3);
    let lay3 = lay_item(&e3, 0, 40, &m, 0.0);
    assert!(lay3.ctrl_bottom.is_some(), "n=3 折叠态有底控件");
    assert_eq!(e3.floors.len() - 2, 1, "底控件文案 = 展开余下 1 楼");
}

/// 钉（BAR-244）④：锚定补偿——底控件展开全程锚行屏幕 y 逐像素不动
/// （offset_bottom 账不变）；展开态点顶控件收起同断言（补偿式钳回，
/// 未撞 clamp）。构造：多卡内容足够高，offset 非 0 非 max 且大于
/// 展开增量（防撞边锚漂——撞边 = 账注明的极端情形，不在本钉射程）
#[test]
fn spec_bar244_锚定补偿_底行不动() {
    use kfm_na::mail_feed::MailKey;
    let m = default_metrics();
    let vp_h: i64 = 1500;
    let mut entries: Vec<MailEntry> = (0..8)
        .map(|i| entry(&format!("n{i}"), "f", "全体", "t", Some("s")))
        .collect();
    entries.insert(3, n_floor_entry("目标信.md", 5));
    let i = 3;
    let name = "目标信.md";
    let mut v = MailPageView::new(MailKey::NaBook);
    v.sync_items(&entries, 40, vp_h, &m, 0);
    let max0 = scroll_max(v.lays(), vp_h);
    assert!(v.scroll_by(1200), "scroll 到非 0 非 max");
    assert!(v.offset_bottom() > 0 && v.offset_bottom() < max0);
    let st_of = |v: &MailPageView| total_h(v.lays()) - vp_h - v.offset_bottom();
    // 底控件展开：锚行屏幕 y 全程不动，offset_bottom 逐值不变（恒等式）
    let sy0 = v.lays()[i].top + i64::from(v.lays()[i].ctrl_bottom.unwrap()) - st_of(&v);
    let off0 = v.offset_bottom();
    assert!(v.toggle_floors(name, AnchorKind::Bottom, 1000));
    for now in [1041u64, 1092, 1170, 1260] {
        v.sync_items(&entries, 40, vp_h, &m, now);
        let lay = &v.lays()[i];
        let sy = lay.top + i64::from(lay.ctrl_bottom.unwrap()) - st_of(&v);
        assert_eq!(sy, sy0, "展开 t={now}：锚行屏幕 y 逐像素不动");
        assert_eq!(v.offset_bottom(), off0, "底锚展开：offset_bottom 账不变");
        assert!(v.offset_bottom() <= scroll_max(v.lays(), vp_h));
    }
    assert!(
        v.lays()[i].ctrl_top.map(|(_, a)| a) == Some(m.row_gap + m.body_lh),
        "t=1260 已 settle 到全展开"
    );
    let th1 = total_h(v.lays());
    // 展开态点顶控件收起：锚行屏幕 y 全程不动（补偿式钳回，offset 渐回）
    let sy1 = v.lays()[i].top + i64::from(v.lays()[i].ctrl_top.unwrap().0) - st_of(&v);
    assert!(v.toggle_floors(name, AnchorKind::Top, 2000));
    let mut checked = 0;
    for now in [2050u64, 2100, 2180, 2200] {
        v.sync_items(&entries, 40, vp_h, &m, now);
        let lay = &v.lays()[i];
        if let Some((ty, _)) = lay.ctrl_top {
            let sy = lay.top + i64::from(ty) - st_of(&v);
            assert_eq!(sy, sy1, "收起 t={now}：顶控件锚行屏幕 y 逐像素不动");
            checked += 1;
        }
        assert!(
            v.offset_bottom() < scroll_max(v.lays(), vp_h) || v.offset_bottom() == 0,
            "补偿不越界"
        );
    }
    assert!(checked >= 2, "收起动画中段必须实际咬到锚断言");
    assert!(v.lays()[i].ctrl_top.is_none(), "settle 后顶控件块消失");
    // 补偿式守恒：scroll_top 全程不动 → 终点 offset = off0 − 展开增量
    assert_eq!(
        v.offset_bottom(),
        off0 - (th1 - total_h(v.lays())),
        "收起 settle：offset 逐值回到 off0−Δ（scroll_top 未动 = 锚未漂）"
    );
}

/// 钉（BAR-244）⑤：中途反向不跳——toggle 后 t+100ms 采样 p1，同刻再
/// toggle 采样 p2，|p1−p2| < 0.02（重定基连续）；反向走离场曲线回 0
#[test]
fn spec_bar244_中途反向不跳() {
    let mut fx = FloorFx::new();
    assert!(fx.settled(), "初态 settled（p=0 折叠定态）");
    fx.toggle(1000);
    assert!(!fx.settled());
    let p1 = fx.sample(1100);
    assert!(p1 > 0.0 && p1 < 1.0, "t+100ms 在动画中段");
    fx.toggle(1100);
    let p2 = fx.sample(1100);
    assert!(
        (p1 - p2).abs() < 0.02,
        "中途反向同刻采样连续：p1={p1} p2={p2}"
    );
    let p3 = fx.sample(1100 + 180);
    assert_eq!(p3, 0.0, "离场 180ms 到点贴死 0");
    assert!(fx.settled());
}

/// 钉（BAR-244）⑥：守恒——折叠 p=0 → 展开 p=1 → 收起 p=0，整份 ItemLay
/// 逐像素回到展开前（不只卡高：控件账/裁剪带/楼层账全等）
#[test]
fn spec_bar244_守恒_折叠回原点() {
    use kfm_na::mail_feed::MailKey;
    let m = default_metrics();
    let e = n_floor_entry("守恒信.md", 5);
    let h0 = lay_item(&e, 0, 40, &m, 0.0).h;
    let h1 = lay_item(&e, 0, 40, &m, 1.0).h;
    assert!(h1 > h0, "展开必高于折叠");
    // 页级往返（走真 toggle/settle 路径，非重调纯函数自欺）
    let entries = vec![entry("垫底.md", "f", "全体", "t", Some("s")), e];
    let mut v = MailPageView::new(MailKey::NaBook);
    v.sync_items(&entries, 40, 4000, &m, 0);
    let lays0 = v.lays().to_vec();
    assert!(v.toggle_floors("守恒信.md", AnchorKind::Bottom, 1000));
    v.sync_items(&entries, 40, 4000, &m, 1260); // 250ms 进场 settle
    assert_ne!(v.lays()[1], lays0[1], "展开态 ≠ 折叠态");
    assert!(v.toggle_floors("守恒信.md", AnchorKind::Top, 2000));
    v.sync_items(&entries, 40, 4000, &m, 2200); // 180ms 离场 settle
    assert_eq!(v.lays(), &lays0[..], "展开→折叠后整册排版逐像素回原点");
}

/// 钉（BAR-244）⑦：接线守卫（变异锚，源码断言）——FloorToggle 变体在
/// mail_page；android_app 点按分发有 toggle_floors 臂；fx_spring 活性表
/// 含 mail_page::fx_active（第十路，漏接 = 动画零帧「抬手即卡」同族病）；
/// termview 涂装含「展开余下」文案锚
#[test]
fn spec_bar244_接线守卫() {
    let mp = include_str!("../src/ui/mail_page.rs");
    assert!(
        mp.contains("FloorToggle"),
        "MailPageHit::FloorToggle 变体必须在"
    );
    assert!(
        mp.contains("panel_ease_pos"),
        "FloorFx 采样必须走 panel_ease_pos 曲线族"
    );
    let app = include_str!("../src/android_app.rs");
    assert!(
        app.contains("mail_page::toggle_floors"),
        "android_app 点按分发必须有 toggle_floors 臂"
    );
    assert!(
        app.contains("mail_page::tick_fx"),
        "android_app 帧泵必须有 tick_fx 推进臂"
    );
    let fx = include_str!("../src/ui/fx_spring.rs");
    assert!(
        fx.contains("mail_page::fx_active"),
        "fx_frame_due 活性表必须读 mail_page::fx_active（第十路）"
    );
    assert!(
        fx.contains("|| mail_floor_fx"),
        "fx_frame_due 活性 OR 链必须含 mail_floor_fx 臂（漏接 = 动画零帧）"
    );
    let tv = include_str!("../src/termview.rs");
    assert!(tv.contains("展开余下"), "涂装必须有「展开余下 N 楼」文案锚");
}
