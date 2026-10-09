//! mail_feed_spec.rs — 信箱列表页数据源 A 档考题（BAR-212，2026-09-30
//! 用户立项：信箱迁 $HOME/90-信箱 服务器级资产，na 侧入口迁解析页左下
//! 常驻槽；两册 = 主册 + NA信箱；摘要懒加载 + 本地化缓存）。
//!
//! 判卷维度：列表响应解析（字头四件/缺字段容错/ok:false 即错）/
//! 摘要响应解析 / 摘要合账（mtime 对账——信变了旧摘要不许冒顶）/
//! 懒加载待取窗裁决 / manifest·summaries 落盘串往返同口径 /
//! 展示序（最新在底 = 名升序）/ 计数字 / url_encode。
//!
//! 变异抽检：①merge 不核 mtime（信变了旧摘要冒顶）必须咬；
//! ②summaries_wanted 把已取到的名再取一遍（每帧重发风暴）必须咬；
//! ③oldest_first 排反（最新回顶 = BAR-212 拍板语义反）必须咬；
//! ④url_encode 漏编码逗号（批量 names 串被切成两半）必须咬。

use kfm_na::mail_feed::*;
use kfm_na::sess_pool;

fn entry(name: &str, mtime: u64) -> MailEntry {
    MailEntry {
        name: name.to_string(),
        bytes: 100,
        mtime,
        time: String::new(),
        from: String::new(),
        to: String::new(),
        title: String::new(),
        summary: None,
        floors: Vec::new(),
    }
}

#[test]
fn spec_bar212_列表解析字头四件与容错() {
    let body = r#"{"ok":true,"letters":[
        {"name":"0001号甲致乙的通报.md","bytes":123,"mtime":999,
         "time":"2026-09-30 08:00 +08:00","from":"研究部清和","to":"开发部闻灯","title":"搬家通告"},
        {"name":"0002号丙致丁.md","bytes":50,"mtime":1000}
    ]}"#;
    let list = parse_mail_list(body).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].time, "2026-09-30 08:00 +08:00");
    assert_eq!(list[0].from, "研究部清和");
    assert_eq!(list[0].to, "开发部闻灯");
    assert_eq!(list[0].title, "搬家通告");
    assert_eq!(list[0].summary, None);
    // 缺字头四件容错为 ""（旧端点/秃信不炸）
    assert_eq!(list[1].time, "");
    assert_eq!(list[1].title, "");
    // ok:false 即错（不许吞表）
    assert!(parse_mail_list(r#"{"ok":false,"error":"boom"}"#).is_err());
    assert!(parse_mail_list("not json").is_err());
}

#[test]
fn spec_bar212_摘要解析与mtime容错() {
    let body = r#"{"ok":true,"summaries":[
        {"name":"a.md","mtime":7,"summary":"一句话"},
        {"name":"b.md","summary":"无mtime"}
    ]}"#;
    let sums = parse_summaries(body).unwrap();
    assert_eq!(sums[0], ("a.md".to_string(), 7, "一句话".to_string()));
    // 缺 mtime 容错 0 = 永过期（合账对不上任何真 mtime 的信）
    assert_eq!(sums[1].1, 0);
    assert!(parse_summaries(r#"{"ok":false}"#).is_err());
}

#[test]
fn spec_bar212_摘要合账mtime对账() {
    let mut es = vec![entry("a.md", 7), entry("b.md", 8)];
    let sums = vec![
        ("a.md".to_string(), 7, "对得上".to_string()),
        ("b.md".to_string(), 999, "mtime不符".to_string()),
        ("c.md".to_string(), 1, "查无此信".to_string()),
    ];
    merge_summaries(&mut es, &sums);
    assert_eq!(es[0].summary.as_deref(), Some("对得上"));
    // mtime 不符 = 信变了，旧摘要不许冒顶
    assert_eq!(es[1].summary, None);
}

#[test]
fn spec_bar212_懒加载待取窗只挑缺摘要的() {
    let mut es: Vec<MailEntry> = (0..10).map(|i| entry(&format!("{i:04}.md"), 1)).collect();
    es[3].summary = Some("已有".into());
    es[7].summary = Some("已有".into());
    let wanted = summaries_wanted(&es, 2..5);
    assert_eq!(wanted, vec!["0002.md".to_string(), "0004.md".to_string()]);
    // 窗外的不取
    assert!(!wanted.contains(&"0007.md".to_string()));
}

#[test]
fn spec_bar212_落盘串往返同口径() {
    let mut es = vec![entry("0001号甲致乙.md", 42)];
    es[0].time = "t".into();
    es[0].from = "f".into();
    es[0].to = "x".into();
    es[0].title = "题".into();
    let back = parse_mail_list(&manifest_json(&es)).unwrap();
    assert_eq!(back[0].name, "0001号甲致乙.md");
    assert_eq!(back[0].mtime, 42);
    assert_eq!(back[0].title, "题");
    let sums = vec![("0001号甲致乙.md".to_string(), 42, "摘".to_string())];
    let back = parse_summaries(&summaries_json(&sums)).unwrap();
    assert_eq!(back, sums);
}

#[test]
fn spec_bar212_展示序最新在底() {
    let es = vec![
        entry("0003.md", 1),
        entry("0001.md", 1),
        entry("0002.md", 1),
    ];
    let sorted = oldest_first(&es);
    let names: Vec<&str> = sorted.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["0001.md", "0002.md", "0003.md"]);
}

#[test]
fn spec_bar212_计数字与册键表() {
    assert_eq!(count_word(Some(96)), "96 封");
    assert_eq!(count_word(None), "…");
    assert_eq!(MailKey::MainBook.api_key(), "main-book");
    assert_eq!(MailKey::NaBook.api_key(), "na-book");
    assert_eq!(MailKey::MainBook.title(), "主册");
    assert_eq!(MailKey::NaBook.title(), "NA信箱");
    assert_eq!(MailKey::all(), [MailKey::MainBook, MailKey::NaBook]);
}

#[test]
fn spec_bar212_url_encode逗号与中文() {
    // 逗号必须编码（批量 names 的分隔符，漏编 = 一名切两名）
    assert_eq!(sess_pool::url_encode("a,b"), "a%2Cb");
    // 数字/点号原样，中文按 UTF-8 字节 %XX 大写
    assert_eq!(sess_pool::url_encode("0001号.md"), "0001%E5%8F%B7.md");
    assert_eq!(sess_pool::url_encode("Ab9-_.~"), "Ab9-_.~");
    assert_eq!(sess_pool::url_encode("号"), "%E5%8F%B7");
}

/// 钉（BAR-222）：列表解析楼层表 + manifest 楼层往返 + 缺字段容错空表
#[test]
fn spec_bar222_楼层透传与缓存往返() {
    let body = r#"{"ok":true,"letters":[{"name":"0001号甲致乙的通报.md","bytes":10,"mtime":7,"time":"2026-10-02 06:23 +08:00","from":"开发部闻灯","to":"测试部承影","title":"题","floors":[{"n":1,"author":"闻灯","to":"承影","time":"2026-10-02 06:23 +08:00","summary":"一楼白话。","detail":"一楼细节。","body":"摘要段\n\n一楼白话。\n\n正文段\n\n一楼细节。"},{"n":2,"author":"承影","to":"1楼闻灯","time":"2026-10-02 07:00 +08:00","summary":null,"detail":null,"body":"旧形二楼正文"}]},{"name":"0002号丙致丁的通报.md","bytes":5,"mtime":8,"time":"","from":"","to":"","title":""}]}"#;
    let list = parse_mail_list(body).expect("解析");
    assert_eq!(list.len(), 2);
    // 楼层逐字段透传（summary/detail 原样透 Option）
    assert_eq!(list[0].floors.len(), 2);
    assert_eq!(list[0].floors[0].n, 1);
    assert_eq!(list[0].floors[0].author, "闻灯");
    assert_eq!(list[0].floors[0].summary.as_deref(), Some("一楼白话。"));
    assert_eq!(list[0].floors[0].detail.as_deref(), Some("一楼细节。"));
    assert_eq!(list[0].floors[1].to, "1楼闻灯");
    assert_eq!(list[0].floors[1].summary, None);
    assert_eq!(list[0].floors[1].body, "旧形二楼正文");
    // 缺 floors 字段（旧服务端/旧缓存）= 空表不炸
    assert!(list[1].floors.is_empty());
    // manifest 往返：楼层随缓存写透读回（断网也要有楼层看）
    let back = parse_mail_list(&manifest_json(&list)).expect("manifest 读回");
    assert_eq!(back[0].floors, list[0].floors, "manifest 楼层往返一致");
    assert!(back[1].floors.is_empty());
}

/// 钉（BAR-231，白露 0153 3楼①用户口述「楼层也只显示摘要」）：卡内
/// 显示文本 = **只出 summary**（与信卡摘要面同律；detail 不进卡，留
/// 端点/详情页消费）；旧形两键 null 回落 body 全文（无段可摘不造假截断）
#[test]
fn spec_bar231_楼层卡只显摘要_display_text() {
    use kfm_na::mail_feed::MailFloor;
    let mk = |summary: Option<&str>, detail: Option<&str>, body: &str| MailFloor {
        n: 1,
        author: "甲".to_string(),
        to: String::new(),
        time: String::new(),
        summary: summary.map(str::to_string),
        detail: detail.map(str::to_string),
        body: body.to_string(),
    };
    assert_eq!(
        mk(Some("白话。"), Some("细节。"), "原文").display_text(),
        "白话。",
        "新形 = 只摘要，detail 不进卡"
    );
    assert_eq!(
        mk(Some("白话。"), None, "原文").display_text(),
        "白话。",
        "细节省略同形 = 只摘要"
    );
    assert_eq!(
        mk(None, None, "旧形原文").display_text(),
        "旧形原文",
        "旧形回落 body 全文"
    );
}

/// BAR-243：信箱列表面必须走大帽档——256KB 默认帽把 626KB 的主册新表
/// 拒成「保持旧表」，解析页恒显 10-04 旧缓存（真机 field-reports 定罪：
/// mail 面 body 超限）。
#[test]
fn spec_bar243_信箱列表大帽档() {
    // 编译期咬：帽被砍回小档 / 放大到掩盖增长档 = 测试二进制编不过
    // （白露 NA0172 楼4：帽给中间值 2–3 倍，不许 8MB 把膨胀盖住）
    const {
        assert!(
            kfm_na::sess_pool::MAIL_BODY_CAP >= 2 * 1024 * 1024,
            "MAIL_BODY_CAP 必须 >= 2MB（BAR-243：主册现役 626KB 的 2–3 倍）"
        );
        assert!(
            kfm_na::sess_pool::MAIL_BODY_CAP <= 4 * 1024 * 1024,
            "MAIL_BODY_CAP 不许 >4MB（白露 NA0172 楼4：帽越大越盖住「表在膨胀」）"
        );
    }
    let feed = include_str!("../src/mail_feed.rs");
    assert!(
        feed.contains("crate::sess_pool::MAIL_BODY_CAP"),
        "mail_feed 列表/摘要两面必须走 MAIL_BODY_CAP 大帽档——共用 256KB \
         默认帽 = 新表被拒、恒显旧缓存（BAR-243 定罪）"
    );
    assert!(
        feed.contains("信箱列表") && feed.contains("体积"),
        "每次拉列表必须报一行体积（白露 NA0172 楼4 b：让「表在长」可见，\
         阈值只说到了、趋势说往哪去）"
    );
    let pool = include_str!("../src/sess_pool.rs");
    assert!(
        pool.contains("pub(crate) fn http_get_cap(port: u16, path: &str, cap: usize)"),
        "http_get_cap 带帽版必须在位（调用方按面给帽）"
    );
}
