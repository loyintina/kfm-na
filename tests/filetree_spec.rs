//! filetree_spec.rs — 文件树页内容核考卷（A 档）。
//! 判卷对象：src/ui/filetree.rs（纯逻辑：几何 / 出参解析 / 状态机 / 帧值）。
//!
//! 参数锚点来自真机截屏实测（用户量）：行高 86/118 物理 px、缩进表 CSS
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

/// 根 = [a(dir), b.txt(file)]（两行各 86 → 总高 172）
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
    // 实测锚点：round(cumsum(d) × 3.06)
    assert_eq!(
        (0..=6).map(indent_px).collect::<Vec<_>>(),
        vec![0, 55, 104, 147, 184, 214, 242]
    );
    // 表值本身（CSS px）
    assert_eq!(shift_css(0), 18);
    assert_eq!(shift_css(1), 16);
    assert_eq!(shift_css(4), 10);
}

#[test]
fn spec_缩进_深层钳489() {
    // 累加到 160 CSS 即封顶（实测锚点取整 489 = 160 × 3.06 截断）
    assert!(indent_px(35) < 489, "{}", indent_px(35));
    assert_eq!(indent_px(36), 489);
    assert_eq!(indent_px(80), 489);
    // 全程非递减（钳制不许出现回缩）
    let mut prev = 0;
    for d in 0..=80 {
        let v = indent_px(d);
        assert!(v >= prev, "深度 {d} 缩进回缩: {prev} → {v}");
        prev = v;
    }
}

#[test]
fn spec_缩进_超表长末档兜() {
    // nz `SHIFT_TABLE[Math.min(d, len-1)] ?? 2` 同款：超表长按末档 2
    assert_eq!(shift_css(19), 2);
    assert_eq!(shift_css(20), 2);
    assert_eq!(shift_css(64), 2);
}

#[test]
fn spec_几何_三角与名字左缘() {
    assert_eq!(tri_x(0), 0);
    assert_eq!(tri_x(4), 184);
    assert_eq!(name_x(4), 184 + TRI_W);
    assert_eq!(name_x(0), TRI_W);
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
    assert_eq!(band_alpha(0), 0.05);
    assert_eq!(border_op(0), 0.3);
    let mut prev = band_alpha(0);
    for d in 0..=40 {
        let a = band_alpha(d);
        assert!((0.05..0.31).contains(&a), "深度 {d} α 出域: {a}");
        assert!(a >= prev);
        prev = a;
    }
    assert!(band_alpha(19) > 0.28, "{}", band_alpha(19));
    assert!(border_op(19) > 0.7 && border_op(19) < 0.75);
    // 深度 0 不画左强调边（nz：row.depth > 0 才挂 span）
    assert!(!left_bar_on(0));
    assert!(left_bar_on(1));
}

// ── 几何：行高 / 行矩形 ─────────────────────────────────────────────

#[test]
fn spec_行高_单行86换行118() {
    assert_eq!(row_h(false), 86);
    assert_eq!(row_h(true), 118);
    assert_eq!(ROW_H, 86);
    assert_eq!(ROW_H_WRAP, 118);
}

#[test]
fn spec_total_h与row_rects一致() {
    let mut s = tree();
    s.set_wrap(1, true, 0); // b.txt 长名换行
    assert_eq!(total_h(&s.rows), 86 + 118);
    let rects = row_rects(&s.rows, 0, 10_000);
    assert_eq!(rects, vec![(0, 0, 86), (1, 86, 118)]);
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
    // 视口 90 高、滚 100：行 0 整体在上方（y+h = -14）→ 不出
    let rects = row_rects(&s.rows, 100, 90);
    assert_eq!(rects, vec![(1, -14, 118)]);
    // 视口高 0 = 没有可画的行
    assert!(row_rects(&s.rows, 0, 0).is_empty());
}

#[test]
fn spec_row_rects_滚动平移() {
    let s = tree();
    let a = row_rects(&s.rows, 0, 10_000);
    let b = row_rects(&s.rows, 30, 10_000);
    assert_eq!(a[1].1 - b[1].1, 30, "同一行随 scroll 平移同一位移");
    assert_eq!(a[1].2, b[1].2, "行高不随滚动变");
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
    assert_eq!(drawer_full_h(&s.rows, 0), 172); // a 的子块 = 两行 × 86
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
    // 规格原文口径：末位 FF 是 alpha（与 termview 的 AARRGGBB 相反）
    assert_eq!(rrggbbaa(CURSOR_LINE_RGB), (0, 212, 255, 255));
    assert_eq!(rrggbbaa(CHEVRON_RGB), (0, 148, 178, 255));
    assert_eq!(CURSOR_LINE_ALPHA, 0.7);
    assert_eq!(CURSOR_FILL_ALPHA, 0.15);
    assert_eq!(CURSOR_INSET, 4);
    assert_eq!(ROW_RADIUS, 12);
    assert_eq!(TRI_W, 20);
    assert_eq!(TRI_H, 22);
    assert_eq!((DRAWER_OPEN_MS, DRAWER_CLOSE_MS), (240, 180));
    assert_eq!((TRI_ROT_MS, CURSOR_MOVE_MS), (180, 180));
    assert_eq!(FT_CSS, 3.06);
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
    assert_eq!(drawer_dy(&anim, 10, 172), -172);
    assert_eq!(drawer_dy(&anim, 250, 172), 0);
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
    s.set_wrap(0, true, 50); // 量宽回填：a 换行 → 行高 86 → 118
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
    let mut s = tree(); // 总高 172
    s.scroll_by(50, 100); // 上界 = 172 − 100 = 72，50 在界内
    assert_eq!(s.scroll, 50);
    s.scroll_by(100, 100);
    assert_eq!(s.scroll, 72, "超界不越");
    s.scroll_by(-1_000, 100);
    assert_eq!(s.scroll, 0, "上滑不许变负");
    s.scroll_by(30, 172); // 一屏装得下 → 上界 0
    assert_eq!(s.scroll, 0);
    s.scroll_by(0, 0); // 病态视口高（未布局）：上界 = 总高，仍钳
    assert_eq!(s.scroll, 0);
}

#[test]
fn spec_滚动_内容缩水钳回() {
    let mut s = tree_a_open(); // 总高 344（四行）
    s.scroll_by(1_000, 100);
    assert_eq!(s.scroll, 244);
    s.collapse(0, 100); // 内容缩回 172
    s.clamp_scroll(100);
    assert_eq!(s.scroll, 72, "内容缩水后视口不许悬在空白上");
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
fn spec_命中_三角带与行其余() {
    let s = tree();
    // 深度 0：行左带就是三角盒
    assert_eq!(s.hit(0, 10, 172), Some(Hit::Toggle(0)));
    assert_eq!(s.hit(TRI_W - 1, 10, 172), Some(Hit::Toggle(0)));
    assert_eq!(s.hit(TRI_W, 10, 172), Some(Hit::Row(0)), "行其余 = 选中");
    assert_eq!(s.hit(400, 10, 172), Some(Hit::Row(0)));
    // 行底边界：y = 行高 落在下一行
    assert_eq!(s.hit(400, ROW_H - 1, 172), Some(Hit::Row(0)));
    assert_eq!(s.hit(400, ROW_H, 172), Some(Hit::Row(1)));
}

#[test]
fn spec_命中_文件行左带也算行() {
    let s = tree();
    assert_eq!(
        s.hit(0, ROW_H + 10, 172),
        Some(Hit::Row(1)),
        "文件没有三角可点"
    );
}

#[test]
fn spec_命中_深行三角盒能点() {
    let s = tree_a_open(); // a/sub 在行 1，深度 1 → 三角盒 [55, 75)
    let y = ROW_H + 10;
    assert_eq!(
        s.hit(60, y, 344),
        Some(Hit::Toggle(1)),
        "画出来的三角必须能点"
    );
    assert_eq!(s.hit(74, y, 344), Some(Hit::Toggle(1)));
    assert_eq!(s.hit(75, y, 344), Some(Hit::Row(1)));
    assert_eq!(s.hit(30, y, 344), Some(Hit::Row(1)), "缩进留白 = 选中");
    // 文件行（a/x.txt，深度 1）左带与三角位都不给 Toggle
    let y2 = ROW_H * 2 + 10;
    assert_eq!(s.hit(0, y2, 344), Some(Hit::Row(2)));
    assert_eq!(s.hit(60, y2, 344), Some(Hit::Row(2)));
}

#[test]
fn spec_命中_滚动后平移() {
    let mut s = tree();
    assert_eq!(s.hit(120, 90, 172), Some(Hit::Row(1)));
    s.scroll_by(80, 100);
    assert_eq!(s.scroll, 72);
    // 同一行：屏上位置 = 内容位置 − scroll
    assert_eq!(s.hit(120, 14, 100), Some(Hit::Row(1)));
    assert_eq!(s.hit(120, 13, 100), Some(Hit::Row(0)), "上边界随滚动平移");
    assert_eq!(s.hit(120, 100, 100), None, "视口外不点");
    assert_eq!(s.hit(120, -1, 100), None);
}

#[test]
fn spec_命中_行外与负x() {
    let s = tree();
    assert_eq!(s.hit(120, 500, 172), None, "行表之下");
    assert_eq!(s.hit(-1, 10, 172), None, "页左之外");
    assert_eq!(s.hit(120, 10, 0), None, "视口高 0 无可点");
    let empty = FileTreeState::new("空");
    assert_eq!(empty.hit(10, 10, 172), None, "空树");
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
    // 起步那一帧：刚体全高 = a 的子块（两行 × 86），整块压在父行下缘之上
    let frame = s.snap_at(10);
    assert_eq!(frame.rows.len(), 4);
    assert_eq!(frame.total_h, 344);
    assert_eq!(frame.epoch, s.epoch);
    let d = frame.drawer.clone().unwrap();
    assert_eq!(d.path, "a");
    assert!(d.opening);
    assert_eq!((d.full_h, d.dy), (172, -172));
    // 中途帧（半程已过大半——ease-out）与收尽帧
    let mid = s.snap_at(130).drawer.unwrap().dy;
    assert!(mid > -86 && mid < 0, "半程已过大半: {mid}");
    assert!(s.snap_at(60).drawer.unwrap().dy > -172);
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
    assert_eq!(s2.snap().total_h, 172);
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
    // 抽屉刚体：200ms 起步 172 高，440ms 到位
    let d = st.snap_at(200).drawer.unwrap();
    assert_eq!((d.full_h, d.dy), (172, -172));
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
