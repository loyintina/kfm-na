//! CLI 端到端 golden 对表：真跑编译产物，产物与 JS 三件套实跑抄录件逐字节比。
//! 全程不依赖 /root/kfmv4 与 /root/.kfm（手机 Termux 也跑 chain）——名册/状态词表走 fixture。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_mailbox-cli");
const ROSTER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/roster.json");
const GEN_FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/gen");
const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");

const LETTER_A: &str = "0001号清和致评审部白露及开发部观澜复0004关于测试事由的通报.md";
const LETTER_B: &str = "0001号白露致全体的提案.md";

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("跑 {BIN} 失败：{e}"))
}

fn assert_ok(out: &Output, ctx: &str) {
    assert!(
        out.status.success(),
        "{ctx} 应 exit 0：\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn assert_fail(out: &Output, ctx: &str) {
    assert!(
        !out.status.success(),
        "{ctx} 应非零 exit：\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "mailbox-cli-test-{tag}-{}-{tag}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn read(p: &Path) -> Vec<u8> {
    fs::read(p).unwrap_or_else(|e| panic!("读 {} 失败：{e}", p.display()))
}

/// 逐字节比：不等就把两边落在 /tmp 供人工 diff
fn assert_bytes_eq(actual: &[u8], expected: &[u8], ctx: &str) {
    if actual != expected {
        fs::write("/tmp/mb-got.bin", actual).ok();
        fs::write("/tmp/mb-want.bin", expected).ok();
        panic!("{ctx} 逐字节不等（got=/tmp/mb-got.bin want=/tmp/mb-want.bin）");
    }
}

// ---------------------------------------------------------------
// new：A/B 两份 golden 逐字节对表（JS new-letter.mjs 实跑抄录）
// ---------------------------------------------------------------

#[test]
fn new_a_byte_exact() {
    let d = tmpdir("new-a");
    let mb = d.to_str().unwrap().to_string();
    let out = run(&[
        "new",
        "--mailbox",
        &mb,
        "--roster",
        ROSTER,
        "--from-func",
        "研究部",
        "--from-name",
        "清和",
        "--to",
        "评审部白露",
        "--to",
        "开发部观澜",
        "--reply",
        "0004",
        "--about",
        "测试事由",
        "--type",
        "通报",
        "--title",
        "测试标题",
        "--kind",
        "征集",
        "--expect",
        "预期表态",
        "--criteria",
        "收敛判据",
        "--status",
        "待回信",
        "--now-local",
        "2026-09-28 23:25 +08:00",
        "--now-utc",
        "2026-09-28T15:25:45.804Z",
        "--nonce",
        "3643be10ce49827a",
    ]);
    assert_ok(&out, "new A");
    assert_bytes_eq(
        &read(&d.join(LETTER_A)),
        &read(Path::new(GOLDEN).join("new_a.md").as_path()),
        "new A 信件",
    );
    assert_bytes_eq(
        &read(&d.join("letter-tokens.jsonl")),
        &read(Path::new(GOLDEN).join("new_a_tokens.jsonl").as_path()),
        "new A 台账",
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn new_b_byte_exact() {
    let d = tmpdir("new-b");
    let mb = d.to_str().unwrap().to_string();
    let out = run(&[
        "new",
        "--mailbox",
        &mb,
        "--roster",
        ROSTER,
        "--from-func",
        "评审部",
        "--from-name",
        "白露",
        "--to-all",
        "--type",
        "提案",
        "--title",
        "全体标题",
        "--now-local",
        "2026-09-28 23:25 +08:00",
        "--now-utc",
        "2026-09-28T15:25:45.842Z",
        "--nonce",
        "f6cc45ae27303c68",
    ]);
    assert_ok(&out, "new B");
    assert_bytes_eq(
        &read(&d.join(LETTER_B)),
        &read(Path::new(GOLDEN).join("new_b.md").as_path()),
        "new B 信件",
    );
    assert_bytes_eq(
        &read(&d.join("letter-tokens.jsonl")),
        &read(Path::new(GOLDEN).join("new_b_tokens.jsonl").as_path()),
        "new B 台账",
    );
    let _ = fs::remove_dir_all(&d);
}

// ---------------------------------------------------------------
// gen：夹具册 回写→check-only 绿→篡改→check-only 红
// ---------------------------------------------------------------

fn setup_gen_book(tag: &str) -> PathBuf {
    let d = tmpdir(tag);
    for f in fs::read_dir(GEN_FIX).unwrap().flatten() {
        let name = f.file_name().into_string().unwrap();
        if name.ends_with(".md") && !name.starts_with("README") {
            fs::copy(f.path(), d.join(&name)).unwrap();
        }
    }
    fs::copy(
        Path::new(GEN_FIX).join("README.pre.md"),
        d.join("README.md"),
    )
    .unwrap();
    d
}

#[test]
fn gen_roundtrip_and_drift() {
    let d = setup_gen_book("gen");
    let mb = d.to_str().unwrap().to_string();
    let out = run(&["gen", "--mailbox", &mb, "--roster", ROSTER]);
    assert_ok(&out, "gen 回写");
    assert_bytes_eq(
        &read(&d.join("README.md")),
        &read(Path::new(GEN_FIX).join("README.post.md").as_path()),
        "gen README 回写",
    );
    assert_bytes_eq(
        &read(&d.join("letters-index.jsonl")),
        &read(
            Path::new(GEN_FIX)
                .join("letters-index.expected.jsonl")
                .as_path(),
        ),
        "gen 派生索引",
    );
    // 幂等：再 check-only 应绿
    let out = run(&["gen", "--mailbox", &mb, "--roster", ROSTER, "--check-only"]);
    assert_ok(&out, "gen --check-only（回写后）");
    // 篡改台账区段 → check-only 红
    let readme = fs::read_to_string(d.join("README.md")).unwrap();
    let tampered = readme.replacen(
        "<!-- gen:pending:end -->",
        "手改一行\n<!-- gen:pending:end -->",
        1,
    );
    fs::write(d.join("README.md"), tampered).unwrap();
    let out = run(&["gen", "--mailbox", &mb, "--roster", ROSTER, "--check-only"]);
    assert_fail(&out, "gen --check-only（篡改后）");
    let _ = fs::remove_dir_all(&d);
}

// ---------------------------------------------------------------
// verify：单信（填实白话块后绿 / 改 fp 红）+ 全册执法
// ---------------------------------------------------------------

/// 以 golden A 建册并把白话块/正文占位填实（fp 不锚正文，填实不动票面）。
/// 册内另补一封 0004（A 的「复」指向它；0002/0003 以撤销票垫号让 new 分到 0004），
/// 供全册执法的「被复信存在性」判据咬合。
fn setup_verified_book(tag: &str) -> PathBuf {
    let d = tmpdir(tag);
    let letter = fs::read_to_string(Path::new(GOLDEN).join("new_a.md")).unwrap()
        .replace(
            "（待填：面向隐藏读者。三句话内说清是什么事、要不要他做事；不写工作术语。）",
            "这是一封测试信，不需要你做任何事。",
        )
        .replace(
            "（待填：结论与推导一起写——精确留给承重的数字和名字，其余白话；每条发现标「明写／推断／补全」。）",
            "（明写）测试正文。",
        );
    fs::write(d.join(LETTER_A), letter).unwrap();
    let mut ledger = fs::read_to_string(Path::new(GOLDEN).join("new_a_tokens.jsonl")).unwrap();
    for pad in ["0002", "0003"] {
        ledger.push_str(&format!(
            "{{\"no\":\"{pad}\",\"file\":\"{pad}号清和致评审部白露的通报.md\",\"nonce\":\"00\",\"fp\":\"00\",\"tpl\":\"v2\",\"createdAt\":\"2026-09-28T00:00:00.000Z\",\"from\":\"清和\",\"revokedAt\":\"2026-09-28T01:00:00.000Z\",\"revokeReason\":\"垫号\"}}\n"
        ));
    }
    fs::write(d.join("letter-tokens.jsonl"), ledger).unwrap();
    // new 分到 0004：白露回清和的首信
    let mb = d.to_str().unwrap().to_string();
    let out = run(&[
        "new",
        "--mailbox",
        &mb,
        "--roster",
        ROSTER,
        "--from-func",
        "评审部",
        "--from-name",
        "白露",
        "--to",
        "研究部清和",
        "--type",
        "通报",
        "--title",
        "被复信",
        "--now-local",
        "2026-09-28 23:30 +08:00",
        "--now-utc",
        "2026-09-28T15:30:00.000Z",
        "--nonce",
        "0011223344556677",
    ]);
    assert_ok(&out, "垫 0004 被复信");
    let letter4 = d.join("0004号白露致研究部清和的通报.md");
    let filled = fs::read_to_string(&letter4).unwrap()
        .replace(
            "（待填：面向隐藏读者。三句话内说清是什么事、要不要他做事；不写工作术语。）",
            "这是被复的那封信，不需要你做任何事。",
        )
        .replace(
            "（待填：结论与推导一起写——精确留给承重的数字和名字，其余白话；每条发现标「明写／推断／补全」。）",
            "（明写）被复信正文。",
        );
    fs::write(&letter4, filled).unwrap();
    d
}

#[test]
fn verify_single_ok_and_bad_fp() {
    let d = setup_verified_book("verify");
    let mb = d.to_str().unwrap().to_string();
    let lp = d.join(LETTER_A).to_str().unwrap().to_string();
    let out = run(&["verify", &lp, "--mailbox", &mb, "--roster", ROSTER]);
    assert_ok(&out, "verify 单信（合规）");
    // fp 翻一位 → 红
    let text = fs::read_to_string(d.join(LETTER_A)).unwrap();
    let bad = text.replacen("fp=9897ac27606e4fde", "fp=9897ac27606e4fdf", 1);
    fs::write(d.join(LETTER_A), bad).unwrap();
    let out = run(&["verify", &lp, "--mailbox", &mb, "--roster", ROSTER]);
    assert_fail(&out, "verify 单信（fp 被改）");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn verify_book_sweep() {
    let d = setup_verified_book("verify-book");
    let mb = d.to_str().unwrap().to_string();
    let out = run(&["verify", "--mailbox", &mb, "--roster", ROSTER]);
    assert_ok(&out, "verify 全册（合规）");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("[mailbox-verify] OK"), "输出前缀：{stdout}");
    // 令牌行整行删掉 → 全册红
    let text = fs::read_to_string(d.join(LETTER_A)).unwrap();
    let no_tok: String = text
        .lines()
        .filter(|l| !l.contains("LETTER-TOKEN"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(d.join(LETTER_A), no_tok).unwrap();
    let out = run(&["verify", "--mailbox", &mb, "--roster", ROSTER]);
    assert_fail(&out, "verify 全册（令牌被删）");
    let _ = fs::remove_dir_all(&d);
}

// ---------------------------------------------------------------
// reticket：改名换票（契约 §八；票面对主册 0016 换票行形制）
// ---------------------------------------------------------------

#[test]
fn reticket_renames_and_revokes() {
    let d = setup_verified_book("reticket");
    let mb = d.to_str().unwrap().to_string();
    let old = d.join(LETTER_A).to_str().unwrap().to_string();
    let new_name = "0001号清和致评审部白露及开发部观澜复0004关于改名测试的通报.md";
    let out = run(&[
        "reticket",
        &old,
        "--mailbox",
        &mb,
        "--new-name",
        new_name,
        "--reason",
        "改名窗口内换票：事由段测试（契约 §八）",
        "--now-utc",
        "2026-09-29T01:00:00.000Z",
        "--nonce",
        "aabbccddeeff0011",
    ]);
    assert_ok(&out, "reticket");
    assert!(!d.join(LETTER_A).exists(), "旧文件应被改名移除");
    assert!(d.join(new_name).exists(), "新文件应存在");

    let ledger = fs::read_to_string(d.join("letter-tokens.jsonl")).unwrap();
    let lines: Vec<&str> = ledger.lines().collect();
    assert_eq!(
        lines.len(),
        5,
        "台账应 5 行（0001 撤销 + 垫号×2 + 0004 + 0001 新票）：{ledger}"
    );
    let old_row = lines[0];
    let new_row = lines[4];
    assert!(
        old_row.contains("\"revokedAt\":\"2026-09-29T01:00:00.000Z\"")
            && old_row.contains("\"revokeReason\":\"改名窗口内换票：事由段测试（契约 §八）\"")
            && old_row.contains(&format!("\"file\":\"{LETTER_A}\"")),
        "旧票应补 revokedAt/revokeReason：{old_row}"
    );
    assert!(
        new_row.contains(&format!("\"file\":\"{new_name}\""))
            && new_row.contains(&format!("\"renamedFrom\":\"{LETTER_A}\""))
            && new_row.contains("\"nonce\":\"aabbccddeeff0011\"")
            && !new_row.contains("revokedAt"),
        "新票应含 renamedFrom、新 nonce、无撤销字段：{new_row}"
    );
    // 新票 fp = sha256("0001|aabbccddeeff0011|<新名>|v2") 前16——与换票后信内令牌行互咬
    let letter = fs::read_to_string(d.join(new_name)).unwrap();
    let fp_in_letter = extract_fp(&letter);
    assert!(
        new_row.contains(&format!("\"fp\":\"{fp_in_letter}\"")),
        "信内令牌 fp 与新票 fp 应一致：{fp_in_letter} vs {new_row}"
    );
    // 换票后全册执法仍绿（新旧票配对、无孤儿票）
    let out = run(&["verify", "--mailbox", &mb, "--roster", ROSTER]);
    assert_ok(&out, "reticket 后全册执法");
    let _ = fs::remove_dir_all(&d);
}

/// 信内令牌行 fp= 后 16 位 hex
fn extract_fp(text: &str) -> &str {
    let i = text.find("fp=").expect("令牌行缺 fp=") + 3;
    let hex = &text[i..i + 16];
    assert!(
        hex.bytes().all(|b| b.is_ascii_hexdigit()),
        "fp 非 16 位 hex：{hex}"
    );
    hex
}
