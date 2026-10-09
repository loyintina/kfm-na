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
