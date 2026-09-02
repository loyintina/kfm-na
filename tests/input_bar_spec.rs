//! tests/input_bar_spec.rs — A 档考题：全局输入栏状态核（src/input_bar.rs）
//!
//! 契约真相源：docs/active/ai-presence.md §二 常驻 chrome 一 / §五 焦点二态。
//! 判卷点：焦点二态 / 文本缓冲（UTF-8 安全退格）/ enter 取文发送 /
//! 几何命中（文本区 vs 发送钮 vs 栏外，键盘 inset 跟手）。
//! 纪律：先验证红，答案生成到绿，绿后变异抽检。本文件是考题，生成器不许改。

use kfm_na::input_bar::{BarHit, HEIGHT_PX, InputBarState, hit, in_bar};

// ========== 焦点二态 ==========

#[test]
fn focus_two_states() {
    let bar = InputBarState::new();
    assert!(!bar.snap().focused, "出生 = 失焦");
    bar.focus();
    assert!(bar.snap().focused);
    assert!(bar.is_focused(), "is_focused 与 snap 同源");
    bar.unfocus();
    assert!(!bar.snap().focused);
}

// ========== 文本缓冲 ==========

#[test]
fn insert_appends_mixed_cjk() {
    let bar = InputBarState::new();
    bar.insert_text("hello");
    bar.insert_text("你好");
    assert_eq!(bar.snap().text, "hello你好");
}

#[test]
fn backspace_removes_whole_char_not_byte() {
    let bar = InputBarState::new();
    bar.insert_text("中a");
    bar.backspace();
    assert_eq!(bar.snap().text, "中", "退格删 a");
    bar.backspace();
    assert_eq!(bar.snap().text, "", "退格删整个「中」，不是撕字节");
    bar.backspace(); // 空串退格不许炸
    assert_eq!(bar.snap().text, "");
}

// ========== enter 取文发送 ==========

#[test]
fn enter_empty_yields_none() {
    let bar = InputBarState::new();
    assert_eq!(bar.enter(), None, "空文本 enter = 无发送");
}

#[test]
fn enter_takes_text_and_clears_keeps_focus() {
    let bar = InputBarState::new();
    bar.focus();
    bar.insert_text("帮我看看启动速度");
    assert_eq!(bar.enter().as_deref(), Some("帮我看看启动速度"));
    let snap = bar.snap();
    assert_eq!(snap.text, "", "发送后清空");
    assert!(snap.focused, "发送后保持聚焦（手机聊天惯例）");
}

// ========== 几何命中（栏 = 屏底 - 键盘 inset 之上一条带） ==========

const W: u32 = 1080;
const H: u32 = 2400;

#[test]
fn geometry_band_follows_ime() {
    use kfm_na::input_bar::HEIGHT_PX;
    // 键盘收：栏贴屏底
    assert!(in_bar(f64::from(H - 1), H, 0, HEIGHT_PX));
    assert!(
        !in_bar(f64::from(H - HEIGHT_PX - 1), H, 0, HEIGHT_PX),
        "栏上方是终端区"
    );
    // 键盘弹起 900：栏跟手上浮
    assert!(in_bar(f64::from(H - 900 - 1), H, 900, HEIGHT_PX));
    assert!(!in_bar(
        f64::from(H - 900 - HEIGHT_PX - 1),
        H,
        900,
        HEIGHT_PX
    ));
    assert!(
        !in_bar(f64::from(H - 100), H, 900, HEIGHT_PX),
        "被键盘盖住的屏底不算栏内"
    );
}

#[test]
fn hit_field_vs_send() {
    use kfm_na::input_bar::{HEIGHT_PX, SEND_W_PX};
    let y = f64::from(H) - f64::from(HEIGHT_PX) / 2.0;
    assert_eq!(hit(100.0, y, W, H, 0, HEIGHT_PX), Some(BarHit::Field));
    assert_eq!(
        hit(f64::from(W - SEND_W_PX) + 10.0, y, W, H, 0, HEIGHT_PX),
        Some(BarHit::Send),
        "右端固定宽 = 发送钮"
    );
    assert_eq!(hit(100.0, 100.0, W, H, 0, HEIGHT_PX), None, "栏外 = None");
}

#[test]
fn geometry_degenerate_screen_no_panic() {
    // 坏几何（屏比栏矮/ inset 比屏大）不许 panic 不许乱判
    assert_eq!(hit(10.0, 10.0, 100, 50, 0, HEIGHT_PX), None);
    assert!(!in_bar(10.0, 50, 9999, HEIGHT_PX));
}

// ========== submit：取文 + 推进发送口（人/AI 同一路径） ==========

#[test]
fn submit_without_sender_takes_text_only() {
    let bar = InputBarState::new();
    bar.insert_text("没装出口也照发");
    assert_eq!(bar.submit().as_deref(), Some("没装出口也照发"));
    assert_eq!(bar.snap().text, "");
}

#[test]
fn submit_delivers_to_installed_sender() {
    let bar = InputBarState::new();
    let got = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let got2 = got.clone();
    bar.install_sender(std::sync::Arc::new(move |t| got2.lock().unwrap().push(t)));
    assert_eq!(bar.submit(), None, "空栏 submit = None 不派送");
    bar.insert_text("第一句");
    bar.insert_text("中文");
    assert_eq!(bar.submit().as_deref(), Some("第一句中文"));
    assert_eq!(got.lock().unwrap().as_slice(), &["第一句中文".to_string()]);
    assert_eq!(bar.snap().text, "", "派送后清空");
}

// ========== 通道十一解析（gate bar-inject 脚本行，钉死） ==========

#[test]
fn bar_inject_parse() {
    use kfm_na::gate::{BarCmd, parse_bar_line};
    assert_eq!(parse_bar_line("focus"), Some(Ok(BarCmd::Focus)));
    assert_eq!(parse_bar_line("unfocus"), Some(Ok(BarCmd::Unfocus)));
    assert_eq!(parse_bar_line("backspace"), Some(Ok(BarCmd::Backspace)));
    assert_eq!(parse_bar_line("clear"), Some(Ok(BarCmd::Clear)));
    assert_eq!(parse_bar_line("submit"), Some(Ok(BarCmd::Submit)));
    assert_eq!(
        parse_bar_line("text 发给 AI：带空格 和中文"),
        Some(Ok(BarCmd::Text("发给 AI：带空格 和中文".to_string()))),
        "text 原文照收（空格保留）"
    );
    assert_eq!(parse_bar_line(""), None);
    assert_eq!(parse_bar_line("# 注释"), None);
    assert!(
        matches!(parse_bar_line("text"), Some(Err(_))),
        "text 必须带内容"
    );
    assert!(matches!(parse_bar_line("胡说"), Some(Err(_))));
}

#[test]
fn bar_inject_script_parse() {
    use kfm_na::gate::{BarCmd, parse_bar_script};
    let (cmds, errs) = parse_bar_script("text 第一句\n# 注释\n胡说\nsubmit\n\nfocus");
    assert_eq!(
        cmds,
        vec![
            BarCmd::Text("第一句".to_string()),
            BarCmd::Submit,
            BarCmd::Focus
        ]
    );
    assert_eq!(errs.len(), 1, "坏行进清单（空行/注释不计）");
}

// ========== textarea 长高几何（2026-08-31 移动端全量复刻拍板） ==========
// 判卷点:行数→带高单调、MAX_LINES 封顶;长高后命中带跟上(眼手同尺)

#[test]
fn height_for_lines_monotonic_capped() {
    use kfm_na::input_bar::{MAX_LINES, height_for_lines};
    let h1 = height_for_lines(1);
    let h2 = height_for_lines(2);
    let h3 = height_for_lines(MAX_LINES);
    assert!(h1 < h2 && h2 < h3, "带高必须随行数单调涨");
    assert_eq!(height_for_lines(MAX_LINES + 5), h3, "超高必须封顶");
    assert_eq!(height_for_lines(0), h1, "0 行按 1 行计(空栏也是一行高)");
}

#[test]
fn hit_band_grows_with_lines() {
    use kfm_na::input_bar::{BarHit, height_for_lines, hit, in_bar};
    let h3 = height_for_lines(3);
    let y_top = f64::from(H) - f64::from(h3) + 5.0;
    assert!(in_bar(y_top, H, 0, h3), "长高后命中带必须跟上(眼手同尺)");
    assert_eq!(hit(100.0, y_top, W, H, 0, h3), Some(BarHit::Field));
    assert!(
        !in_bar(y_top, H, 0, height_for_lines(1)),
        "单行高时该位置还在终端区——尺必须跟当前行数走"
    );
}

// ========== 光标插入点 + 定位柄（2026-08-31 用户指认浏览器控件行为） ==========
// 判卷点:插入点围绕 cursor 转(中文不撕字节)/退格删 cursor 前一字/
// 点按定位钳边界/定位柄生命周期(定位亮,打字·清空·发送收)

#[test]
fn cursor_insert_mid_string_not_byte_tear() {
    let bar = InputBarState::new();
    bar.insert_text("中文abc");
    bar.set_cursor(1); // 「中」之后
    assert_eq!(bar.cursor(), 1);
    bar.insert_text("X");
    assert_eq!(bar.snap().text, "中X文abc", "插入点在中间,不是撕字节追加");
    assert_eq!(bar.cursor(), 2, "cursor 指下一个字的落点");
}

#[test]
fn cursor_backspace_deletes_before_cursor() {
    let bar = InputBarState::new();
    bar.insert_text("中文");
    bar.set_cursor(1);
    bar.backspace();
    assert_eq!(bar.snap().text, "文", "删的是 cursor 前一个字");
    assert_eq!(bar.cursor(), 0);
    bar.backspace();
    assert_eq!(bar.snap().text, "文", "cursor=0 无可删,no-op 不炸");
}

#[test]
fn cursor_set_clamps_and_handle_lifecycle() {
    let bar = InputBarState::new();
    bar.insert_text("abc");
    bar.set_cursor(99);
    assert_eq!(bar.cursor(), 3, "越界钳到末尾");
    assert!(bar.snap().handle, "点按定位 → 定位柄亮");
    bar.insert_text("d");
    assert!(!bar.snap().handle, "打字收起定位柄");
    bar.set_cursor(0);
    assert!(bar.snap().handle);
    bar.clear();
    assert_eq!(bar.cursor(), 0);
    assert!(!bar.snap().handle, "清空复位定位柄");
}

#[test]
fn cursor_submit_resets() {
    let bar = InputBarState::new();
    bar.insert_text("待发送");
    bar.set_cursor(2);
    assert_eq!(bar.submit().as_deref(), Some("待发送"));
    let snap = bar.snap();
    assert_eq!(snap.text, "");
    assert_eq!(bar.cursor(), 0, "发送后光标复位");
    assert!(!snap.handle);
}

// ========== IME 组合态（2026-09-01 编辑对齐第 1 批：拼音预编辑入栏） ==========
// 判卷点:display_text 拼接单源/组合尾退格/finish 落字/commit 取代组合/
// 点按先收组合/清空发送不复读半截拼音

#[test]
fn composing_display_text_拼接单源() {
    let bar = InputBarState::new();
    bar.insert_text("你好");
    bar.set_cursor(1); // 「你」后
    bar.set_composing("pinyin");
    let snap = bar.snap();
    assert_eq!(snap.composing, "pinyin");
    assert_eq!(
        kfm_na::input_bar::InputBarState::display_text(&snap),
        "你pinyin好",
        "组合文本插在光标处显示"
    );
    bar.set_composing("");
    assert_eq!(bar.snap().composing, "", "空串 = 组合清空");
    let snap = bar.snap();
    assert_eq!(
        kfm_na::input_bar::InputBarState::display_text(&snap),
        "你好"
    );
}

#[test]
fn composing_backspace_删组合尾() {
    let bar = InputBarState::new();
    bar.insert_text("你");
    bar.set_composing("pin");
    bar.backspace();
    assert_eq!(bar.snap().composing, "pi", "组合态退格删拼音尾字母");
    bar.backspace();
    bar.backspace();
    assert_eq!(bar.snap().composing, "", "组合删空 = 清态");
    bar.backspace();
    assert_eq!(bar.snap().text, "", "组合没了才退格删已上屏字(光标在其后)");
    assert_eq!(bar.cursor(), 0);
}

#[test]
fn composing_finish_落字跟进() {
    let bar = InputBarState::new();
    bar.insert_text("你");
    bar.set_composing("hao");
    bar.finish_composing();
    let snap = bar.snap();
    assert_eq!(snap.text, "你hao", "组合文本落为真字");
    assert_eq!(snap.composing, "");
    assert_eq!(bar.cursor(), 4, "光标跟进到落字之后");
    bar.finish_composing();
    assert_eq!(snap.text, "你hao", "无组合态 finish = no-op");
}

#[test]
fn composing_commit_取代虚拟区() {
    let bar = InputBarState::new();
    bar.set_composing("nihao");
    bar.insert_text("你好");
    let snap = bar.snap();
    assert_eq!(snap.text, "你好", "落字取代虚拟组合区(拼音不上屏)");
    assert_eq!(snap.composing, "");
}

#[test]
fn composing_点按先收组合() {
    let bar = InputBarState::new();
    bar.insert_text("ab");
    bar.set_cursor(2);
    bar.set_composing("pin");
    bar.set_cursor(0); // 点到最前
    let snap = bar.snap();
    assert_eq!(snap.text, "abpin", "点按先 finishComposing 再定位");
    assert_eq!(bar.cursor(), 0);
    assert!(snap.handle);
}

#[test]
fn composing_发送不复读半截拼音() {
    let bar = InputBarState::new();
    bar.insert_text("在吗");
    bar.set_composing("hai");
    let sent = bar.submit();
    assert_eq!(sent.as_deref(), Some("在吗"), "半截组合拼音不跟去下游");
    let snap = bar.snap();
    assert_eq!(snap.composing, "");
}

#[test]
fn bar_inject_parse_组合态指令() {
    use kfm_na::gate::{BarCmd, parse_bar_line};
    assert_eq!(
        parse_bar_line("composing nihao"),
        Some(Ok(BarCmd::Composing("nihao".to_string())))
    );
    assert_eq!(
        parse_bar_line("composing-end"),
        Some(Ok(BarCmd::ComposingEnd))
    );
}

// ========== 视口滚动（2026-09-01 输入框可滚:拖动看头部,编辑回跟随） ==========
// 判卷点:scroll_by 脱离跟随/尾锚→手动交接播种(BAR-043)/写入即钳制/编辑回跟随

#[test]
fn scroll_拖动脱跟随_编辑回跟随() {
    let bar = InputBarState::new();
    bar.insert_text("长文本");
    bar.set_lines(10); // 量行写回(设备上由 poll 做;考题里显式给)
    bar.scroll_by(5, 408); // 先到尾锚
    bar.scroll_by_px(-50, 408); // 手指下拖 50px:往头部(1:1 钳制区间内)
    let snap = bar.snap();
    assert!(!snap.follow, "拖动 = 脱离跟随");
    assert_eq!(snap.scroll_px, 172, "222-50 = 离头 172px(钳制区间内 1:1)");
    bar.insert_text("字");
    assert!(bar.snap().follow, "编辑回跟随(尾锚,光标可见)");
}

#[test]
fn scroll_by_行单位换算像素() {
    let bar = InputBarState::new();
    bar.set_lines(10); // 10 行:strip 630,field 408 → 可滚 222px
    bar.scroll_by(2, 408); // 尾锚交接播种 222,+2 行往尾 → 钳在尾(BAR-043)
    assert_eq!(bar.snap().scroll_px, 222, "尾锚态往尾滚=钳尾(交接播种可见)");
    bar.scroll_by(-2, 408); // 往头 2 行:222-126=96 → 换算 1 行=63px 可见
    assert_eq!(bar.snap().scroll_px, 96, "行单位换算:2 行=126px(222-96)");
    bar.scroll_by(-5, 408); // 往头 5 行=315px:96-315<0 → 钳 0
    assert_eq!(bar.snap().scroll_px, 0, "头部边界钳制(不越界)");
}

#[test]
fn bar_inject_parse_scroll指令() {
    use kfm_na::gate::{BarCmd, parse_bar_line};
    assert_eq!(parse_bar_line("scroll -2"), Some(Ok(BarCmd::Scroll(-2))));
    assert_eq!(parse_bar_line("scroll 5"), Some(Ok(BarCmd::Scroll(5))));
    assert_eq!(
        parse_bar_line("scrollpx -80"),
        Some(Ok(BarCmd::ScrollPx(-80))),
        "像素级滚动指令(AI 监控/驱动用)"
    );
    assert!(matches!(parse_bar_line("scroll abc"), Some(Err(_))));
}

#[test]
fn bar_inject_parse_selection指令() {
    use kfm_na::gate::{BarCmd, parse_bar_line};
    assert_eq!(parse_bar_line("select-all"), Some(Ok(BarCmd::SelectAll)));
    assert_eq!(parse_bar_line("unselect"), Some(Ok(BarCmd::Unselect)));
    assert_eq!(parse_bar_line("select 2 5"), Some(Ok(BarCmd::Select(2, 5))));
    assert!(matches!(parse_bar_line("select 2"), Some(Err(_))));
    assert!(matches!(parse_bar_line("select a b"), Some(Err(_))));
}

#[test]
fn bar_inject_parse_clipboard指令() {
    use kfm_na::gate::{BarCmd, parse_bar_line};
    assert_eq!(parse_bar_line("copy"), Some(Ok(BarCmd::Copy)));
    assert_eq!(parse_bar_line("cut"), Some(Ok(BarCmd::Cut)));
    assert_eq!(parse_bar_line("paste"), Some(Ok(BarCmd::Paste)));
}

// ========== 文本选择系统（BAR-046，2026-09-02） ==========
// 判卷点：选区不越界/替换选区/删除选区/全选/组合态先落字

#[test]
fn selection_enter_clears_composing() {
    let bar = InputBarState::new();
    bar.insert_text("你好");
    bar.set_composing("ni");
    bar.enter_selection(1);
    let snap = bar.snap();
    assert!(snap.selecting, "进入选择模式");
    assert_eq!(
        snap.text, "你好ni",
        "组合态在光标处落真字（cursor=2 在'你好'后）"
    );
    assert_eq!(snap.selection_start, 1, "选区起点=传入位置");
    assert_eq!(snap.selection_end, 1, "初始选区为空");
    assert!(snap.composing.is_empty(), "组合态清空");
}

#[test]
fn selection_anchors_do_not_cross() {
    let bar = InputBarState::new();
    bar.insert_text("一二三四五");
    bar.enter_selection(2);
    bar.set_selection_end(4);
    assert_eq!(bar.snap().selection_end, 4, "右锚点扩展");
    bar.set_selection_start(5); // 试图越过右锚点
    assert_eq!(bar.snap().selection_start, 4, "左锚点被钳在右锚点");
    bar.set_selection_start(1);
    assert_eq!(bar.snap().selection_start, 1, "左锚点正常收缩");
    bar.set_selection_end(0); // 试图越过左锚点
    assert_eq!(bar.snap().selection_end, 1, "右锚点被钳在左锚点");
}

#[test]
fn selection_selected_text_and_delete() {
    let bar = InputBarState::new();
    bar.insert_text("abcdef");
    bar.enter_selection(2);
    bar.set_selection_end(4);
    assert_eq!(bar.selected_text().as_deref(), Some("cd"), "选区文本正确");
    assert!(bar.delete_selection(), "发生了删除");
    let snap = bar.snap();
    assert_eq!(snap.text, "abef", "选区被删除");
    assert_eq!(snap.cursor, 2, "光标落在选区起点");
    assert!(!snap.selecting, "退出选择模式");
}

#[test]
fn selection_insert_replaces_selection() {
    let bar = InputBarState::new();
    bar.insert_text("abcdef");
    bar.enter_selection(2);
    bar.set_selection_end(4);
    bar.insert_text("XYZ");
    let snap = bar.snap();
    assert_eq!(snap.text, "abXYZef", "选区被替换");
    assert_eq!(snap.cursor, 5, "光标在插入文本后");
    assert!(!snap.selecting, "退出选择模式");
}

#[test]
fn selection_backspace_deletes_selection() {
    let bar = InputBarState::new();
    bar.insert_text("abcdef");
    bar.enter_selection(2);
    bar.set_selection_end(4);
    bar.backspace();
    assert_eq!(bar.snap().text, "abef", "退格删整个选区");
}

#[test]
fn selection_select_all() {
    let bar = InputBarState::new();
    bar.insert_text("你好世界");
    bar.set_composing("abc");
    bar.select_all();
    let snap = bar.snap();
    assert!(snap.selecting, "全选后处于选择模式");
    assert_eq!(snap.selection_start, 0, "从头选");
    assert_eq!(snap.selection_end, 7, "到尾（4 中文+3 拼音落字后）");
    assert_eq!(
        bar.selected_text().as_deref(),
        Some("你好世界abc"),
        "全选文本正确"
    );
}

#[test]
fn selection_clear_and_submit_exit_selection() {
    let bar = InputBarState::new();
    bar.insert_text("abcdef");
    bar.enter_selection(1);
    bar.set_selection_end(3);
    bar.clear();
    assert!(!bar.snap().selecting, "clear 退出选择");
    assert_eq!(bar.snap().selection_end, 0, "选区复位");

    bar.insert_text("xyz");
    bar.enter_selection(1);
    bar.set_selection_end(2);
    assert_eq!(bar.submit().as_deref(), Some("xyz"), "submit 取走全部文本");
    assert!(!bar.snap().selecting, "submit 后退出选择");
}
