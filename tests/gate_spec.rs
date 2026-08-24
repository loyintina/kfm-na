//! screendump 考题（A 档）——画面回传的编码与触发语义
//!
//! 契约（2026-08-24 与用户定）：
//! ①XRGB u32 小端编码，每像素 4 字节，字节序 = B,G,R,X；
//! ②触发文件不在 → 不动；在 → 倒 shot.rgb + shot.dim(“w h”)并摘触发；
//! ③倒出来的字节数必须 = w*h*4（缺斤短两=画面错位）。

use kfm_na::gate::{encode_rgb, maybe_dump, trigger_pending};

#[test]
fn spec_编码_小端xrgb字节序() {
    // 0x00RRGGBB 的小端内存序 = BB GG RR 00
    let buf = [0x00A1B2C3u32, 0x00000000, 0x00FFFFFF];
    let bytes = encode_rgb(&buf);
    assert_eq!(bytes.len(), 12);
    assert_eq!(&bytes[0..4], &[0xC3, 0xB2, 0xA1, 0x00]);
    assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x00, 0x00]);
    assert_eq!(&bytes[8..12], &[0xFF, 0xFF, 0xFF, 0x00]);
}

#[test]
fn spec_触发_无触发文件不倒() {
    let dir = std::env::temp_dir().join(format!("kfm-shot-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let d = dir.to_str().unwrap();
    let buf = vec![0x00FF00FFu32; 6];
    assert!(!maybe_dump(d, &buf, 3, 2), "没触发文件不该倒");
    assert!(!dir.join("shot.rgb").exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn spec_触发_倒帧摘触发_尺寸正确() {
    let dir = std::env::temp_dir().join(format!("kfm-shot2-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let d = dir.to_str().unwrap();
    std::fs::write(dir.join("shot-req"), b"").unwrap();
    let buf = vec![0x00112233u32; 6]; // 3x2
    assert!(maybe_dump(d, &buf, 3, 2), "有触发文件必须倒");
    assert!(!dir.join("shot-req").exists(), "倒完必须摘触发");
    assert_eq!(
        std::fs::metadata(dir.join("shot.rgb")).unwrap().len(),
        3 * 2 * 4,
        "字节数必须 = w*h*4"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("shot.dim")).unwrap(),
        "3 2"
    );
    // 单次触发单次倒:再来一次没触发文件,文件内容不许变
    let before = std::fs::read(dir.join("shot.rgb")).unwrap();
    assert!(!maybe_dump(d, &buf, 3, 2));
    assert_eq!(std::fs::read(dir.join("shot.rgb")).unwrap(), before);
    std::fs::remove_dir_all(&dir).ok();
}

/// 后台倒帧全链路考题（2026-08-24）：真 TermView 喂字节 → 离屏光栅化进
/// Vec → 触发 → 倒出的字节流必须就是这一帧的编码，且帧里真有字形像素
/// （防「离屏路径渲染了但没真画/倒了但倒的不是刚渲染的帧」两类断链）
#[test]
fn spec_离屏倒帧_渲染到dump全链路() {
    use kfm_na::termview::{CELL_H, CELL_W, DEFAULT_BG, TermView};
    let font_path = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/data/data/com.termux/files/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    ]
    .iter()
    .find(|p| std::path::Path::new(p).exists())
    .expect("host 测试字体缺失");
    let font = fontdue::Font::from_bytes(
        std::fs::read(font_path).unwrap(),
        fontdue::FontSettings::default(),
    )
    .expect("fontdue 不认 DejaVuSansMono");

    let (cols, rows) = (8u32, 2u32);
    let (w, h) = (cols * CELL_W, rows * CELL_H);
    let mut tv = TermView::new(font, None, cols, rows, CELL_W, CELL_H);
    tv.feed(b"hi");
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_into(&mut buf, w, h);
    assert!(
        buf.iter().any(|&px| px != DEFAULT_BG),
        "喂了 hi 的帧里必须有非背景像素"
    );

    let dir = std::env::temp_dir().join(format!("kfm-shot3-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let d = dir.to_str().unwrap();
    assert!(!trigger_pending(d), "没放触发文件时不许报待倒");
    std::fs::write(dir.join("shot-req"), b"").unwrap();
    assert!(trigger_pending(d));
    assert!(maybe_dump(d, &buf, w, h));
    assert_eq!(
        std::fs::read(dir.join("shot.rgb")).unwrap(),
        encode_rgb(&buf),
        "倒出的必须就是刚渲染的这一帧"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 后台值守考题（2026-08-24）：dump_now 必须从注册的共享终端句柄取画面、
/// 按 note_frame_size 记的尺寸离屏光栅化——这条链就是挂起态的唯一倒帧
/// 通道（事件循环叫不醒，实拍验证过）。登记缺失/尺寸为零时不许倒
#[test]
fn spec_后台值守_dump_now走注册终端() {
    use kfm_na::gate::{dump_now, note_frame_size, register_dump_term};
    use kfm_na::termview::{CELL_H, CELL_W, TermEmu, TermView};
    use std::sync::{Arc, Mutex};

    let font_path = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/data/data/com.termux/files/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    ]
    .iter()
    .find(|p| std::path::Path::new(p).exists())
    .expect("host 测试字体缺失");
    let font = fontdue::Font::from_bytes(
        std::fs::read(font_path).unwrap(),
        fontdue::FontSettings::default(),
    )
    .unwrap();
    let mut tv = TermView::new(font, None, 8, 2, CELL_W, CELL_H);
    tv.feed(b"ok");
    let term: Arc<Mutex<Box<dyn TermEmu>>> = Arc::new(Mutex::new(Box::new(tv)));
    register_dump_term(&term);

    let (w, h) = (8 * CELL_W, 2 * CELL_H);
    let dir = std::env::temp_dir().join(format!("kfm-shot5-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let d = dir.to_str().unwrap();

    // 尺寸未记账 → 有触发也不许倒(触发留着,等尺寸就位)
    std::fs::write(dir.join("shot-req"), b"").unwrap();
    dump_now(d);
    assert!(dir.join("shot-req").exists(), "没尺寸记账不许消费触发");
    assert!(!dir.join("shot.rgb").exists());

    // 尺寸就位 → 倒,且倒的是注册终端的画面(有非背景像素)
    note_frame_size(w, h);
    dump_now(d);
    assert!(!dir.join("shot-req").exists(), "倒完必须摘触发");
    assert_eq!(
        std::fs::read_to_string(dir.join("shot.dim")).unwrap(),
        format!("{w} {h}")
    );
    let raw = std::fs::read(dir.join("shot.rgb")).unwrap();
    assert_eq!(raw.len(), (w * h * 4) as usize);
    // 解码回 u32(B,G,R,X 小端)找非背景像素——证明倒的是真终端画面
    let has_glyph = raw.chunks_exact(4).any(|c| {
        let px = (c[2] as u32) << 16 | (c[1] as u32) << 8 | c[0] as u32;
        px != kfm_na::termview::DEFAULT_BG
    });
    assert!(has_glyph, "喂了 ok 的终端倒出来必须有字形像素");
    std::fs::remove_dir_all(&dir).ok();
}

// ---------- keys-in 注入通道考题（2026-08-24，三件套之动手） ----------

/// drain_keys_in 契约：无文件/空文件/只写一半(.new 未 mv)→ None；
/// 有内容 → 原文返回且文件被消费（原子取走，半写安全）
#[test]
fn spec_keys_in_原子取走协议() {
    use kfm_na::gate::drain_keys_in;
    let dir = std::env::temp_dir().join(format!("kfm-keys-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let d = dir.to_str().unwrap();

    assert_eq!(drain_keys_in(d), None, "无文件 → None");

    // 半写安全:只写了 .new 还没 mv,消费端不许看到
    std::fs::write(dir.join("keys-in.new"), b"ls").unwrap();
    assert_eq!(drain_keys_in(d), None, ".new 未 mv = 半写,不许读到");

    // 正式投递:mv 就位 → 原样取出,文件消费
    std::fs::rename(dir.join("keys-in.new"), dir.join("keys-in")).unwrap();
    assert_eq!(drain_keys_in(d).as_deref(), Some("ls"));
    assert!(!dir.join("keys-in").exists(), "取走后触发文件必须消失");
    assert!(
        !dir.join("keys-in.reading").exists(),
        "reading 残档也必须清"
    );

    // 空文件 = 无内容,但照样消费(不卡死队列)
    std::fs::write(dir.join("keys-in"), b"").unwrap();
    assert_eq!(drain_keys_in(d), None, "空文件 → None");
    assert!(!dir.join("keys-in").exists(), "空文件也要消费掉");

    // 控制字节/中文原样过(注入语义 = 裸字节)
    std::fs::write(dir.join("keys-in"), "你好\x03\r").unwrap();
    assert_eq!(drain_keys_in(d).as_deref(), Some("你好\x03\r"));
    std::fs::remove_dir_all(&dir).ok();
}
