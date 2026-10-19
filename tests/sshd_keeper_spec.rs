//! sshd_keeper_spec.rs — BAR-230 sshd 看门狗纯逻辑面 + 哨兵链接三态。
//!
//! 判卷维度：banner 首行判健（A 档纯逻辑）/ bin/sh 链接三态（A 档，
//! 编址照 kfmv4 旧路径存在性三分类——小满 MAIN0116 指路）/ start 幂等
//! 与永久挂起路径（host 冒烟：prefix 假路径 → spawn ENOENT → 看门狗
//! 线程自己永久失败退出，正好咬到分类分支，host 不起真 sshd）。
//! 进程胶水面（重拉/退避/冻僵 kill）是 B 档，判卷 = 真机 C 档
//! （kill sshd 后自动复活 + 9022 banner 恒在，NA0160 判卷口径）。

use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn spec_bar230_banner首行判健() {
    assert!(kfm_na::sshd_keeper::banner_ok("SSH-2.0-OpenSSH_10.5"));
    assert!(kfm_na::sshd_keeper::banner_ok("SSH-1.99-Cisco-1.25"));
    // 非 banner 首行（HTTP/空/半行）都不得判健——冻僵的活体证据就是
    // 连上但读不到 SSH- 开头（BAR-029 冬眠形态与本案判据同口径）
    assert!(!kfm_na::sshd_keeper::banner_ok("HTTP/1.1 400 Bad Request"));
    assert!(!kfm_na::sshd_keeper::banner_ok(""));
    assert!(!kfm_na::sshd_keeper::banner_ok("SSH"));
}

#[test]
fn spec_bar230_binsh链接三态() {
    use kfm_na::bootstrap::ShLink;
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("usr");
    fs::create_dir_all(prefix.join("bin")).unwrap();
    // 三档之一：链接本身没了（克隆丢符号链接形态——find -type l = 0）
    assert_eq!(kfm_na::bootstrap::sh_link_state(&prefix), ShLink::Missing);
    // 三档之二：链接在但目标没了（真断）
    std::os::unix::fs::symlink("nowhere-dash", prefix.join("bin/sh")).unwrap();
    assert_eq!(kfm_na::bootstrap::sh_link_state(&prefix), ShLink::Dangling);
    // 三档之三：链接在且目标可解析（健康）——先摘断链再建健康链
    fs::remove_file(prefix.join("bin/sh")).unwrap();
    fs::write(prefix.join("bin/dash"), b"#!/fake").unwrap();
    fs::set_permissions(prefix.join("bin/dash"), fs::Permissions::from_mode(0o700)).unwrap();
    std::os::unix::fs::symlink("dash", prefix.join("bin/sh")).unwrap();
    assert_eq!(kfm_na::bootstrap::sh_link_state(&prefix), ShLink::Healthy);
}

#[test]
fn spec_bar230_哨兵含链接判据() {
    // 前提件：bin/ 有 x 位（BAR-229 哨兵半边）——验证 BAR-230 另半边：
    // x 位齐但 bin/sh 没了 = 环境死（承影形态：只丢链接不丢 x 位）
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("usr");
    let zip = crate::bootstrap_fixture::fixture_zip(true);
    kfm_na::bootstrap::ensure_prefix(&prefix, &zip).unwrap();
    assert!(kfm_na::bootstrap::prefix_ready(&prefix));
    fs::remove_file(prefix.join("bin/sh")).unwrap();
    assert!(
        !kfm_na::bootstrap::prefix_ready(&prefix),
        "只丢链接不丢 x 位也必须判环境死（变异：摘链接判据 → 此处红）"
    );
    // 自愈重装恢复（SYMLINKS.txt 重建 bin/sh）
    let st = kfm_na::bootstrap::ensure_prefix(&prefix, &zip).unwrap();
    assert!(matches!(st, kfm_na::bootstrap::InstallStatus::Installed));
    assert!(kfm_na::bootstrap::prefix_ready(&prefix));
}

#[test]
fn spec_bar230_start幂等_假prefix永久挂起() {
    // host 冒烟：不存在的前缀 → spawn ENOENT（永久档）→ 看门狗线程
    // 自己退出且 start 幂等（两次调用不 panic；第二次打「已在跑」报表）
    let fake = std::env::temp_dir().join("bar230-no-such-prefix");
    kfm_na::sshd_keeper::start(fake.clone());
    kfm_na::sshd_keeper::start(fake);
    // 给线程一拍时间走永久失败分支（spawn 前有 backoff 睡眠）
    std::thread::sleep(std::time::Duration::from_millis(1500));
    // 无法直接断言线程退出（无句柄）——判据走报表/真机 C 档；
    // 本钉至少咬住：幂等闸 + 永久分类不 panic 不死循环烧 CPU（1.5s 内返回）。
}

/// 夹具 zip 的最小复刻（bootstrap_spec 的 fixture_zip 是私有 fn，
/// 这里独立一份避免测试间耦合；与那边保持同一形制：bin/dash +
/// SYMLINKS.txt 建 bin/sh）。
mod bootstrap_fixture {
    pub fn fixture_zip(with_symlinks: bool) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opt = zip::write::SimpleFileOptions::default();
        w.start_file("bin/dash", opt).unwrap();
        std::io::Write::write_all(&mut w, b"fake-dash-binary").unwrap();
        // MAIN0192 判据收紧：健康 prefix 必带 bin/sshd 与 etc/ssh/kfm-sshd.conf
        w.start_file("bin/sshd", opt).unwrap();
        std::io::Write::write_all(&mut w, b"fake-sshd").unwrap();
        w.start_file("etc/ssh/kfm-sshd.conf", opt).unwrap();
        std::io::Write::write_all(&mut w, b"Port 8024\n").unwrap();
        w.start_file("etc/motd", opt).unwrap();
        std::io::Write::write_all(&mut w, b"welcome").unwrap();
        if with_symlinks {
            w.start_file("SYMLINKS.txt", opt).unwrap();
            std::io::Write::write_all(&mut w, "dash←bin/sh\n".as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }
}

#[test]
fn spec_bar230_外来占口定性判据() {
    use kfm_na::sshd_keeper::should_note_foreign;
    // 娃不在 + banner 健康 = 外来者占口 → 报（一次）
    assert!(should_note_foreign(false, true, false));
    // 已报过 → 一集不二报（娃绑上口后旗复位才再报）
    assert!(!should_note_foreign(false, true, true));
    // 娃活着 → 不适用此口径（健康/冻僵走探活臂）
    assert!(!should_note_foreign(true, true, false));
    // 娃不在 + banner 也不在 → 正常重拉路径，不是占口
    assert!(!should_note_foreign(false, false, false));
}

#[test]
fn spec_bar247_让位期不spawn() {
    use kfm_na::sshd_keeper::should_spawn;
    // 娃不在 + 口上健康 banner（外来占口）= 真让位，不 spawn
    // （变异口：把 !banner_healthy 改成 banner_healthy → 此行红）
    assert!(!should_spawn(false, true));
    // 娃不在 + banner 也没了（口腾出/本来空）= 照常重拉接管
    // （变异口：把 !child_alive 改成 child_alive → 此行红）
    assert!(should_spawn(false, false));
    // 娃活着 → spawn 段本就不该进（健康/冻僵归探活臂）
    assert!(!should_spawn(true, true));
    assert!(!should_spawn(true, false));
}

// ---- BAR-259（追踪信 NA0188）：conf 自给／stderr 落盘／秒退熔断 ----
// 定罪：9 机 kfm-sshd.conf 全树零命中（overlay 装机带走、postinst 配置步
// 没落）→ sshd -f 缺件秒退 RC=1，1s 重拉永动 7960 行 8 天无人知（承影
// MAIN0175 楼21 取证，楼22 裁定 a+b+d）。

#[test]
fn spec_bar259_conf缺_自生成且幂等() {
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("usr");
    fs::create_dir_all(prefix.join("bin")).unwrap();
    fs::write(prefix.join("bin/sshd"), b"fake").unwrap();
    let conf = prefix.join("etc/ssh/kfm-sshd.conf");
    assert!(!conf.exists(), "前提：conf 缺");
    kfm_na::sshd_keeper::ensure_conf(&prefix).unwrap();
    let body = fs::read_to_string(&conf).unwrap();
    assert!(
        body.contains("Port 8024"),
        "模板端口钉死（与 NA_SSHD_PORT 同源）"
    );
    assert!(
        body.contains(
            &prefix
                .join("etc/ssh/na_authorized_keys")
                .display()
                .to_string()
        ),
        "路径全随 prefix 走（变异：模板写死绝对路径 → 此处红）"
    );
    // 幂等：在位件不许被覆盖（手写标记后 ensure 不动它）
    fs::write(&conf, b"# hand\n").unwrap();
    kfm_na::sshd_keeper::ensure_conf(&prefix).unwrap();
    assert_eq!(fs::read_to_string(&conf).unwrap(), "# hand\n");
}

#[test]
fn spec_bar259_conf生成失败_不抢spawn分类() {
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("usr");
    fs::create_dir_all(prefix.join("bin")).unwrap();
    fs::write(prefix.join("bin/sshd"), b"fake").unwrap();
    // etc 是个文件 → mkdir etc/ssh 必败 → Err（调用方报红＋永久挂起）
    fs::write(prefix.join("etc"), b"block").unwrap();
    assert!(kfm_na::sshd_keeper::ensure_conf(&prefix).is_err());
    // 二进制缺 = spawn 永久分类的领地（BAR-229/230），ensure_conf 不抢戏：
    // 应 Ok 且一个目录都不建（BAR-230 假 prefix 钉行为不变）
    let tmp2 = tempfile::tempdir().unwrap();
    let p2 = tmp2.path().join("usr");
    kfm_na::sshd_keeper::ensure_conf(&p2).unwrap();
    assert!(!p2.join("etc").exists(), "二进制缺时 conf 侧不许动土");
}

#[test]
fn spec_bar259_熔断_秒退连败() {
    use kfm_na::sshd_keeper::{ExitClass, fuse_step};
    use std::time::Duration;
    let env = ExitClass::EnvDead;
    // 内因秒退才计账（conf／host key 缺这类，同一环境重试不自愈）
    assert_eq!(fuse_step(Some(Duration::from_secs(1)), 0, env), (1, false));
    assert_eq!(fuse_step(Some(Duration::from_secs(1)), 6, env), (7, false));
    // 到上限（变异：上限调大／判断改 > → 此处红）
    assert_eq!(fuse_step(Some(Duration::from_secs(1)), 7, env), (8, true));
    // 活得久＝清账
    assert_eq!(fuse_step(Some(Duration::from_secs(5)), 3, env), (0, false));
    // 恰达阈值不算秒退（边界归健康侧）
    assert_eq!(fuse_step(Some(Duration::from_secs(2)), 3, env), (0, false));
    // None＝冻僵 kill／try_wait 错（我们动手杀的）不计连败
    assert_eq!(fuse_step(None, 5, env), (0, false));
    // 楼291／293：撞口与未知 rc **一律不计账**（外因记成内因＝把占口升级成不再重试）
    assert_eq!(
        fuse_step(Some(Duration::from_secs(1)), 3, ExitClass::BindBusy),
        (3, false)
    );
    assert_eq!(
        fuse_step(Some(Duration::from_secs(1)), 7, ExitClass::Unknown),
        (7, false)
    );
}

#[test]
fn spec_bar293_退出分档与退避封顶() {
    use kfm_na::sshd_keeper::{
        BACKOFF_CAP_SECS, BACKOFF_EXTERNAL_SECS, ExitClass, classify_exit, next_backoff,
    };
    // rc 优先：255＝撞口（清和楼283 定案读数），1＝环境死
    assert_eq!(classify_exit(Some(255), ""), ExitClass::BindBusy);
    assert_eq!(classify_exit(Some(1), ""), ExitClass::EnvDead);
    // stderr 兜底：rc 判不出时按关键词；撞口类关键词优先于 conf 类
    assert_eq!(
        classify_exit(None, "sshd: bind() failed: Address already in use"),
        ExitClass::BindBusy
    );
    assert_eq!(
        classify_exit(Some(1), "sshd: no hostkeys available"),
        ExitClass::EnvDead
    );
    assert_eq!(
        classify_exit(Some(1), "Address already in use"),
        ExitClass::BindBusy
    );
    // 其它 rc 不许默认进账（楼291 承影）：判不出＝Unknown
    assert_eq!(classify_exit(Some(42), ""), ExitClass::Unknown);
    assert_eq!(classify_exit(None, ""), ExitClass::Unknown);
    // 退避：撞口/未知走长档且恒定（口腾即恢复）；环境死才指数增长并封顶
    assert_eq!(
        next_backoff(ExitClass::BindBusy, 1),
        BACKOFF_EXTERNAL_SECS,
        "撞口用长档，不必越退越久"
    );
    assert_eq!(next_backoff(ExitClass::Unknown, 1), BACKOFF_EXTERNAL_SECS);
    assert_eq!(next_backoff(ExitClass::EnvDead, 1), 2);
    assert_eq!(next_backoff(ExitClass::EnvDead, 200), BACKOFF_CAP_SECS);
    assert!(
        next_backoff(ExitClass::EnvDead, BACKOFF_CAP_SECS) <= BACKOFF_CAP_SECS,
        "退避必须封顶（永久挂起这个出口整体取消，楼293）"
    );
}

#[test]
fn spec_bar259_接线守卫() {
    // 锚源码（BAR-238 先例：看门狗线程私有账无法单测驱动，直接锚）：
    // spawn 前必过 ensure_conf、stderr 必落盘、退出臂必过熔断
    let src = include_str!("../src/sshd_keeper.rs");
    let i_ensure = src
        .find("ensure_conf(&prefix)")
        .expect("spawn 前必过 ensure_conf");
    let i_spawn = src.find("spawn_sshd(&prefix)").expect("spawn 调用点在");
    assert!(i_ensure < i_spawn, "ensure_conf 必须在 spawn 之前");
    assert!(
        src.contains(".stderr(sshd_err_stdio(prefix))"),
        "stderr 必落 tmp/sshd-err.log（Stdio::null 灭口已拆；只留函数不接线的变异也咬）"
    );
    assert!(src.contains("fuse_step"), "退出臂必过秒退连败熔断");
}

#[test]
fn spec_bar259_秒退取样在spawn后() {
    // 承影 NA0188 楼5 判卷红：存活若等到下一巡检拍（30s）才计时，秒退永远
    // 算成 ≥2s，熔断臂形同虚设。钉两件：①spawn 后当拍以 QUICK_DEATH 窗探；
    // ②探到即喂 fuse_step（纯函数钉只看逻辑，这条钉接线）。
    let src = include_str!("../src/sshd_keeper.rs");
    let i_spawn_ok = src.find("sshd 已重拉").expect("spawn 成功臂在");
    let i_probe = src
        .find("QUICK_DEATH.as_millis()")
        .expect("秒退探针必须用 QUICK_DEATH 窗（变异：回退到下一拍计时 → 红）");
    let i_quick = src.find("sshd 秒退（").expect("秒退报账臂在");
    assert!(
        i_spawn_ok < i_probe && i_probe < i_quick,
        "探针必须在 spawn 成功之后、秒退报账之前"
    );
    assert!(
        src[i_probe..].contains("fuse_step("),
        "秒退探到的存活必须喂 fuse_step（不是只在纯函数里自证）"
    );
}

#[test]
fn spec_bar259_err日志追加不截断() {
    // 承影 NA0188 楼5 副伤：create 截断会把上一轮证词抹掉——正好抹掉要定罪
    // 的那一轮。改为追加；超帽才轮转。
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().to_path_buf();
    {
        let mut f = kfm_na::sshd_keeper::open_err_log(&prefix).unwrap();
        std::io::Write::write_all(&mut f, b"first-run\n").unwrap();
    }
    {
        let mut f = kfm_na::sshd_keeper::open_err_log(&prefix).unwrap();
        std::io::Write::write_all(&mut f, b"second-run\n").unwrap();
    }
    let body = fs::read_to_string(prefix.join("tmp/sshd-err.log")).unwrap();
    assert!(body.contains("first-run"), "上一轮证词不许被截断：{body}");
    assert!(body.contains("second-run"), "本轮证词要落：{body}");
    // 超帽轮转（64KB）：旧证词让位，新写从空文件起
    let big = "x".repeat(70 * 1024);
    fs::write(prefix.join("tmp/sshd-err.log"), &big).unwrap();
    {
        let mut f = kfm_na::sshd_keeper::open_err_log(&prefix).unwrap();
        std::io::Write::write_all(&mut f, b"after-rotate\n").unwrap();
    }
    let body = fs::read_to_string(prefix.join("tmp/sshd-err.log")).unwrap();
    assert!(!body.contains(&big), "超帽必须轮转清空旧内容");
    assert!(body.contains("after-rotate"));
}

#[test]
fn spec_bar311_心跳行带状态维且看门狗不停止() {
    // 楼310 白露：无所作为也必须留痕（否则「没有消息」与「它死了」在账上同形）；
    // 楼311 清和：心跳行带状态维（state／attempt／最近 rc＋存活），**一行答尽**，
    // 不开第二条心跳；楼306：永不产生停止状态（只许报红＋加长退避）。
    let keeper = include_str!("../src/sshd_keeper.rs");

    // ① 快照五维齐全
    let at = keeper
        .find("pub fn health_snapshot()")
        .expect("health_snapshot 必须在");
    let snap = &keeper[at..(at + keeper[at..].find("\n}\n").expect("函数必须闭合"))];
    for dim in ["state=", "attempt=", "last_rc=", "存活=", "退避="] {
        assert!(snap.contains(dim), "心跳状态维缺 {dim}（缺维判红）");
    }

    // ② 载体是既有 alive 心跳行，不许另开第二条心跳
    let app = include_str!("../src/android_app.rs");
    assert!(
        app.contains("sshd_keeper::health_snapshot()"),
        "alive 心跳行必须带上状态维（否则不可核）"
    );
    assert!(
        !keeper.contains("sshd 心跳"),
        "不许在 keeper 里另开第二条心跳——「一行答尽」"
    );

    // ③ spawn 尝试必须落行（「变相停止」的判据靠它）
    assert!(
        keeper.contains("sshd spawn 尝试"),
        "spawn 尝试行必须在（成功失败都算一次尝试）"
    );

    // ④ keep_loop 内不许有 return：停止状态整体取消，只许报红＋拉长退避
    let loop_at = keeper.find("fn keep_loop(").expect("keep_loop 必须在");
    assert!(
        !keeper[loop_at..].contains("return;"),
        "keep_loop 内不许出现 return——停止状态只能靠退避拉长，不许退出看门狗"
    );
    assert!(
        keeper.matches("return;").count() <= 1,
        "全文件只允许 start() 那处幂等 return（防重复线程），其余一律不停止"
    );
}
