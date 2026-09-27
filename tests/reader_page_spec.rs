//! tests/reader_page_spec.rs — A 档考题：阅读页状态核（BAR-170）
//!
//! 答案区：src/ui/reader_page.rs。本文件是考题，生成器不许改。
//! 咬点设计：本体三条（consumed≠text.len / 到位不空涨代际 / 记忆只进不出）
//! 各配一根专咬钉；分块帽/错账守卫/预取阈/恢复落一次全在咬区内。

use kfm_na::ui::reader_page::*;

fn open_and_first_chunk(p: &mut ReaderPage, path: &str) {
    p.open(path.to_string(), "n".to_string());
    assert_eq!(p.next_request(), Some(0), "开门首块 = Some(0)");
    p.mark_loading();
    assert_eq!(p.next_request(), None, "在途不许重发");
}

#[test]
fn spec_reader_kind_of() {
    assert_eq!(kind_of("a/b.md"), ReaderKind::Md);
    assert_eq!(kind_of("a/b.MD"), ReaderKind::Md, "大小写不敏感");
    assert_eq!(kind_of("a/b.markdown"), ReaderKind::Md);
    assert_eq!(kind_of("a/b.txt"), ReaderKind::Plain);
    assert_eq!(kind_of("a/Makefile"), ReaderKind::Plain);
    assert_eq!(kind_of("a/md"), ReaderKind::Plain, "无扩展名点不算");
}

#[test]
fn spec_reader_scroll_max与progress() {
    assert_eq!(scroll_max(1000, 300), 700);
    assert_eq!(scroll_max(300, 1000), 0, "内容矮于视口 = 0");
    let mut p = ReaderPage::new();
    assert_eq!(p.progress(0), None, "max=0 不画进度线");
    p.scroll = 50;
    assert_eq!(p.progress(200), Some((50, 200)));
}

#[test]
fn spec_reader_open清态与旧路径回写() {
    let mut p = ReaderPage::new();
    open_and_first_chunk(&mut p, "x/a.md");
    p.apply_chunk(0, 100, false, "hello");
    p.scroll_by(40, 100);
    assert_eq!(p.kind, ReaderKind::Md);
    // 换文件：a.md 的滚动 40 回写 mem；新文件全清
    p.open("x/b.txt".to_string(), "b".to_string());
    assert_eq!(p.mem_of("x/a.md"), Some(40), "旧路径滚动没回写");
    assert_eq!(p.text, "");
    assert_eq!(p.scroll, 0);
    assert_eq!(p.kind, ReaderKind::Plain);
    assert!(!p.eof && !p.capped && !p.loading);
    assert_eq!(p.phase, ReaderPhase::Loading);
}

#[test]
fn spec_reader_恢复滚动_包得住才落_落一次() {
    let mut p = ReaderPage::new();
    open_and_first_chunk(&mut p, "x/a.md");
    p.apply_chunk(0, 100, false, "t");
    p.scroll_by(500, 5000);
    // 重开：pending=500
    p.open("x/a.md".to_string(), "a".to_string());
    p.apply_chunk(0, 100, true, "t"); // 小内容，max 只有 100
    // max=100 < 500 且未 eof → 不落
    p.tick_restore(400, 300);
    assert_eq!(p.scroll, 0, "内容包不住恢复值时不许提前落");
    // eof 了 → 落，钳在 max=100
    p.apply_chunk(100, 200, false, "t");
    p.tick_restore(400, 300);
    assert_eq!(p.scroll, 100, "eof 时必须落（钳 max 内）");
    let e = p.epoch;
    p.tick_restore(400, 300);
    assert_eq!(p.epoch, e, "落一次即摘，重复 tick 不动作不空涨");
    // 落完后用户滚走，pending 不许复活把人拽回去（咬⑤ 专钉）
    p.scroll_by(-60, 100);
    p.tick_restore(400, 300);
    assert_eq!(
        p.scroll, 40,
        "pending 落过即摘——用户的新位置不许被恢复值拽回"
    );
}

#[test]
fn spec_reader_记忆帽满摘字典序最小() {
    let mut p = ReaderPage::new();
    for i in 0..MEM_CAP {
        let path = format!("p/{i:03}.md");
        p.open(path, "n".into());
        p.scroll_by(1, 10);
    }
    assert_eq!(p.mem_of("p/000.md"), Some(1));
    // 第 129 个：摘字典序最小 p/000.md
    p.open("p/zzz.md".to_string(), "n".into());
    p.scroll_by(1, 10);
    p.open("p/128.md".to_string(), "n".into()); // 触发 zzz 回写 + 满帽
    p.open("p/129.md".to_string(), "n".into());
    assert_eq!(p.mem_of("p/000.md"), None, "帽满该摘字典序最小");
    assert_eq!(p.mem_of("p/zzz.md"), Some(1), "新客该在册");
}

#[test]
fn spec_reader_分块账_consumed走回执不走textlen() {
    let mut p = ReaderPage::new();
    open_and_first_chunk(&mut p, "x/a.md");
    // 服务端 lossy 场景：回执文本 2 字节但原始侧消费 100（本体①——
    // 拿 text.len() 记账的实现这里必死）
    p.apply_chunk(0, 100, true, "ab");
    assert_eq!(
        p.next_request(),
        Some(100),
        "下一块 offset 必须走 next_offset"
    );
    assert_eq!(p.text, "ab");
    assert!(!p.eof);
    assert_eq!(p.phase, ReaderPhase::Reading);
    // 错账守卫：offset 不等于已消费量 → Error
    p.mark_loading();
    p.apply_chunk(50, 150, true, "cd");
    assert!(
        matches!(p.phase, ReaderPhase::Error(_)),
        "块序错账该落 Error: {:?}",
        p.phase
    );
    assert_eq!(p.next_request(), None, "Error 相不再发请求");
}

#[test]
fn spec_reader_块帽_eof与capped() {
    let mut p = ReaderPage::new();
    open_and_first_chunk(&mut p, "x/a.md");
    let mut off = 0u64;
    for i in 1..=MAX_CHUNKS {
        p.apply_chunk(off, off + 100, true, "x");
        off += 100;
        if i < MAX_CHUNKS {
            assert!(!p.eof, "第 {i} 块不该 eof");
        }
    }
    assert!(p.eof, "到帽必须 eof");
    assert!(p.capped, "到帽必须 capped（页脚显形凭据）");
    assert_eq!(p.next_request(), None, "eof 不再取");
}

#[test]
fn spec_reader_占位相() {
    let mut p = ReaderPage::new();
    open_and_first_chunk(&mut p, "x/b.bin");
    p.apply_binary();
    assert_eq!(p.phase, ReaderPhase::Binary);
    assert!(p.eof && !p.loading);
    assert_eq!(p.next_request(), None);
    let mut q = ReaderPage::new();
    open_and_first_chunk(&mut q, "x/g.md");
    q.apply_error("读取失败：HTTP 404");
    assert!(matches!(q.phase, ReaderPhase::Error(_)));
    assert_eq!(q.need_prefetch(10_000, 100), None, "Error 相不预取");
}

#[test]
fn spec_reader_预取阈() {
    let mut p = ReaderPage::new();
    open_and_first_chunk(&mut p, "x/a.md");
    // 首块到：内容高 200 < 1.5 视口（450）→ 立即连取（开门满屏半）
    p.apply_chunk(0, 100, true, "t");
    assert_eq!(p.need_prefetch(200, 300), Some(100));
    // 内容够长且滚动在顶 → 不取
    assert_eq!(p.need_prefetch(10_000, 300), None);
    // 滚近尾 1.5 视口内 → 取
    p.scroll_by(9560, 9700);
    assert_eq!(p.need_prefetch(10_000, 300), Some(100));
    // 在途 → 不取
    p.mark_loading();
    assert_eq!(p.need_prefetch(200, 300), None);
    // eof → 不取
    let mut q = ReaderPage::new();
    open_and_first_chunk(&mut q, "x/a.md");
    q.apply_chunk(0, 100, false, "t");
    assert_eq!(q.need_prefetch(200, 300), None, "eof 不预取");
}

#[test]
fn spec_reader_滚动钳与到位不空涨() {
    let mut p = ReaderPage::new();
    open_and_first_chunk(&mut p, "x/a.md");
    p.apply_chunk(0, 100, false, "t");
    let e = p.epoch;
    p.scroll_by(0, 100);
    assert_eq!(p.epoch, e, "零位移空涨代际（本体②）");
    p.scroll_by(150, 100);
    assert_eq!(p.scroll, 100, "上钳 max");
    p.scroll_by(-999, 100);
    assert_eq!(p.scroll, 0, "下钳 0");
    p.scroll_by(-1, 100);
    assert_eq!(p.scroll, 0, "已在 0 再往下不动");
}

#[test]
fn spec_reader_close回写_mem留本体清() {
    let mut p = ReaderPage::new();
    open_and_first_chunk(&mut p, "x/a.md");
    p.apply_chunk(0, 100, false, "t");
    p.scroll_by(30, 100);
    p.close();
    assert_eq!(p.mem_of("x/a.md"), Some(30), "close 回写");
    assert_eq!(p.path, "");
    assert_eq!(p.text, "");
    assert_eq!(p.scroll, 0);
    // 重开同文件：恢复值还在
    p.open("x/a.md".to_string(), "a".to_string());
    p.apply_chunk(0, 100, false, "t");
    p.tick_restore(1000, 100);
    assert_eq!(p.scroll, 30, "close 后重开照样恢复");
}
