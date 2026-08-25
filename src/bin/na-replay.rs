//! na-replay — 飞行记录仪 host 回放器（2026-08-24 自观测·确定性回放）
//!
//! 把 na 落盘的 flight-rec.bin 喂进同一台 TermView：渲染现场不用碰手机
//! 就能复现、慢看、比对。v1 = 文本级（dump_text，字体无关所以 host 字体
//! 差异不影响判读）；像素级留给需要时再加（生产内嵌字体 host 也能载）。
//!
//! 用法:
//!   cargo run --quiet --bin na-replay -- <flight-rec.bin> [会话名,默认 local]
//! 输出:末屏纯文本(对齐闸门 text 通道的「所见」) + stderr 统计行。

use kfm_na::gate::{RecEvent, rec_decode_all};
use kfm_na::termview::TermView;

fn host_font() -> fontdue::Font {
    for p in [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/data/data/com.termux/files/usr/share/fonts/TTF/DejaVuSans.ttf",
    ] {
        if let Ok(bytes) = std::fs::read(p) {
            return fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .expect("host 回放字体 fontdue 不认");
        }
    }
    panic!("host 回放字体缺失(DejaVuSans.ttf 候选路径全灭)");
}

fn main() {
    let mut args = std::env::args().skip(1);
    let file = args.next().unwrap_or_else(|| {
        eprintln!("用法: na-replay <flight-rec.bin> [会话名,默认 local]");
        std::process::exit(64);
    });
    let want = args.next().unwrap_or_else(|| "local".into());

    let buf = std::fs::read(&file).unwrap_or_else(|e| panic!("读 {file} 失败: {e}"));
    let evs = rec_decode_all(&buf).unwrap_or_else(|e| panic!("解码失败: {e}"));

    // 末值尺寸起手(没有 resize 记录 = 开局占位尺寸,首个 resize 到了再纠)
    let mut tv = TermView::new(host_font(), None, 120, 40, 8, 16);
    let (mut n_out, mut n_resize) = (0u64, 0u64);
    let mut last_ts = 0u64;
    for ev in &evs {
        last_ts = ev.ts_ms();
        match ev {
            // 网格共享——尺寸事件不分会话全应用(与现实同构)
            RecEvent::Resize { cols, rows, .. } => {
                tv.resize_cells(*cols, *rows);
                n_resize += 1;
            }
            RecEvent::Output { name, data, .. } => {
                if *name == want {
                    tv.feed(data);
                    n_out += 1;
                }
            }
        }
    }
    eprintln!(
        "[na-replay] 事件 {} 条(输出 {n_out} / 尺寸 {n_resize}),时间线 {}ms,会话过滤 = {want}",
        evs.len(),
        last_ts
    );
    print!("{}", tv.dump_text());
}
