//! mailbox-core A 档考题：fp golden / 文件名文法 golden / 信封与状态词 /
//! verify 负样本向量 / gen 投影 golden / next_number / block_field。
//!
//! golden 数据来源（非手编）：
//! - fp：主册 docs/ledger/agent-inbox/letter-tokens.jsonl 真实票据 6 行
//!   （含 0016 换票行 renamedFrom）+ na 册 4 行旧票据；
//! - gen 投影：JS gen-agent-inbox.mjs 对本目录 fixtures/gen 夹具实跑的回写产物
//!   （README.post.md / letters-index.expected.jsonl 逐字节抄录）。

use mailbox_core::header::{header_get, parse_header, parse_to_field};
use mailbox_core::name::{ToItem, is_v21_name, parse_v21_name, v21_no_of};
use mailbox_core::newletter::{self, SkeletonParams};
use mailbox_core::projection::{self, ScanTarget};
use mailbox_core::roster::{PoolEntry, Roster, check_pools};
use mailbox_core::status;
use mailbox_core::token::{self, fingerprint};
use mailbox_core::verify::{self, BookCheck, LetterText};
use std::collections::HashSet;

// ---------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------

/// 主名册的考题用缩录（字段与主册 roster.json 同构）
const ROSTER_JSON: &str = r#"{
  "projects": ["nz", "na", "kfmv4"],
  "functions": {
    "评审部": { "en": "review", "scope": "跨线裁决" },
    "研究部": { "en": "research", "scope": "外部信息整编" },
    "开发部": { "en": "development", "scope": "交付性实现" },
    "测试部": { "en": "quality", "scope": "验证" },
    "运维部": { "en": "sre", "scope": "资源纪律" },
    "调度部": { "en": "dispatcher", "scope": "接单" }
  },
  "names": {
    "白露": { "functions": ["评审部"], "project": "kfmv4" },
    "蔚然": { "functions": ["运维部"], "project": "kfmv4" },
    "清和": { "functions": ["研究部"], "project": "na" },
    "观澜": { "functions": ["开发部"], "project": "na" },
    "闻灯": { "functions": ["开发部"], "project": "na" },
    "小满": { "functions": ["研究部", "测试部", "开发部"], "project": "nz", "primary": "研究部" },
    "南舟": { "functions": ["研究部"], "project": null }
  },
  "reserved": ["全体"]
}"#;

fn roster() -> Roster {
    Roster::from_json_str(ROSTER_JSON).expect("名册夹具应可解析")
}

/// README 规则区「合法状态词表」片段（与主册/na 册 README 同款换行形态）
const README_RULES: &str = r#"
- **状态列维护人（按线分工）**：状态机
  `待回信 → 已回 → 已落地 → 已验证`。合法状态词表（2026-08-18 收编编外实践，
  含状态机四态与过程态）：已收到 / 已回 / 已回应 / 已裁决 / 用户终审通过 /
  通报完毕 / 已落地 / 已核 / 已验证 / 待回信 / 待评审表态 / 待落地通报 / 待核 /
  已会签（2026-08-18 D3 回填收编：茉莉会签步 0 达标线）/ 已收编（2026-08-18
  评审：案例归档收编态，用于信件内容转入案例库后封账）。维护对象 = 信头。
"#;

/// 造一封令牌合法的 v2.1 信（fp 用真算法算）
fn v21_letter(file: &str, nonce: &str, to: &str, reply: &str, status: &str, plain: &str) -> String {
    let no = v21_no_of(file).unwrap();
    let fp = fingerprint(&no, nonce, file);
    let from = parse_v21_name(file).from_name.unwrap_or_default();
    format!(
        "# 测试信\n\n> 日期: 2026-09-28 10:00 +08:00\n> 从: 研究部{from}\n> 致: {to}\n> 复: {reply}\n> 状态: {status}\n\n<!-- LETTER-TOKEN v2 no={no} nonce={nonce} fp={fp} -->\n\n## 白话结论（写给隐藏读者：测试）\n\n{plain}\n\n## 正文\n\n正文。\n"
    )
}

fn ledger_line(no: &str, file: &str, nonce: &str) -> String {
    let fp = fingerprint(no, nonce, file);
    format!(
        "{{\"no\":\"{no}\",\"file\":\"{file}\",\"nonce\":\"{nonce}\",\"fp\":\"{fp}\",\"tpl\":\"v2\",\"createdAt\":\"2026-09-28T00:00:00.000Z\",\"from\":\"测试\"}}"
    )
}

fn book<'a>(
    letters: &'a [LetterText],
    tokens: &'a str,
    v1: &'a HashSet<String>,
    roster: &'a Roster,
) -> BookCheck<'a> {
    BookCheck {
        letters,
        tokens_text: Some(tokens),
        roster: Some(roster),
        readme_text: README_RULES,
        name_prefix: "kfm-na|kfmv4",
        v1_files: v1,
        strict_pools: true,
    }
}

// ---------------------------------------------------------------
// 1. fp golden：真实票据重算必须全对上
// ---------------------------------------------------------------

#[test]
fn fp_golden_main_book_real_tickets() {
    // (no, nonce, file, fp) —— 主册 letter-tokens.jsonl 实录
    let cases = [
        // 存量批量迁移后的新票（0001，ASCII→中文句法）
        (
            "0001",
            "7ea7f6c2336359da",
            "0001号小满致全体关于报告可读性契约的提案.md",
            "f8b91ad0ed6d789a",
        ),
        // 0004：跨册引用那封（清和致研究部小满）
        (
            "0004",
            "5f02ca5dc2e17e40",
            "0004号清和致研究部小满复0001关于可读性契约采纳的回信.md",
            "0f5873cc4e971058",
        ),
        // 0010：日报
        (
            "0010",
            "12b7893d4286c87a",
            "0010号白露致全体的日报.md",
            "fd5da9ca289cd47b",
        ),
        // 0016：换票行——带 renamedFrom 的现行票（验收硬要求）
        (
            "0016",
            "c3d44cb28ae48043",
            "0016号白露致全体关于名字征集的提案.md",
            "418022fb85d60041",
        ),
        // 0017：名字征集回信
        (
            "0017",
            "c4f1973141f82556",
            "0017号清和致评审部白露复0016关于名字征集的回信.md",
            "5d5b244066386bc5",
        ),
        // 0022：勘误换票后的现行票
        (
            "0022",
            "534808ea501f23b7",
            "0022号小满致评审部白露复0016关于名字征集的勘误.md",
            "49152dc99f3a1f74",
        ),
    ];
    for (no, nonce, file, want) in cases {
        assert_eq!(fingerprint(no, nonce, file), want, "fp 对不上：{file}");
    }
}

#[test]
fn fp_golden_na_book_legacy_tickets() {
    // na 册 4 行旧票据（/root/.kfm/session/信箱/letter-tokens.jsonl 实录）
    let cases = [
        (
            "0001",
            "fe0c66a1b423ceba",
            "0001-kfm-na-git-stage-flow-report.md",
            "8a7e8c49ee5426c2",
        ),
        (
            "0002",
            "2ee606c7e8969627",
            "0002-na-agent-bar164-blocked-by-filetree-report.md",
            "fdd2b875cbc84c6f",
        ),
        (
            "0003",
            "84853c9fe3c2d268",
            "0003-na-bar167-progress-chain-blocker-report.md",
            "2dc2f210cf83af7d",
        ),
        (
            "0004",
            "0e4eb608b78b49ea",
            "0004-kfm-na-review-rust-localization-submission.md",
            "245bc1eddad19c22",
        ),
    ];
    for (no, nonce, file, want) in cases {
        assert_eq!(fingerprint(no, nonce, file), want, "fp 对不上：{file}");
    }
}

#[test]
fn fp_golden_revoked_ticket_also_recomputes() {
    // 撤销票同样按公式重算（主册 0016 被撤销的旧票）
    assert_eq!(
        fingerprint("0016", "91132d8fa62e1484", "0016号白露致全体的提案.md"),
        "df2f55846f23626e"
    );
}

// ---------------------------------------------------------------
// 2. 文件名文法 golden：契约 §二 例句 + 边界
// ---------------------------------------------------------------

#[test]
fn name_grammar_contract_examples() {
    let p = parse_v21_name("0025号空山的日报.md");
    assert!(p.errs.is_empty(), "{:?}", p.errs);
    assert_eq!(p.no.as_deref(), Some("0025"));
    assert_eq!(p.sorting.as_deref(), Some(""));
    assert_eq!(p.from_name.as_deref(), Some("空山"));
    assert_eq!(p.to, None);
    assert_eq!(p.type_word.as_deref(), Some("日报"));

    let p = parse_v21_name("0023号思衍复0001的回执.md");
    assert!(p.errs.is_empty(), "{:?}", p.errs);
    assert_eq!(p.reply.as_deref(), Some("0001"));
    assert_eq!(p.type_word.as_deref(), Some("回执"));

    let p = parse_v21_name("NA0024号茉莉致研究部空谷的通报.md");
    assert!(p.errs.is_empty(), "{:?}", p.errs);
    assert_eq!(p.sorting.as_deref(), Some("NA"));
    assert_eq!(p.no.as_deref(), Some("0024"));

    let p = parse_v21_name("0016号白露致全体关于名字征集的提案.md");
    assert!(p.errs.is_empty(), "{:?}", p.errs);
    assert_eq!(p.subject.as_deref(), Some("名字征集"));
    let items = p.to_items.unwrap();
    assert_eq!(
        items,
        vec![ToItem {
            func: None,
            name: "全体".into(),
            inherited: false
        }]
    );

    let p = parse_v21_name("0023号思衍复0001关于口径勘误的回执.md");
    assert!(p.errs.is_empty(), "{:?}", p.errs);
    assert_eq!(p.reply.as_deref(), Some("0001"));
    assert_eq!(p.subject.as_deref(), Some("口径勘误"));
}

#[test]
fn name_grammar_to_items_and_inheritance() {
    // 同一职能两人：第二项纯两字继承职能
    let p = parse_v21_name("0026号空山致评审部空谷及空山的通报.md");
    assert!(p.errs.is_empty(), "{:?}", p.errs);
    let items = p.to_items.unwrap();
    assert_eq!(
        items[0],
        ToItem {
            func: Some("评审部".into()),
            name: "空谷".into(),
            inherited: false
        }
    );
    assert_eq!(
        items[1],
        ToItem {
            func: Some("评审部".into()),
            name: "空山".into(),
            inherited: true
        }
    );

    // 三条以上「等」收尾（等 被剥掉、不进项）
    let p = parse_v21_name("0027号空山致评审部空山及开发部思衍等的通报.md");
    assert!(p.errs.is_empty(), "{:?}", p.errs);
    let items = p.to_items.unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[1].func.as_deref(), Some("开发部"));

    // 复跨信箱全码
    let p = parse_v21_name("0001号清和致评审部白露复MAIN0014关于测试事由的提案.md");
    assert!(p.errs.is_empty(), "{:?}", p.errs);
    assert_eq!(p.reply.as_deref(), Some("MAIN0014"));
}

#[test]
fn name_grammar_error_vectors() {
    // 缺「的」（契约 §二 0027 例句原文形态——无 的+类型词收尾即非法）
    let p = parse_v21_name("0027号空山致评审部空山及开发部思衍等通报.md");
    assert!(
        p.errs.iter().any(|e| e.contains("缺连接字「的」")),
        "{:?}",
        p.errs
    );

    // 缺「号」
    let p = parse_v21_name("空山致全体的通报.md");
    assert!(
        p.errs.iter().any(|e| e.contains("缺连接字「号」")),
        "{:?}",
        p.errs
    );

    // 编号段非法（分拣码超 4 位）
    let p = parse_v21_name("ABCDE0001号空山的日报.md");
    assert!(
        p.errs.iter().any(|e| e.contains("编号段非法")),
        "{:?}",
        p.errs
    );

    // 类型词出表（落地 已移出词表）
    let p = parse_v21_name("0099号空山致评审部空谷的落地.md");
    assert!(
        p.errs.iter().any(|e| e.contains("类型词出表")),
        "{:?}",
        p.errs
    );

    // 关于段混连接字
    let p = parse_v21_name("0010号空山致全体关于征文的事的提案.md");
    assert!(
        p.errs.iter().any(|e| e.contains("事由含连接字")),
        "{:?}",
        p.errs
    );

    // 纯两字首项无职能可继承
    let p = parse_v21_name("0011号空山致空谷的通报.md");
    assert!(
        p.errs.iter().any(|e| e.contains("无前项职能可继承")),
        "{:?}",
        p.errs
    );

    // 发信人含连接字
    let p = parse_v21_name("0005号复山致评审部空谷的通报.md");
    assert!(
        p.errs.iter().any(|e| e.contains("发信人名")),
        "{:?}",
        p.errs
    );
}

#[test]
fn era_detection() {
    assert!(is_v21_name("0016号白露致全体关于名字征集的提案.md"));
    assert!(is_v21_name("NA0024号茉莉致研究部空谷的通报.md"));
    assert!(!is_v21_name("0001-kfm-na-git-stage-flow-report.md"));
    assert!(!is_v21_name("README.md"));
    assert_eq!(
        v21_no_of("NA0024号茉莉致研究部空谷的通报.md").as_deref(),
        Some("NA0024")
    );
    assert_eq!(v21_no_of("0001-kfm-na-x-report.md"), None);
}

// ---------------------------------------------------------------
// 3. 信封解析 / 致字段
// ---------------------------------------------------------------

#[test]
fn header_first_quote_block_last_wins_fullwidth_colon() {
    let text = "# 标题\n\n> 日期： 2026-09-28 10:00 +08:00\n> 从: 研究部清和\n> 状态: 待回信\n> 状态: 已回（2026-09-28 11:00 +08:00 评审部白露 更新：x）\n\n> 状态: 不在首块\n\n正文 > 致: 不在块内\n";
    let h = parse_header(text, &["日期", "从", "致", "状态"]);
    assert_eq!(header_get(&h, "日期"), Some("2026-09-28 10:00 +08:00"));
    assert_eq!(header_get(&h, "从"), Some("研究部清和"));
    // 同名字段取最后一次（首块内）
    assert_eq!(
        header_get(&h, "状态"),
        Some("已回（2026-09-28 11:00 +08:00 评审部白露 更新：x）")
    );
}

#[test]
fn to_field_inheritance_and_deng() {
    let mut errs = vec![];
    let items = parse_to_field("研究部清和、小满", &mut errs);
    assert!(errs.is_empty(), "{errs:?}");
    assert_eq!(
        items[1],
        ToItem {
            func: Some("研究部".into()),
            name: "小满".into(),
            inherited: true
        }
    );

    let mut errs = vec![];
    parse_to_field("评审部白露等", &mut errs);
    assert!(
        errs.iter().any(|e| e.contains("不得以「等」结尾")),
        "{errs:?}"
    );

    let mut errs = vec![];
    parse_to_field("空谷", &mut errs);
    assert!(
        errs.iter().any(|e| e.contains("无前项职能可继承")),
        "{errs:?}"
    );

    let mut errs = vec![];
    let items = parse_to_field("全体", &mut errs);
    assert_eq!(
        items,
        vec![ToItem {
            func: None,
            name: "全体".into(),
            inherited: false
        }]
    );
}

// ---------------------------------------------------------------
// 4. 状态词表 / 代际戳 / 日期
// ---------------------------------------------------------------

#[test]
fn status_words_from_readme_rules() {
    let words = status::parse_status_words(README_RULES);
    assert_eq!(words.len(), 15, "{words:?}");
    assert!(words.contains(&"待回信".to_string()));
    assert!(words.contains(&"已收编".to_string()));
    assert!(words.contains(&"用户终审通过".to_string()));

    let re = status::build_status_re(&words).expect("词表非空应出 regex");
    assert!(re.is_match("待回信"));
    assert!(re.is_match("✅ 已验证（2026-08-15 NA 通报：规格书落地）"));
    assert!(re.is_match("📢 通报完毕（无需回信）"));
    assert!(re.is_match("✅ 茉莉已会签（2026-08-18 三数字达标）"));
    assert!(!re.is_match("随便什么状态"));
    assert!(!re.is_match("已完毕"));
}

#[test]
fn status_re_anchor_and_alternation_order() {
    let words = status::parse_status_words(README_RULES);
    let re = status::build_status_re(&words).expect("词表非空应出 regex");
    // 锚在串首：词前只许非字母符号 + 至多 4 个汉字（如「✅ 茉莉已会签」）；
    // 5 个汉字的前缀是垃圾状态，不许靠词中匹配蒙混（去掉 ^ 锚的变异必红）
    assert!(re.is_match("✅ 茉莉已会签（略）"));
    assert!(!re.is_match("某某某部门已会签过待回信"));
    // alternation 按词长降序拼接（JS 同款构造，稳定排序）——regex 源串逐字节钉死，
    // 去掉长度降序的变异必红
    let toy = vec!["待".to_string(), "待回信".to_string(), "已结案".to_string()];
    let toy_re = status::build_status_re(&toy).expect("toy 词表非空");
    assert!(
        toy_re.as_str().contains("待回信|已结案|待"),
        "alternation 须长度降序：{}",
        toy_re.as_str()
    );
}

#[test]
fn stamp_and_date_rules() {
    assert!(status::is_valid_v21_date("2026-09-28 10:00 +08:00"));
    assert!(!status::is_valid_v21_date("2026-09-28"));
    assert!(!status::is_valid_v21_date("2026-09-28 10:00"));
    assert!(status::has_v21_stamp(
        "已回（2026-09-24 13:03 +08:00 评审部白露 更新：采纳）"
    ));
    assert!(status::has_v21_stamp(
        "通报完毕（2026-09-16 08:00 +08:00 运维部蔚然 更新:x）"
    ));
    assert!(!status::has_v21_stamp("已回"));
    assert!(status::is_debt("待回信"));
    assert!(status::is_debt("🔥 待评审表态"));
    assert!(!status::is_debt("已回"));
    // to_ms：同日不同时刻/时区可比大小
    assert!(status::to_ms("2026-09-28 10:00 +08:00") < status::to_ms("2026-09-28 11:00 +08:00"));
    // 同一本地时刻，+08:00 的 UTC 毫秒更小（北京 10:00 = UTC 02:00）
    assert!(status::to_ms("2026-09-28 10:00 +08:00") < status::to_ms("2026-09-28 10:00 +00:00"));
    assert!(status::to_ms("不是日期").is_nan());
}

// ---------------------------------------------------------------
// 5. verify 负样本向量（单信模式 + 全册模式）
// ---------------------------------------------------------------

#[test]
fn verify_single_clean_letter_passes() {
    let file = "0001号清和致评审部白露关于测试事由的提案.md";
    let text = v21_letter(
        file,
        "aabbccddeeff0011",
        "评审部白露",
        "无（首信）",
        "待回信",
        "已填实的白话。",
    );
    let tokens = ledger_line("0001", file, "aabbccddeeff0011") + "\n";
    let d = verify::verify_single(file, &text, Some(&tokens), Some(&roster()), true);
    assert!(d.errs.is_empty(), "{:?}", d.errs);
}

#[test]
fn verify_single_missing_token() {
    let file = "0001号清和致评审部白露关于测试事由的提案.md";
    let text = v21_letter(
        file,
        "aabbccddeeff0011",
        "评审部白露",
        "无（首信）",
        "待回信",
        "已填实。",
    );
    let text = text
        .lines()
        .filter(|l| !l.contains("LETTER-TOKEN"))
        .collect::<Vec<_>>()
        .join("\n");
    let d = verify::verify_single(file, &text, None, Some(&roster()), true);
    assert!(d.errs.iter().any(|e| e.contains("缺令牌")), "{:?}", d.errs);
}

#[test]
fn verify_single_tampered_fp() {
    let file = "0001号清和致评审部白露关于测试事由的提案.md";
    let text = v21_letter(
        file,
        "aabbccddeeff0011",
        "评审部白露",
        "无（首信）",
        "待回信",
        "已填实。",
    );
    let tampered = text.replace(
        &fingerprint("0001", "aabbccddeeff0011", file),
        "0000000000000000",
    );
    let tokens = ledger_line("0001", file, "aabbccddeeff0011") + "\n";
    let d = verify::verify_single(file, &tampered, Some(&tokens), Some(&roster()), true);
    assert!(
        d.errs.iter().any(|e| e.contains("指纹重算不符")),
        "{:?}",
        d.errs
    );
}

#[test]
fn verify_single_placeholder_plain_block() {
    let file = "0001号清和致评审部白露关于测试事由的提案.md";
    let text = v21_letter(
        file,
        "aabbccddeeff0011",
        "评审部白露",
        "无（首信）",
        "待回信",
        "（待填：面向隐藏读者。）",
    );
    let tokens = ledger_line("0001", file, "aabbccddeeff0011") + "\n";
    let d = verify::verify_single(file, &text, Some(&tokens), Some(&roster()), true);
    assert!(d.errs.iter().any(|e| e.contains("占位")), "{:?}", d.errs);
}

#[test]
fn verify_single_to_ends_with_deng() {
    let file = "0001号清和致评审部白露关于测试事由的提案.md";
    let text = v21_letter(
        file,
        "aabbccddeeff0011",
        "评审部白露等",
        "无（首信）",
        "待回信",
        "已填实。",
    );
    let tokens = ledger_line("0001", file, "aabbccddeeff0011") + "\n";
    let d = verify::verify_single(file, &text, Some(&tokens), Some(&roster()), true);
    assert!(
        d.errs.iter().any(|e| e.contains("不得以「等」结尾")),
        "{:?}",
        d.errs
    );
}

#[test]
fn verify_single_reply_bad_format_and_missing_stamp() {
    let file = "0001号清和致评审部白露关于测试事由的提案.md";
    let text = v21_letter(
        file,
        "aabbccddeeff0011",
        "评审部白露",
        "见上月那封",
        "已回",
        "已填实。",
    );
    let tokens = ledger_line("0001", file, "aabbccddeeff0011") + "\n";
    let d = verify::verify_single(file, &text, Some(&tokens), Some(&roster()), true);
    assert!(
        d.errs.iter().any(|e| e.contains("「复」格式非法")),
        "{:?}",
        d.errs
    );
    assert!(d.errs.iter().any(|e| e.contains("代际戳")), "{:?}", d.errs);
}

#[test]
fn verify_book_bare_function_and_reply_existence() {
    let v1: HashSet<String> = HashSet::new();
    let r = roster();
    // 裸职能：信封致只写「研究部」
    let f1 = "0001号清和致研究部的提案.md";
    let letters = vec![LetterText {
        file: f1.into(),
        dir: "active".into(),
        text: v21_letter(
            f1,
            "aabbccddeeff0011",
            "研究部",
            "无（首信）",
            "待回信",
            "已填实。",
        ),
    }];
    let tokens = ledger_line("0001", f1, "aabbccddeeff0011") + "\n";
    let d = verify::verify_book(&book(&letters, &tokens, &v1, &r));
    assert!(
        d.errs.iter().any(|e| e.contains("裸职能非法")),
        "{:?}",
        d.errs
    );

    // 复不存在的编号（本册）= 硬红
    let f2 = "0002号清和致评审部白露复0099的回信.md";
    let letters = vec![LetterText {
        file: f2.into(),
        dir: "active".into(),
        text: v21_letter(
            f2,
            "aabbccddeeff0022",
            "评审部白露",
            "0099",
            "待回信",
            "已填实。",
        ),
    }];
    let tokens = ledger_line("0002", f2, "aabbccddeeff0022") + "\n";
    let d = verify::verify_book(&book(&letters, &tokens, &v1, &r));
    assert!(
        d.errs.iter().any(|e| e.contains("被复信不存在")),
        "{:?}",
        d.errs
    );

    // 跨册（MAIN 开头）= 只警告不红
    let f3 = "0003号清和致评审部白露复MAIN0099的回信.md";
    let letters = vec![LetterText {
        file: f3.into(),
        dir: "active".into(),
        text: v21_letter(
            f3,
            "aabbccddeeff0033",
            "评审部白露",
            "MAIN0099",
            "待回信",
            "已填实。",
        ),
    }];
    let tokens = ledger_line("0003", f3, "aabbccddeeff0033") + "\n";
    let d = verify::verify_book(&book(&letters, &tokens, &v1, &r));
    assert!(
        !d.errs.iter().any(|e| e.contains("被复信不存在")),
        "{:?}",
        d.errs
    );
    assert!(
        d.warns.iter().any(|w| w.contains("跨信箱被复信")),
        "{:?}",
        d.warns
    );
}

#[test]
fn verify_book_status_word_and_stamp() {
    let v1: HashSet<String> = HashSet::new();
    let r = roster();
    // 状态词出表
    let f1 = "0001号清和致评审部白露的通报.md";
    let letters = vec![LetterText {
        file: f1.into(),
        dir: "active".into(),
        text: v21_letter(
            f1,
            "aabbccddeeff0011",
            "评审部白露",
            "无（首信）",
            "已完毕（2026-09-28 10:00 +08:00 研究部清和 更新：x）",
            "已填实。",
        ),
    }];
    let tokens = ledger_line("0001", f1, "aabbccddeeff0011") + "\n";
    let d = verify::verify_book(&book(&letters, &tokens, &v1, &r));
    assert!(
        d.errs.iter().any(|e| e.contains("状态词出表")),
        "{:?}",
        d.errs
    );

    // 非待* 缺代际戳
    let f2 = "0002号清和致评审部白露的通报.md";
    let letters = vec![LetterText {
        file: f2.into(),
        dir: "active".into(),
        text: v21_letter(
            f2,
            "aabbccddeeff0022",
            "评审部白露",
            "无（首信）",
            "已回",
            "已填实。",
        ),
    }];
    let tokens = ledger_line("0002", f2, "aabbccddeeff0022") + "\n";
    let d = verify::verify_book(&book(&letters, &tokens, &v1, &r));
    assert!(d.errs.iter().any(|e| e.contains("代际戳")), "{:?}", d.errs);
}

#[test]
fn verify_book_cross_letter_rules() {
    let v1: HashSet<String> = HashSet::new();
    let r = roster();
    // 编号重复 + 日期非单调（第二封日期早于第一封）+ 台账编号重复
    let f1 = "0001号清和致评审部白露的通报.md";
    let t1 = v21_letter(
        f1,
        "aabbccddeeff0011",
        "评审部白露",
        "无（首信）",
        "待回信",
        "已填实。",
    )
    .replace("2026-09-28 10:00", "2026-09-28 12:00");
    let f2 = "0001号观澜致评审部白露的通报.md";
    let t2 = v21_letter(
        f2,
        "aabbccddeeff0022",
        "评审部白露",
        "无（首信）",
        "待回信",
        "已填实。",
    )
    .replace("2026-09-28 10:00", "2026-09-28 09:00");
    let letters = vec![
        LetterText {
            file: f1.into(),
            dir: "active".into(),
            text: t1,
        },
        LetterText {
            file: f2.into(),
            dir: "active".into(),
            text: t2,
        },
    ];
    let tokens = ledger_line("0001", f1, "aabbccddeeff0011")
        + "\n"
        + &ledger_line("0001", f2, "aabbccddeeff0022")
        + "\n";
    let d = verify::verify_book(&book(&letters, &tokens, &v1, &r));
    assert!(
        d.errs.iter().any(|e| e.contains("编号重复")),
        "{:?}",
        d.errs
    );
    assert!(
        d.errs.iter().any(|e| e.contains("日期非单调")),
        "{:?}",
        d.errs
    );
    assert!(
        d.errs.iter().any(|e| e.contains("台账编号重复")),
        "{:?}",
        d.errs
    );

    // 孤儿票据：台账登记的文件不在两栏
    let f3 = "0005号清和致评审部白露的通报.md";
    let letters = vec![LetterText {
        file: f3.into(),
        dir: "active".into(),
        text: v21_letter(
            f3,
            "aabbccddeeff0055",
            "评审部白露",
            "无（首信）",
            "待回信",
            "已填实。",
        ),
    }];
    let tokens = ledger_line("0005", f3, "aabbccddeeff0055")
        + "\n"
        + &ledger_line(
            "0006",
            "0006号清和致评审部白露的通报.md",
            "aabbccddeeff0066",
        )
        + "\n";
    let d = verify::verify_book(&book(&letters, &tokens, &v1, &r));
    assert!(
        d.errs.iter().any(|e| e.contains("孤儿票据")),
        "{:?}",
        d.errs
    );
}

#[test]
fn verify_book_reticket_pairing_warning() {
    let v1: HashSet<String> = HashSet::new();
    let r = roster();
    let f1 = "0001号清和致评审部白露关于新名的通报.md";
    let letters = vec![LetterText {
        file: f1.into(),
        dir: "active".into(),
        text: v21_letter(
            f1,
            "aabbccddeeff0011",
            "评审部白露",
            "无（首信）",
            "待回信",
            "已填实。",
        ),
    }];
    // 现行票记 renamedFrom，但台账缺旧票的撤销记录 → 警告（非红）
    let mut line = ledger_line("0001", f1, "aabbccddeeff0011");
    line.pop();
    line.push_str(",\"renamedFrom\":\"0001号清和致评审部白露的通报.md\"}");
    let tokens = line + "\n";
    let d = verify::verify_book(&book(&letters, &tokens, &v1, &r));
    assert!(!d.errs.iter().any(|e| e.contains("换票")), "{:?}", d.errs);
    assert!(
        d.warns.iter().any(|w| w.contains("换票留痕不完整")),
        "{:?}",
        d.warns
    );
}

#[test]
fn spec_bar179_verify_日期单调_分拣码混排按数字序() {
    // BAR-179：编号数字段序 = 签发序（next_number 跨前缀统一 max+1）。
    // 字符串序在混排前缀下会把 0018 排到 NA0015 前，把数字序单调的真册误判
    // 成「日期非单调」红（na 册 NA 前缀与纯数字混排实录，0018 撞墙定罪；
    // 主册前缀划一从未踩到，JS check-letter-token.mjs localeCompare 同款潜伏）。
    let v1: HashSet<String> = HashSet::new();
    let r = roster();
    let mk = |no: &str, nonce: &str, time: &str| {
        let f = format!("{no}号清和致评审部白露关于日期序的通报.md");
        let text = v21_letter(&f, nonce, "评审部白露", "无（首信）", "待回信", "已填实。")
            .replace("2026-09-28 10:00", time);
        (f, text)
    };

    // 数字序 NA0015 < 0016 < 0018 且日期 10:00 < 11:00 < 12:00：单调，不该红。
    // 旧字符串序 0016 < 0018 < NA0015 → NA0015(10:00) 早于 0018(12:00) 误判红。
    let (f1, t1) = mk("NA0015", "aabbccddeeff0015", "2026-09-29 10:00");
    let (f2, t2) = mk("0016", "aabbccddeeff0016", "2026-09-29 11:00");
    let (f3, t3) = mk("0018", "aabbccddeeff0018", "2026-09-29 12:00");
    let letters = vec![
        LetterText {
            file: f1.clone(),
            dir: "active".into(),
            text: t1.clone(),
        },
        LetterText {
            file: f2.clone(),
            dir: "active".into(),
            text: t2.clone(),
        },
        LetterText {
            file: f3.clone(),
            dir: "active".into(),
            text: t3.clone(),
        },
    ];
    let tokens = ledger_line("NA0015", &f1, "aabbccddeeff0015")
        + "\n"
        + &ledger_line("0016", &f2, "aabbccddeeff0016")
        + "\n"
        + &ledger_line("0018", &f3, "aabbccddeeff0018")
        + "\n";
    let d = verify::verify_book(&book(&letters, &tokens, &v1, &r));
    assert!(
        !d.errs.iter().any(|e| e.contains("日期非单调")),
        "{:?}",
        d.errs
    );

    // 负样本：NA0015 真回退（13:00 晚于 0016/0018）——数字序下必红，
    // 防判据被整体摘除（字符串序反而看不到这条红）。
    let (f1, t1) = mk("NA0015", "aabbccddeeff0015", "2026-09-29 13:00");
    let letters = vec![
        LetterText {
            file: f1.clone(),
            dir: "active".into(),
            text: t1,
        },
        LetterText {
            file: f2.clone(),
            dir: "active".into(),
            text: t2,
        },
        LetterText {
            file: f3.clone(),
            dir: "active".into(),
            text: t3,
        },
    ];
    let tokens = ledger_line("NA0015", &f1, "aabbccddeeff0015")
        + "\n"
        + &ledger_line("0016", &f2, "aabbccddeeff0016")
        + "\n"
        + &ledger_line("0018", &f3, "aabbccddeeff0018")
        + "\n";
    let d = verify::verify_book(&book(&letters, &tokens, &v1, &r));
    assert!(
        d.errs.iter().any(|e| e.contains("日期非单调")),
        "{:?}",
        d.errs
    );
}

#[test]
fn spec_bar180_verify_换票配对按renamed_from() {
    // BAR-180：§八.8 格式性勘误换票编号同步去码（NA0015→0015 实录）——
    // 配对键按 renamedFrom 文件名；旧「no 相等」判据对跨码换票每次 chain 误报
    let v1: HashSet<String> = HashSet::new();
    let r = roster();
    let f = "0015号清和致评审部白露关于署名勘误的通报.md";
    let letters = vec![LetterText {
        file: f.into(),
        dir: "active".into(),
        text: v21_letter(
            f,
            "aabbccddeeff0015",
            "评审部白露",
            "无（首信）",
            "待回信",
            "已填实。",
        ),
    }];
    let old_f = "NA0015号清和致评审部白露关于署名勘误的通报.md";
    let mut cur = ledger_line("0015", f, "aabbccddeeff0015");
    cur.pop();
    cur.push_str(&format!(",\"renamedFrom\":\"{old_f}\"}}"));
    let mut rev = ledger_line("NA0015", old_f, "aabbccddeeff0014");
    rev.pop();
    rev.push_str(
        ",\"revokedAt\":\"2026-09-29T03:29:03.836Z\",\"revokeReason\":\"格式性勘误（契约 §八 第 8 条）：去本册自指码\"}",
    );
    // 跨码换票对（旧票 no=NA0015 ≠ 新票 no=0015）：文件名配上 → 不 warn
    let tokens = format!("{cur}\n{rev}\n");
    let d = verify::verify_book(&book(&letters, &tokens, &v1, &r));
    assert!(
        !d.warns.iter().any(|w| w.contains("换票留痕不完整")),
        "{:?}",
        d.warns
    );
    // 负样本：renamedFrom 找不到同名撤销票 → warn 仍在（防判据被摘）
    let tokens = format!("{cur}\n");
    let d = verify::verify_book(&book(&letters, &tokens, &v1, &r));
    assert!(
        d.warns.iter().any(|w| w.contains("换票留痕不完整")),
        "{:?}",
        d.warns
    );
}

#[test]
fn verify_book_legacy_placeholder_and_clean() {
    let v1: HashSet<String> = HashSet::new();
    let r = roster();
    let legacy = |no: &str, file: &str, nonce: &str, status: &str| {
        let fp = fingerprint(no, nonce, file);
        LetterText {
            file: file.into(),
            dir: "active".into(),
            text: format!(
                "# 旧信\n\n> 日期: 2026-09-26\n> 致: 研究线\n> 流型: 线程\n> 预期表态方: 无\n> 收敛判据: 无需回信（知会）\n> 回: 无\n> 状态: {status}\n\n<!-- LETTER-TOKEN v2 no={no} nonce={nonce} fp={fp} -->\n\n## 白话结论（写给隐藏读者：用户）\n\n已填实。\n"
            ),
        }
    };
    let f1 = "0001-kfm-na-clean-report.md";
    let letters = vec![legacy(
        "0001",
        f1,
        "aabbccddeeff0011",
        "通报完毕（2026-09-28 评审：x）",
    )];
    let tokens = ledger_line("0001", f1, "aabbccddeeff0011") + "\n";
    let d = verify::verify_book(&book(&letters, &tokens, &v1, &r));
    assert!(d.errs.is_empty(), "{:?}", d.errs);
    assert_eq!(d.legacy_count, 1);

    // 存量信机读头占位残留 = d0 红
    let f2 = "0002-kfm-na-dirty-report.md";
    let letters = vec![legacy("0002", f2, "aabbccddeeff0022", "（待填")];
    let tokens = ledger_line("0002", f2, "aabbccddeeff0022") + "\n";
    let d = verify::verify_book(&book(&letters, &tokens, &v1, &r));
    assert!(
        d.errs.iter().any(|e| e.contains("占位未填")),
        "{:?}",
        d.errs
    );
}

#[test]
fn verify_book_unknown_name_hard_error_strict() {
    let v1: HashSet<String> = HashSet::new();
    let r = roster();
    let f1 = "0001号清和致评审部玄甲的提案.md";
    let letters = vec![LetterText {
        file: f1.into(),
        dir: "active".into(),
        text: v21_letter(
            f1,
            "aabbccddeeff0011",
            "评审部玄甲",
            "无（首信）",
            "待回信",
            "已填实。",
        ),
    }];
    let tokens = ledger_line("0001", f1, "aabbccddeeff0011") + "\n";
    // 严格模式：未登记名字 = 硬红
    let d = verify::verify_book(&book(&letters, &tokens, &v1, &r));
    assert!(
        d.errs.iter().any(|e| e.contains("不在名字池")),
        "{:?}",
        d.errs
    );
    // 降级开关：同样本转警告
    let soft = BookCheck {
        strict_pools: false,
        ..book(&letters, &tokens, &v1, &r)
    };
    let d = verify::verify_book(&soft);
    assert!(
        !d.errs.iter().any(|e| e.contains("不在名字池")),
        "{:?}",
        d.errs
    );
    assert!(
        d.warns.iter().any(|w| w.contains("不在名字池")),
        "{:?}",
        d.warns
    );
}

// ---------------------------------------------------------------
// 6. 池与名册
// ---------------------------------------------------------------

#[test]
fn pools_project_prefix_split_and_combo() {
    let r = roster();
    let (mut errs, mut warns) = (vec![], vec![]);
    // na-研究部清和：项目前缀拆分后组合合法
    check_pools(
        Some(&r),
        &[PoolEntry {
            who: "收件人".into(),
            func: Some("na-研究部".into()),
            name: "清和".into(),
        }],
        true,
        &mut errs,
        &mut warns,
    );
    assert!(errs.is_empty(), "{errs:?}");

    // 组合不在名册：清和 不是评审部
    check_pools(
        Some(&r),
        &[PoolEntry {
            who: "收件人".into(),
            func: Some("评审部".into()),
            name: "清和".into(),
        }],
        true,
        &mut errs,
        &mut warns,
    );
    assert!(
        errs.iter()
            .any(|e| e.contains("组合") && e.contains("不在名册内")),
        "{errs:?}"
    );

    // 职能出池
    let (mut errs, mut warns) = (vec![], vec![]);
    check_pools(
        Some(&r),
        &[PoolEntry {
            who: "发信人".into(),
            func: Some("研究".into()),
            name: "清和".into(),
        }],
        true,
        &mut errs,
        &mut warns,
    );
    assert!(errs.iter().any(|e| e.contains("不在职能池")), "{errs:?}");

    // 全体 跳过校验
    let (mut errs, mut warns) = (vec![], vec![]);
    check_pools(
        Some(&r),
        &[PoolEntry {
            who: "收件人".into(),
            func: None,
            name: "全体".into(),
        }],
        true,
        &mut errs,
        &mut warns,
    );
    assert!(errs.is_empty() && warns.is_empty());
}

// ---------------------------------------------------------------
// 7. gen 投影 golden（JS 实跑夹具产物逐字节对表）
// ---------------------------------------------------------------

fn fixture_letters() -> Vec<LetterText> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gen/letters");
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".md"))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|f| LetterText {
            text: std::fs::read_to_string(dir.join(&f)).unwrap(),
            file: f,
            dir: "active".into(),
        })
        .collect()
}

#[test]
fn gen_projection_golden_matches_js() {
    let letters = fixture_letters();
    let out = projection::render_gen(&letters, Some(&roster()));
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    assert_eq!(out.rows, 4);
    assert_eq!(out.active, 4);
    assert_eq!(out.archive, 0);

    // 两区段拼回 README.pre，必须逐字节等于 JS 回写产物 README.post
    let pre = include_str!("fixtures/gen/README.pre.md");
    let post = include_str!("fixtures/gen/README.post.md");
    let step1 = projection::splice_section(
        pre,
        projection::PENDING_START,
        projection::PENDING_END,
        &out.pending_section,
    )
    .unwrap();
    let step2 = projection::splice_section(
        &step1,
        projection::MARK_START,
        projection::MARK_END,
        &out.ledger_section,
    )
    .unwrap();
    assert_eq!(step2, post, "gen 区段渲染与 JS 不一致");

    // 派生索引逐字节等于 JS 产物
    let expected_index = include_str!("fixtures/gen/letters-index.expected.jsonl");
    assert_eq!(out.index_text, expected_index, "letters-index 与 JS 不一致");
}

#[test]
fn gen_check_only_detects_drift() {
    let letters = fixture_letters();
    let out = projection::render_gen(&letters, Some(&roster()));
    let pre = include_str!("fixtures/gen/README.pre.md");
    // 未回写的 pre（空标记段）≠ 应有区段 → 漂移判据成立
    let drifted = projection::splice_section(
        pre,
        projection::PENDING_START,
        projection::PENDING_END,
        &out.pending_section,
    )
    .unwrap();
    assert_ne!(drifted, pre);
    // 回写后再 render 应幂等（第二次 splice 不变）
    let step2 = projection::splice_section(
        &drifted,
        projection::MARK_START,
        projection::MARK_END,
        &out.ledger_section,
    )
    .unwrap();
    let again1 = projection::splice_section(
        &step2,
        projection::PENDING_START,
        projection::PENDING_END,
        &out.pending_section,
    )
    .unwrap();
    let again2 = projection::splice_section(
        &again1,
        projection::MARK_START,
        projection::MARK_END,
        &out.ledger_section,
    )
    .unwrap();
    assert_eq!(again2, step2, "回写非幂等");
}

#[test]
fn block_field_extracts_paper_blocks() {
    let letters = fixture_letters();
    let l1 = letters
        .iter()
        .find(|l| l.file.starts_with("0001号"))
        .unwrap();
    assert_eq!(
        projection::block_field(&l1.text, "类别").as_deref(),
        Some("征集")
    );
    assert_eq!(
        projection::block_field(&l1.text, "要办").as_deref(),
        Some("评审部表态")
    );
    assert_eq!(
        projection::block_field(&l1.text, "算完").as_deref(),
        Some("白露回信")
    );
    let l2 = letters
        .iter()
        .find(|l| l.file.starts_with("0002号"))
        .unwrap();
    assert_eq!(projection::block_field(&l2.text, "类别"), None);
}

// ---------------------------------------------------------------
// 8. 归属扫描（跨册口径的核内部分）
// ---------------------------------------------------------------

#[test]
fn scan_debt_for_matches_broadcast_and_name() {
    let letters = fixture_letters();
    let main_book: Vec<LetterText> = vec![{
        let file = "0016号白露致全体关于名字征集的提案.md";
        LetterText {
            file: file.into(),
            dir: "active".into(),
            text: v21_letter(
                file,
                "aabbccddeeff0011",
                "全体",
                "无（首信）",
                "待回信",
                "已填实。",
            ),
        }
    }];
    let books: Vec<(&str, &[LetterText])> =
        [("NA", letters.as_slice()), ("MAIN", main_book.as_slice())].to_vec();
    // --for=清和：主册「致全体」广播必须命中（跨册验收点的核内形态）
    let (errs, hits) = projection::scan_debts(&books, ScanTarget::For("清和"));
    assert!(errs.is_empty(), "{errs:?}");
    assert!(
        hits.iter()
            .any(|h| h.book == "MAIN" && h.file.contains("0016号")),
        "{hits:?}"
    );
    // 夹具里 0002 已裁决非欠账 → 不命中
    assert!(!hits.iter().any(|h| h.file.contains("0002号")), "{hits:?}");
    // --for=x1-NaMain：na 夹具旧信按词位命中
    let (_, hits) = projection::scan_debts(&books, ScanTarget::For("x1-NaMain"));
    assert!(
        hits.iter()
            .any(|h| h.book == "NA" && h.file.contains("0004-na-agent")),
        "{hits:?}"
    );
    // --by=清和：谁发的（限 v2.1 信）
    let (_, hits) = projection::scan_debts(&books, ScanTarget::By("清和"));
    assert!(
        hits.iter().any(|h| h.file.contains("0001号清和")),
        "{hits:?}"
    );
    assert!(!hits.iter().any(|h| h.file.contains("0016号")), "{hits:?}");
}

// ---------------------------------------------------------------
// 9. new 生成件（核内部分）+ next_number
// ---------------------------------------------------------------

#[test]
fn newletter_filename_and_skeleton_shape() {
    let display = vec!["评审部白露".to_string(), "开发部观澜".to_string()];
    let file = newletter::build_v21_file_name(
        "0001",
        "",
        "清和",
        &display,
        Some("0004"),
        Some("测试事由"),
        "通报",
    );
    assert_eq!(
        file,
        "0001号清和致评审部白露及开发部观澜复0004关于测试事由的通报.md"
    );

    // 三条收件人 → 前二 + 等
    let display3 = vec![
        "评审部白露".to_string(),
        "开发部观澜".to_string(),
        "研究部小满".to_string(),
    ];
    let file = newletter::build_v21_file_name("0009", "NA", "清和", &display3, None, None, "提案");
    assert_eq!(file, "NA0009号清和致评审部白露及开发部观澜等的提案.md");

    // 骨架含令牌占位三连换行（JS 用 \n\n\n 定位替换）
    let p = SkeletonParams {
        title: "测试标题",
        date_stamp: "2026-09-28 23:25 +08:00",
        from_func: "研究部",
        from_name: "清和",
        display: &display,
        reply: Some("0004"),
        status: "待回信",
        kind: "征集",
        expect: "预期表态",
        criteria: "收敛判据",
    };
    let skel = newletter::build_v21_skeleton(&p);
    assert!(skel.contains("\n\n\n"), "骨架缺令牌插入位");
    let token = token::token_line("0001", "3643be10ce49827a", "9897ac27606e4fde");
    let filled = newletter::insert_token(&skel, &token);
    assert!(!filled.contains("\n\n\n"));
    assert!(filled.contains(&format!("> 状态: 待回信\n{token}\n\n## 白话结论")));
    assert!(skel.ends_with('\n'), "骨架末行换行（join 尾元素为空串）");
}

#[test]
fn next_number_scans_files_and_ledger() {
    let files = vec![
        "0001-kfm-na-x-report.md".to_string(),
        "NA0012号茉莉致研究部空谷的通报.md".to_string(),
        "0009号空山的日报.md".to_string(),
        "README.md".to_string(),
    ];
    assert_eq!(newletter::next_number(&files, &["0015", "0002"]), "0016");
    assert_eq!(newletter::next_number(&[], &[]), "0001");
}

#[test]
fn ledger_record_key_order_and_escaping() {
    let line = newletter::ledger_record_line(
        "0001",
        "0001号白露致全体的提案.md",
        "f6cc45ae27303c68",
        "a74021ec34eee976",
        "2026-09-28T15:25:45.842Z",
        "白露",
        None,
    );
    assert_eq!(
        line,
        r#"{"no":"0001","file":"0001号白露致全体的提案.md","nonce":"f6cc45ae27303c68","fp":"a74021ec34eee976","tpl":"v2","createdAt":"2026-09-28T15:25:45.842Z","from":"白露"}"#
    );
    let with_from = newletter::ledger_record_line(
        "0016",
        "0016号白露致全体关于名字征集的提案.md",
        "c3d44cb28ae48043",
        "418022fb85d60041",
        "2026-09-28T07:28:34.804Z",
        "白露",
        Some("0016号白露致全体的提案.md"),
    );
    assert!(
        with_from.ends_with(r#","renamedFrom":"0016号白露致全体的提案.md"}"#),
        "{with_from}"
    );
}

// ---------------------------------------------------------------
// 10. 令牌行解析 / 台账解析
// ---------------------------------------------------------------

#[test]
fn token_line_parse_and_ledger_revoke_split() {
    let text =
        "前言\n<!-- LETTER-TOKEN v2 no=NA0012 nonce=0011223344556677 fp=8899aabbccddeeff -->\n后文";
    let hit = token::find_token(text).expect("应找到令牌行");
    assert_eq!(hit.no, "NA0012");
    assert_eq!(hit.nonce, "0011223344556677");
    assert_eq!(hit.fp, "8899aabbccddeeff");
    assert!(token::find_token("没有令牌").is_none());

    let ledger_text = concat!(
        "{\"no\":\"0001\",\"file\":\"a.md\",\"nonce\":\"n1n1n1n1n1n1n1n1\",\"fp\":\"f1f1f1f1f1f1f1f1\",\"tpl\":\"v2\",\"createdAt\":\"x\",\"from\":\"t\",\"revokedAt\":\"y\",\"revokeReason\":\"r\"}\n",
        "{\"no\":\"0001\",\"file\":\"b.md\",\"nonce\":\"n2n2n2n2n2n2n2n2\",\"fp\":\"f2f2f2f2f2f2f2f2\",\"tpl\":\"v2\",\"createdAt\":\"x\",\"from\":\"t\",\"renamedFrom\":\"a.md\"}\n",
        "不是 json\n",
    );
    let led = token::parse_ledger(ledger_text);
    assert_eq!(led.revoked.len(), 1);
    assert_eq!(led.current.len(), 1);
    assert_eq!(led.current[0].renamed_from.as_deref(), Some("a.md"));
    assert_eq!(led.errs.len(), 1, "坏行应记错：{:?}", led.errs);
}
