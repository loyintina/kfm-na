//! BAR-254 服务端全量对表钉（MAIN0173 白露楼2 通则：验收只认全量对表零误，
//! 样本只当冒烟；清和 NA0183 楼3：对表脚本放 tests 里可复跑）。
//!
//! 口径 = 南舟 MAIN0173 楼3 §二（逐项可复）：
//!   面＝三册顶层 .md（00-主册／10-NA信箱／20-评审信箱），不含 archive-*
//!   目录，README.md 不算信；字段＝文首 40 行内 从/致/抄（同名键取最后
//!   一次）；单位＝项；计数面只按 、，, 分项（及/等不拆——那 7 条及式
//!   正是这样浮进异常的）。
//!   判定：整项「全体」＝广播；职能最长前缀＋余恰两字 ∈ names＝规范；
//!   其余＝异常。
//!
//! 钉法：①打印三册分布（对表报告用）；②异常集必须等于已知 12 项
//! （定罪日 11＝7 及式＋4 职能+全体；2026-10-17 追认 PSH0008 同族 1 项）
//! ——新信长出新异常即红，
//! 「分布数对不上先查口径再喊人」（清和楼3）；③及式 7 行用生产函数
//! split_display_items 断言继承归属（解析层必须归属——白露楼4）；
//! ④0109 回归（顿号式＝规范+广播两段都对）。
//!
//! 环境面：信箱根缺席（如手机 Termux 跑 chain）＝跳过不红。

use mailbox_core::roster::{DispKind, Roster, split_display_items};

fn mail_root() -> Option<String> {
    let root = std::env::var("NA_AGENT_MAIL_ROOT")
        .unwrap_or_else(|_| format!("{}/90-信箱", std::env::var("HOME").unwrap_or_default()));
    std::path::Path::new(&root)
        .join("00-主册")
        .is_dir()
        .then_some(root)
}

/// 计数面一项（南舟口径）
#[derive(Debug, PartialEq)]
enum Count {
    规范,
    广播,
    异常,
}

fn classify_counting(roster: &Roster, item: &str) -> Count {
    if item == "全体" {
        return Count::广播;
    }
    let mut best = "";
    for f in &roster.functions {
        if item.starts_with(f.as_str()) && f.len() > best.len() {
            best = f;
        }
    }
    if !best.is_empty() {
        let rest = &item[best.len()..];
        if rest.chars().count() == 2 && roster.names.iter().any(|(n, _)| n == rest) {
            return Count::规范;
        }
    }
    Count::异常
}

/// (letter_file, field, item, count)
struct Row {
    book: &'static str,
    letter: String,
    field: String,
    item: String,
    count: Count,
}

fn scan_book(root: &str, book: &'static str, dir: &str, roster: &Roster) -> Vec<Row> {
    let mut out = Vec::new();
    let dirp = format!("{root}/{dir}");
    let mut names: Vec<String> = std::fs::read_dir(&dirp)
        .unwrap_or_else(|e| panic!("读册目录失败 {dirp}: {e}"))
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".md") && n != "README.md")
        .collect();
    names.sort();
    for name in names {
        let path = format!("{dirp}/{name}");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        // 文首 40 行内 从/致/抄，同名键取最后一次
        let mut fields: Vec<(&str, String)> = Vec::new();
        for line in text.lines().take(40) {
            let ln = line.trim_start_matches('>').trim();
            for key in ["从", "致", "抄"] {
                if let Some(rest) = ln
                    .strip_prefix(key)
                    .and_then(|r| r.strip_prefix(':'))
                    .map(str::trim)
                {
                    if let Some(slot) = fields.iter_mut().find(|(k, _)| *k == key) {
                        slot.1 = rest.to_string();
                    } else {
                        fields.push((key, rest.to_string()));
                    }
                }
            }
        }
        for (field, val) in fields {
            for item in val.split(['、', '，', ',']) {
                let item = item.trim();
                if item.is_empty() {
                    continue;
                }
                out.push(Row {
                    book,
                    letter: name.clone(),
                    field: field.to_string(),
                    item: item.to_string(),
                    count: classify_counting(roster, item),
                });
            }
        }
    }
    out
}

/// 2026-10-16 定罪日已知异常 11 项（南舟楼3 定版分布的异常列全账）
/// ＋2026-10-17 追认 1 项：PSH 0008「乾部一组全体」＝职能+全体同族
/// （新编制职能已入名册，新信长出已知形态，非新病种——查口径后追认）。
/// (册, 信文件名片头, 字段, 原项)
const KNOWN_ANOMALIES: [(&str, &str, &str, &str); 12] = [
    ("主册", "0112号", "致", "研究部清和及运维部卡萝"),
    ("NA", "0001号", "致", "研究部全体"),
    ("NA", "0003号", "致", "研究部全体"),
    ("NA", "0004号", "致", "开发部闻灯及观澜"),
    ("NA", "0010号", "抄", "开发部闻灯及观澜"),
    ("NA", "0020号", "致", "开发部闻灯及观澜"),
    ("NA", "0022号", "致", "开发部闻灯及观澜"),
    ("NA", "0038号", "致", "开发部全体"),
    ("NA", "0039号", "致", "开发部闻灯及观澜"),
    ("NA", "0083号", "致", "开发部闻灯及观澜"),
    ("PSH", "PSH0003号", "致", "乾部一组全体"),
    ("PSH", "0008号", "致", "乾部一组全体"),
];

#[test]
fn spec_bar254_全量对表() {
    let Some(root) = mail_root() else {
        eprintln!("BAR-254 对表：信箱根缺席，跳过（非服务器环境）");
        return;
    };
    let roster = Roster::from_json_str(
        &std::fs::read_to_string(format!("{root}/00-主册/roster.json")).expect("roster.json 可读"),
    )
    .expect("roster.json 可解析");

    let mut all = Vec::new();
    for (book, dir) in [
        ("主册", "00-主册"),
        ("NA", "10-NA信箱"),
        ("PSH", "20-评审信箱"),
    ] {
        all.extend(scan_book(&root, book, dir, &roster));
    }

    // ①分布打印（对表报告直接抄这段）
    for book in ["主册", "NA", "PSH"] {
        let rows: Vec<&Row> = all.iter().filter(|r| r.book == book).collect();
        let g = rows.iter().filter(|r| r.count == Count::规范).count();
        let b = rows.iter().filter(|r| r.count == Count::广播).count();
        let a = rows.iter().filter(|r| r.count == Count::异常).count();
        eprintln!(
            "BAR-254 对表 {book}: 总{} 规范{g} 广播{b} 异常{a}",
            rows.len()
        );
    }
    eprintln!("BAR-254 对表 合计: 总{}", all.len());

    // ②异常集 == 定罪日已知 11 项（新异常即红：先查口径再喊人）
    let anomalies: Vec<&Row> = all.iter().filter(|r| r.count == Count::异常).collect();
    assert_eq!(
        anomalies.len(),
        KNOWN_ANOMALIES.len(),
        "异常项数漂移（现 {} / 定罪日 {}）——先查口径（新信形态／名册变动）再喊人；分布见上方 eprintln",
        anomalies.len(),
        KNOWN_ANOMALIES.len(),
    );
    for (book, prefix, field, item) in KNOWN_ANOMALIES {
        let hit = anomalies
            .iter()
            .filter(|r| {
                r.book == book && r.letter.starts_with(prefix) && r.field == field && r.item == item
            })
            .count();
        assert_eq!(
            hit, 1,
            "已知异常 {book}|{prefix}|{field}|{item} 未恰好命中一次（命中 {hit}）"
        );
    }

    // ③及式 7 行：生产函数 split_display_items 的继承归属（白露楼4：
    // 解析层必须归属，否则欠账算错人）——6 行「开发部闻灯及观澜」全字段级
    let ji = split_display_items(Some(&roster), "开发部闻灯及观澜");
    assert_eq!(ji.len(), 2);
    assert_eq!(ji[1].kind, DispKind::Person);
    assert_eq!(ji[1].func.as_deref(), Some("开发部"));
    assert!(ji[1].inherited, "及式裸名必须标继承");
    let ji2 = split_display_items(Some(&roster), "研究部清和及运维部卡萝");
    assert_eq!(ji2.len(), 2);
    assert!(!ji2[1].inherited && ji2[1].func.as_deref() == Some("运维部"));

    // ④「职能+全体」4 行：生产面 = 广播原样不切
    for raw in ["研究部全体", "开发部全体", "乾部一组全体"] {
        let items = split_display_items(Some(&roster), raw);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, DispKind::Broadcast, "{raw} 应为广播项");
    }

    // ⑤0109 回归（南舟楼3 §四.3）：顿号式 = 规范+广播两段都对
    let r0109 = split_display_items(Some(&roster), "评审部白露、全体");
    assert_eq!(r0109.len(), 2);
    assert_eq!(r0109[0].kind, DispKind::Person);
    assert_eq!(r0109[1].kind, DispKind::Broadcast);
}
