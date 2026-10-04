//! tests/bar232_wiring_spec.rs — BAR-232 仪器臂接线守卫（源码钉，仿
//! viewer_fling_wiring_spec 同款）：「键盘弹起不滚到末行」（MAIN0120，
//! 新机 23d6ec4 时代有实锤幕、0696f16 后无幕）在 field-reports 里
//! 可能是「无声坏法」（查询连败 None 路径零日志 / 零跟随不变化零日志），
//! 故两条仪器臂必须接线在位——下次复现一拍定罪。

const APP: &str = include_str!("../src/android_app.rs");
const TERM: &str = include_str!("../src/termview.rs");

#[test]
fn spec_bar232_键盘跟随仪器臂_接线守卫() {
    // ① 零跟随诊断臂：键盘开（遮挡>300px）而 kb_shift 恒 0 时，
    // 现场三态串必须上报（进态闩一报防刷屏——全局 AtomicBool 同
    // note_frame_size 先例）
    assert!(APP.contains("键盘开零跟随诊断"), "零跟随诊断上报必须在位");
    assert!(
        APP.contains("static KB_ZERO_DIAG_LATCH"),
        "诊断闩必须挂全局（涂装装配函数无 self）"
    );
    assert!(
        APP.contains("kb_occlude > 300 && term.kb_shift_px() == 0"),
        "诊断臂判据 = 键盘开着且零跟随"
    );
    // ② 诊断面单源：三态串（offset/cursor_row/visible_h）出自
    // termview::kb_zero_diag，涂装壳只转述不自算
    assert!(
        TERM.contains("fn kb_zero_diag(&self, win_h: u32, occlude_px: u32) -> String;"),
        "kb_zero_diag 必须进 TermEmu trait（term 是 dyn 派发，inherent 面够不着）"
    );
    // ③ 查询连败臂：None 路径本零日志——连败 30 拍必须吼一嗓子，
    // 成功即清零（不留陈账）
    assert!(
        APP.contains("IME inset 查询连败 30 拍"),
        "查询连败上报必须在位"
    );
    assert!(
        APP.contains("self.ime_none_run = 0;"),
        "连败计数必须在成功时清零"
    );
    assert!(
        APP.contains("self.ime_none_run = self.ime_none_run.saturating_add(1);"),
        "连败必须有计数增量（None 路径零日志盲区的主哨）"
    );
}
