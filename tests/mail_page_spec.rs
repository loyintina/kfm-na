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
/// （const 钉死是 f32 ceil/round 非 const 的妥协，本钉是同源性的牙）
#[test]
fn spec_bar214_行高同源对表() {
    use kfm_na::ui::demo_page as dp;
    use kfm_na::ui::md_layout::line_h_grid;
    assert_eq!(BODY_LH, line_h_grid(CELL_H, 1.0));
    assert_eq!(H1_LH, line_h_grid(CELL_H, dp::H1_SCALE));
    assert_eq!(H1_BLOCK_H, dp::HU + H1_LH + dp::HU);
}

/// 钉③：第一栏高 = max(H1 块, 收件人行数)；两收件人上下并置长行
#[test]
fn spec_bar214_首栏高随收件人() {
    let one = lay_item(&entry("a", "开发部观澜", "全体", "t", Some("s")), 0, 40);
    assert_eq!(one.meta_h, H1_BLOCK_H, "单收件人栏高 = H1 块高");
    assert_eq!(one.recipients, vec!["全体".to_string()]);
    let two = lay_item(
        &entry("b", "开发部观澜", "评审部白露、全体", "t", Some("s")),
        0,
        40,
    );
    assert_eq!(
        two.meta_h,
        H1_BLOCK_H.max(2 * BODY_LH),
        "双收件人栏高 = max(H1 块, 两行)"
    );
    assert_eq!(two.recipients.len(), 2);
    let three = lay_item(&entry("c", "f", "甲、乙、丙", "t", Some("s")), 0, 40);
    assert_eq!(three.meta_h, H1_BLOCK_H.max(3 * BODY_LH));
}

/// 钉④：渲染器不设最大高度——长标题/长摘要折行往下长，卡高 = 三栏实量和
#[test]
fn spec_bar214_折行长高无上限() {
    let short = lay_item(&entry("a", "f", "全体", "短", Some("短")), 0, 40);
    assert_eq!(short.title_lines.len(), 1);
    assert_eq!(short.sum_lines.len(), 1);
    let expect_h = ITEM_PAD_V * 2 + short.meta_h + ROW_GAP + BODY_LH + ROW_GAP + BODY_LH;
    assert_eq!(short.h, expect_h);
    // 长标题（40 格宽，每汉字 2 格 → 21 字折两行）
    let long_title = "这是一段足够长的标题用来验证折行机制是否按格正确工作";
    let long = lay_item(&entry("b", "f", "全体", long_title, Some("短")), 0, 40);
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
    let huge = lay_item(&entry("c", "f", "全体", "t", Some(&huge_sum)), 0, 40);
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
    let pending = lay_item(&entry("a", "f", "全体", "t", None), 0, 40);
    assert_eq!(pending.sum_lines.len(), 1);
    let empty = lay_item(&entry("b", "f", "全体", "t", Some("")), 0, 40);
    assert_eq!(empty.sum_lines.len(), 1);
    let no_title = lay_item(&entry("c", "f", "全体", "", Some("s")), 0, 40);
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
    let lays = lay_items(&entries, 40);
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
        hit(&area, vp, &lays, 0, r9.x + 5, r9.y + 5),
        Some(MailPageHit::Item(9))
    );
    // 卡缝（条目 8 底与 9 顶之间）
    let r8 = item_rect(&area, vp, &lays, 8, 0);
    let gap_y = r8.y + i64::from(r8.h) + i64::from(ITEM_GAP) / 2;
    assert_eq!(
        hit(&area, vp, &lays, 0, r9.x + 5, gap_y),
        Some(MailPageHit::Page)
    );
}

/// 钉⑦：状态核追底语义（follow_tail 同款）
#[test]
fn spec_bar214_追底状态机() {
    use kfm_na::mail_feed::MailKey;
    let mut v = MailPageView::new(MailKey::NaBook);
    let entries: Vec<MailEntry> = (0..20)
        .map(|i| entry(&format!("n{i}"), "f", "全体", "t", Some("s")))
        .collect();
    v.sync_items(&entries, 40, 300);
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
    v.sync_items(&entries, 40, 300);
    assert_eq!(v.offset_bottom(), 300, "非追底同步不动视口");
    // 内容缩水钳回上限
    v.sync_items(&entries[..3], 40, 300);
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
        text_cells_of(area.w),
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
