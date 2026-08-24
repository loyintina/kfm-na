//! screendump 考题（A 档）——画面回传的编码与触发语义
//!
//! 契约（2026-08-24 与用户定）：
//! ①XRGB u32 小端编码，每像素 4 字节，字节序 = B,G,R,X；
//! ②触发文件不在 → 不动；在 → 倒 shot.rgb + shot.dim(“w h”)并摘触发；
//! ③倒出来的字节数必须 = w*h*4（缺斤短两=画面错位）。

use kfm_na::screendump::{encode_rgb, maybe_dump};

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
