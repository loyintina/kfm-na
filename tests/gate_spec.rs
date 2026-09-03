//! screendump 考题（A 档）——画面回传的编码与触发语义
//!
//! 契约（2026-08-24 与用户定）：
//! ①XRGB u32 小端编码，每像素 4 字节，字节序 = B,G,R,X；
//! ②触发文件不在 → 不动；在 → 倒 shot.rgb + shot.dim(“w h”)并摘触发；
//! ③倒出来的字节数必须 = w*h*4（缺斤短两=画面错位）。

use kfm_na::gate::{encode_rgb, maybe_dump, trigger_pending};
use kfm_na::session::SessionEvent;

/// 全局 PUMP 是进程级单例：凡碰它的考题必须串行进场（BAR-057
/// 2026-09-03 手机 chain 实拍竞态 flake——fed与分桶字节账 的 "LL"
/// 被并行考题的 pump_once 截胡进待机 replay → fed=false 误红）。
/// 文件头注释本就写着「单测串行使用」，但 cargo test 默认并行，
/// 注释挡不住调度——同一把锁机械执法。
static PUMP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    // 碰 DUMP_WH/DUMP_TERM 全局账的考题都得串行（BAR-057 教训；
    // 期 0③ 起又添 spec_dump装帧 同场——不锁就是尺寸账互踩）
    let _g = PUMP_LOCK.lock().unwrap();
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

/// dump 装帧对拍（期 0③）：AI 全屏页倒出来的必须是**真消息**不是占位
/// 空壳——值守线程读 AI_CHAT 注册位（D9 同源：前台 rasterize 与后台
/// 装帧同一份 AiChatState）。消息区（顶部边距起）无字 = 装帧断了眼
#[test]
fn spec_dump装帧_ai页画真消息() {
    let _g = PUMP_LOCK.lock().unwrap();
    use kfm_na::ai_chat::AiChatState;
    use kfm_na::ai_presence::AiPresenceState;
    use kfm_na::gate::{
        dump_now, note_frame_size, register_ai_chat, register_ai_presence, register_dump_term,
    };
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
    let tv = TermView::new(font, None, 8, 2, CELL_W, CELL_H);
    let term: Arc<Mutex<Box<dyn TermEmu>>> = Arc::new(Mutex::new(Box::new(tv)));
    register_dump_term(&term);

    // AI 全屏 + 一条真消息（ASCII——host 无 CJK 备用字体，tofu 会跳过）
    let ai = Arc::new(AiPresenceState::new());
    ai.set_bounds(800, 600, 0);
    ai.tap_orb(); // terminal → AiFullscreen
    register_ai_presence(&ai);
    let chat = Arc::new(AiChatState::new());
    chat.user_send("dump-frame-token");
    register_ai_chat(&chat);

    let (w, h) = (800u32, 600u32);
    let dir = std::env::temp_dir().join(format!("kfm-shot6-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let d = dir.to_str().unwrap();
    note_frame_size(w, h);
    std::fs::write(dir.join("shot-req"), b"").unwrap();
    dump_now(d);
    let raw = std::fs::read(dir.join("shot.rgb")).unwrap();
    assert_eq!(raw.len(), (w * h * 4) as usize);
    // 消息区 = y 48..180 × x 60..500（角色标签行+正文行；避开右下光球晕）
    let mut has_text = false;
    for y in 48..180u32 {
        for x in 60..500u32 {
            let i = ((y * w + x) * 4) as usize;
            let px = (raw[i + 2] as u32) << 16 | (raw[i + 1] as u32) << 8 | raw[i] as u32;
            if px != kfm_na::termview::AI_PAGE_BG {
                has_text = true;
                break;
            }
        }
        if has_text {
            break;
        }
    }
    assert!(
        has_text,
        "dump 装帧必须画真消息——AI_CHAT 注册位同源读数断了"
    );
    std::fs::remove_dir_all(&dir).ok();
    // 静态复位（同场考题互不累）：page 回终端、尺寸账归零——
    // spec_后台值守 的「没尺寸记账」前提就是被我打破的（合跑实拍）
    ai.tap_orb();
    note_frame_size(0, 0);
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

// ---- 死亡观测考题(2026-08-25,与用户定:panic 落盘 + loop 看门狗) ----

/// 卡死判定边界:龄期 ≤ 阈值不报警,> 阈值才报警(阈值含在「正常」侧,
/// 忙轮询循环 3s 没盖戳已是铁案,不许误报也不许漏报)
#[test]
fn spec_看门狗_卡死判定边界() {
    use kfm_na::gate::{LOOP_STALL_MS, is_stall};
    assert!(!is_stall(0));
    assert!(!is_stall(LOOP_STALL_MS - 1));
    assert!(!is_stall(LOOP_STALL_MS), "恰达阈值不报警(边界归正常侧)");
    assert!(is_stall(LOOP_STALL_MS + 1));
    assert!(is_stall(u64::MAX));
}

/// panic 档案行格式钉死:unix=秒 thread=名 at=位置 msg=消息,
/// 单行(消息里哪怕有换行也不许撕成两行——一行一案,grep 友好)
#[test]
fn spec_panic行_格式钉() {
    use kfm_na::gate::panic_line;
    let line = panic_line(1787654400, "main", "src/gate.rs:100", "boom");
    assert_eq!(
        line,
        "unix=1787654400 thread=main at=src/gate.rs:100 msg=boom"
    );
    // 无名线程与缺位置的组合也要成形
    let line = panic_line(1, "<无名>", "-", "<非串荷载>");
    assert_eq!(line, "unix=1 thread=<无名> at=- msg=<非串荷载>");
    assert!(!line.contains('\n'), "一行一案");
}

/// 多行消息必须压成一行(换行→␤),档案 grep 不被撕碎
#[test]
fn spec_panic行_多行消息压单行() {
    use kfm_na::gate::panic_line;
    let line = panic_line(2, "t", "x.rs:1", "第一行\n第二行\n第三行");
    assert_eq!(line, "unix=2 thread=t at=x.rs:1 msg=第一行␤第二行␤第三行");
    assert_eq!(line.matches('\n').count(), 0);
}

/// BAR-036:看门狗四态判决钉——前台门控是前提:挂起态(退后台)循环合法
/// 停跳,不许报 STALL(首装实拍误报:退后台 5 分钟 beat_age=355s 报警)
#[test]
fn spec_bar036_看门狗_前台门控四态() {
    use kfm_na::gate::{LOOP_STALL_MS, WatchState, watch_verdict};
    // 退后台:龄期再大也是休假,不判
    assert_eq!(watch_verdict(false, Some(u64::MAX)), WatchState::Background);
    assert_eq!(watch_verdict(false, None), WatchState::Background);
    // 前台未起跳
    assert_eq!(watch_verdict(true, None), WatchState::NoBeat);
    // 前台正常/卡死边界
    assert_eq!(watch_verdict(true, Some(0)), WatchState::Alive(0));
    assert_eq!(
        watch_verdict(true, Some(LOOP_STALL_MS)),
        WatchState::Alive(LOOP_STALL_MS)
    );
    assert_eq!(
        watch_verdict(true, Some(LOOP_STALL_MS + 1)),
        WatchState::Stall(LOOP_STALL_MS + 1)
    );
}

#[test]
fn spec_switch_req_置位取走协议() {
    // 通道九:文件存在=置位,switch_take 取走即清(一次性,toggle 语义)
    let dir = std::env::temp_dir().join(format!("kfm-switch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let req = dir.join("switch-req");
    std::fs::write(&req, b"").unwrap();

    kfm_na::gate::switch_req_check(dir.to_str().unwrap());
    assert!(kfm_na::gate::switch_take(), "switch-req 存在必须置位");
    assert!(
        !kfm_na::gate::switch_take(),
        "取走即清:第二次必须 false(一次性语义)"
    );
    assert!(!req.exists(), "触发文件必须被消费删除");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn spec_包装层_fed与分桶字节账() {
    // 通道考题(变异 triage r1 补题):pump_once 包装层的 fed 协议 +
    // 分桶字节账(match local/remote 臂此前无人判卷,臂删除幸存针)。
    // 全局 PUMP 单测串行使用,会话名独占避免互相干扰。
    // 注意:分桶字节账按精确名 "local"/"remote" 匹配——名字带前缀会落 OTHER 桶
    let _g = PUMP_LOCK.lock().unwrap();
    let (tx_l, rx_l) = std::sync::mpsc::channel();
    let (tx_r, rx_r) = std::sync::mpsc::channel();
    kfm_na::gate::pump_register("local", rx_l);
    kfm_na::gate::pump_register("remote", rx_r);

    let before = kfm_na::gate::stats_snap();
    tx_l.send(SessionEvent::Output { data: "LL".into() })
        .unwrap();
    tx_r.send(SessionEvent::Output { data: "RR".into() })
        .unwrap();

    // active=wrap-local:活跃进 sink,待机进 replay;分桶账两臂都记
    let mut got = Vec::new();
    let fed = kfm_na::gate::pump_once("local", &mut |b| {
        got.push(String::from_utf8_lossy(b).into_owned())
    });
    assert!(fed, "有活跃输出必须报 fed=true");
    assert_eq!(got, ["LL"], "活跃方输出进 sink");

    let after = kfm_na::gate::stats_snap();
    assert_eq!(
        after.bytes_local - before.bytes_local,
        2,
        "local 臂删除变异由此断言按住"
    );
    assert_eq!(
        after.bytes_remote - before.bytes_remote,
        2,
        "remote 臂删除变异由此断言按住"
    );

    // 空转一圈:fed=false(无输出不谎报)
    let fed = kfm_na::gate::pump_once("local", &mut |_| {});
    assert!(!fed, "无输出必须报 fed=false");
}

#[test]
fn spec_包装层_control与take_replay() {
    // 控制事件出队 + 待机 replay 取走即清(包装层协议)
    let _g = PUMP_LOCK.lock().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    kfm_na::gate::pump_register("wrap-ctl", rx);
    tx.send(SessionEvent::Exited { code: 7 }).unwrap();
    tx.send(SessionEvent::Output {
        data: "cached".into(),
    })
    .unwrap();
    kfm_na::gate::pump_once("wrap-other", &mut |_| {}); // wrap-ctl 为待机方

    let ctl = kfm_na::gate::pump_take_control();
    assert!(
        ctl.iter()
            .any(|(n, e)| *n == "wrap-ctl" && matches!(e, SessionEvent::Exited { code: 7 })),
        "控制事件必须一粒不少出队"
    );

    let replay = kfm_na::gate::pump_take_replay("wrap-ctl");
    assert_eq!(replay, ["cached"], "待机输出补屏料完整");
    assert!(
        kfm_na::gate::pump_take_replay("wrap-ctl").is_empty(),
        "取走即清"
    );
}
