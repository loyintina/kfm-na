//! utf8x.rs — 增量 UTF-8 解码（A 档纯逻辑，考题 utf8x_spec.rs）
//!
//! 病灶（redroid 判卷实证）：v1 按 8KB 读块 from_utf8_lossy——块界劈开
//! 多字节字符时两侧各产一个 U+FFFD 替换符（终端 tofu 目击）。正身 =
//! 跨块续帧：解不完的尾巴（≤3 字节）攒进 carry，下一口接着拼。
//! 非法字节（不是半截合法序列）照旧替换符顶一个，流不炸。

/// 喂一块，返回（能解出的字符串， 新尾巴）。
/// carry 必须是上一次的返回值（或空）——只攒「半截合法序列」。
pub fn utf8_feed(carry: &[u8], chunk: &[u8]) -> (String, Vec<u8>) {
    let mut buf = Vec::with_capacity(carry.len() + chunk.len());
    buf.extend_from_slice(carry);
    buf.extend_from_slice(chunk);

    let mut out = String::new();
    let mut pos = 0usize;
    loop {
        match std::str::from_utf8(&buf[pos..]) {
            Ok(s) => {
                out.push_str(s);
                return (out, Vec::new());
            }
            Err(e) => {
                let valid = e.valid_up_to();
                // valid 前缀必合法（from_utf8 刚验过），unwrap 不炸
                out.push_str(std::str::from_utf8(&buf[pos..pos + valid]).expect("已验前缀"));
                pos += valid;
                match e.error_len() {
                    // 非法字节：替换符顶一个，越过它继续
                    Some(bad) => {
                        out.push('\u{FFFD}');
                        pos += bad;
                    }
                    // 尾巴是半截合法序列：攒进 carry 等下一口
                    None => return (out, buf[pos..].to_vec()),
                }
            }
        }
    }
}
