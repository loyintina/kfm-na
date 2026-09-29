//! BAR-186 臂② 追赶模式壳接线守卫（源码钉，同 BAR-174 律：壳断了
//! 宿主全绿也照烂）。钉的每个字面串都是接缝——断一根 = 追赶模式
//! 静默失效，弱网慢滚复辟。

const APP: &str = include_str!("../src/android_app.rs");
const LIB: &str = include_str!("../src/lib.rs");
const TERMVIEW: &str = include_str!("../src/termview.rs");

#[test]
fn spec_bar186_追赶接线守卫() {
    // ⓪ 模块归位：mod 不接线 = 纯逻辑件再绿也上不了车
    assert!(
        LIB.contains("pub mod catchup;"),
        "lib.rs 必须接 pub mod catchup（追赶状态机纯逻辑件）"
    );
    // ① App 持状态机：字段不在 = 全壳无追赶账
    assert!(
        APP.contains("catchup: crate::catchup::Catchup,"),
        "App 必须持 catchup 字段（追赶模式状态机）"
    );
    // ② 两条进料通路都登记字节且追赶期不置脏——只接一条 = 另一条
    //    通路弱网照滚
    assert!(
        APP.contains("pump_bytes += b.len();")
            && APP.contains("if !self.catchup.note_bytes(now, pump_bytes) {"),
        "live 泵通路必须记字节量 + 追赶期抑制置脏"
    );
    assert!(
        APP.contains("if !self.catchup.note_bytes(now, bytes.len()) {"),
        "v4 Feed 臂必须记字节量 + 追赶期抑制置脏"
    );
    // ③ 显式入场钩：重连 = 重播种/重画风暴开端
    assert!(
        APP.contains("self.catchup.enter(crate::report::boot_ms());"),
        "respawn_session 必须挂 catchup.enter（重连入场）"
    );
    // ④ 追平判据二：播种尾锚（画布安装）落地
    assert!(
        APP.contains("if self.catchup.anchor() == crate::catchup::CatchAct::Land {"),
        "ctrl_drain 画布安装处必须挂 catchup.anchor 落地"
    );
    // ⑤ 追平判据一：about_to_wait 每圈 tick 静默窗
    assert!(
        APP.contains(
            "if self.catchup.tick(crate::report::boot_ms()) == crate::catchup::CatchAct::Land {"
        ),
        "about_to_wait 必须挂 catchup.tick（静默窗追平）"
    );
    // ⑥ 落地帧三件套：跳底（仅贴底时）+ 置脏；用户上翻读历史不抢滚动条
    assert!(
        APP.contains("fn catchup_land(&mut self)")
            && APP.contains("if g.display_offset() == 0 {")
            && APP.contains("g.land_bottom();"),
        "catchup_land 必须贴底才跳底（display_offset>0 = 用户在读历史，只补画）"
    );
    // ⑦ TermView 落地件：跳底 + 像素零头一刀齐（scroll_to_bottom 不清
    //    零头，缺这刀 = 落地帧留半格残影）
    assert!(
        TERMVIEW.contains("pub fn land_bottom(&mut self)"),
        "TermView 必须有 land_bottom（追平落地件）"
    );
    assert!(
        TERMVIEW.matches("self.scroll_frac_px = 0.0;").count() >= 4,
        "land_bottom 必须同旧三先例一样清像素零头（scroll_frac_px 归零）"
    );
}

const OFFLINE: &str = include_str!("../src/offline_keys.rs");

/// BAR-186 臂③ 输入 WAL 壳接线守卫
#[test]
fn spec_bar186_wal接线守卫() {
    // ① 壳必须挂 WAL（BAR-174 缓存根同位）：不挂 = 进程死输入全灭照烂
    assert!(
        APP.contains(r#"attach_wal(&dir.join("cache/offline-input.wal"))"#),
        "壳必须在私有目录就绪处 attach_wal（断线输入落盘）"
    );
    // ② 模块三件：读回 / 追加 / 重写——缺一臂 WAL 语义不全
    assert!(
        OFFLINE.contains("pub fn attach_wal")
            && OFFLINE.contains("fn wal_append")
            && OFFLINE.contains("fn wal_rewrite")
            && OFFLINE.contains("fn hex_dec"),
        "offline_keys 必须有 attach_wal/wal_append/wal_rewrite/hex_dec 四件"
    );
    // ③ 回冲清账：drain 必须 remove_file（不回冲 = 重启重放已发输入 = 事故）
    assert!(
        OFFLINE.contains("std::fs::remove_file"),
        "drain 必须清 WAL 账（remove_file）——漏清 = 重启重复回冲"
    );
    // ④ 降级律：落盘失败摘 WAL 降级纯内存，不反复撞 IO 不 panic
    assert!(
        OFFLINE.contains("self.wal = None;"),
        "落盘失败必须摘 WAL 降级（缓存是加强不是命脉）"
    );
}
