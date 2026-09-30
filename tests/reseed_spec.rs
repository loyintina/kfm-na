//! BAR-186 臂① 播种对账合并钉（A 档纯逻辑 + 网格级等价钉）。
//! 核心判卷：reseed 尾块喂旧画布 ≡ 新 capture 全量重建（dump_all
//! 网格文本 + 游标位逐格同）——对账合并不是「差不多」，是同真。

use kfm_na::reseed::{ReseedPlan, plan_reseed};
use kfm_na::termview::Canvas;

/// 造 capture 文本：hist 行历史（H0001…）+ rows 行屏（S00<tag>…）
fn mkcap(hist: usize, rows: usize, tag: &str) -> String {
    let mut v: Vec<String> = (0..hist).map(|i| format!("H{i:04}")).collect();
    v.extend((0..rows).map(|i| format!("S{i:02}{tag}")));
    v.join("\r\n") // ctrl_feed Build 接缝：cap = 行.join("\r\n")
}

/// cap1 基础上「新输出 k 行」后的新 capture：旧屏顶 k 行挤进历史，
/// 屏 = 旧屏剩余 + 新行（真实 tmux 滚动形态）
fn rolled(cap: &str, rows: usize, k: usize, tag: &str) -> String {
    let lines: Vec<&str> = cap.split("\r\n").collect();
    let mut v: Vec<String> = lines[..lines.len() - rows]
        .iter()
        .map(|s| s.to_string())
        .collect();
    // 旧屏顶 min(k,rows) 行入史；k>rows 时超出的全是新输出洪峰行
    let kk = k.min(rows);
    v.extend(
        lines[lines.len() - rows..lines.len() - rows + kk]
            .iter()
            .map(|s| s.to_string()),
    );
    v.extend((0..k.saturating_sub(rows)).map(|i| format!("F{i:02}{tag}")));
    if k <= rows {
        // 新屏：旧屏剩余 k..rows + k 行新输出
        v.extend(
            lines[lines.len() - rows + k..]
                .iter()
                .map(|s| s.to_string()),
        );
        v.extend((0..k).map(|i| format!("N{i:02}{tag}")));
    } else {
        v.extend((0..rows).map(|i| format!("N{i:02}{tag}"))); // 洪峰全新屏
    }
    v.join("\r\n")
}

#[test]
fn spec_bar186_对账_判负全表() {
    let rows = 5;
    let cap1 = mkcap(20, rows, "a");
    // ① 清史：历史缩短 → Rebuild
    let shorter = mkcap(10, rows, "a");
    assert_eq!(
        plan_reseed(&cap1, &shorter, rows, (0, 4)),
        ReseedPlan::Rebuild,
        "历史缩短（对端清史）必须判负"
    );
    // ② 超一屏洪峰：k > rows → Rebuild（merge_capture 同款回落）
    let flood = rolled(&cap1, rows, rows + 1, "x");
    assert_eq!(
        plan_reseed(&cap1, &flood, rows, (0, 4)),
        ReseedPlan::Rebuild,
        "k>rows（刷新间隔滚超一屏）必须判负"
    );
    // ③ 历史被改写（resize 重排/回写历史）→ Rebuild
    let mut rewritten = rolled(&cap1, rows, 2, "x");
    rewritten = rewritten.replacen("H0007", "HXXXX", 1);
    assert_eq!(
        plan_reseed(&cap1, &rewritten, rows, (0, 4)),
        ReseedPlan::Rebuild,
        "旧历史段原样不在位必须判负"
    );
    // ④ 屏高 0 / 文本不足一屏 → Rebuild
    assert_eq!(
        plan_reseed(&cap1, &cap1, 0, (0, 0)),
        ReseedPlan::Rebuild,
        "rows=0 必须判负"
    );
    let tiny = "a\nb";
    assert_eq!(
        plan_reseed(tiny, tiny, rows, (0, 0)),
        ReseedPlan::Rebuild,
        "不足一屏必须判负"
    );
}

#[test]
fn spec_bar186_对账_尾块形制() {
    let rows = 5;
    let cap1 = mkcap(20, rows, "a");
    let cap2 = rolled(&cap1, rows, 3, "b");
    let ReseedPlan::Tail(tail) = plan_reseed(&cap1, &cap2, rows, (2, 3)) else {
        panic!("合规对账必须产 Tail");
    };
    let t = String::from_utf8(tail).unwrap();
    assert!(
        t.starts_with("\x1b[5;1H"),
        "尾块必先 CUP 底行（append 起点）"
    );
    assert!(t.contains("\x1b[5;1H\n\n\n"), "k=3 个裸 LF 顶行入史");
    assert!(t.contains("\x1b[1;1H"), "LF 后必 CUP 回顶重画");
    assert_eq!(t.matches("\x1b[K").count(), rows, "每行重画必带 EL 清残");
    assert!(
        t.ends_with("\x1b[4;3H\x1b[?25l"),
        "尾块必以游标归位+藏光标收"
    );
}

/// 等价钉（本臂判卷核心）：reseed 尾块 ≡ 全量重建——网格文本与游标
/// 位逐格同。k=3（部分滚屏）与 k=0（纯屏变）两态都咬。
#[test]
fn spec_bar186_对账_尾块等价全量重建() {
    let rows = 5;
    for k in [0usize, 3] {
        let cap1 = mkcap(20, rows, "a");
        let cap2 = rolled(&cap1, rows, k, "b");
        let cursor = (2u32, 3u32);
        let reference = Canvas::build(&cap2, 80, rows, cursor);
        let mut merged = Canvas::build(&cap1, 80, rows, (0, 4));
        let ReseedPlan::Tail(tail) = plan_reseed(&cap1, &cap2, rows, cursor) else {
            panic!("k={k} 合规对账必须产 Tail");
        };
        merged.reseed(&tail);
        assert_eq!(
            merged.dump_all(),
            reference.dump_all(),
            "k={k}：对账续播网格文本必须与全量重建逐格同"
        );
        assert_eq!(
            merged.term.grid().cursor.point,
            reference.term.grid().cursor.point,
            "k={k}：游标位必须同（BAR-156 归位语义不漂）"
        );
    }
}
