//! tests/theme_spec.rs — 设计 token 层考题（src/theme.rs，2026-09-01 立层）
//!
//! 契约真相源：kfmv4 base.css 配方直译（默认主题）；四层架构纪律
//! （图元族/token/控件/主题包，见 src/theme.rs 头注）。
//! 判卷点：①默认配方逐项钉值（防手滑改配色）；②换肤生效——控件只读
//! token，不认字面颜色（token 化的存在理由）。
//! 纪律：先验证红，答案生成到绿。本文件是考题，生成器不许改。

use kfm_na::termview::TermView;
use kfm_na::theme::{BarTheme, Theme};

// ========== 默认配方 = kfmv4 紫-青暗色系（逐项钉值，防手滑） ==========

#[test]
fn spec_theme_默认kfmv4配方() {
    let t = Theme::default();
    assert_eq!(
        t.bar,
        BarTheme {
            bg: 0x0011_1119,         // 栏带 rgba(18,18,26,.85) 叠 #0a0a0f
            field_bg_l: 0x0018_1532, // 内芯左紫调 (29,23,57) 稍沉一档
            field_bg_r: 0x000D_2231, // 内芯右青调 (12,40,54)
            field_focus_bg_l: 0x0020_1C40,
            field_focus_bg_r: 0x0012_2B3E,
            text: 0x00E0_E0E0,
            placeholder: 0x0061_6165, // rgba(224,224,224,.4) 叠近黑
            border_l: 0x007C_3AED,    // --primary #7c3aed
            border_r: 0x0003_ADD1,    // rgba(0,212,255,.8) 事后色
            accent: 0x0000_D4FF,      // --accent #00d4ff
            glow: 0x007C_3AED,
            send_tl: 0x007C_3AED,
            send_br: 0x0003_ADD1,
            send_tri: 0x00FF_FFFF,
            select_bg: 0x0044_88DD,     // 品牌蓝 α0.35
            select_handle: 0x0000_D4FF, // 品牌青，与定位柄同族
            menu_bg: 0x0020_2028,       // 近黑半透明
            menu_text: 0x00E0_E0E0,
            menu_disabled: 0x0061_6165,
            menu_pressed: 0x003A_3A4A,
        },
        "默认配方 = kfmv4 base.css 直译,改值要走换肤流程不许顺手改"
    );
}

// ========== 换肤生效：控件只读 token，不认字面颜色 ==========

#[test]
fn spec_theme_换肤生效_控件只读token() {
    let (mut tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1200u32);
    let snap = kfm_na::input_bar::BarSnap {
        text: String::new(),
        focused: false,
        lines: 1,
        cursor: 0,
        handle: false,
        composing: String::new(),
        scroll_px: 0,
        follow: true,
        selecting: false,
        selection_start: 0,
        selection_end: 0,
    };
    // 默认肤:栏带底 RGB = 0x111119 + 高字节 = CHROME_BAND_ALPHA(BAR-067
    // 半透契约,kfmv4 rgba(18,18,26,.85) 还原)。采样点取文本区下缘的带底
    // 留白(32px 区),别落进内芯渐变/发光里
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut buf, w, h, 0, &snap, false, false);
    let band_mid = (h - 15) as usize * w as usize + (w / 2) as usize;
    assert_eq!(
        buf[band_mid] & 0x00FF_FFFF,
        0x0011_1119,
        "默认肤栏带底 RGB 必须是 theme.bar.bg 的字面值"
    );
    assert_eq!(
        (buf[band_mid] >> 24) & 0xFF,
        kfm_na::theme::CHROME_BAND_ALPHA,
        "栏带底必须携带半透 α(BAR-067)"
    );
    // 换肤:改 token 再倒帧,同一像素跟着变——渲染没偷读常量
    tv.theme.bar.bg = 0x00FF_8800;
    let mut buf2 = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut buf2, w, h, 0, &snap, false, false);
    assert_eq!(
        buf2[band_mid] & 0x00FF_FFFF,
        0x00FF_8800,
        "换肤后栏带必须跟 token 走(控件只读 token 的存在理由)"
    );
    assert_eq!(
        (buf2[band_mid] >> 24) & 0xFF,
        kfm_na::theme::CHROME_BAND_ALPHA,
        "换肤不动半透 α"
    );
    assert_ne!(buf[band_mid], buf2[band_mid]);
    // TermView 出厂默认肤 = Theme::default()(零行为变化的锚)
    let fresh: TermView = kfm_na::termview::build_vendored().expect("必成").0;
    assert_eq!(fresh.theme, Theme::default(), "出厂必须挂默认配方");
}

// ========== 快捷键行配色组（2026-09-01 token 化补全） ==========

#[test]
fn spec_theme_keybar默认配方_换肤生效() {
    let t = Theme::default();
    assert_eq!(
        t.keybar,
        kfm_na::theme::KeybarTheme {
            bg: 0x0010_1216,
            key_bg: 0x0023_272E,
            mod_on: 0x003E_6FB4,
            label: 0x00E8_EAED,
        },
        "keybar 配方 = 原 termview KEYBAR_* 常量逐项直迁"
    );
    // 换肤生效:改 token 倒帧,行带底像素逐字面值跟 token 走
    let (mut tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1200u32);
    tv.theme.keybar.bg = 0x0000_FF00;
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_keybar(&mut buf, w, h, 0, 0);
    // 行带在屏底 60px 带(keybar::HEIGHT_PX=120? 取带上缘+4 的纯底区)
    let sample = (h - kfm_na::keybar::HEIGHT_PX + 2) as usize * w as usize + 2;
    assert_eq!(
        buf[sample], 0x0000_FF00,
        "keybar 行带底必须读 theme.keybar.bg"
    );
}
