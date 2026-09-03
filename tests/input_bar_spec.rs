//! tests/input_bar_spec.rs — A 档考题：全局输入栏状态核（src/input_bar.rs）
//!
//! 契约真相源：docs/active/ai-presence.md §二 常驻 chrome 一 / §五 焦点二态。
//! 判卷点：焦点二态 / 文本缓冲（UTF-8 安全退格）/ enter 取文发送 /
//! 几何命中（文本区 vs 发送钮 vs 栏外，键盘 inset 跟手）。
//! 纪律：先验证红，答案生成到绿，绿后变异抽检。本文件是考题，生成器不许改。

use kfm_na::input_bar::{BarHit, HEIGHT_PX, InputBarState, SelAnchor, clamp_to_field, hit, in_bar};

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

// BAR-056 2026-09-03 用户实机：把一端的拖拽柄拖过另一端（上面那行的柄
// 拖到下面那行柄的下方）→ 整个选择框消失。病灶：set_selection_start/end
// 旧钳制语义——越过对端即被钳死在原地，选区压成零宽不可见。契约：换锚
// （Android/浏览器标准）——拖过界两锚交换，旧对端定身为新端点，指头改持
// 新锚继续拖；贴合（pos == 对端）不算交叉，零宽停留不换锚。程序侧双端
// 同设走 set_selection_span 原子落，不被换锚截胡。
#[test]
fn spec_bar056_拖锚交叉换锚() {
    let bar = InputBarState::new();
    bar.insert_text("一二三四五六七八九十");
    bar.enter_selection(2);
    bar.set_selection_end(4); // 选区 (2,4)，指头持右锚

    // 右锚向左拖过左锚：换锚——旧左锚(2)定身为新右锚，指头改持新左锚
    let held = bar.set_selection_end(1);
    let snap = bar.snap();
    assert_eq!(
        (snap.selection_start, snap.selection_end),
        (1, 2),
        "右锚左越 → 换锚成 (1,2)，不压零宽"
    );
    assert_eq!(held, SelAnchor::Left, "指头改持左锚");

    // 继续向左拖：新左锚正常收缩，不换锚
    let held = bar.set_selection_start(0);
    assert_eq!(held, SelAnchor::Left, "未交叉方向不变");
    assert_eq!(
        (bar.snap().selection_start, bar.snap().selection_end),
        (0, 2),
        "新左锚正常收缩"
    );

    // 左锚向右拖过右锚：换回——旧右锚(2)定身为新左锚，指头改持新右锚
    let held = bar.set_selection_start(5);
    let snap = bar.snap();
    assert_eq!(
        (snap.selection_start, snap.selection_end),
        (2, 5),
        "左锚右越 → 换锚成 (2,5)"
    );
    assert_eq!(held, SelAnchor::Right, "指头改持右锚");

    // 不交叉的扩展/收缩：原语义不变
    bar.set_selection_end(8);
    assert_eq!(
        (bar.snap().selection_start, bar.snap().selection_end),
        (2, 8),
        "右锚正常扩展"
    );
    let held = bar.set_selection_start(3);
    assert_eq!(held, SelAnchor::Left);
    assert_eq!(
        (bar.snap().selection_start, bar.snap().selection_end),
        (3, 8),
        "左锚正常收缩"
    );

    // 贴合（pos == 对端）不触发换锚：零宽停留，方向不变
    let held = bar.set_selection_end(3);
    let snap = bar.snap();
    assert_eq!(
        (snap.selection_start, snap.selection_end),
        (3, 3),
        "贴合 = 零宽停留"
    );
    assert_eq!(held, SelAnchor::Right, "贴合不换锚");
}

#[test]
fn spec_bar056_程序侧双端同设不换锚() {
    let bar = InputBarState::new();
    bar.insert_text("abcdefghij");
    bar.enter_selection(5);
    bar.set_selection_end(8); // (5,8)
    // 枢轴扩选要落 (0,3)：若拆成 set_start(0)+set_end(3) 两发，第二发
    // 3 < 5 触发换锚会错成 (3,8)；原子设必须精准落 (0,3)
    bar.set_selection_span(0, 3);
    assert_eq!(
        (bar.snap().selection_start, bar.snap().selection_end),
        (0, 3),
        "双端原子设不被换锚截胡"
    );
    // 逆序入参自动理序
    bar.set_selection_span(7, 2);
    assert_eq!(
        (bar.snap().selection_start, bar.snap().selection_end),
        (2, 7),
        "逆序入参理序"
    );
}

// BAR-055 2026-09-03 用户实机：抓拖拽柄时在同一行里上下挪动就「断触」。
// 病灶：拖动连续态沿用点按的严格命中尺（bar_field_char_at），指头纵坐标
// 一出文本框（抓柄时指心本就在框下沿外）返回 None → 选区冻结，挪回界内
// 又复活 = 断触感。契约：拖动连续态专用 clamp_to_field 钳制尺——框内
// 原样、框外按最近边；点按/命中判定仍用严格尺（钳制尺会误中，禁混用）。
#[test]
fn spec_bar055_拖动钳制尺() {
    // 框: left=60 top=100 w=200 h=50 → x∈[60,259] y∈[100,149]
    let (x, y) = clamp_to_field(150.0, 120.0, 60, 100, 200, 50);
    assert_eq!((x, y), (150.0, 120.0), "框内原样");
    let (_x, y) = clamp_to_field(150.0, 500.0, 60, 100, 200, 50);
    assert_eq!(y, 149.0, "下出界钳到下沿内（抓柄指心在框下）");
    let (_x, y) = clamp_to_field(150.0, -30.0, 60, 100, 200, 50);
    assert_eq!(y, 100.0, "上出界钳到顶行");
    let (x, _y) = clamp_to_field(0.0, 120.0, 60, 100, 200, 50);
    assert_eq!(x, 60.0, "左出界钳到左缘");
    let (x, _y) = clamp_to_field(9999.0, 120.0, 60, 100, 200, 50);
    assert_eq!(x, 259.0, "右出界钳到右缘");
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

// BAR-053 2026-09-03 用户实机指认：拖拽柄只有「全选」一个入口能召唤，
// 长按触发不了 = 选择功能不可用。病灶链三环：①长按进选择用的是
// enter_selection（空选区 start=end，无高亮不可见）；②长按抬手落进
// Field 点按分路 → set_cursor 顺手把选择清了（刚召唤即销毁）；
// ③长按后滑指走滚动分路（不扩选）。契约：长按 = 选词高亮（词恒整选），
// 按住滑 = 词枢轴扩选，抬手保持。本钉先验词跨度纯函数（终端侧同字符集
// is_word_char：CJK 连续句读段、ascii/路径串整段；非词字符单选；
// 越界按末词）。
#[test]
fn spec_bar053_长按选词_词跨度() {
    use kfm_na::input_bar::word_span_at;
    // ascii 词：落点词内任意处 → 整词
    assert_eq!(word_span_at("hello world", 1), Some((0, 5)));
    assert_eq!(word_span_at("hello world", 4), Some((0, 5)));
    assert_eq!(word_span_at("hello world", 7), Some((6, 11)));
    // 空格/标点非词字符 → 单选该字
    assert_eq!(word_span_at("hello world", 5), Some((5, 6)));
    // CJK 连续段算一词（标点断句）
    assert_eq!(word_span_at("你好，世界", 0), Some((0, 2)));
    assert_eq!(word_span_at("你好，世界", 3), Some((3, 5)));
    // 路径串整段拎出（_-./:~ 入词字符集）
    assert_eq!(word_span_at("/tmp/a-b.txt", 5), Some((0, 12)));
    // 中英连排：词字符连续即同词（中+-+eng+混合 整段一词）
    assert_eq!(word_span_at("中-eng混合", 2), Some((0, 7)));
    // 标点才真正断词
    assert_eq!(word_span_at("中-eng，混合", 2), Some((0, 5)));
    assert_eq!(word_span_at("中-eng，混合", 6), Some((6, 8)));
    // 越界（按在文本尾后）→ 末词
    assert_eq!(word_span_at("hello world", 99), Some((6, 11)));
    // 空文本 → None
    assert_eq!(word_span_at("", 0), None);
}

// BAR-053 ②：词枢轴拖动扩选纯函数——词恒整选 + 扩向指头一侧；
// 指头入词内 → 回词本体（同次拖动可缩回）；产出 start ≤ ps ≤ pe ≤ end。
#[test]
fn spec_bar053_词枢轴扩选() {
    use kfm_na::input_bar::pivot_drag_span;
    let pivot = (2, 5);
    assert_eq!(pivot_drag_span(pivot, 0), (0, 5), "指头在词左 → 向左扩");
    assert_eq!(pivot_drag_span(pivot, 9), (2, 9), "指头在词右 → 向右扩");
    assert_eq!(pivot_drag_span(pivot, 3), (2, 5), "指头入词内 → 回词本体");
    assert_eq!(pivot_drag_span(pivot, 2), (2, 5), "词首边界 = 词内");
    assert_eq!(pivot_drag_span(pivot, 5), (2, 5), "词尾边界 = 词内");
}

// BAR-053 ③：状态核落选区——长按选词后选择态非空可见（高亮+双锚点+
// 菜单的活选区），活动锚 = 右锚（续滑扩选从词尾起）；空文本不进选择态。
#[test]
fn spec_bar053_长按选词_落选区非空可见() {
    let bar = InputBarState::new();
    bar.insert_text("hello world");
    let span = bar.enter_selection_word(1);
    assert_eq!(span, Some((0, 5)), "返回词跨度供枢轴登记");
    let snap = bar.snap();
    assert!(snap.selecting, "长按后处于选择模式");
    assert_eq!(
        (snap.selection_start, snap.selection_end),
        (0, 5),
        "选区 = 整词（非空可见高亮）"
    );
    assert!(!snap.handle, "定位柄让位双锚点");

    // 落点越界 → 末词
    let bar2 = InputBarState::new();
    bar2.insert_text("你好世界");
    assert_eq!(bar2.enter_selection_word(99), Some((0, 4)));

    // 空文本 → 不进选择态（长按无词可选 = 无操作）
    let bar3 = InputBarState::new();
    assert_eq!(bar3.enter_selection_word(0), None);
    assert!(!bar3.snap().selecting, "空文本不进选择模式");
}

// BAR-054 续（2026-09-03 受控实验定案后第三刀）：IME 剪切拿到选区、
// 复制成功，但删除指令不见——疑似走未覆写的 getTextBeforeCursor/
// AfterCursor 算范围（默认实现不认状态核，算出 0 长度删个寂寞），或
// deleteSurroundingTextInCodePoints/replaceText/setSelection 姊妹路径。
// 契约：前后查询选择态取选区起/终点之外（Android 契约），否则光标前后；
// set_caret_or_selection start==end=光标定位退出选择、不等=原子选区；
// replace_range 区间替换+光标落插入尾+退出选择。
#[test]
fn spec_bar054_光标前后查询() {
    let bar = InputBarState::new();
    bar.insert_text("一二三四五六七八九十");
    bar.set_cursor(6);
    assert_eq!(bar.text_before_cursor(3), "四五六", "光标前三字");
    assert_eq!(bar.text_after_cursor(2), "七八", "光标后两字");
    assert_eq!(bar.text_before_cursor(99), "一二三四五六", "不足全给");
    assert_eq!(bar.text_after_cursor(99), "七八九十", "不足全给");
    // 选择态：before=选区起点前，after=选区终点后
    bar.enter_selection(3);
    bar.set_selection_end(7); // 选「四五六七」
    assert_eq!(
        bar.text_before_cursor(2),
        "二三",
        "选择态 before=选区开始前"
    );
    assert_eq!(bar.text_after_cursor(2), "八九", "选择态 after=选区结束后");
}

#[test]
fn spec_bar054_直设光标或选区() {
    let bar = InputBarState::new();
    bar.insert_text("abcdef");
    bar.set_caret_or_selection(1, 4);
    let s = bar.snap();
    assert!(s.selecting, "不等 = 进选择态");
    assert_eq!((s.selection_start, s.selection_end), (1, 4));
    bar.set_caret_or_selection(3, 3);
    let s = bar.snap();
    assert!(!s.selecting, "start==end = 光标定位退出选择");
    assert_eq!(s.cursor, 3);
    bar.set_caret_or_selection(5, 2);
    let s = bar.snap();
    assert_eq!((s.selection_start, s.selection_end), (2, 5), "逆序理序");
    bar.set_caret_or_selection(0, 99);
    let s = bar.snap();
    assert_eq!((s.selection_start, s.selection_end), (0, 6), "越界钳全文");
}

#[test]
fn spec_bar054_区间替换() {
    let bar = InputBarState::new();
    bar.insert_text("一二三四五六");
    bar.replace_range(2, 4, "XY");
    let s = bar.snap();
    assert_eq!(s.text, "一二XY五六", "区间被替换");
    assert_eq!(s.cursor, 4, "光标落插入文本尾");
    assert!(!s.selecting, "退出选择态");
    // 空串替换 = 删除区间（IME 剪切删除半若走 replaceText 即此形态）
    bar.replace_range(2, 4, "");
    assert_eq!(bar.snap().text, "一二五六");
}
