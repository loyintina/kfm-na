//! parser_page_spec.rs — 解析页 tmux 插件核考题（A 档：几何/命中/状态机）
//!
//! 判卷维度：
//! - layout（2026-09-21 三区排布 v2，用户拍板「tmux 竖排放右下角常驻」）：
//!   卡 = 右下常驻槽（排布器配给：右列固定网格宽 × 钉键盘感知可视底，
//!   永不被滚出屏）；动态高 = 内容定；**竖排一行一框**（框 = 三级框行
//!   主形态双色渐变，内容只有 名字+×）；常态按钮带 = [重排][新窗]
//!   **竖排一钮一行全宽**（2026-09-21 v3 拍板：窄列并排两钮更窄）；
//!   命名态 [确定][取消]；确认态走跳框模态；**分隔线**（正渐变 c1→c2
//!   底线家变异）在会话表与按钮带之间、非交互件；**可见窗上限 6 框**
//!   （超出内部滚动：框全量发出 + scroll 平移 + list_clip 裁/判，
//!   scroll 钳制语义唯一在 layout_in）；**卡高账 = tmux_card_h 与
//!   layout_in 同一份 used_lines 账**（自报高与卡内件漂移 = 鬼影）
//! - 跳框几何：confirm_card 居中、双钮在卡内不重叠、高 3 格框行纪律
//! - hit：框命中（本体=Session / 框尾 × 带=Kill）、按钮命中、卡外=None；
//!   确认态只认跳框；**滚动后 list_clip 带外的框只显不点**；命中与
//!   涂装吃同一份 Layout（眼手同尺的本体）
//! - 状态机：set_sessions 收确认态防下标悬空；epoch 凡变更必 +1；
//!   命名/确认模式流转；scroll_by/clamp_scroll 钳 [0, max]、没变不 bump
//! - **三区双滚动账**（2026-09-21 v2，取代单页 page_scroll）：
//!   left_scroll_by/right_scroll_by 钳 [0, max]、没变不 bump、snap 带
//!   两本账（漏维 = 鬼影）；常驻槽钉底随键盘感知可视底上移（BAR-120
//!   线视口化契约：布局不吃键盘，钉底吃）
//!
//! 变异抽检方向：会话框忘全宽（窄列里半分 = 名字挤爆）、hit 框尾 × 带
//! 判据改 >（少 1px）、跳框卡外命中退化为 None、set_sessions 忘收
//! confirming、hit 漏 list_clip 闸、scroll 钳制漏 max（拖穿底）、
//! 保底两行退化为钳进池区（BAR-119 塌卡回魂）、常驻槽钉顶不钉底
//! （拍板语义反）、触摸漂移补偿条件反向/健康基线也补（BAR-145 修复臂
//! 偷换 = 命中尺反向错位）——本文件必须红。

use kfm_na::tmux_ctl::TmuxSession;
use kfm_na::ui::parser_chain;
use kfm_na::ui::parser_page::{
    self, Action, Hit, Mode, ParserPage, Status, button_action, button_labels,
};

const W: u32 = 1080;
const H: u32 = 2280;
const INSET: u32 = 0;

fn ss(names: &[&str]) -> Vec<TmuxSession> {
    names
        .iter()
        .map(|n| TmuxSession {
            name: (*n).into(),
            windows: 1,
            attached: false,
        })
        .collect()
}

// ---- layout ----

#[test]
fn spec_layout_常驻槽钉底钉右列() {
    let l = parser_page::layout(W, H, INSET, 3, Mode::Normal, 0);
    let area = parser_chain::page_area(W, H, INSET);
    // 右列 = 页全区 1/2 比例制（宪法 v3）× 右缘贴全区右缘
    assert_eq!(l.card.w, parser_chain::col_w(area.w));
    assert_eq!(l.card.x + i64::from(l.card.w), area.x + i64::from(area.w));
    // 钉键盘感知可视底（无键盘 = 页环底内缘）——永不被滚出屏
    assert_eq!(
        l.card.y + i64::from(l.card.h),
        parser_page::visible_bottom(H, INSET),
        "常驻槽必须钉可视底"
    );
    assert_eq!(l.rows.len(), 3);
    assert_eq!(l.buttons.len(), 2); // 常态 [重排][新窗]
}

#[test]
fn spec_layout_键盘感知钉底上移() {
    // BAR-120 线视口化契约：布局不吃键盘（卡高不变），常驻槽钉底吃
    // ——键盘在场可视底上移，槽跟着上移，永不被键盘盖住
    let l0 = parser_page::layout(W, H, INSET, 3, Mode::Normal, 0);
    let lk = parser_page::layout(W, H, 600, 3, Mode::Normal, 0);
    assert_eq!(lk.card.h, l0.card.h, "键盘不许重排卡内账（BAR-119 红线）");
    assert_eq!(
        lk.card.y + i64::from(lk.card.h),
        parser_page::visible_bottom(H, 600),
        "键盘在场常驻槽钉新可视底"
    );
    assert!(lk.card.y < l0.card.y, "键盘在场槽必须上移");
}

#[test]
fn spec_layout_竖排一行一框() {
    // 4 会话 = 四行一列；框高 = 3 格（宪法三级框行最小高）；框宽 =
    // 卡内全宽（窄列半分 = 名字挤爆——变异必咬）；纵序等 stride
    let l = parser_page::layout(W, H, INSET, 4, Mode::Normal, 0);
    assert_eq!(l.rows.len(), 4);
    let stride = (parser_page::BOX_H + parser_page::ROW_GAP) as i64;
    let cw = l.card.w - parser_page::CARD_PAD_H * 2;
    for (i, r) in l.rows.iter().enumerate() {
        assert_eq!(
            r.h,
            parser_page::BOX_H,
            "会话框高必须 3 格（修宪：三级框至少两行高）"
        );
        assert_eq!(r.x, l.card.x + i64::from(parser_page::CARD_PAD_H));
        assert_eq!(r.w, cw, "竖排框宽 = 卡内全宽");
        if i > 0 {
            assert_eq!(r.y, l.rows[i - 1].y + stride, "纵序等 stride");
        }
        assert!(r.x >= l.card.x, "框左出卡");
        assert!(r.x + r.w as i64 <= l.card.x + l.card.w as i64, "框右出卡");
    }
}

#[test]
fn spec_bar145_卡高容量化_行位与会话数脱钩() {
    // BAR-145 顶锚修约（2026-09-24 用户拍板，取代旧「卡高随内容长」）：
    // 底锚槽 + 动态卡高 = 名单翻动时全行位移 126px（点击行漂移的力学
    // 根源；9-23 夜实证名单 4↔5↔8 横跳）。修约：卡高吃容量不吃实际，
    // 行/分隔线/按钮对卡底钉死——会话数增减只影响列表区底部空位
    let l3 = parser_page::layout(W, H, INSET, 3, Mode::Normal, 0);
    let l4 = parser_page::layout(W, H, INSET, 4, Mode::Normal, 0);
    assert_eq!(l3.card.h, l4.card.h, "卡高必须与会话数脱钩（吃容量）");
    assert_eq!(l3.rows[0].y, l4.rows[0].y, "首行一像素不挪");
    assert_eq!(l3.rows[2].y, l4.rows[2].y, "已有行一像素不挪");
    assert_eq!(l3.divider.y, l4.divider.y, "分隔线钉死");
    assert_eq!(l3.buttons[0].y, l4.buttons[0].y, "按钮带钉死");
    let stride = (parser_page::BOX_H + parser_page::ROW_GAP) as i64;
    assert_eq!(
        l4.rows[3].y,
        l3.rows[2].y + stride,
        "新会话进列表区底部空位——接前行底 + 行距"
    );
    // 卡高账 = tmux_card_h 与 layout_in 同一份账（自报高 ≠ 卡内件 = 鬼影）
    let area = parser_chain::page_area(W, H, INSET);
    let cap = (parser_page::visible_bottom(H, 0) - area.y).max(0) as u32;
    assert_eq!(
        l4.card.h,
        parser_page::tmux_card_h(4, Mode::Normal, cap),
        "自报高与卡内布局必须同源"
    );
    assert_eq!(
        parser_page::tmux_card_h(3, Mode::Normal, cap),
        parser_page::tmux_card_h(4, Mode::Normal, cap),
        "自报高同样与会话数脱钩"
    );
}

#[test]
fn spec_bar145_命中吃屏代快照() {
    // 源码守卫钉（BAR-145 修复·眼手同尺的真义）：解析页命中路径唯一
    // 合法快照源 = 屏代烘焙快照（baked_snap）——用活体 pg.snap() 直接
    // 命中 = 名单翻动期点中没看到的名单（病灶回潮即红）
    let app = include_str!("../src/android_app.rs");
    assert!(
        app.contains("crate::ui::parser_page::baked_snap()"),
        "解析页命中必须吃屏代快照（BAR-145：活体命中 = 点中没看到的名单）"
    );
    let pp = include_str!("../src/ui/parser_page.rs");
    assert!(
        pp.contains("pub fn note_baked_snap"),
        "烘焙完成必须落屏代账（note_baked_snap 被摘 = 命中快照断供）"
    );
}

#[test]
fn spec_bar145_仪器退役闸() {
    // BAR-145 修复 2026-09-24 上机，仪器（[bake]名[]/[touch]屏代/屏代
    // 快照账）留场观察一周（用户拍板「过一周没事自己取消或让我们
    // 知道」）：2026-10-02 起本钉转红强制裁决——无再现 = 拆仪器+
    // bugs.md 结案；有再现 = 凭屏代账定罪续修
    let deadline = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_790_899_200);
    assert!(
        std::time::SystemTime::now() < deadline,
        "BAR-145 仪器观察期满（2026-10-02）——无再现则拆屏代/名单仪器并结案，有再现凭账续修"
    );
}

#[test]
fn spec_bar145_触摸漂移补偿_零不补_正补回_负不反向() {
    // BAR-145 修复臂（2026-09-26 同步证据链定罪）：唤醒后 Android 把**输入侧**
    // 窗顶错记到状态栏之下（捕获器实测 winTop=135，健康基线 0），显示侧全屏——
    // 用户按显示空间 y=1787 点 nz，na 收到窗口空间 1653，整屏命中上移一行。
    // 补偿 = 窗口空间 + 窗顶 = 显示空间；健康基线必须零影响（否则治好一行、
    // 换来全屏错位）。变异方向：条件反向 / 无条件加 / 零值也加，本钉必红。
    use kfm_na::insets::drift_compensate;
    assert_eq!(
        drift_compensate(1653.0, 0),
        1653.0,
        "健康基线 winTop=0 零影响（y 一像素不动）"
    );
    assert_eq!(
        drift_compensate(1653.0, 135),
        1788.0,
        "发病 winTop=135 补回显示空间（nz 视觉中心 1787）"
    );
    assert_eq!(
        drift_compensate(1000.0, -135),
        1000.0,
        "负窗顶不反向（宁不动，不许把用户的手往反方向推）"
    );
    assert_eq!(
        drift_compensate(1481.0, 1),
        1482.0,
        "1px 也要补（无下限档）"
    );
    assert_eq!(drift_compensate(0.0, 135), 135.0, "屏顶触摸同样吃补偿");
}

#[test]
fn spec_bar145_触摸入口吃补偿_注入通道不吃() {
    // 眼手同尺的边界契约（BAR-145 修复臂）：判卷尺（几何/渲染）全在显示
    // 空间，真手指进来的是**窗口空间**——补偿只许做在真触摸入口；通道八
    // 注入的坐标本就写自显示空间（脚本按看到的画面点），进门再补就反推。
    let app = include_str!("../src/android_app.rs");
    let arm = app
        .split("WindowEvent::Touch(touch) =>")
        .nth(1)
        .expect("window_event 里没有 Touch 臂（真触摸入口锚点被摘）");
    let arm = &arm[..arm
        .find("// IME 事件链")
        .expect("Touch 臂尾锚点（IME 事件链）被摘")];
    assert!(
        arm.contains("crate::insets::drift_compensate"),
        "真触摸入口必须过窗顶漂移补偿（BAR-145 修复臂被摘 = 命中继续上移一行）"
    );
    assert!(
        arm.contains("sample_touch_win_top"),
        "起手必须活读窗顶（缓存会拿愈合前的旧值把用户的手反向推）"
    );
    let inj = app
        .split("fn drain_touch_in")
        .nth(1)
        .expect("通道八注入抽干被摘");
    let inj = &inj[..inj.find("\n    fn ").unwrap_or(inj.len())];
    assert!(
        !inj.contains("drift_compensate"),
        "注入通道不许连带补偿（脚本坐标写自显示空间，再补就是反推）"
    );
}

#[test]
fn spec_bar145_resume重贴沉浸式标志() {
    // 治本臂（2026-09-26）：唤醒/焦点重协商时点重贴沉浸式标志——迫使系统
    // 重算窗口 frame，输入/显示两侧快照同源。摘掉 = 只留补偿一条腿。
    let java = include_str!("../android/java/dev/kfm/na/MainActivity.java");
    assert!(
        java.contains("private void reapplyImmersive()"),
        "治本臂（重贴沉浸式标志）被摘"
    );
    let resume = java
        .split("protected void onResume()")
        .nth(1)
        .expect("onResume 被摘");
    assert!(
        resume.contains("reapplyImmersive()"),
        "onResume 必须重贴沉浸式标志（熄屏→解锁复现钥匙的时点）"
    );
    let focus = java
        .split("public void onWindowFocusChanged(boolean hasFocus)")
        .nth(1)
        .expect("onWindowFocusChanged 被摘");
    let focus = &focus[..focus
        .find("private void reapplyImmersive()")
        .expect("reapplyImmersive 定义锚点被摘")];
    assert!(
        focus.contains("reapplyImmersive()"),
        "焦点重获必须重贴沉浸式标志（IME 召收/唤窗后的重协商时点）"
    );
}

#[test]
fn spec_bar119_挤压不吃卡内账_钉底独吞位移() {
    // BAR-119 修约升级（2026-09-24 BAR-145 顶锚修约，用户拍板，取代
    // 「保底两行」旧修约）：旧设计在极端挤压下把卡塌到两行使下半
    // 可见——那是动态卡高时代对「卡高随内容」的补救；卡高容量化后
    // 卡内账与压力全脱钩（BAR-119 红线纯度更高）：挤压再狠，卡高/
    // 行位/按钮一像素不动，唯钉底随可视底位移，出屏部分归键盘遮盖
    // 与页缘裁剪带（既有机制），不许塌行自残
    let squeeze = H; // 键盘把 bottom_inset 顶到整屏高的极端相
    let l0 = parser_page::layout(W, H, INSET, 4, Mode::Normal, 0);
    let l = parser_page::layout(W, H, squeeze, 4, Mode::Normal, 0);
    assert_eq!(l.card.h, l0.card.h, "挤压不许动卡高（卡内账不吃压力）");
    assert_eq!(l.rows.len(), 4);
    for (a, b) in l.rows.iter().zip(l0.rows.iter()) {
        assert_eq!(
            a.y - l.card.y,
            b.y - l0.card.y,
            "行相对卡顶一像素不动（位移唯钉底）"
        );
    }
    assert_eq!(
        l.divider.y - l.card.y,
        l0.divider.y - l0.card.y,
        "分隔线相对卡顶不动"
    );
    assert_eq!(
        l.card.y + i64::from(l.card.h),
        parser_page::visible_bottom(H, squeeze),
        "极端相照钉新可视底（出屏归遮盖/裁剪，不塌行）"
    );
    // 可见窗容量账不变：1 会话可见 1 行、6 会话满窗 6 行
    let l1 = parser_page::layout(W, H, squeeze, 1, Mode::Normal, 0);
    assert_eq!(l1.visible_rows, 1);
    let l6 = parser_page::layout(W, H, squeeze, 6, Mode::Normal, 0);
    assert_eq!(l6.visible_rows, 6, "容量窗满六行——挤压不许吞");
}

#[test]
fn spec_layout_模式按钮带() {
    assert_eq!(button_labels(Mode::Normal), ["重排", "新窗"]);
    assert_eq!(button_labels(Mode::Naming), ["确定", "取消"]);
    assert!(button_labels(Mode::Confirming).is_empty()); // 确认走跳框，卡区无钮
    let ln = parser_page::layout(W, H, INSET, 1, Mode::Naming, 0);
    assert!(ln.naming.is_some());
    assert_eq!(ln.buttons.len(), 2);
    // 确认态 = 卡按 Normal 几何（跳框模态不占卡高；v3 竖排后钮带
    // 几何照常排位——涂装吃 button_labels(Confirming) 空列不画，
    // 命中模态臂屏蔽不可点）
    let lc = parser_page::layout(W, H, INSET, 1, Mode::Confirming, 0);
    let lnor = parser_page::layout(W, H, INSET, 1, Mode::Normal, 0);
    assert!(lc.naming.is_none());
    assert_eq!(
        lc.buttons.len(),
        lnor.buttons.len(),
        "确认态钮带几何 = 常态（模态底下零跳变）"
    );
    assert_eq!(lc.card.h, lnor.card.h, "确认态卡高 = 常态（跳框不占卡高）");
}

#[test]
fn spec_layout_跳框几何() {
    let card = parser_page::confirm_card(W, H);
    // 居中
    assert_eq!(card.x, (W as i64 - card.w as i64) / 2);
    assert_eq!(card.y, (H as i64 - card.h as i64) / 2);
    let btns = parser_page::confirm_buttons(&card);
    // 双钮：在卡内、同 y、不重叠、高 3 格
    assert_eq!(btns[0].h, parser_page::BTN_H);
    assert_eq!(btns[0].y, btns[1].y);
    assert!(btns[1].x >= btns[0].x + btns[0].w as i64, "双钮重叠");
    for b in &btns {
        assert!(b.x >= card.x && b.x + b.w as i64 <= card.x + card.w as i64);
        assert!(b.y >= card.y && b.y + b.h as i64 <= card.y + card.h as i64);
    }
    // 小屏兜底：卡宽钳制不出屏
    let small = parser_page::confirm_card(400, 800);
    assert!(small.x >= 0 && small.x + small.w as i64 <= 400);
}

#[test]
fn spec_layout_按钮竖排全宽互不重叠() {
    // v3（2026-09-21 用户拍板）：钮带竖排一钮一行全宽——窄列里并排
    // 两钮更窄的观感病灶就此拔掉（变异：横排回潮/半宽回潮必须咬）
    for mode in [Mode::Normal, Mode::Naming] {
        let l = parser_page::layout(W, H, INSET, 2, mode, 0);
        let cw = l.card.w - parser_page::CARD_PAD_H * 2;
        for (i, b) in l.buttons.iter().enumerate() {
            assert_eq!(b.x, l.card.x + i64::from(parser_page::CARD_PAD_H));
            assert_eq!(b.w, cw, "{mode:?} 钮{i} 必须全宽（竖排）");
            assert!(
                b.y + b.h as i64 <= l.card.y + l.card.h as i64,
                "{mode:?} 钮{i} 底出卡"
            );
            if i > 0 {
                let prev = &l.buttons[i - 1];
                assert_eq!(
                    b.y,
                    prev.y + (prev.h + parser_page::ROW_GAP) as i64,
                    "{mode:?} 钮{i} 必须接前钮底 + 一行距（纵序等 stride）"
                );
            }
        }
    }
}

#[test]
fn spec_layout_超高截窗内部滚动() {
    // 小屏高塞 100 会话：卡高封顶常驻槽可用高，框全量发出（滚动可见性
    // 靠 list_clip），scroll_max > 0 可滚
    let l = parser_page::layout(600, 900, INSET, 100, Mode::Normal, 0);
    let vb = parser_page::visible_bottom(900, INSET);
    let area = parser_chain::page_area(600, 900, INSET);
    let cap = (vb - area.y).max(0) as u32;
    assert!(l.card.h <= cap, "卡高不许越常驻槽可用高");
    assert_eq!(l.rows.len(), 100, "框全量发出，滚动裁窗不截表");
    assert!(l.scroll_max > 0, "内容超高 = 可滚");
    assert!(l.visible_rows <= 6);
    assert!(l.visible_rows >= 1);
}

#[test]
fn spec_layout_上限六框与滚动几何() {
    // 10 会话 = 10 行：可见窗硬上限 6 行（6 框），卡高按 6 行账；
    // scroll_max = 超出的 4 行；scroll 平移框 y；超 max 钳到 max
    let l = parser_page::layout(W, H, INSET, 10, Mode::Normal, 0);
    let stride = (parser_page::BOX_H + parser_page::ROW_GAP) as i64;
    assert_eq!(l.rows.len(), 10);
    assert_eq!(l.visible_rows, 6, "可见窗容量 = 6 行 × 1 列");
    assert_eq!(l.scroll_max, 4 * stride, "10 行内容 − 6 行窗 = 4 行可滚");
    let visible_h = 6 * stride - parser_page::ROW_GAP as i64;
    assert_eq!(l.list_clip.1 - l.list_clip.0, visible_h);
    assert_eq!(l.rows[0].y, l.list_clip.0, "scroll=0 首框贴窗顶");
    // scroll = 一行：首框移出窗上沿，第二框贴窗顶
    let ls = parser_page::layout(W, H, INSET, 10, Mode::Normal, stride);
    assert_eq!(ls.rows[0].y, ls.list_clip.0 - stride);
    assert_eq!(ls.rows[1].y, ls.list_clip.0);
    // 超 max 钳制：scroll=9999 几何 ≡ scroll=max（钳制语义唯一在 layout_in）
    let lc = parser_page::layout(W, H, INSET, 10, Mode::Normal, 9999);
    let lm = parser_page::layout(W, H, INSET, 10, Mode::Normal, l.scroll_max);
    assert_eq!(lc.rows[0].y, lm.rows[0].y);
    // 卡高与会话数脱钩（BAR-145 顶锚修约）：2 会话卡 = 6 框满窗卡
    // （同容量），空位在列表区底部；一屏装得下 = 不可滚
    let l2 = parser_page::layout(W, H, INSET, 2, Mode::Normal, 0);
    assert_eq!(l2.card.h, l.card.h, "卡高吃容量不吃实际（BAR-145）");
    assert_eq!(l2.scroll_max, 0, "一屏装得下 = 不可滚");
}

#[test]
fn spec_layout_分隔线在表与钮之间() {
    let l = parser_page::layout(W, H, INSET, 2, Mode::Normal, 0);
    assert_eq!(l.divider.h, parser_page::DIVIDER_H);
    assert!(l.divider.y >= l.list_clip.1, "分隔线压在会话表窗内");
    assert!(
        l.divider.y + l.divider.h as i64 <= l.buttons[0].y,
        "分隔线淹到按钮带"
    );
    let cw = l.card.w - parser_page::CARD_PAD_H * 2;
    assert_eq!(l.divider.w, cw, "分隔线与卡内区同宽");
    // 命中：分隔线不是交互件——点它 = None（不是 Session 也不是 Button）
    let my = l.divider.y + l.divider.h as i64 / 2;
    assert_eq!(
        parser_page::hit(
            &l,
            l.divider.x + l.divider.w as i64 / 2,
            my,
            W,
            H,
            Mode::Normal
        ),
        None
    );
}

#[test]
fn spec_hit_滚动后裁剪带外不可点() {
    let stride = (parser_page::BOX_H + parser_page::ROW_GAP) as i64;
    let l = parser_page::layout(W, H, INSET, 10, Mode::Normal, stride);
    let hit = |x: i64, y: i64| parser_page::hit(&l, x, y, W, H, Mode::Normal);
    // rows[0] 整体滚出窗上沿：点它原位（现在在卡头区）≠ Session(0)
    let b0 = &l.rows[0];
    assert_ne!(
        hit(b0.x + 10, b0.y + b0.h as i64 / 2),
        Some(Hit::Session(0)),
        "裁窗外的框不许点"
    );
    // rows[1] 贴窗顶 = 窗内首框，可点
    let b1 = &l.rows[1];
    assert_eq!(hit(b1.x + 10, b1.y + 2), Some(Hit::Session(1)));
    // rows[7]（第 8 行）在窗下沿外（scroll=一行后可见 1..=6）：不可点
    let b7 = &l.rows[7];
    assert_eq!(hit(b7.x + 10, b7.y + 2), None, "窗下沿外的框不许点");
}

#[test]
fn spec_滚动状态机() {
    let mut p = ParserPage::new();
    // scroll_by：clamp [0, max]，变了才 bump
    let e0 = p.epoch();
    p.scroll_by(-50, 200); // 下钳 0
    assert_eq!(p.scroll(), 0);
    assert_eq!(p.epoch(), e0, "钳住没变不许 bump");
    p.scroll_by(120, 200);
    assert_eq!(p.scroll(), 120);
    assert!(p.epoch() > e0);
    p.scroll_by(120, 200); // 上钳 max
    assert_eq!(p.scroll(), 200);
    // clamp_scroll：行表变少 max 缩
    p.clamp_scroll(80);
    assert_eq!(p.scroll(), 80);
    p.clamp_scroll(80);
    let e1 = p.epoch();
    p.clamp_scroll(80);
    assert_eq!(p.epoch(), e1, "同值钳不许 bump");
}

// ---- hit ----

#[test]
fn spec_hit_框本体与kill带() {
    let l = parser_page::layout(W, H, INSET, 4, Mode::Normal, 0);
    let hit = |x: i64, y: i64| parser_page::hit(&l, x, y, W, H, Mode::Normal);
    let b0 = &l.rows[0];
    // 框本体中点 = Session(0)
    assert_eq!(
        hit(b0.x + 10, b0.y + b0.h as i64 / 2),
        Some(Hit::Session(0))
    );
    // 框尾 × 带内 = Kill(0)
    let kx = b0.x + b0.w as i64 - parser_page::KILL_W as i64 + 2;
    assert_eq!(hit(kx, b0.y + b0.h as i64 / 2), Some(Hit::Kill(0)));
    // × 带左缘界上 = Kill（边界归属钉死：>= 含左缘）
    assert_eq!(hit(kx - 2, b0.y + b0.h as i64 / 2), Some(Hit::Kill(0)));
    // × 带左缘外 1px 仍是 Session
    assert_eq!(hit(kx - 3, b0.y + b0.h as i64 / 2), Some(Hit::Session(0)));
    // 第二行框 = Session(1)，其 × 带 = Kill(1)
    let b1 = &l.rows[1];
    assert_eq!(hit(b1.x + 10, b1.y + 2), Some(Hit::Session(1)));
    let kx1 = b1.x + b1.w as i64 - parser_page::KILL_W as i64 + 2;
    assert_eq!(hit(kx1, b1.y + 2), Some(Hit::Kill(1)));
}

#[test]
fn spec_hit_按钮与卡外() {
    let l = parser_page::layout(W, H, INSET, 2, Mode::Normal, 0);
    let hit = |x: i64, y: i64| parser_page::hit(&l, x, y, W, H, Mode::Normal);
    let b0 = &l.buttons[0];
    assert_eq!(
        hit(b0.x + b0.w as i64 / 2, b0.y + b0.h as i64 / 2),
        Some(Hit::Button(0))
    );
    let b1 = &l.buttons[1];
    assert_eq!(hit(b1.x + 2, b1.y + 2), Some(Hit::Button(1)));
    // 卡外（池区空白 / 屏外）= None
    assert_eq!(hit(0, 0), None);
    assert_eq!(hit(l.card.x + 2, l.card.y + l.card.h as i64 + 200), None);
}

#[test]
fn spec_hit_确认态只认跳框() {
    let l = parser_page::layout(W, H, INSET, 2, Mode::Confirming, 0);
    let hit = |x: i64, y: i64| parser_page::hit(&l, x, y, W, H, Mode::Confirming);
    let card = parser_page::confirm_card(W, H);
    let btns = parser_page::confirm_buttons(&card);
    // 双钮
    assert_eq!(
        hit(btns[0].x + 5, btns[0].y + 5),
        Some(Hit::ModalOk),
        "左钮 = 确定关闭"
    );
    assert_eq!(
        hit(btns[1].x + 5, btns[1].y + 5),
        Some(Hit::ModalCancel),
        "右钮 = 取消"
    );
    // 卡内非钮区 = 吞（None，不许穿透）
    assert_eq!(hit(card.x + 5, card.y + 5), None);
    // 卡外 = Dismiss（点框外取消）；哪怕点在背后的会话框上也一样
    // （BAR-145 卡加高后行 0 与居中跳框取消钮几何重叠——取跳框之下
    // 的行 1，屏蔽语义不变：点在背景卡件上 = 框外取消，不许穿透）
    assert_eq!(hit(2, 2), Some(Hit::ModalDismiss));
    let b1 = &l.rows[1];
    assert_eq!(
        hit(b1.x + 10, b1.y + b1.h as i64 / 2),
        Some(Hit::ModalDismiss),
        "模态在时卡区命中必须屏蔽（点在框上也只算框外取消）"
    );
}

#[test]
fn spec_button_action_全模式映射() {
    assert_eq!(button_action(Mode::Normal, 0), Some(Action::Reflow));
    assert_eq!(button_action(Mode::Normal, 1), Some(Action::New));
    assert_eq!(button_action(Mode::Naming, 0), Some(Action::NamingOk));
    assert_eq!(button_action(Mode::Naming, 1), Some(Action::NamingCancel));
    assert_eq!(button_action(Mode::Normal, 2), None); // 刷新钮已撤
    assert_eq!(button_action(Mode::Confirming, 0), None); // 确认走跳框
}

// ---- 状态机 ----

#[test]
fn spec_epoch_凡变更必进位() {
    let mut p = ParserPage::new();
    let e0 = p.epoch();
    p.set_loading();
    assert!(p.epoch() > e0);
    let e1 = p.epoch();
    p.set_sessions(ss(&["a", "b"]));
    assert!(p.epoch() > e1);
    let e2 = p.epoch();
    p.set_error("x".into());
    assert!(p.epoch() > e2);
    let e3 = p.epoch();
    p.begin_naming();
    assert!(p.epoch() > e3);
    let e4 = p.epoch();
    p.naming_push("甲");
    assert!(p.epoch() > e4);
    let e5 = p.epoch();
    p.naming_pop();
    assert!(p.epoch() > e5);
    // 无变更不 bump（set_attached 同值重喂）
    p.set_attached(Some("a".into()));
    let e6 = p.epoch();
    p.set_attached(Some("a".into()));
    assert_eq!(p.epoch(), e6, "同值 attached 重喂不许 bump（sig 鬼影）");
}

#[test]
fn spec_set_sessions_收确认态防下标悬空() {
    let mut p = ParserPage::new();
    p.set_sessions(ss(&["a", "b", "c"]));
    p.begin_confirm(2);
    assert_eq!(p.mode(), Mode::Confirming);
    assert_eq!(p.confirm_target(), Some("c".into()));
    // 行表刷新（可能行数变少）——确认态必须收，不许指到别的会话上
    p.set_sessions(ss(&["a"]));
    assert_eq!(p.mode(), Mode::Normal);
    assert_eq!(p.confirm_target(), None);
}

#[test]
fn spec_命名流转() {
    let mut p = ParserPage::new();
    assert_eq!(p.mode(), Mode::Normal);
    p.begin_naming();
    assert_eq!(p.mode(), Mode::Naming);
    assert!(p.naming_active());
    p.naming_push("work");
    assert_eq!(p.naming_take(), Some("work".into()));
    assert_eq!(p.mode(), Mode::Normal);
    assert!(!p.naming_active());
    // 取消路径
    p.begin_naming();
    p.naming_push("x");
    p.cancel_naming();
    assert_eq!(p.mode(), Mode::Normal);
}

#[test]
fn spec_确认越界不开() {
    let mut p = ParserPage::new();
    p.set_sessions(ss(&["a"]));
    p.begin_confirm(9); // 下标越界 = 不开确认态（杀错会话不可挽回）
    assert_eq!(p.mode(), Mode::Normal);
}

#[test]
fn spec_status_流转() {
    let mut p = ParserPage::new();
    assert_eq!(p.status(), &Status::Idle);
    p.set_loading();
    assert_eq!(p.status(), &Status::Loading);
    p.set_sessions(ss(&[]));
    assert_eq!(p.status(), &Status::Ready);
    p.set_error("超时".into());
    assert_eq!(p.status(), &Status::Error("超时".into()));
}

// ---- 三区双滚动账（2026-09-21 v2，取代单页 page_scroll）----

#[test]
fn spec_可视底公式() {
    // 可视底 = 屏高 − 底 inset（键盘+输入栏带）− 页环边距 − 环底缘厚
    // = 页环底内缘（BAR-121 层级律：框压内容，内容止于环底内缘）
    assert_eq!(
        parser_page::visible_bottom(2280, 200),
        2280 - 200
            - i64::from(kfm_na::termview::AI_PAGE_FRAME_MARGIN)
            - i64::from(kfm_na::termview::AI_PAGE_FRAME_W)
    );
}

#[test]
fn spec_区滚动状态机() {
    let mut p = ParserPage::new();
    // 左账：clamp [0, max]，变了才 bump
    let e0 = p.epoch();
    p.left_scroll_by(-50, 200); // 下钳 0
    assert_eq!(p.left_scroll(), 0);
    assert_eq!(p.epoch(), e0, "钳住没变不许 bump");
    p.left_scroll_by(120, 200);
    assert_eq!(p.left_scroll(), 120);
    assert!(p.epoch() > e0);
    p.left_scroll_by(120, 200); // 上钳 max
    assert_eq!(p.left_scroll(), 200);
    p.clamp_left_scroll(80);
    assert_eq!(p.left_scroll(), 80);
    let e1 = p.epoch();
    p.clamp_left_scroll(80);
    assert_eq!(p.epoch(), e1, "同值钳不许 bump");
    // 右账：同一纪律，与左账互不沾（两本账独立）
    p.right_scroll_by(60, 100);
    assert_eq!(p.right_scroll(), 60);
    assert_eq!(p.left_scroll(), 80, "右账动不许带左账");
    p.clamp_right_scroll(30);
    assert_eq!(p.right_scroll(), 30);
    assert_eq!(p.left_scroll(), 80);
    // snap 必须带两本账（涂装 sig 代际同源——漏维 = 鬼影）
    assert_eq!(p.snap().left_scroll, 80);
    assert_eq!(p.snap().right_scroll, 30);
}
