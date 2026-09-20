//! crates/na-server/tests/utf8x_spec.rs — A 档考题：增量 UTF-8 解码
//! （redroid 判卷发现的 U+FFFD tofu 病灶：v1 按 8KB 块 from_utf8_lossy，
//! 块界劈开多字节字符 = 双侧替换符。正身 = 跨块续帧拼接）
//!
//! 答案区：crates/na-server/src/utf8x.rs。本文件是考题，生成器不许改。

use na_server::utf8x::utf8_feed;

/// 顺手包装：喂一块，拿字符串
fn feed<'a>(carry: &'a [u8], chunk: &'a [u8]) -> (String, Vec<u8>) {
    utf8_feed(carry, chunk)
}

#[test]
fn spec_ascii_passthrough() {
    let (s, c) = feed(b"", b"hello\r\n");
    assert_eq!(s, "hello\r\n");
    assert!(c.is_empty(), "无尾巴");
}

#[test]
fn spec_complete_multibyte_one_chunk() {
    let (s, c) = feed(b"", "中🌕".as_bytes());
    assert_eq!(s, "中🌕");
    assert!(c.is_empty());
}

/// 三字节字符「中」(E4 B8 AD) 逐字节喂：前两口必须忍住不吐，
/// 第三口拼出整字——块界劈字 = 本考题的存在理由
#[test]
fn spec_three_byte_char_split_bytewise() {
    let (s1, c1) = feed(b"", &[0xE4]);
    assert_eq!(s1, "", "半字不许吐替换符");
    assert_eq!(c1, vec![0xE4], "半字节进尾巴");
    let (s2, c2) = feed(&c1, &[0xB8]);
    assert_eq!(s2, "");
    assert_eq!(c2, vec![0xE4, 0xB8]);
    let (s3, c3) = feed(&c2, &[0xAD]);
    assert_eq!(s3, "中", "第三字节到齐 = 整字出");
    assert!(c3.is_empty());
}

/// 四字节 emoji (F0 9F 8C 95) 2+2 劈法
#[test]
fn spec_four_byte_emoji_split_2_2() {
    let (s1, c1) = feed(b"", &[0xF0, 0x9F]);
    assert_eq!(s1, "");
    let (s2, c2) = feed(&c1, &[0x8C, 0x95]);
    assert_eq!(s2, "🌕");
    assert!(c2.is_empty());
}

/// 劈在句子中间：前缀文字先出，尾巴攒着，下一口接续
#[test]
fn spec_split_mid_sentence() {
    let mut chunk = b"abc".to_vec();
    chunk.push(0xE4);
    let (s1, c1) = feed(b"", &chunk);
    assert_eq!(s1, "abc", "完整前缀照常出");
    assert_eq!(c1, vec![0xE4]);
    let mut chunk2 = vec![0xB8, 0xAD];
    chunk2.extend_from_slice(b"def");
    let (s2, c2) = feed(&c1, &chunk2);
    assert_eq!(s2, "中def");
    assert!(c2.is_empty());
}

/// 非法字节：替换符顶一个，流不炸、后续照常
#[test]
fn spec_invalid_byte_replacement() {
    let (s, c) = feed(b"", &[0xFF]);
    assert_eq!(s, "\u{FFFD}");
    assert!(c.is_empty(), "非法字节不进尾巴（它不是半截合法序列）");
    let (s2, _) = feed(b"", &[0x61, 0xFF, 0x62]);
    assert_eq!(s2, "a\u{FFFD}b");
}

/// 尾巴带新块里的完整字符：同一口里先拼旧尾巴再解新字
#[test]
fn spec_carry_plus_new_complete() {
    let (s1, c1) = feed(b"", &[0xE4, 0xB8]);
    assert_eq!(s1, "");
    let mut chunk2 = vec![0xAD];
    chunk2.extend_from_slice("🌕".as_bytes());
    let (s2, c2) = feed(&c1, &chunk2);
    assert_eq!(s2, "中🌕");
    assert!(c2.is_empty());
}

/// 空块喂养：只凭尾巴没有新字节，凑不齐还是不出
#[test]
fn spec_empty_chunk_keeps_carry() {
    let (s, c) = feed(&[0xE4], b"");
    assert_eq!(s, "");
    assert_eq!(c, vec![0xE4]);
}
