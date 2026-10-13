//! a11y_spec — BAR-246 无障碍服务 A 档考题（解析/日志/轮转/批处理全钉）
//!
//! 覆盖：parse_a11y_line 全命令正反例 / log_line 字段序与转义 /
//! append_log 半切轮转 / a11y_check 批处理（坏行不毒整批、host 执行落空
//! 账、状态快照与结果件落盘）/ 白名单同票（gate_poller ↔ na-server
//! gateq ↔ gate 头表三处同见 a11y-req）。
//!
//! 变异抽检（手工执行过，2026-10-13）：①解析器把未知命令改成放行 →
//! 「未知命令拒」钉红；②轮转帽临时改 64B → 轮转钉红；③白名单钉把
//! na-server 侧源文件路径改错 → 钉红。三咬三中。

use kfm_na::a11y::*;
use std::fmt::Write as _;

// ---------- 解析钉 ----------

#[test]
fn spec_bar246_解析全命令() {
    assert_eq!(parse_a11y_line("status"), Ok(A11yCmd::Status));
    assert_eq!(parse_a11y_line("  status  "), Ok(A11yCmd::Status));
    assert_eq!(parse_a11y_line("dump"), Ok(A11yCmd::Dump));
    assert_eq!(parse_a11y_line("dump-log"), Ok(A11yCmd::DumpLog));
    assert_eq!(parse_a11y_line("scroll-fwd"), Ok(A11yCmd::ScrollFwd));
    assert_eq!(parse_a11y_line("scroll-back"), Ok(A11yCmd::ScrollBack));
    assert_eq!(
        parse_a11y_line("click-text 无线调试"),
        Ok(A11yCmd::ClickText("无线调试".into()))
    );
    // 参数含空白整体收（设置页条目名常带空格）
    assert_eq!(
        parse_a11y_line("click-text 开发者 选项"),
        Ok(A11yCmd::ClickText("开发者 选项".into()))
    );
    assert_eq!(
        parse_a11y_line("click-id com.android.settings:id/toggle"),
        Ok(A11yCmd::ClickId("com.android.settings:id/toggle".into()))
    );
    assert_eq!(
        parse_a11y_line("click-point 540 1200"),
        Ok(A11yCmd::ClickPoint(540, 1200))
    );
    assert_eq!(
        parse_a11y_line("set-text hello world"),
        Ok(A11yCmd::SetText("hello world".into()))
    );
    assert_eq!(
        parse_a11y_line("global back"),
        Ok(A11yCmd::Global(GlobalAction::Back))
    );
    assert_eq!(
        parse_a11y_line("global quick-settings"),
        Ok(A11yCmd::Global(GlobalAction::QuickSettings))
    );
}

#[test]
fn spec_bar246_解析拒收() {
    // 未知命令
    assert!(parse_a11y_line("fly").unwrap_err().contains("未知命令"));
    // 空行
    assert!(parse_a11y_line("   ").is_err());
    // 缺参数
    assert!(
        parse_a11y_line("click-text")
            .unwrap_err()
            .contains("缺参数")
    );
    assert!(parse_a11y_line("set-text").unwrap_err().contains("缺参数"));
    // 多给参数（无参命令）
    assert!(
        parse_a11y_line("status x")
            .unwrap_err()
            .contains("不带参数")
    );
    assert!(
        parse_a11y_line("dump now")
            .unwrap_err()
            .contains("不带参数")
    );
    // click-point 参数数错/非整数
    assert!(parse_a11y_line("click-point 1").is_err());
    assert!(parse_a11y_line("click-point 1 2 3").is_err());
    assert!(
        parse_a11y_line("click-point a b")
            .unwrap_err()
            .contains("非整数")
    );
    // global 未知动作
    assert!(
        parse_a11y_line("global moon")
            .unwrap_err()
            .contains("未知动作")
    );
}

#[test]
fn spec_bar246_op_arg账() {
    assert_eq!(A11yCmd::ClickText("无线调试".into()).op(), "click-text");
    assert_eq!(A11yCmd::ClickText("无线调试".into()).arg(), "无线调试");
    assert_eq!(A11yCmd::ClickPoint(3, 4).arg(), "3,4");
    assert_eq!(A11yCmd::Status.arg(), "");
    // 全局动作选择子与 Java 侧 case 序同表（0..=4）
    assert_eq!(GlobalAction::Back.sel(), 0);
    assert_eq!(GlobalAction::QuickSettings.sel(), 4);
}

// ---------- 日志行钉 ----------

#[test]
fn spec_bar246_日志行格式() {
    let l = log_line(1000, "click-text", "无线调试", true, "", 7);
    assert_eq!(
        l,
        "{\"ts\":1000,\"op\":\"click-text\",\"arg\":\"无线调试\",\"ok\":true,\"err\":\"\",\"dur_ms\":7}"
    );
    // 转义：引号/反斜杠/换行/控制符
    let l = log_line(1, "op", "a\"b\\c\nd\te\u{1}f", false, "坏\t了", 2);
    assert_eq!(
        l,
        "{\"ts\":1,\"op\":\"op\",\"arg\":\"a\\\"b\\\\c\\nd\\te\\u0001f\",\"ok\":false,\"err\":\"坏\\t了\",\"dur_ms\":2}"
    );
}

#[test]
fn spec_bar246_日志轮转半切() {
    let dir = std::env::temp_dir().join(format!("a11y-spec-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join(LOG_FILE);
    let _ = std::fs::remove_file(&log);
    // 写出超帽体量（LOG_CAP=512KB：每行约 100B，写 6000 行 ≈ 600KB）
    for i in 0..6000 {
        let mut s = String::new();
        write!(s, "填充行{i:06}").unwrap();
        append_log(dir.to_str().unwrap(), &s.repeat(8));
    }
    let meta = std::fs::metadata(&log).unwrap();
    // 轮转后体量必须小于帽+一次追加的余量（不许无限涨）
    assert!(
        (meta.len() as usize) <= 512 * 1024 + 900,
        "轮转后超帽: {}",
        meta.len()
    );
    // 帽值本身也咬（变异抽检实证：只钉「不超帽」咬不住「帽被改小」——
    // 轮转必须发生在 512KB 量级：半切留存 ≥ 半帽−一行）
    assert!(
        meta.len() as usize >= 512 * 1024 / 2 - 900,
        "轮转留存过少（帽疑被改小）: {}",
        meta.len()
    );
    let body = std::fs::read_to_string(&log).unwrap();
    // 最新行必须在（保新），最旧的第 0 行必须已被切掉（弃旧）
    assert!(body.contains("填充行005999"));
    assert!(!body.contains("填充行000000"));
    // 半切对齐行首：首行必须是整行（不含半个填充行的残片）——
    // 每个非首行都以 填充行 开头，首行也必须是完整行起始
    let first = body.lines().next().unwrap();
    assert!(first.starts_with("填充行"), "首行残缺: {first}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- 批处理钉（host 全链：解析→执行落空→日志/快照/结果件）----------

#[test]
fn spec_bar246_批处理全链() {
    let dir = std::env::temp_dir().join(format!("a11y-spec-batch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let d = dir.to_str().unwrap();
    for f in [REQ_FILE, LOG_FILE, STATUS_FILE, RES_FILE] {
        let _ = std::fs::remove_file(dir.join(f));
    }
    std::fs::write(
        dir.join(REQ_FILE),
        "# 注释行跳过\n\nstatus\nbad-cmd\nclick-text 无线调试\n",
    )
    .unwrap();
    a11y_check(d);
    // 触发文件被消费（原子取走）
    assert!(!dir.join(REQ_FILE).exists());
    // 审计日志：3 行（注释/空行不进账），status 与 click-text 在 host
    // 落空（ok=false，err 含「仅安卓」），bad-cmd 是 parse 拒收
    let log = std::fs::read_to_string(dir.join(LOG_FILE)).unwrap();
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 3, "日志行数: {log}");
    assert!(lines[0].contains("\"op\":\"status\""));
    assert!(lines[0].contains("\"ok\":false"));
    assert!(lines[0].contains("仅安卓"));
    assert!(lines[1].contains("\"op\":\"parse\""));
    assert!(lines[1].contains("未知命令"));
    assert!(lines[2].contains("\"arg\":\"无线调试\""));
    // 状态快照：up=0（host 无服务）+ 最近动作摘要
    let st = std::fs::read_to_string(dir.join(STATUS_FILE)).unwrap();
    assert!(st.contains("up=0"), "快照: {st}");
    assert!(st.contains("click-text ERR"), "快照: {st}");
    // 结果件：逐行结果，坏行也在（坏行不毒整批——第 3 行照样执行）
    let res = std::fs::read_to_string(dir.join(RES_FILE)).unwrap();
    assert!(res.contains("status => ERR"));
    assert!(res.contains("bad-cmd => ERR"));
    assert!(res.contains("click-text 无线调试 => ERR"));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- 白名单同票钉（三处同见，缺一处即红） ----------

#[test]
fn spec_bar246_白名单三处同票() {
    // 锚要吃「代码形态」不吃字面串（变异抽检实证：裸 contains("a11y-req")
    // 会被注释行骗绿——白名单项/值守调用必须带引号/调用形态）
    let poller = include_str!("../src/gate_poller.rs");
    let gateq = include_str!("../crates/na-server/src/gateq.rs");
    let gate = include_str!("../src/gate.rs");
    assert!(
        poller.contains("\"a11y-req\","),
        "gate_poller CHANNELS 缺 a11y-req 白名单项"
    );
    assert!(
        gateq.contains("\"a11y-req\","),
        "gateq CHANNELS 缺 a11y-req 白名单项"
    );
    assert!(
        gate.contains("a11y_check(DUMP_DIR)"),
        "gate 值守循环缺 a11y_check 调用（通道十六）"
    );
}
