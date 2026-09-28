//! mailbox-cli — kfm-na 信箱工具链 CLI（IO 壳；逻辑全在 mailbox-core）。
//!
//! 五子命令：
//!   new       写信 + 发令牌（v2.1 模式，移植 new-letter.mjs 主流程）
//!   verify    [信件路径]  不带参数 = 全册执法（check-letter-token.mjs 主循环）；
//!             带文件 = 单信自检（new-letter.mjs --verify）
//!   gen       回写 README gen:pending + gen:agent-inbox 两区段 + letters-index.jsonl
//!             （--check-only 只查不写；--mailbox 指向主册时拒绝写入——写者分区）
//!   scan      --for=<职能|名字|旧线名> | --by=<名字>  跨册欠账扫描（na 册 + 主册）
//!   reticket  <信件路径> --new-name <新文件名>  改名换票（契约 §八改名窗口）
//!
//! 公共选项：--mailbox / --roster / --name-prefix / --v1-manifest /
//! --no-strict-pools（env KFM_MAILBOX_STRICT_POOLS=0 同效）。

use mailbox_core::json::{JVal, parse_json, to_json_string};
use mailbox_core::name::{CONNECT_CHARS, V21_TYPES, is_v21_name, parse_v21_name, v21_no_of};
use mailbox_core::newletter::{
    SkeletonParams, build_v21_file_name, build_v21_skeleton, insert_token, ledger_record_line,
    next_number, revoke_fields,
};
use mailbox_core::projection::{
    self, MARK_END, MARK_START, PENDING_END, PENDING_START, ScanTarget,
};
use mailbox_core::roster::{PoolEntry, Roster, check_pools};
use mailbox_core::token::{self, fingerprint, ledger_nos, parse_ledger};
use mailbox_core::verify::{BookCheck, LetterText, verify_book, verify_single};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

const DEFAULT_MAILBOX: &str = "/root/.kfm/session/信箱";
const MAIN_BOOK: &str = "/root/kfmv4/docs/ledger/agent-inbox";
/// na 册存量信命名前缀默认词表（--name-prefix 覆盖）
const DEFAULT_NAME_PREFIX: &str = "kfm-na|na";

// ---------------------------------------------------------------
// 参数解析（--k v / --k=v / --flag；多值键收集全列）
// ---------------------------------------------------------------

#[derive(Default)]
struct Args {
    pos: Vec<String>,
    opts: Vec<(String, String)>,
    flags: HashSet<String>,
}

impl Args {
    fn parse(argv: &[String]) -> Result<Args, String> {
        let mut a = Args::default();
        let mut i = 0;
        while i < argv.len() {
            let s = &argv[i];
            if let Some(rest) = s.strip_prefix("--") {
                if let Some((k, v)) = rest.split_once('=') {
                    a.opts.push((k.to_string(), v.to_string()));
                } else if i + 1 < argv.len() && !argv[i + 1].starts_with("--") {
                    a.opts.push((rest.to_string(), argv[i + 1].clone()));
                    i += 1;
                } else {
                    a.flags.insert(rest.to_string());
                }
            } else {
                a.pos.push(s.clone());
            }
            i += 1;
        }
        Ok(a)
    }
    fn opt(&self, k: &str) -> Option<&str> {
        self.opts
            .iter()
            .rev()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }
    fn all(&self, k: &str) -> Vec<&str> {
        self.opts
            .iter()
            .filter(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
            .collect()
    }
    fn has(&self, k: &str) -> bool {
        self.flags.contains(k) || self.opts.iter().any(|(n, _)| n == k)
    }
}

fn die(prefix: &str, msg: &str) -> ! {
    eprintln!("[{prefix}] {msg}");
    exit(1);
}

// ---------------------------------------------------------------
// 环境件：时间戳 / nonce（new 与 reticket 可注入，golden 用）
// ---------------------------------------------------------------

fn date_cmd(args: &[&str]) -> String {
    let out = Command::new("date")
        .args(args)
        .output()
        .unwrap_or_else(|e| die("mailbox-cli", &format!("date 命令不可用：{e}")));
    if !out.status.success() {
        die("mailbox-cli", "date 命令执行失败");
    }
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// `YYYY-MM-DD HH:MM ±HH:MM`（信封「日期」戳）
fn now_local_stamp() -> String {
    date_cmd(&["+%Y-%m-%d %H:%M %:z"])
}

/// ISO UTC 毫秒戳（台账 createdAt）
fn now_utc_iso() -> String {
    date_cmd(&["-u", "+%Y-%m-%dT%H:%M:%S.%3NZ"])
}

fn gen_nonce() -> String {
    let b = fs::read("/dev/urandom")
        .unwrap_or_else(|e| die("mailbox-cli", &format!("读 /dev/urandom 失败：{e}")));
    if b.len() < 8 {
        die("mailbox-cli", "/dev/urandom 可读字节不足");
    }
    b[..8].iter().map(|x| format!("{x:02x}")).collect()
}

// ---------------------------------------------------------------
// 信箱上下文：名册 / README / v1 名单 / 信件枚举
// ---------------------------------------------------------------

fn read_opt(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok()
}

/// 名册解析：--roster > <dir>/roster.json > 主册 roster.json（JS resolveRoster 同款）
fn resolve_roster(args: &Args, dir: &Path) -> Option<Roster> {
    let candidates = [
        args.opt("roster").map(PathBuf::from),
        Some(dir.join("roster.json")),
        Some(Path::new(MAIN_BOOK).join("roster.json")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find_map(|p| read_opt(&p).and_then(|s| Roster::from_json_str(&s)))
}

/// 状态词表出处文本：信箱 README > 主册 README > 空串
fn resolve_readme(mailbox: &Path) -> String {
    read_opt(&mailbox.join("README.md"))
        .or_else(|| read_opt(&Path::new(MAIN_BOOK).join("README.md")))
        .unwrap_or_default()
}

/// v1 冻结名单：--v1-manifest > 信箱/archive-v1/manifest-v1.json > 信箱/manifest-v1.json。
/// 缺失 = 警告并对全部信件执法（JS 同款文案）。
fn resolve_v1_files(args: &Args, mailbox: &Path, warns: &mut Vec<String>) -> HashSet<String> {
    let candidates = [
        args.opt("v1-manifest").map(PathBuf::from),
        Some(mailbox.join("archive-v1").join("manifest-v1.json")),
        Some(mailbox.join("manifest-v1.json")),
    ];
    let found = candidates.into_iter().flatten().find(|p| p.is_file());
    let Some(path) = found else {
        warns.push(
            "未找到 archive-v1/manifest-v1.json——v1 名单缺失，本检查将对全部信件执法（可能是首次运行）"
                .to_string(),
        );
        return HashSet::new();
    };
    let mut out = HashSet::new();
    if let Some(text) = read_opt(&path)
        && let Ok(v) = parse_json(&text)
    {
        for key in ["files", "activeAtFreeze", "v1Files"] {
            if let Some(arr) = v.get(key).and_then(JVal::as_arr) {
                out.extend(arr.iter().filter_map(JVal::as_str).map(str::to_string));
            }
        }
    }
    out
}

fn list_md(dir: &Path) -> Vec<String> {
    let Ok(rd) = fs::read_dir(dir) else {
        return vec![];
    };
    let mut v: Vec<String> = rd
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|f| f.ends_with(".md") && f != "README.md")
        .collect();
    v.sort();
    v
}

/// 信箱信件全集：在册（active）+ 归档（archive-v1）
fn load_letters(mailbox: &Path) -> Vec<LetterText> {
    let mut out = vec![];
    for (sub, loc) in [("", "active"), ("archive-v1", "archive-v1")] {
        let dir = if sub.is_empty() {
            mailbox.to_path_buf()
        } else {
            mailbox.join(sub)
        };
        for f in list_md(&dir) {
            if let Some(text) = read_opt(&dir.join(&f)) {
                out.push(LetterText {
                    file: f,
                    dir: loc.to_string(),
                    text,
                });
            }
        }
    }
    out
}

fn strict_pools(args: &Args) -> bool {
    !args.has("no-strict-pools")
        && std::env::var("KFM_MAILBOX_STRICT_POOLS").ok().as_deref() != Some("0")
}

fn mailbox_of(args: &Args) -> PathBuf {
    PathBuf::from(args.opt("mailbox").unwrap_or(DEFAULT_MAILBOX))
}

// ---------------------------------------------------------------
// new：写信 + 发令牌
// ---------------------------------------------------------------

fn cmd_new(args: &Args) {
    let p = "mailbox-new";
    let mailbox = mailbox_of(args);
    let from_func = args.opt("from-func").unwrap_or_else(|| {
        die(p, "v2.1 模式需要 --from-func <职能> --from-name <两字名>（如 --from-func 研究部 --from-name 清和）")
    });
    let from_name = args.opt("from-name").unwrap_or_else(|| {
        die(p, "v2.1 模式需要 --from-func <职能> --from-name <两字名>（如 --from-func 研究部 --from-name 清和）")
    });
    let type_word = args
        .opt("type")
        .unwrap_or_else(|| die(p, "v2.1 模式需要 --type <类型词> --title \"<标题>\""));
    let title = args
        .opt("title")
        .unwrap_or_else(|| die(p, "v2.1 模式需要 --type <类型词> --title \"<标题>\""));
    if !V21_TYPES.contains(&type_word) {
        die(
            p,
            &format!("--type 出表：{type_word}（∈ {}）", V21_TYPES.join("/")),
        );
    }
    let to_all = args.has("to-all");
    let to_args = args.all("to");
    if to_args.is_empty() && !to_all {
        die(
            p,
            "v2.1 模式需要 --to \"<职能><名字>\"（可多次）或 --to-all",
        );
    }
    let sorting_raw = args.opt("sorting").unwrap_or("");
    let sorting = if sorting_raw.is_empty() || sorting_raw == "MAIN" {
        String::new()
    } else {
        sorting_raw.to_uppercase()
    };
    if !sorting.is_empty() && !regex_lite_upper(&sorting) {
        die(p, &format!("--sorting 非法：{sorting_raw}（如 MAIN／NA）"));
    }

    // 收件人拆解 + 三池校验（JS new-letter 同款：错即 die）
    let mut recipients: Vec<(Option<String>, String)> = vec![];
    for raw in &to_args {
        if *raw == "全体" {
            recipients.push((None, "全体".to_string()));
            continue;
        }
        if raw.chars().count() < 3 {
            die(
                p,
                &format!(
                    "--to 非法：{raw}（须 <职能><名字>，如 研究部空谷；裸职能或裸名字都不行）"
                ),
            );
        }
        let name: String = raw.chars().skip(raw.chars().count() - 2).collect();
        let func: String = raw.chars().take(raw.chars().count() - 2).collect();
        recipients.push((Some(func), name));
    }
    if to_all {
        recipients.push((None, "全体".to_string()));
    }
    let mut seen = HashSet::new();
    for (func, name) in &recipients {
        let key = format!("{}{}", func.clone().unwrap_or_default(), name);
        if !seen.insert(key.clone()) {
            die(p, &format!("--to 重复：{key}"));
        }
    }
    let roster = resolve_roster(args, &mailbox);
    let mut entries = vec![PoolEntry {
        who: "发信人".to_string(),
        func: Some(from_func.to_string()),
        name: from_name.to_string(),
    }];
    entries.extend(recipients.iter().map(|(func, name)| PoolEntry {
        who: "收件人".to_string(),
        func: func.clone(),
        name: name.clone(),
    }));
    let mut pool_errs = vec![];
    let mut pool_warns = vec![];
    check_pools(
        roster.as_ref(),
        &entries,
        strict_pools(args),
        &mut pool_errs,
        &mut pool_warns,
    );
    for w in &pool_warns {
        println!("[{p}] 注意 — {w}");
    }
    if !pool_errs.is_empty() {
        for e in &pool_errs {
            eprintln!("[{p}] ✗ {e}");
        }
        exit(1);
    }

    let reply = args.opt("reply");
    if let Some(r) = reply
        && !regex_reply_no(r)
    {
        die(p, &format!("--reply 非法：{r}（须 NNNN 或 <分拣码>NNNN）"));
    }
    let about = args.opt("about");
    if let Some(ab) = about {
        let n = ab.chars().count();
        let all_han = ab.chars().all(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
        if !(2..=12).contains(&n) || !all_han {
            die(
                p,
                &format!("--about 非法：{ab}（事由须为 2–12 个汉字，建议 4–8）"),
            );
        }
        if ab.chars().any(|c| CONNECT_CHARS.contains(&c)) || ab.contains("关于") {
            die(
                p,
                &format!("--about 非法：{ab}（事由不得含连接字：号/致/复/的/及/等/关于）"),
            );
        }
        if type_word == "日报" {
            println!(
                "[{p}] 注意 — 契约 §二判据②：日报/例行通报不写事由（--about 已给，请确认确有区分度）"
            );
        }
    }

    let tokens_path = mailbox.join("letter-tokens.jsonl");
    let tokens_text = read_opt(&tokens_path).unwrap_or_default();
    let mut all_files = list_md(&mailbox);
    all_files.extend(list_md(&mailbox.join("archive-v1")));
    let nos: Vec<String> = ledger_nos(&tokens_text);
    let no = next_number(
        &all_files,
        &nos.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let display: Vec<String> = recipients
        .iter()
        .map(|(func, name)| format!("{}{}", func.clone().unwrap_or_default(), name))
        .collect();
    let file = build_v21_file_name(&no, &sorting, from_name, &display, reply, about, type_word);
    if mailbox.join(&file).is_file() || mailbox.join("archive-v1").join(&file).is_file() {
        die(p, &format!("目标已存在：{file}"));
    }

    let date_stamp = args
        .opt("now-local")
        .map(str::to_string)
        .unwrap_or_else(now_local_stamp);
    let created_at = args
        .opt("now-utc")
        .map(str::to_string)
        .unwrap_or_else(now_utc_iso);
    let nonce = args
        .opt("nonce")
        .map(str::to_string)
        .unwrap_or_else(gen_nonce);
    let status = args.opt("status").unwrap_or("待回信");
    let kind = args.opt("kind").unwrap_or("链条");
    let expect = args.opt("expect").unwrap_or("（待填：要办什么）");
    let criteria = args.opt("criteria").unwrap_or("（待填：什么算收敛）");

    let skeleton = build_v21_skeleton(&SkeletonParams {
        title,
        date_stamp: &date_stamp,
        from_func,
        from_name,
        display: &display,
        reply,
        status,
        kind,
        expect,
        criteria,
    });
    let full_no = format!("{sorting}{no}");
    let fp = fingerprint(&full_no, &nonce, &file);
    let text = insert_token(&skeleton, &token::token_line(&full_no, &nonce, &fp));
    fs::write(mailbox.join(&file), &text).unwrap_or_else(|e| die(p, &format!("写信件失败：{e}")));
    let rec = ledger_record_line(&full_no, &file, &nonce, &fp, &created_at, from_name, None);
    let mut ledger = tokens_text;
    if !ledger.is_empty() && !ledger.ends_with('\n') {
        ledger.push('\n');
    }
    ledger.push_str(&rec);
    ledger.push('\n');
    fs::write(&tokens_path, ledger).unwrap_or_else(|e| die(p, &format!("登记令牌失败：{e}")));
    println!("[{p}] 已生成 {file}（v2.1，编号 {full_no}，令牌已登记）");
    println!(
        "[{p}] 下一步：填白话结论块与正文 → mailbox-cli verify {} → mailbox-cli gen",
        mailbox.join(&file).display()
    );
}

fn regex_lite_upper(s: &str) -> bool {
    !s.is_empty() && s.len() <= 4 && s.bytes().all(|b| b.is_ascii_uppercase())
}

fn regex_reply_no(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_uppercase() {
        i += 1;
    }
    let uppers = i;
    let digits = b.len() - i;
    uppers <= 4 && digits == 4 && b[i..].iter().all(|c| c.is_ascii_digit())
}

// ---------------------------------------------------------------
// verify：全册执法 / 单信自检
// ---------------------------------------------------------------

fn print_diags(p: &str, d: &mailbox_core::verify::Diags) {
    for w in &d.warns {
        println!("[{p}] 注意 — {w}");
    }
    for e in &d.errs {
        eprintln!("[{p}] ✗ {e}");
    }
}

fn cmd_verify(args: &Args) {
    let p = "mailbox-verify";
    let strict = strict_pools(args);
    if let Some(path) = args.pos.get(1) {
        // 单信自检（new-letter --verify）：台账按「文件所在信箱」取，--mailbox 显式优先
        let lp = PathBuf::from(path);
        if !lp.is_file() {
            die(p, &format!("verify 需要存在的文件路径：{path}"));
        }
        let file = lp
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let text = read_opt(&lp).unwrap_or_default();
        let vdir = lp.parent().map(Path::to_path_buf).unwrap_or_default();
        let (tokens_path, roster_dir) = if args.opt("mailbox").is_some() {
            (
                mailbox_of(args).join("letter-tokens.jsonl"),
                mailbox_of(args),
            )
        } else {
            (vdir.join("letter-tokens.jsonl"), vdir)
        };
        let tokens_text = read_opt(&tokens_path);
        let roster = resolve_roster(args, &roster_dir);
        let d = verify_single(
            &file,
            &text,
            tokens_text.as_deref(),
            roster.as_ref(),
            strict,
        );
        print_diags(p, &d);
        if !d.is_clean() {
            exit(1);
        }
        let era_desc = if is_v21_name(&file) {
            "v2.1：文件名文法/信封/三池/令牌/白话面"
        } else {
            "v2：编号/令牌/白话面"
        };
        println!("[{p}] ✓ {file} 通过（{era_desc}齐备）");
        return;
    }
    // 全册执法（check-letter-token 主循环）
    let mailbox = mailbox_of(args);
    let letters = load_letters(&mailbox);
    let tokens_text = read_opt(&mailbox.join("letter-tokens.jsonl"));
    let roster = resolve_roster(args, &mailbox);
    let readme = resolve_readme(&mailbox);
    let name_prefix = args.opt("name-prefix").unwrap_or(DEFAULT_NAME_PREFIX);
    let mut warns = vec![];
    let v1 = resolve_v1_files(args, &mailbox, &mut warns);
    for w in &warns {
        println!("[{p}] 注意 — {w}");
    }
    let d = verify_book(&BookCheck {
        letters: &letters,
        tokens_text: tokens_text.as_deref(),
        roster: roster.as_ref(),
        readme_text: &readme,
        name_prefix,
        v1_files: &v1,
        strict_pools: strict,
    });
    print_diags(p, &d);
    if !d.is_clean() {
        eprintln!("\n[{p}] {} 处问题，构建中断。", d.errs.len());
        exit(1);
    }
    println!(
        "[{p}] OK — v2.1 信件 {} 封（文法/信封/三池/令牌/白话面）+ 存量 v2 信件 {} 封（编号/令牌/白话面）全合规（v1 冻结名单 {} 封不追改；台账 {} 张现行票据{}{}）",
        d.v21_count,
        d.legacy_count,
        v1.len(),
        d.current_tickets,
        if d.revoked_tickets > 0 {
            format!(" + {} 张撤销票", d.revoked_tickets)
        } else {
            String::new()
        },
        if strict {
            "；名字池严格模式"
        } else {
            ""
        },
    );
}

// ---------------------------------------------------------------
// gen：README 两区段 + letters-index.jsonl 回写
// ---------------------------------------------------------------

fn cmd_gen(args: &Args) {
    let p = "mailbox-gen";
    let mailbox = mailbox_of(args);
    let check_only = args.has("check-only");
    // 写者分区：主册由 kfmv4 侧 gen-agent-inbox.mjs 独占回写，本器只许 --check-only
    if !check_only {
        let canon = fs::canonicalize(&mailbox).unwrap_or_else(|_| mailbox.clone());
        let main_canon = fs::canonicalize(MAIN_BOOK).unwrap_or_else(|_| PathBuf::from(MAIN_BOOK));
        if canon == main_canon {
            die(
                p,
                "写者分区：--mailbox 指向主册（kfmv4 docs/ledger/agent-inbox）时本器拒绝写入——主册回写归 kfmv4 侧 gen-agent-inbox.mjs（只许 --check-only）",
            );
        }
    }
    let letters = load_letters(&mailbox);
    let roster = resolve_roster(args, &mailbox);
    let out = projection::render_gen(&letters, roster.as_ref());
    let mut errors = out.errors.clone();

    let readme_path = mailbox.join("README.md");
    let Some(doc) = read_opt(&readme_path) else {
        die(p, &format!("{} 不存在", readme_path.display()));
    };
    // 两区段同在 README：先回填顶部活信清单，再在其结果上重找文末台账标记
    let mut next = doc.clone();
    match projection::splice_section(&next, PENDING_START, PENDING_END, &out.pending_section) {
        Ok(s) => next = s,
        Err(e) => errors.push(e),
    }
    match projection::splice_section(&next, MARK_START, MARK_END, &out.ledger_section) {
        Ok(s) => next = s,
        Err(e) => errors.push(e),
    }
    let index_path = mailbox.join("letters-index.jsonl");
    let prev_index = read_opt(&index_path);
    if check_only {
        if next != doc {
            errors.push("README 台账投影漂移（gen:pending/gen:agent-inbox 区段与信件不一致）——跑 mailbox-cli gen 回写".to_string());
        }
        if prev_index.as_deref() != Some(out.index_text.as_str()) {
            errors.push(
                "派生索引漂移：letters-index.jsonl 与信件不一致——跑 mailbox-cli gen 回写"
                    .to_string(),
            );
        }
    } else {
        if errors.is_empty() && next != doc {
            fs::write(&readme_path, &next)
                .unwrap_or_else(|e| die(p, &format!("回写 README 失败：{e}")));
        }
        if errors.is_empty() && prev_index.as_deref() != Some(out.index_text.as_str()) {
            fs::write(&index_path, &out.index_text)
                .unwrap_or_else(|e| die(p, &format!("回写 letters-index.jsonl 失败：{e}")));
        }
    }
    if !errors.is_empty() {
        for e in &errors {
            eprintln!("[{p}] {e}");
        }
        eprintln!(
            "[{p}] {} 处问题{}",
            errors.len(),
            if check_only {
                "——跑 mailbox-cli gen 回写"
            } else {
                ""
            }
        );
        exit(1);
    }
    if check_only {
        println!(
            "[{p}] OK — {} 封信台账投影与机读头一致（在册 {} + 归档 {}）",
            out.rows, out.active, out.archive
        );
    } else {
        println!(
            "[{p}] 已回写信件清单（{} 封：在册 {} + 归档 {}）+ letters-index.jsonl（{} 行）",
            out.rows, out.active, out.archive, out.rows
        );
    }
}

// ---------------------------------------------------------------
// scan：跨册欠账扫描（na 册 + 主册，标签 [NA]/[MAIN]）
// ---------------------------------------------------------------

fn cmd_scan(args: &Args) {
    let p = "mailbox-scan";
    let (target, label) = match (args.opt("for"), args.opt("by")) {
        (Some(t), None) => (ScanTarget::For(t), format!("发给「{t}」")),
        (None, Some(n)) => (ScanTarget::By(n), format!("从名字「{n}」")),
        _ => die(
            p,
            "scan 需要且只需要 --for=<职能|名字|旧线名> 或 --by=<名字> 之一",
        ),
    };
    let na_book = mailbox_of(args);
    let main_book = PathBuf::from(args.opt("main-book").unwrap_or(MAIN_BOOK));
    let na_letters = load_letters(&na_book);
    let main_letters = load_letters(&main_book);
    let mut books: Vec<(&str, &[LetterText])> = vec![("NA", &na_letters)];
    if main_book.is_dir() {
        books.push(("MAIN", &main_letters));
    }
    let (errors, hits) = projection::scan_debts(&books, target);
    if !errors.is_empty() {
        for e in &errors {
            eprintln!("[{p}] {e}");
        }
        exit(1);
    }
    println!("[{p}] 归属行扫描：{} 当前欠账 {} 封", label, hits.len());
    for h in &hits {
        println!(
            "  [{}] {}{} ｜ {}",
            h.book,
            if h.archived { "[归档] " } else { "" },
            h.file,
            h.status
        );
    }
}

// ---------------------------------------------------------------
// reticket：改名换票（契约 §八改名窗口）
// ---------------------------------------------------------------

fn cmd_reticket(args: &Args) {
    let p = "mailbox-reticket";
    let path = args.pos.get(1).unwrap_or_else(|| {
        die(
            p,
            "用法：mailbox-cli reticket <信件路径> --new-name <新文件名> [--reason <说明>]",
        )
    });
    let lp = PathBuf::from(path);
    if !lp.is_file() {
        die(p, &format!("信件不存在：{path}"));
    }
    let old_file = lp
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let new_name = args
        .opt("new-name")
        .unwrap_or_else(|| die(p, "reticket 需要 --new-name <新文件名>（改名不改号）"));
    let mailbox = mailbox_of(args);
    if mailbox.join(new_name).is_file() || mailbox.join("archive-v1").join(new_name).is_file() {
        die(p, &format!("目标已存在：{new_name}"));
    }
    let text = read_opt(&lp).unwrap_or_default();
    let Some(tm) = token::find_token(&text) else {
        die(
            p,
            &format!("缺令牌行：{old_file} 没有 LETTER-TOKEN v2 行——非生成器签发的信不能换票"),
        );
    };
    // 改名不改号：新名编号须与令牌编号一致
    let new_no = v21_no_of(new_name)
        .or_else(|| mailbox_core::name::legacy_no_of(new_name))
        .unwrap_or_else(|| die(p, &format!("新文件名缺编号：{new_name}")));
    if new_no != tm.no {
        die(
            p,
            &format!("改名不改号：新名编号 {new_no} ≠ 令牌编号 {}", tm.no),
        );
    }
    if is_v21_name(new_name) {
        let parsed = parse_v21_name(new_name);
        if !parsed.errs.is_empty() {
            die(p, &format!("新文件名文法不合：{}", parsed.errs.join("；")));
        }
    }
    let tokens_path = mailbox.join("letter-tokens.jsonl");
    let ledger_text = read_opt(&tokens_path)
        .unwrap_or_else(|| die(p, &format!("台账不存在：{}", tokens_path.display())));
    let ledger = parse_ledger(&ledger_text);
    let Some(cur) = ledger.find_current(&tm.no) else {
        die(p, &format!("台账找不到 no={} 的现行票据", tm.no));
    };
    if cur.file.as_deref() != Some(old_file.as_str()) {
        die(
            p,
            &format!(
                "台账登记的 file={} 与本文件 {old_file} 不一致——先查清票据归属再换票",
                cur.file.as_deref().unwrap_or("")
            ),
        );
    }
    let now = args
        .opt("now-utc")
        .map(str::to_string)
        .unwrap_or_else(now_utc_iso);
    let reason = args
        .opt("reason")
        .map(str::to_string)
        .unwrap_or_else(|| "改名窗口内换票（契约 §八）".to_string());
    let nonce = args
        .opt("nonce")
        .map(str::to_string)
        .unwrap_or_else(gen_nonce);
    let new_fp = fingerprint(&tm.no, &nonce, new_name);
    // 旧行 from 原样继承（保序 JVal 取）
    let from = ledger_text
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| parse_json(l).ok())
        .find(|v| {
            v.get("no").and_then(JVal::as_str) == Some(tm.no.as_str())
                && v.get("revokedAt").is_none()
        })
        .and_then(|v| v.get("from").and_then(JVal::as_str).map(str::to_string))
        .unwrap_or_else(|| tm.no.clone());

    // 台账：旧行补 revokedAt/revokeReason（保序追加），append 新行含 renamedFrom
    let mut lines: Vec<String> = vec![];
    let mut patched = false;
    for line in ledger_text.split('\n') {
        if line.trim().is_empty() {
            continue;
        }
        let mut v =
            parse_json(line).unwrap_or_else(|_| die(p, "台账含非法 JSON 行，先修台账再换票"));
        if !patched
            && v.get("no").and_then(JVal::as_str) == Some(tm.no.as_str())
            && v.get("revokedAt").is_none()
            && v.get("file").and_then(JVal::as_str) == Some(old_file.as_str())
        {
            if let JVal::Obj(pairs) = &mut v {
                pairs.extend(revoke_fields(&now, &reason));
            }
            patched = true;
        }
        lines.push(to_json_string(&v));
    }
    if !patched {
        die(
            p,
            &format!("台账找不到 no={} file={old_file} 的现行票据行", tm.no),
        );
    }
    lines.push(ledger_record_line(
        &tm.no,
        new_name,
        &nonce,
        &new_fp,
        &now,
        &from,
        Some(&old_file),
    ));
    let mut out = lines.join("\n");
    out.push('\n');

    // 信内令牌行换新 fp；文件改名
    let old_line = token::token_line(&tm.no, &tm.nonce, &tm.fp);
    let new_line = token::token_line(&tm.no, &nonce, &new_fp);
    if !text.contains(&old_line) {
        die(p, "信内令牌行与台账票面不符——先跑 mailbox-cli verify 查清");
    }
    let new_text = text.replacen(&old_line, &new_line, 1);
    fs::write(mailbox.join(new_name), &new_text)
        .unwrap_or_else(|e| die(p, &format!("写新信件失败：{e}")));
    fs::write(&tokens_path, out).unwrap_or_else(|e| die(p, &format!("回写台账失败：{e}")));
    if lp != mailbox.join(new_name) {
        fs::remove_file(&lp).unwrap_or_else(|e| die(p, &format!("移除旧信件失败：{e}")));
    }
    println!(
        "[{p}] 已换票：{old_file} → {new_name}（编号 {} 不变；旧票已记 revokedAt，新票 renamedFrom={old_file}）",
        tm.no
    );
}

// ---------------------------------------------------------------
// main
// ---------------------------------------------------------------

const USAGE: &str = "mailbox-cli — kfm-na 信箱工具链（逻辑核 mailbox-core，IO 全在本壳）

用法：
  mailbox-cli new --from-func <职能> --from-name <两字名> --to \"<职能><名字>\" [--to …] [--to-all]
                  [--reply NNNN] [--about 事由] [--sorting NA] --type <类型词> --title \"<标题>\"
                  [--kind 链条] [--expect …] [--criteria …] [--status 待回信]
                  [--now-local \"YYYY-MM-DD HH:MM ±HH:MM\"] [--now-utc <ISO>] [--nonce <16hex>]
  mailbox-cli verify [信件路径]      不带参数=全册执法；带文件=单信自检
  mailbox-cli gen [--check-only]     回写 README 两区段 + letters-index.jsonl（主册拒绝写入）
  mailbox-cli scan --for=<目标> | --by=<名字>   跨册欠账扫描（na 册 [NA] + 主册 [MAIN]）
  mailbox-cli reticket <信件路径> --new-name <新文件名> [--reason <说明>]   改名换票（契约 §八）

公共选项：
  --mailbox <dir>       信箱根（默认 /root/.kfm/session/信箱）
  --roster <path>       名册（默认 信箱/roster.json → 主册 roster.json）
  --name-prefix <re>    存量信命名前缀（默认 kfm-na|na）
  --v1-manifest <path>  v1 冻结名单（默认 信箱/archive-v1/manifest-v1.json → 信箱/manifest-v1.json）
  --main-book <dir>     scan 的主册路径（默认 /root/kfmv4/docs/ledger/agent-inbox）
  --no-strict-pools     名字池严格模式降级（env KFM_MAILBOX_STRICT_POOLS=0 同效）";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(&argv) {
        Ok(a) => a,
        Err(e) => die("mailbox-cli", &e),
    };
    if args.has("help") {
        println!("{USAGE}");
        return;
    }
    if args.pos.is_empty() {
        println!("{USAGE}");
        exit(1);
    }
    match args.pos[0].as_str() {
        "new" => cmd_new(&args),
        "verify" => cmd_verify(&args),
        "gen" => cmd_gen(&args),
        "scan" => cmd_scan(&args),
        "reticket" => cmd_reticket(&args),
        other => die("mailbox-cli", &format!("未知子命令：{other}\n\n{USAGE}")),
    }
}
