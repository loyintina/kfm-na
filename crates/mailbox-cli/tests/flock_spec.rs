//! BAR-181 钉：信箱写者子命令 flock 互斥（白露 0022 条件①——信是交付面，
//! 编号 scan-then-write 的 TOCTOU 与 bar-new.sh 治的 BAR 号同族）。
//!
//! ① 互斥钉（确定性，变异锚）：测试进程持锁 → new/reticket 必阻塞；放锁 → 放行。
//!    摘了 flock 调用这枚钉必红（子命令秒完 = 锁没咬住）。
//! ② 并发钉（行为面）：6 路并发 new → 编号唯一连号 0001–0006、台账不丢行。

use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::thread::sleep;
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_mailbox-cli");
const ROSTER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/roster.json");

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mailbox-flock-test-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn run_new(mb: &Path, title: &str) -> Output {
    Command::new(BIN)
        .args([
            "new",
            "--mailbox",
            mb.to_str().unwrap(),
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
            title,
        ])
        .output()
        .unwrap()
}

fn spawn_new(mb: &Path, title: &str) -> Child {
    Command::new(BIN)
        .args([
            "new",
            "--mailbox",
            mb.to_str().unwrap(),
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
            title,
        ])
        .spawn()
        .unwrap()
}

fn book_lock(mb: &Path) -> File {
    let f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false) // 锁文件只是令牌，从不写内容
        .open(mb.join(".mailbox.lock"))
        .unwrap();
    let rc = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) };
    assert_eq!(rc, 0, "测试进程持锁失败");
    f
}

fn book_unlock(f: &File) {
    unsafe {
        libc::flock(f.as_raw_fd(), libc::LOCK_UN);
    }
}

/// 持锁期间子命令必须还在等（锁咬住）；跑完了 = 锁没咬住 = 钉红
fn assert_blocked(child: &mut Child, ctx: &str) {
    sleep(Duration::from_millis(2000));
    assert!(
        child.try_wait().unwrap().is_none(),
        "{ctx}：册锁被持期间子命令竟能跑完——flock 没咬住（BAR-181）"
    );
    child.kill().ok();
    child.wait().ok();
}

#[test]
fn spec_bar181_写者持锁_new与reticket阻塞_放锁放行() {
    let d = tmpdir("mutex");
    // 先无锁造一封 0001（reticket 的操作对象）
    let out = run_new(&d, "立册首信");
    assert!(out.status.success(), "无锁 new 失败：{:?}", out.status);
    let letter = fs::read_dir(&d)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.file_name()))
        .find(|n| n.to_string_lossy().ends_with(".md"))
        .expect("首信未落盘");

    // 测试进程持册锁
    let guard = book_lock(&d);

    // new 必阻塞
    let mut c1 = spawn_new(&d, "阻塞探针一");
    assert_blocked(&mut c1, "new");

    // reticket 必阻塞（同一把锁，不只 new）
    let mut c2 = Command::new(BIN)
        .args([
            "reticket",
            d.join(&letter).to_str().unwrap(),
            "--mailbox",
            d.to_str().unwrap(),
            "--new-name",
            &letter.to_string_lossy(),
        ])
        .spawn()
        .unwrap();
    assert_blocked(&mut c2, "reticket");

    // 放锁 → new 放行
    book_unlock(&guard);
    drop(guard);
    let out2 = run_new(&d, "放锁探针");
    assert!(
        out2.status.success(),
        "放锁后 new 仍失败：{:?}",
        out2.status
    );

    let _ = fs::remove_dir_all(&d);
}

#[test]
fn spec_bar181_并发领号_编号唯一() {
    let d = tmpdir("race");
    let mut children: Vec<Child> = (0..6).map(|i| spawn_new(&d, &format!("并发{i}"))).collect();
    for (i, c) in children.iter_mut().enumerate() {
        let st = c.wait().unwrap();
        assert!(st.success(), "并发 new #{i} 失败：{st}");
    }
    // 编号唯一连号：0001–0006 各一封
    let mut nos: Vec<String> = fs::read_dir(&d)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".md"))
        .map(|n| n.chars().take(4).collect())
        .collect();
    nos.sort();
    nos.dedup();
    assert_eq!(
        nos,
        vec!["0001", "0002", "0003", "0004", "0005", "0006"],
        "并发领号出现重号/断号（BAR-181）"
    );
    // 台账六行不重号
    let tokens = fs::read_to_string(d.join("letter-tokens.jsonl")).unwrap();
    let mut tnos: Vec<&str> = tokens
        .lines()
        .filter_map(|l| l.split("\"no\":\"").nth(1))
        .filter_map(|s| s.split('\"').next())
        .collect();
    tnos.sort();
    tnos.dedup();
    assert_eq!(tnos.len(), 6, "台账重号/丢行（BAR-181）：{tnos:?}");

    let _ = fs::remove_dir_all(&d);
}
