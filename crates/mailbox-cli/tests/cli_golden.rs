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
    // 册身份（契约 §六）：夹具册是新册，落身份文件 MAIN（缺文件时非主册路径会
    // 被 gen 拒并要求 --book-sorting；各钉不必逐条传该选项）
    fs::write(
        d.join(".mailbox.json"),
        "{\"sorting\":\"MAIN\",\"name\":\"fixture\"}\n",
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

/// 册身份（契约 §六《册身份》）钉：索引 sorting 的兜底 = 本册身份码，不再是
/// 硬编码 MAIN。考卷四幕：①非主册 + 无身份文件 → 拒（要 --book-sorting）；
/// ②落身份 NA → 索引 sorting 全 NA 且逐字节等于 JS 实跑抄录件；③无身份文件 +
/// 显式 --book-sorting NA → 同产物；④身份 NA 与 --book-sorting MAIN 冲突 → 拒。
/// 变异方向：兜底写回 "MAIN" → ②红；去掉①的要求（静默 MAIN）→ ①红。
#[test]
fn gen_book_identity_none_main_requires_flag() {
    // ① 非主册 + 无身份文件 + 无 --book-sorting → 拒，且不写盘
    let d = setup_gen_book("book-id");
    fs::remove_file(d.join(".mailbox.json")).unwrap();
    let mb = d.to_str().unwrap().to_string();
    let readme_before = read(&d.join("README.md"));
    let out = run(&["gen", "--mailbox", &mb, "--roster", ROSTER]);
    assert_fail(&out, "①非主册缺身份文件必须拒（不静默写 MAIN）");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("警告") && stderr.contains("--book-sorting"),
        "报错应点名「警告」与 --book-sorting：{stderr}"
    );
    assert!(
        !d.join("letters-index.jsonl").exists(),
        "被拒后不得落衍生索引"
    );
    assert_eq!(
        read(&d.join("README.md")),
        readme_before,
        "被拒后 README 不得变动"
    );
    fs::remove_file(d.join("letters-index.jsonl")).ok();

    // ② 落身份 NA → 索引 sorting 全 NA，逐字节等于 JS 实跑抄录件
    fs::copy(
        Path::new(GEN_FIX).join("mailbox-na.json"),
        d.join(".mailbox.json"),
    )
    .unwrap();
    let out = run(&["gen", "--mailbox", &mb, "--roster", ROSTER]);
    assert_ok(&out, "②身份 NA 应绿");
    assert_bytes_eq(
        &read(&d.join("letters-index.jsonl")),
        &read(
            Path::new(GEN_FIX)
                .join("letters-index.na.expected.jsonl")
                .as_path(),
        ),
        "②NA 册身份派生索引",
    );
    // ③ 换回「无身份文件 + 显式 --book-sorting NA」→ 同产物
    fs::remove_file(d.join(".mailbox.json")).unwrap();
    let out = run(&[
        "gen",
        "--mailbox",
        &mb,
        "--roster",
        ROSTER,
        "--book-sorting",
        "NA",
    ]);
    assert_ok(&out, "③显式 --book-sorting 兜底应绿");
    assert_bytes_eq(
        &read(&d.join("letters-index.jsonl")),
        &read(
            Path::new(GEN_FIX)
                .join("letters-index.na.expected.jsonl")
                .as_path(),
        ),
        "③--book-sorting NA 派生索引与身份文件同产物",
    );
    // ④ 身份 NA 与 --book-sorting MAIN 冲突 → 拒（册码是事实，不许参数压过）
    fs::copy(
        Path::new(GEN_FIX).join("mailbox-na.json"),
        d.join(".mailbox.json"),
    )
    .unwrap();
    let out = run(&[
        "gen",
        "--mailbox",
        &mb,
        "--roster",
        ROSTER,
        "--book-sorting",
        "MAIN",
    ]);
    assert_fail(&out, "④身份与 --book-sorting 冲突必须拒");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("冲突"), "报错应点名冲突：{stderr}");
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
    // 册身份（契约 §六）：本册也是新册，落身份文件 MAIN——BAR-177 换票收尾会
    // 调投影回写，缺身份文件会被 gen 拒（非主册路径）
    fs::write(
        d.join(".mailbox.json"),
        "{\"sorting\":\"MAIN\",\"name\":\"fixture\"}\n",
    )
    .unwrap();
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
    // BAR-177 起 reticket 扫两册复信——钉必须不依赖宿主主册，指到不存在路径
    let no_main = d.join("no-main-book").to_str().unwrap().to_string();
    let old = d.join(LETTER_A).to_str().unwrap().to_string();
    let new_name = "0001号清和致评审部白露及开发部观澜复0004关于改名测试的通报.md";
    let out = run(&[
        "reticket",
        &old,
        "--mailbox",
        &mb,
        "--roster",
        ROSTER,
        "--main-book",
        &no_main,
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

// ---------------------------------------------------------------
// BAR-177：reticket 四项必修判据钉（白露 0010 审查 §二/§三）
// ---------------------------------------------------------------

/// 窗口拒绝钉（必修①考卷）：状态已翻（非待*）的信、已有复信的信，
/// reticket 必须拒（契约 §八第 7 条）；--force 逃生必须把 force 事实与
/// 改动内容写进撤销票 revokeReason。顺带 ④：新名降级旧 ASCII 形态必须拒
/// （报错指向契约 §二）。变异方向：窗口判据/④文法跳检摘除 → 本题全红
#[test]
fn spec_bar177_reticket_窗口拒绝与force留痕() {
    // 场景一：状态已翻「已回」→ 拒，且信件/台账分毫不动
    let d = setup_verified_book("bar177-window");
    let mb = d.to_str().unwrap().to_string();
    let no_main = d.join("no-main-book").to_str().unwrap().to_string();
    let old = d.join(LETTER_A).to_str().unwrap().to_string();
    let new_name = "0001号清和致评审部白露及开发部观澜复0004关于窗口判据的通报.md";
    let text = fs::read_to_string(d.join(LETTER_A)).unwrap();
    fs::write(
        d.join(LETTER_A),
        text.replacen("> 状态: 待回信", "> 状态: 已回", 1),
    )
    .unwrap();
    let ledger_before = fs::read_to_string(d.join("letter-tokens.jsonl")).unwrap();
    let out = run(&[
        "reticket",
        &old,
        "--mailbox",
        &mb,
        "--roster",
        ROSTER,
        "--main-book",
        &no_main,
        "--new-name",
        new_name,
        "--now-utc",
        "2026-09-29T02:00:00.000Z",
        "--nonce",
        "aabbccddeeff0022",
    ]);
    assert_fail(&out, "BAR-177 窗口：状态已翻必须拒");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("改名窗口已关闭") && stderr.contains("已回"),
        "报错应点名窗口与状态：{stderr}"
    );
    assert!(d.join(LETTER_A).exists() && !d.join(new_name).exists());
    assert_eq!(
        fs::read_to_string(d.join("letter-tokens.jsonl")).unwrap(),
        ledger_before,
        "被拒后台账不得变动"
    );
    // 场景二：同信 --force 逃生 → 成功，撤销票 revokeReason 带 force 事实与改动内容
    let out = run(&[
        "reticket",
        &old,
        "--mailbox",
        &mb,
        "--roster",
        ROSTER,
        "--main-book",
        &no_main,
        "--new-name",
        new_name,
        "--reason",
        "钉 force 留痕",
        "--force",
        "--now-utc",
        "2026-09-29T02:00:00.000Z",
        "--nonce",
        "aabbccddeeff0022",
    ]);
    assert_ok(&out, "BAR-177 force 逃生闸");
    let ledger = fs::read_to_string(d.join("letter-tokens.jsonl")).unwrap();
    assert!(
        ledger.contains("【force 强制换票】")
            && ledger.contains("已回")
            && ledger.contains(&format!("{LETTER_A} → {new_name}")),
        "revokeReason 须写清 force 事实与改动内容：{ledger}"
    );
    let _ = fs::remove_dir_all(&d);

    // 场景三：已有复信的信（LETTER_A 复: 0004 → 0004 号信已有回应）→ 拒
    let d = setup_verified_book("bar177-replied");
    let mb = d.to_str().unwrap().to_string();
    let no_main = d.join("no-main-book").to_str().unwrap().to_string();
    let old4 = d
        .join("0004号白露致研究部清和的通报.md")
        .to_str()
        .unwrap()
        .to_string();
    let new4 = "0004号白露致研究部清和关于回件后改名的通报.md";
    let out = run(&[
        "reticket",
        &old4,
        "--mailbox",
        &mb,
        "--roster",
        ROSTER,
        "--main-book",
        &no_main,
        "--new-name",
        new4,
        "--now-utc",
        "2026-09-29T02:00:00.000Z",
        "--nonce",
        "aabbccddeeff0044",
    ]);
    assert_fail(&out, "BAR-177 窗口：已有复信必须拒");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("改名窗口已关闭")
            && stderr.contains("已有复信")
            && stderr.contains(LETTER_A),
        "报错应点名复信：{stderr}"
    );
    // 场景四（必修④）：新名降级旧 ASCII 形态 → 拒，报错指向契约 §二
    let old = d.join(LETTER_A).to_str().unwrap().to_string();
    let out = run(&[
        "reticket",
        &old,
        "--mailbox",
        &mb,
        "--roster",
        ROSTER,
        "--main-book",
        &no_main,
        "--new-name",
        "0001-kfm-na-downgrade-report.md",
        "--now-utc",
        "2026-09-29T02:00:00.000Z",
        "--nonce",
        "aabbccddeeff0055",
    ]);
    assert_fail(&out, "BAR-177 ④：降级改名必须拒");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("v2.1") && stderr.contains("§二"),
        "报错应指向契约 §二：{stderr}"
    );
    let _ = fs::remove_dir_all(&d);
}

/// 换票后投影复绿钉（必修②考卷）：窗口内换票后 na 册直接回写投影，
/// gen 再跑幂等、verify + gen --check-only 全绿。变异方向：收尾投影摘除 →
/// 「check-only 当场绿」红
#[test]
fn spec_bar177_reticket_投影复绿() {
    let d = setup_verified_book("bar177-proj");
    // 带 gen 标记与状态词表的 README 进册（收尾投影回写的落点）
    fs::write(
        d.join("README.md"),
        "# BAR-177 钉夹具\n\n合法状态词表（测试钉）：待回信 / 已回 / 已落地 / 已验证。\n\n<!-- gen:pending:start -->\n<!-- gen:pending:end -->\n\n## 信件清单\n\n<!-- gen:agent-inbox:start -->\n<!-- gen:agent-inbox:end -->\n",
    )
    .unwrap();
    let mb = d.to_str().unwrap().to_string();
    let no_main = d.join("no-main-book").to_str().unwrap().to_string();
    let old = d.join(LETTER_A).to_str().unwrap().to_string();
    let new_name = "0001号清和致评审部白露及开发部观澜复0004关于投影回写的通报.md";
    let out = run(&[
        "reticket",
        &old,
        "--mailbox",
        &mb,
        "--roster",
        ROSTER,
        "--main-book",
        &no_main,
        "--new-name",
        new_name,
        "--now-utc",
        "2026-09-29T02:10:00.000Z",
        "--nonce",
        "aabbccddeeff0033",
    ]);
    assert_ok(&out, "BAR-177 窗口内换票");
    // 收尾已直接回写：check-only 当场绿（不先跑 gen）
    let out = run(&["gen", "--mailbox", &mb, "--roster", ROSTER, "--check-only"]);
    assert_ok(
        &out,
        "BAR-177 换票后 gen --check-only 当场绿（投影已随换票回写）",
    );
    // 考卷口径：gen 回写幂等 + verify 全册绿 + check-only 复绿
    let out = run(&["gen", "--mailbox", &mb, "--roster", ROSTER]);
    assert_ok(&out, "BAR-177 换票后 gen 回写幂等");
    let out = run(&["verify", "--mailbox", &mb, "--roster", ROSTER]);
    assert_ok(&out, "BAR-177 换票后 verify 全册绿");
    let out = run(&["gen", "--mailbox", &mb, "--roster", ROSTER, "--check-only"]);
    assert_ok(&out, "BAR-177 gen --check-only 复绿");
    let _ = fs::remove_dir_all(&d);
}

/// ⑦防篡改钉：台账现行票 nonce 被改 → reticket 必须拒（修复前旧实现是
/// 恒真死码——拿信内令牌反拼行再 contains，台账改坏照过 exit 0）
#[test]
fn spec_bar177_reticket_台账票面篡改即拒() {
    let d = setup_verified_book("bar177-tamper");
    let mb = d.to_str().unwrap().to_string();
    let no_main = d.join("no-main-book").to_str().unwrap().to_string();
    let ledger = fs::read_to_string(d.join("letter-tokens.jsonl")).unwrap();
    let bad = ledger.replacen(
        "\"nonce\":\"3643be10ce49827a\"",
        "\"nonce\":\"3643be10ce49827b\"",
        1,
    );
    assert_ne!(bad, ledger, "夹具前提：台账应含 0001 现行票 nonce");
    fs::write(d.join("letter-tokens.jsonl"), bad).unwrap();
    let old = d.join(LETTER_A).to_str().unwrap().to_string();
    let out = run(&[
        "reticket",
        &old,
        "--mailbox",
        &mb,
        "--roster",
        ROSTER,
        "--main-book",
        &no_main,
        "--new-name",
        "0001号清和致评审部白露及开发部观澜复0004关于篡改钉的通报.md",
        "--now-utc",
        "2026-09-29T02:20:00.000Z",
        "--nonce",
        "aabbccddeeff0066",
    ]);
    assert_fail(&out, "BAR-177 ⑦：台账 nonce 被改必须拒");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("台账现行票与信内令牌不符"),
        "报错应点名票面对咬：{stderr}"
    );
    assert!(d.join(LETTER_A).exists(), "被拒后旧信应在");
    let _ = fs::remove_dir_all(&d);
}

/// ⑧脏台账钉：台账预置同号双现行票（parse_ledger 必记 errs）→ reticket
/// 必须拒（修复前忽略 ledger.errs 照加票）
#[test]
fn spec_bar177_reticket_脏台账即拒() {
    let d = setup_verified_book("bar177-dirty");
    let mb = d.to_str().unwrap().to_string();
    let no_main = d.join("no-main-book").to_str().unwrap().to_string();
    let mut ledger = fs::read_to_string(d.join("letter-tokens.jsonl")).unwrap();
    ledger.push_str(
        "{\"no\":\"0001\",\"file\":\"幽灵.md\",\"nonce\":\"00\",\"fp\":\"00\",\"tpl\":\"v2\",\"createdAt\":\"2026-09-29T00:00:00.000Z\",\"from\":\"清和\"}\n",
    );
    fs::write(d.join("letter-tokens.jsonl"), ledger).unwrap();
    let old = d.join(LETTER_A).to_str().unwrap().to_string();
    let out = run(&[
        "reticket",
        &old,
        "--mailbox",
        &mb,
        "--roster",
        ROSTER,
        "--main-book",
        &no_main,
        "--new-name",
        "0001号清和致评审部白露及开发部观澜复0004关于脏账钉的通报.md",
        "--now-utc",
        "2026-09-29T02:30:00.000Z",
        "--nonce",
        "aabbccddeeff0077",
    ]);
    assert_fail(&out, "BAR-177 ⑧：脏台账必须拒");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("台账自身有错"),
        "报错应点名脏台账：{stderr}"
    );
    let _ = fs::remove_dir_all(&d);
}

// ---------------------------------------------------------------
// BAR-173：gen_nonce 用 fs::read 读 /dev/urandom——无限设备文件读至 OOM
// （真机实爆：reticket 真跑 anon-rss 冲 10G 被 oom-kill）。病灶根 = 全部既有
// 考题都注了 --nonce，随机路从未被跑到。钉：不注 --nonce 的 new 必须返回
// 且 nonce 为 16 位 hex（修复前本钉表现为挂死，修复后秒绿）。
// ---------------------------------------------------------------

#[test]
fn spec_bar173_new_不注nonce_随机路定长秒回() {
    let d = tmpdir("bar173");
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
        "--to-all",
        "--type",
        "通报",
        "--title",
        "nonce 钉",
        "--now-local",
        "2026-09-29 01:20 +08:00",
        "--now-utc",
        "2026-09-29T01:20:00.000Z",
    ]);
    assert_ok(&out, "BAR-173 new 不注 nonce（修复前此路挂死读 urandom）");
    let letter = fs::read_to_string(d.join("0001号清和致全体的通报.md")).unwrap();
    let i = letter.find("nonce=").expect("信内缺 nonce") + 6;
    let nonce = &letter[i..i + 16];
    assert!(
        nonce.bytes().all(|b| b.is_ascii_hexdigit()),
        "nonce 非 16 位 hex：{nonce}"
    );
    let _ = fs::remove_dir_all(&d);
}

// ---------------------------------------------------------------
// BAR-180：new 去自指码 + reticket §八.8 格式性勘误窄例外 + 例外之外改号仍拒
// ---------------------------------------------------------------

use mailbox_core::token::fingerprint;

/// 带 NA 册身份的夹具册：NA0007 令牌信（状态 通报完毕 = 窗口关闭）+
/// 0008 复信（复: NA0007，窗口实锤关闭——白露 NA0015→0015 先例同款场景）
fn setup_na_book(tag: &str) -> PathBuf {
    let d = tmpdir(tag);
    fs::write(
        d.join(".mailbox.json"),
        "{\"sorting\":\"NA\",\"name\":\"fixture\"}\n",
    )
    .unwrap();
    let old = "NA0007号清和致评审部白露的通报.md";
    let nonce = "aabbccddeeff0007";
    let fp = fingerprint("NA0007", nonce, old);
    fs::write(
        d.join(old),
        format!(
            "# 测试信\n\n> 日期: 2026-09-29 10:00 +08:00\n> 从: 研究部清和\n> 致: 评审部白露\n> 复: 无（首信）\n> 状态: 通报完毕（2026-09-29 10:30 +08:00 研究部清和 更新：钉）\n\n<!-- LETTER-TOKEN v2 no=NA0007 nonce={nonce} fp={fp} -->\n\n## 摘要\n\n已填实，不需要你做任何事。\n\n## 正文\n\n正文。\n"
        ),
    )
    .unwrap();
    let f8 = "0008号白露致研究部清和复NA0007的通报.md";
    let n8 = "aabbccddeeff0008";
    let fp8 = fingerprint("0008", n8, f8);
    fs::write(
        d.join(f8),
        format!(
            "# 回复\n\n> 日期: 2026-09-29 10:20 +08:00\n> 从: 评审部白露\n> 致: 研究部清和\n> 复: NA0007\n> 状态: 待回信\n\n<!-- LETTER-TOKEN v2 no=0008 nonce={n8} fp={fp8} -->\n\n## 摘要\n\n已填实，不需要你做任何事。\n\n## 正文\n\n正文。\n"
        ),
    )
    .unwrap();
    fs::write(
        d.join("letter-tokens.jsonl"),
        format!(
            "{{\"no\":\"NA0007\",\"file\":\"{old}\",\"nonce\":\"{nonce}\",\"fp\":\"{fp}\",\"tpl\":\"v2\",\"createdAt\":\"2026-09-29T02:00:00.000Z\",\"from\":\"清和\"}}\n{{\"no\":\"0008\",\"file\":\"{f8}\",\"nonce\":\"{n8}\",\"fp\":\"{fp8}\",\"tpl\":\"v2\",\"createdAt\":\"2026-09-29T02:20:00.000Z\",\"from\":\"白露\"}}\n"
        ),
    )
    .unwrap();
    d
}

#[test]
fn spec_bar180_new_编号不拼自指码() {
    // 册身份 NA 的册里发新信：文件名/令牌/台账编号全纯流水号（§八.8 本册
    // 自指码为误用）；显式 --sorting NA = 已废止，硬拒
    let d = setup_na_book("bar180-new");
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
        "--type",
        "通报",
        "--title",
        "测试新信",
        "--now-local",
        "2026-09-29 12:00 +08:00",
        "--now-utc",
        "2026-09-29T04:00:00.000Z",
        "--nonce",
        "aabbccddeeff0009",
    ]);
    assert_ok(&out, "BAR-180 new 纯流水号");
    let f9 = "0009号清和致评审部白露的通报.md";
    assert!(d.join(f9).is_file(), "新信文件名应为纯流水号：{f9}");
    assert!(
        !d.join("NA0009号清和致评审部白露的通报.md").exists(),
        "不许再拼自指码 NA"
    );
    let text = fs::read_to_string(d.join(f9)).unwrap();
    let fp9 = fingerprint("0009", "aabbccddeeff0009", f9);
    assert!(
        text.contains(&format!("no=0009 nonce=aabbccddeeff0009 fp={fp9}")),
        "令牌行应为纯流水号且 fp 按纯号算"
    );
    let ledger = fs::read_to_string(d.join("letter-tokens.jsonl")).unwrap();
    assert!(
        ledger.contains(&format!("\"no\":\"0009\",\"file\":\"{f9}\"")),
        "台账新票应为纯流水号：{ledger}"
    );
    // --sorting NA 显式传 = 已废止硬拒，且分毫不落盘
    let before = fs::read_to_string(d.join("letter-tokens.jsonl")).unwrap();
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
        "--type",
        "通报",
        "--title",
        "测试新信二",
        "--sorting",
        "NA",
        "--now-local",
        "2026-09-29 12:10 +08:00",
        "--now-utc",
        "2026-09-29T04:10:00.000Z",
        "--nonce",
        "aabbccddeeff0010",
    ]);
    assert_fail(&out, "BAR-180 --sorting 已废止");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--sorting 已废止") && stderr.contains("纯流水号"),
        "报错应点名废止与纯流水号口径：{stderr}"
    );
    assert_eq!(
        fs::read_to_string(d.join("letter-tokens.jsonl")).unwrap(),
        before,
        "被拒后台账不得变动"
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn spec_bar180_reticket_格式性勘误窄例外() {
    // §八.8 窄例外：新名 = 旧名仅去本册自指码（NA0007号… → 0007号…），
    // 状态 通报完毕 + 已有复信（窗口实锤关闭）也放行；旧票 revokedAt +
    // revokeReason 必带「格式性勘误（契约 §八 第 8 条）」，新票 renamedFrom；
    // 收尾 verify 全册绿且配对键按 renamedFrom 不再误报
    let d = setup_na_book("bar180-fix");
    let mb = d.to_str().unwrap().to_string();
    let no_main = d.join("no-main-book").to_str().unwrap().to_string();
    let old = "NA0007号清和致评审部白露的通报.md";
    let new_name = "0007号清和致评审部白露的通报.md";
    let out = run(&[
        "reticket",
        d.join(old).to_str().unwrap(),
        "--mailbox",
        &mb,
        "--roster",
        ROSTER,
        "--main-book",
        &no_main,
        "--new-name",
        new_name,
        "--reason",
        "去本册自指码 NA——钉",
        "--now-utc",
        "2026-09-29T05:00:00.000Z",
        "--nonce",
        "aabbccddeeff0011",
    ]);
    assert_ok(&out, "BAR-180 §八.8 窄例外（窗口关闭也放行）");
    assert!(!d.join(old).exists() && d.join(new_name).is_file());
    let text = fs::read_to_string(d.join(new_name)).unwrap();
    let fp_new = fingerprint("0007", "aabbccddeeff0011", new_name);
    assert!(
        text.contains(&format!("no=0007 nonce=aabbccddeeff0011 fp={fp_new}")),
        "新信令牌应为去码后的新号且 fp 重算"
    );
    let ledger = fs::read_to_string(d.join("letter-tokens.jsonl")).unwrap();
    assert!(
        ledger.contains("\"revokedAt\":\"2026-09-29T05:00:00.000Z\"")
            && ledger.contains("格式性勘误（契约 §八 第 8 条窄例外）：去本册自指码 NA——钉"),
        "旧票 revokeReason 必带格式性勘误（契约 §八 第 8 条）：{ledger}"
    );
    assert!(
        ledger.contains(&format!(
            "\"no\":\"0007\",\"file\":\"{new_name}\",\"nonce\":\"aabbccddeeff0011\",\"fp\":\"{fp_new}\""
        )) && ledger.contains(&format!("\"renamedFrom\":\"{old}\"")),
        "新票应去码改名挂 renamedFrom：{ledger}"
    );
    let v = run(&["verify", "--mailbox", &mb, "--roster", ROSTER]);
    assert_ok(&v, "窄例外换票后 verify 全册");
    let out_all = format!(
        "{}{}",
        String::from_utf8_lossy(&v.stdout),
        String::from_utf8_lossy(&v.stderr)
    );
    assert!(
        !out_all.contains("换票留痕不完整"),
        "配对键按 renamedFrom：跨码换票不得误报：{out_all}"
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn spec_bar180_reticket_例外之外改号仍拒() {
    // ①改数字号：NA0007 → 0009 → 拒（改名不改号铁律不变）
    let d = setup_na_book("bar180-deny1");
    let mb = d.to_str().unwrap().to_string();
    let no_main = d.join("no-main-book").to_str().unwrap().to_string();
    let old = "NA0007号清和致评审部白露的通报.md";
    let out = run(&[
        "reticket",
        d.join(old).to_str().unwrap(),
        "--mailbox",
        &mb,
        "--roster",
        ROSTER,
        "--main-book",
        &no_main,
        "--new-name",
        "0009号清和致评审部白露的通报.md",
        "--now-utc",
        "2026-09-29T05:00:00.000Z",
        "--nonce",
        "aabbccddeeff0011",
    ]);
    assert_fail(&out, "BAR-180 改数字号仍拒");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("改名不改号"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(d.join(old).exists());
    let _ = fs::remove_dir_all(&d);

    // ②去码但其余字符动了（收件人改名）→ 拒（逐字节不动是窄例外硬条件）
    let d = setup_na_book("bar180-deny2");
    let mb = d.to_str().unwrap().to_string();
    let no_main = d.join("no-main-book").to_str().unwrap().to_string();
    let out = run(&[
        "reticket",
        d.join(old).to_str().unwrap(),
        "--mailbox",
        &mb,
        "--roster",
        ROSTER,
        "--main-book",
        &no_main,
        "--new-name",
        "0007号清和致评审部玄甲的通报.md",
        "--now-utc",
        "2026-09-29T05:00:00.000Z",
        "--nonce",
        "aabbccddeeff0011",
    ]);
    assert_fail(&out, "BAR-180 去码但改他字仍拒");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("改名不改号"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(d.join(old).exists());
    let _ = fs::remove_dir_all(&d);

    // ③缺册身份文件：无法证明「自指」，窄例外 fail-closed → 拒
    let d = tmpdir("bar180-deny3");
    let mb = d.to_str().unwrap().to_string();
    let no_main = d.join("no-main-book").to_str().unwrap().to_string();
    let nonce = "aabbccddeeff0007";
    let fp = fingerprint("NA0007", nonce, old);
    fs::write(
        d.join(old),
        format!(
            "# 测试信\n\n> 日期: 2026-09-29 10:00 +08:00\n> 从: 研究部清和\n> 致: 评审部白露\n> 复: 无（首信）\n> 状态: 待回信\n\n<!-- LETTER-TOKEN v2 no=NA0007 nonce={nonce} fp={fp} -->\n\n## 摘要\n\n已填实，不需要你做任何事。\n\n## 正文\n\n正文。\n"
        ),
    )
    .unwrap();
    fs::write(
        d.join("letter-tokens.jsonl"),
        format!(
            "{{\"no\":\"NA0007\",\"file\":\"{old}\",\"nonce\":\"{nonce}\",\"fp\":\"{fp}\",\"tpl\":\"v2\",\"createdAt\":\"2026-09-29T02:00:00.000Z\",\"from\":\"清和\"}}\n"
        ),
    )
    .unwrap();
    let out = run(&[
        "reticket",
        d.join(old).to_str().unwrap(),
        "--mailbox",
        &mb,
        "--roster",
        ROSTER,
        "--main-book",
        &no_main,
        "--new-name",
        "0007号清和致评审部白露的通报.md",
        "--now-utc",
        "2026-09-29T05:00:00.000Z",
        "--nonce",
        "aabbccddeeff0011",
    ]);
    assert_fail(&out, "BAR-180 缺册身份窄例外 fail-closed");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("改名不改号"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(d.join(old).exists());
    let _ = fs::remove_dir_all(&d);
}
