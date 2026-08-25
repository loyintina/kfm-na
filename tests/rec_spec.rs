//! tests/rec_spec.rs — 飞行记录仪考题(2026-08-24 自观测·确定性回放)
//!
//! 判的坑:bug 现场随时间流走,事后只剩一句「刚才花了」——飞行记录仪把
//! 输出流/尺寸事件带时间戳落盘,host 回放器喂进同一台 TermView 复现。
//! 本文件钉死容器格式本身:编码/解码往返、截尾容忍(崩在半条记录上
//! 不许全军覆没)、坏魔数拒绝、超帽压缩保新丢旧。

use kfm_na::gate::{RecEvent, rec_compact, rec_decode_all, rec_encode};

fn out(ts: u64, name: &str, data: &str) -> RecEvent {
    RecEvent::Output {
        ts_ms: ts,
        name: name.into(),
        data: data.as_bytes().to_vec(),
    }
}

fn resize(ts: u64, name: &str, cols: u32, rows: u32) -> RecEvent {
    RecEvent::Resize {
        ts_ms: ts,
        name: name.into(),
        cols,
        rows,
        cell_w: 8,
        cell_h: 16,
    }
}

/// 考题 1:编码→解码往返——输出(含 UTF-8 中文与二进制 0 字节)、
/// 尺寸事件,顺序与内容一粒不走样
#[test]
fn spec_rec_编解码往返() {
    let evs = vec![
        out(0, "local", "u0_a376@localhost ~$ "),
        out(12, "local", "你好\x00\x1b[32m世界"),
        resize(20, "local", 120, 40),
        out(33, "remote", "root@server:~# "),
    ];
    let mut buf = kfm_na::gate::REC_MAGIC.to_vec();
    for ev in &evs {
        buf.extend_from_slice(&rec_encode(ev));
    }
    let decoded = rec_decode_all(&buf).expect("合法流必须解开");
    assert_eq!(decoded, evs);
}

/// 考题 2:截尾容忍——进程死在半条记录上(写盘被掐),前面完好的记录
/// 必须全部解得出,尾巴安静丢弃(坠机现场多数死于最后一条)
#[test]
fn spec_rec_截尾容忍() {
    let mut buf = kfm_na::gate::REC_MAGIC.to_vec();
    buf.extend_from_slice(&rec_encode(&out(0, "local", "aaa")));
    buf.extend_from_slice(&rec_encode(&out(5, "local", "bbb")));
    let mut half = rec_encode(&out(9, "local", "ccc"));
    half.truncate(7); // 半条记录
    buf.extend_from_slice(&half);

    let decoded = rec_decode_all(&buf).expect("截尾不许报错");
    assert_eq!(
        decoded,
        vec![out(0, "local", "aaa"), out(5, "local", "bbb")]
    );
}

/// 考题 3:坏魔数拒绝——不是记录仪文件的,一粒都不许解(防拿错文件白分析)
#[test]
fn spec_rec_坏魔数拒绝() {
    assert!(rec_decode_all(b"NOTAREC\nxxxx").is_err());
    assert!(rec_decode_all(b"").is_err());
}

/// 考题 4:超帽压缩——保新丢旧、魔数保留、结果可解、总量压回帽下;
/// 帽内不动(原样返回)
#[test]
fn spec_rec_超帽保新丢旧() {
    let mut buf = kfm_na::gate::REC_MAGIC.to_vec();
    for i in 0..100u64 {
        buf.extend_from_slice(&rec_encode(&out(i, "local", &"x".repeat(1000))));
    }
    let cap = 30_000;
    let compacted = rec_compact(&buf, cap);
    assert!(compacted.len() <= cap, "压缩后必须压回帽下");
    assert!(compacted.starts_with(kfm_na::gate::REC_MAGIC));
    let decoded = rec_decode_all(&compacted).expect("压缩结果必须可解");
    assert!(!decoded.is_empty());
    // 最新的一定在,最旧的一定丢了
    assert_eq!(
        decoded.last().unwrap(),
        &out(99, "local", &"x".repeat(1000))
    );
    assert!(!decoded.contains(&out(0, "local", &"x".repeat(1000))));
    // 保序:留下的事件时间戳单调不减
    let ts: Vec<u64> = decoded.iter().map(|e| e.ts_ms()).collect();
    assert!(ts.windows(2).all(|w| w[0] <= w[1]));

    // 帽内原样
    let small = rec_compact(&compacted, cap);
    assert_eq!(small, compacted);
}

/// 考题 5:单条超帽也留最新一条(宁爆帽不丢现场——最后一条是案发点)
#[test]
fn spec_rec_单条超帽留最新() {
    let mut buf = kfm_na::gate::REC_MAGIC.to_vec();
    buf.extend_from_slice(&rec_encode(&out(0, "local", &"y".repeat(5000))));
    buf.extend_from_slice(&rec_encode(&out(1, "local", &"z".repeat(5000))));
    let compacted = rec_compact(&buf, 1000);
    let decoded = rec_decode_all(&compacted).expect("可解");
    assert_eq!(decoded, vec![out(1, "local", &"z".repeat(5000))]);
}
