//! 断线输入暂存考题（A 档，BAR-135 用户拍板「断联影响最小化」）：
//! 远程死会话 + 隧道不可用期的击键进队，Opened 回冲——不盲孵必死
//! 连接、不丢输入、不重复。
//! 变异抽检：①容量闸摘除（不丢最旧）必须咬；②drain 不保序/不清零
//! 必须咬；③should_hold 判据放宽（如本地也收）必须咬。

use kfm_na::offline_keys::{CAP_BYTES, OfflineKeys, should_hold};

#[test]
fn spec_bar135_暂存_收冲保序() {
    let mut q = OfflineKeys::new();
    assert!(q.is_empty());
    q.push("ls".to_string());
    q.push("\r".to_string());
    q.push("echo hi".to_string());
    assert_eq!(q.pending_bytes(), 2 + 1 + 7);
    assert!(!q.is_empty());
    let got = q.drain();
    assert_eq!(got, vec!["ls", "\r", "echo hi"]); // 保序 = 击键序即回冲序
    assert!(q.is_empty());
    assert_eq!(q.pending_bytes(), 0); // 冲完清零（状态行字节数跟着灭）
}

#[test]
fn spec_bar135_暂存_容量闸丢最旧() {
    let mut q = OfflineKeys::new();
    let big = "x".repeat(CAP_BYTES); // 一条顶满
    q.push(big.clone());
    q.push("tail".to_string()); // 这条进来，最旧的那条必须让位
    assert_eq!(q.dropped(), 1, "超容量必须丢最旧且记账");
    assert!(q.pending_bytes() <= CAP_BYTES);
    let got = q.drain();
    assert_eq!(got, vec!["tail"]); // 留新不留旧
    // 丢账不随 drain 清零（它是账不是态）
    assert_eq!(q.dropped(), 1);
}

#[test]
fn spec_bar135_收编判定_只收远程死会话隧道断() {
    // 真值表：session_over × is_remote × tunnel_usable
    assert!(should_hold(true, true, false)); // 收编：远程死会话隧道断
    assert!(!should_hold(true, true, true)); // 隧道通 = 走 kick_reconnect 原路
    assert!(!should_hold(true, false, false)); // 本地死会话与隧道无关
    assert!(!should_hold(false, true, false)); // 活会话不收
    assert!(!should_hold(false, false, false));
    assert!(!should_hold(false, true, true));
    assert!(!should_hold(true, false, true));
    assert!(!should_hold(false, false, true));
}

// ---- BAR-186 臂③：WAL 落盘钉（进程死全灭的修；落盘/读回/帽截断/坏行容错）----

#[test]
fn spec_bar186_wal_落盘读回保序() {
    let dir = tempfile::tempdir().unwrap();
    let wal = dir.path().join("offline-input.wal");
    {
        let mut q = OfflineKeys::new();
        q.attach_wal(&wal);
        q.push("ls".to_string());
        q.push("\r\n中文\x1b[A".to_string()); // 控制符+多字节+换行全下锅
        assert!(wal.exists(), "push 后 WAL 文件必须在盘");
    } // 进程死 = 实例灭
    let mut q2 = OfflineKeys::new();
    q2.attach_wal(&wal); // 启动读回
    assert_eq!(
        q2.drain(),
        vec!["ls", "\r\n中文\x1b[A"],
        "读回必须保序保字节——换行/ESC/中文一个不许丢"
    );
    assert!(!wal.exists(), "drain 回冲成功 = WAL 清账（文件灭）");
}

#[test]
fn spec_bar186_wal_容量闸丢最旧文件同账() {
    let dir = tempfile::tempdir().unwrap();
    let wal = dir.path().join("k.wal");
    let mut q = OfflineKeys::new();
    q.attach_wal(&wal);
    let big = "x".repeat(CAP_BYTES);
    q.push(big);
    q.push("tail".to_string()); // 顶掉最旧 → 触发整文件重写
    assert_eq!(q.dropped(), 1);
    // 丢最旧必须触发整文件重写——不重写 = 文件只涨不瘦，进程死前
    // 文件即已失真（读回虽自愈，盘账本身是烂账）。盘账逐字节对咬
    // （"tail" 的 hex = 7461696c）：恒追加变异下文件还背着被丢的 big
    assert_eq!(
        std::fs::read_to_string(&wal).unwrap(),
        "7461696c
",
        "丢最旧后 WAL 必须整写归账——文件只许等于现队列的 hex 流"
    );
    // 文件账必须等于内存账（重写生效，不是尾巴失真地追加）
    let mut q2 = OfflineKeys::new();
    q2.attach_wal(&wal);
    assert_eq!(q2.drain(), vec!["tail"], "WAL 读回必须与丢后队列同账");
}

#[test]
fn spec_bar186_wal_坏行容错不炸() {
    let dir = tempfile::tempdir().unwrap();
    let wal = dir.path().join("k.wal");
    // 三型坏行：非 hex 字符 / 奇数长 / 合法 hex 但非法 UTF-8（0xFF）
    std::fs::write(&wal, "zz非hex\nabc\nff\n6c73\n").unwrap();
    let mut q = OfflineKeys::new();
    q.attach_wal(&wal);
    assert_eq!(
        q.drain(),
        vec!["ls"],
        "坏行跳过、好行读回，不许 panic 不许全弃"
    );
}

#[test]
fn spec_bar186_wal_回冲后照挂与纯内存兼容() {
    let dir = tempfile::tempdir().unwrap();
    let wal = dir.path().join("k.wal");
    let mut q = OfflineKeys::new();
    q.attach_wal(&wal);
    q.push("a".to_string());
    assert_eq!(q.drain(), vec!["a"]);
    assert_eq!(
        q.wal_path(),
        Some(wal.as_path()),
        "drain 不许摘 WAL——再断线还要落盘"
    );
    q.push("b".to_string()); // 再断线收编
    let mut q2 = OfflineKeys::new();
    q2.attach_wal(&wal);
    assert_eq!(q2.drain(), vec!["b"], "回冲后再收编的条目照样落盘读回");
    // 不挂 WAL = 纯内存旧行为（不碰 fs）
    let mut q3 = OfflineKeys::new();
    assert_eq!(q3.wal_path(), None);
    q3.push("x".to_string());
    assert_eq!(q3.drain(), vec!["x"]);
}
