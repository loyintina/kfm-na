//! sess_mode.rs — 每会话终端模式快照（BAR-125，2026-09-21 用户真机报
//! 「切回本地后屏幕还是服务器 tmux 内容 + 上下滑跳手势操作符乱码」）
//!
//! 病灶：双会话共用一个终端仿真器，切换只换路由与补屏，**模式状态不
//! 换**——服务器 tmux 开的鼠标上报（?1000/1002/1003+?1006）残留在网格
//! mode 里，切到本地裸 shell 后 mouse_report_active() 仍 true → 滑动
//! 手势走 BAR-016② 路径翻成 SGR 滚轮序列 \x1b[<64;列;行M 发本地 pty，
//! shell 没开鼠标模式原样回显 = 用户看到的「乱码」。alt-screen/应用光
//! 标/bracketed paste 同属泄漏面。
//!
//! 修法 = 快照换-mode：切出时存旧活跃会话的 mode bits，切入时先复位
//! 受管模式 + 清屏（用户拍板「切换后直接自动清屏」），再恢复切入会话
//! 自己的快照（无快照 = 全干净，裸 shell 天然如此）。tmux 快照带鼠标
//! 上报 → 切回远程滚轮翻译不丢（不依赖 tmux 重发模式——它不一定发）。
//! 每会话网格快照（内容级）仍是挂账的根治，本模块只治模式层。
//!
//! 纯函数零 IO，host 可判卷（A 档）。

use alacritty_terminal::term::TermMode;

/// 受管模式映射表：TermMode 位 → DEC 私有模式号。只收「会话间不该
/// 泄漏」的；LINE_WRAP 等双端默认一致的不管（快照面越小越好）。
const MANAGED: &[(u32, u32)] = &[
    (TermMode::APP_CURSOR.bits(), 1),
    (TermMode::MOUSE_REPORT_CLICK.bits(), 1000),
    (TermMode::MOUSE_DRAG.bits(), 1002),
    (TermMode::MOUSE_MOTION.bits(), 1003),
    (TermMode::BRACKETED_PASTE.bits(), 2004),
    (TermMode::SGR_MOUSE.bits(), 1006),
    (TermMode::ALT_SCREEN.bits(), 1049),
];

/// 受管位并集（快照存全量 bits 也行，但判读/恢复只认受管面）
pub fn managed_mask() -> u32 {
    MANAGED.iter().fold(0, |m, (bit, _)| m | bit)
}

/// 切入复位序列：当前置位的受管模式逐个 DECRST（?1049l 打头——先回
/// 主屏，后续清屏才作用在主网格）；顺带 `\x1b[r` 复位滚动区 +
/// `\x1b[?25h` 光标复显（两件非 mode 快照管但同属泄漏面的壳状态：
/// tmux 的滚动区/隐光标泄漏给裸 shell 同样是病；tmux 重画会重设
/// 它自己要的状态）。
pub fn reset_seq(cur_bits: u32) -> String {
    let mut s = String::new();
    // ?1049l 必须第一个发（在 alt 屏里清别的模式没问题，但回主屏要趁早）
    if cur_bits & TermMode::ALT_SCREEN.bits() != 0 {
        s.push_str("\x1b[?1049l");
    }
    for (bit, num) in MANAGED {
        if *bit != TermMode::ALT_SCREEN.bits() && cur_bits & bit != 0 {
            s.push_str(&format!("\x1b[?{num}l"));
        }
    }
    s.push_str("\x1b[r\x1b[?25h");
    s
}

/// 切入恢复序列：快照里置位的受管模式逐个 DECSET
pub fn restore_seq(saved_bits: u32) -> String {
    let mut s = String::new();
    for (bit, num) in MANAGED {
        if saved_bits & bit != 0 {
            s.push_str(&format!("\x1b[?{num}h"));
        }
    }
    s
}
