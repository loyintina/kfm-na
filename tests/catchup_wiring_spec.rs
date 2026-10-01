//! BAR-186 臂② 追赶模式壳接线守卫（源码钉，同 BAR-174 律：壳断了
//! 宿主全绿也照烂）。钉的每个字面串都是接缝——断一根 = 追赶模式
//! 静默失效，弱网慢滚复辟。

const APP: &str = include_str!("../src/android_app.rs");
const LIB: &str = include_str!("../src/lib.rs");
const TERMVIEW: &str = include_str!("../src/termview.rs");
const RESEED: &str = include_str!("../src/reseed.rs");
const GATE: &str = include_str!("../src/gate.rs");

/// 源码取函数体（4 空格方法级）：从签名行到下一个同级 fn。接线钉
/// 按函数粒度断言——全文件 contains 分不清同串挂在哪条路上
fn fn_body<'a>(src: &'a str, sig: &str) -> &'a str {
    let start = src
        .find(sig)
        .unwrap_or_else(|| panic!("源码里找不到 {sig}"));
    let rest = &src[start..];
    let end = rest[1..]
        .find("\n    fn ")
        .map(|i| i + 1)
        .unwrap_or(rest.len());
    &rest[..end]
}

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
            && APP.contains("let suppress = self.catchup.note_bytes(now, pump_bytes);"),
        "live 泵通路必须记字节量 + 追赶期抑制置脏"
    );
    assert!(
        APP.contains("let suppress = self.catchup.note_bytes(now, bytes.len());"),
        "v4 Feed 臂必须记字节量 + 追赶期抑制置脏"
    );
    // ②b BAR-216 观测账：两条通路都必须取进场沿报账（漏一条 = 该路
    //    追赶风暴无账——回显无影案判卷靠此）
    assert!(
        APP.matches("if let Some(cause) = self.catchup.take_enter() {")
            .count()
            >= 2,
        "泵/画布两通路都必须挂 take_enter 进场沿报账（BAR-216 仪器）"
    );
    // ③ 显式入场钩（BAR-209① 一扩三）：重连/attach/切会话三条重播种
    //    风暴路都必须挂 catchup.enter——漏一条 = 该路清场黑屏+回显压制
    //    复辟（0075 定罪：enter 只挂 respawn_session，另两路漏网）
    for sig in [
        "fn respawn_session(",
        "fn respawn_named_with(",
        "fn switch_session(",
    ] {
        let body = fn_body(APP, sig);
        assert!(
            body.contains("self.catchup.enter(crate::report::boot_ms());"),
            "{sig} 必须挂 catchup.enter（重连/attach/切会话三路同入追赶）"
        );
    }
    // enter 必须先于清场/补屏——追赶窗罩住整个风暴才有压帧意义
    let body = fn_body(APP, "fn respawn_named_with(");
    let enter_at = body.find("self.catchup.enter(").unwrap();
    let clear_at = body.find("self.reset_modes_on_respawn(name)").unwrap();
    assert!(enter_at < clear_at, "attach 路 enter 必须先于重孵清场");
    let body = fn_body(APP, "fn switch_session(");
    let enter_at = body.find("self.catchup.enter(").unwrap();
    let feed_at = body.find("g.feed(chunk.as_bytes())").unwrap();
    assert!(enter_at < feed_at, "切会话路 enter 必须先于 replay 补屏");
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
        APP.contains("fn catchup_land(&mut self, cause: &str)")
            && APP.contains("if g.display_offset() == 0 {")
            && APP.contains("g.land_bottom();"),
        "catchup_land 必须贴底才跳底（display_offset>0 = 用户在读历史，只补画）"
    );
    // ⑥b BAR-216 观测账：落地沿必须报压制量/持续时长（三因齐：
    //    播种尾锚/续播尾锚/静默窗满——漏一因 = 该落地路无账）
    assert!(
        APP.contains("self.catchup_land(\"播种尾锚\");")
            && APP.contains("self.catchup_land(\"续播尾锚\");")
            && APP.contains("self.catchup_land(\"静默窗满\");"),
        "三条落地路都必须带因报账（BAR-216 仪器）"
    );
    assert!(
        APP.contains("\"追赶落地: {cause} 压制="),
        "落地报账行必须带压制量与持续时长（BAR-216 仪器）"
    );
    // ⑥c 追赶期落键账（回显无影案第一现场）
    assert!(
        APP.contains("\"追赶期落键——回显随压帧等落地\""),
        "drain_ime_inject 必须报追赶期落键（BAR-216 仪器）"
    );
    // ⑥d BAR-216 修复：速率轮节拍帧必须挂 about_to_wait（tick 同位）
    //    ——断接 = 洪峰压帧回到全冻，回显无影+频闪复辟
    assert!(
        APP.contains("if self.catchup.throttle_frame(crate::report::boot_ms()) {"),
        "about_to_wait 必须挂 throttle_frame 节拍帧（BAR-216 修复）"
    );
    assert!(
        APP.contains(" 节拍={} "),
        "落地报账行必须带节拍帧数（BAR-216 仪器：限拍账随落地报）"
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

/// BAR-186 臂① 播种对账合并壳接线守卫
#[test]
fn spec_bar186_对账接线守卫() {
    // ⓪ 模块归位
    assert!(
        LIB.contains("pub mod reseed;"),
        "lib.rs 必须接 pub mod reseed（播种对账纯逻辑件）"
    );
    // ① Build 臂必须先对账再定路：无对账 = 每次播种万行重建照烂
    //    （BAR-215 扩第 5 参 cap_fed：%output 喂态 → 尾块免 LF）
    assert!(
        APP.contains("crate::reseed::plan_reseed(old, &cap, rows, (x, y), e.cap_fed)"),
        "Build 臂必须挂 plan_reseed 对账（旧播种账 × 新 capture × 画布喂态）"
    );
    // ② 尾块双落点：浏览画布 + 后台画布，缺一 = 该态照重建
    assert!(
        APP.contains("reseed_browse(&tail)"),
        "浏览中画布必须走 reseed_browse 尾块续播"
    );
    assert!(
        APP.contains("canvas.reseed(&tail)"),
        "后台画布必须走 canvas.reseed 尾块续播"
    );
    // ③ 账纪律：last_cap 只在落地归账（在途虚账不许进）——
    //    Build 臂记 build_cap，安装处过账，resize 作废同焚
    assert!(
        APP.contains("e.build_cap = Some(cap);"),
        "Build 臂必须记 build_cap 在途账"
    );
    assert!(
        APP.contains("if let Some(cap) = e.build_cap.take() {"),
        "画布安装处必须过账 build_cap → last_cap"
    );
    assert!(
        APP.contains("self.last_cap = None; // 尺变行宽变，播种对账旧账同焚"),
        "invalidate 必须焚播种账（尺变对账无意义）"
    );
    // ④ 回落路还在：对账判负/无处可喂 = 全量重建旧路（Build 线程
    //    + 在途账 build_cap 随 rx 挂账）
    assert!(
        APP.contains("let cap_build = cap.clone();") && APP.contains("e.build = Some(rx);"),
        "对账判负必须回落全量重建（Build 线程旧路）"
    );
    // ⑤ TermView/Canvas 三件：reseed_browse（trait+实现）/ Canvas::reseed
    //    / dump_all（等价钉对账件）
    assert!(
        TERMVIEW.contains("pub fn reseed_browse(&mut self, tail: &[u8]) -> bool"),
        "TermView 必须有 reseed_browse"
    );
    assert!(
        TERMVIEW
            .matches("fn reseed_browse(&mut self, tail: &[u8]) -> bool")
            .count()
            >= 2,
        "reseed_browse 必须进 TermEmu trait 且有委托实现"
    );
    assert!(
        TERMVIEW.contains("pub fn reseed(&mut self, tail: &[u8])")
            && TERMVIEW.contains("pub fn dump_all(&self) -> String"),
        "Canvas 必须有 reseed + dump_all（等价钉对账件）"
    );
}

/// BAR-209② 重孵/判负不焚画布（清场改对账续传）接线守卫：通道死 ≠
/// 画布死——尺没变，旧画布+对账账留着给下次播种做 plan_reseed 前缀
/// 对账（命中 = 尾块原位续长免清场；不命中它自会判 Rebuild 回旧路
/// 保底）。焚了 = 断线回播只能全量重建（57.8s 风暴旧路）。
#[test]
fn spec_bar209_对账续传接线守卫() {
    // ① 判负拆除只拆通道：Close + 相位归零照旧，画布/对账账不许焚
    let body = fn_body(APP, "fn ctrl_teardown(");
    assert!(
        body.contains("TermCmd::Close") && body.contains("e.feed.reset();"),
        "ctrl_teardown 仍须关通道+相位归零（拆除本分不动）"
    );
    assert!(
        !body.contains("e.canvas = None") && !body.contains("e.last_cap = None"),
        "ctrl_teardown 不许焚画布/对账账（BAR-209②：留账对账续传）"
    );
    // ② 焚账唯一口 = invalidate（换尺）——旧律不动：尺变行宽变，
    //    旧尺画布/旧账留着就是错屏
    let inv = fn_body(APP, "fn invalidate(&mut self, now: u64)");
    assert!(
        inv.contains("self.canvas = None;") && inv.contains("self.last_cap = None;"),
        "invalidate（换尺）必须仍是焚画布/焚对账账的唯一口"
    );
}

/// BAR-209③ 播种风暴调度（活动排头 + 在途并发帽）接线守卫：自重启
/// 后全会话同时起播 = capture/构建洪峰滚雪球（0107 §二 第二批 96.5s）
#[test]
fn spec_bar209_播种风暴接线守卫() {
    // ⓪ 模块归位
    assert!(
        LIB.contains("pub mod seed_sched;"),
        "lib.rs 必须接 pub mod seed_sched（播种调度纯逻辑件）"
    );
    let body = fn_body(APP, "fn ctrl_ensure(");
    // ① 扩臂播种序必须走 order_seed（活动会话排头先点亮）
    assert!(
        body.contains("crate::seed_sched::order_seed("),
        "ctrl_ensure 扩臂必须用 order_seed 排播种序（活动会话排头）"
    );
    // ② 扩臂与养臂都必须过 admit_seed 并发帽（缺一臂 = 该臂照滚雪球）
    assert!(
        body.matches("crate::seed_sched::admit_seed(").count() >= 2,
        "ctrl_ensure 扩臂与养臂都必须过 admit_seed 并发帽"
    );
    // ③ 在途计数单一口径（各算各的 = 帽形同虚设）
    assert!(
        body.contains("crate::seed_sched::inflight_of("),
        "ctrl_ensure 在途计数必须走 inflight_of 单一口径"
    );
}

/// BAR-215 双通道同喂修复壳接线守卫：Steady 相 %output 喂布不更
/// last_cap 账 → Tail 的 k-LF 把已入史的行再推一遍 = 每档续播复制
/// k 行重复带（redroid 定罪三证：遥测 k=5+%output=210B 落同布、
/// 画布史超额随档线性增长、94 行 T 行双份交替带）。修复五件套——
/// 喂态账（cap_fed/cap_stale）× plan_reseed 第 5 参与 fed 产 Skip
/// （活镜像一笔不画：LF 重发=重复带，全屏重画=抹快照后竞速行，
/// redroid 复判画布史差值 50→57 线性漂移定罪）× 喂点挂账 ×
/// 通道重开强制重建 × 落地 fed 凭据不归零，缺一件病灶回潮
#[test]
fn spec_bar215_双喂接线守卫() {
    // ① WarmSess 带双账：cap_fed（画布已领先账）+ cap_stale（分歧强制重建）
    assert!(
        APP.contains("cap_fed: bool") && APP.contains("cap_stale: bool"),
        "WarmSess 必须带 cap_fed/cap_stale 双账"
    );
    // ② plan_reseed 吃喂态（第 5 参）——纯逻辑件签名钉
    assert!(
        RESEED.contains("canvas_fed: bool"),
        "plan_reseed 必须吃 canvas_fed（fed = 活镜像）"
    );
    assert!(
        RESEED.contains("if canvas_fed {\n        return ReseedPlan::Skip;"),
        "fed 对账过必须产 Skip（LF 重发=重复带；全屏重画=抹竞速行）"
    );
    // ②b Build 臂必须三路分派且 Skip 臂一笔不画（只取样遥测）
    assert!(
        APP.contains("crate::reseed::ReseedPlan::Skip => {"),
        "Build 臂必须分派 Skip（活镜像免画）"
    );
    assert!(
        APP.contains("stay_fed = true;"),
        "Skip 落地必须留 fed 凭据（画布仍领先账，归零=下档重复带回潮）"
    );
    // ③ Feed 臂挂账：喂进 cap_fed=true / 无布可喂 cap_stale=true
    assert!(
        APP.contains("e.cap_fed = true;") && APP.contains("e.cap_stale = true;"),
        "Feed 臂必须按送达与否挂 cap_fed/cap_stale"
    );
    // ④ Build 臂：cap_stale 短路判负 + Tail 落地双账归位
    assert!(
        APP.contains("if e.cap_stale {"),
        "Build 臂必须对 cap_stale 短路判负（分歧画布只许全量重建）"
    );
    assert!(
        APP.contains(
            "e.cap_fed = stay_fed || pend_fed;\n                        e.cap_stale = false;"
        ),
        "落地必须按 Skip/补喂留 fed 凭据 + stale 归位（归零 = 下档 k-LF 重复带回潮）"
    );
    // ⑤ 通道重开：旧画布领先账不可知 → 强制一档全量重建对齐真源
    assert!(
        APP.contains("if e.canvas.is_some() {\n                e.cap_stale = true;\n            }"),
        "ctrl 开必须对存活画布挂 cap_stale（通道断档 = 领先账不可知）"
    );
    // ⑥ 观测闸两件：fed_bytes 账 + canvas-req 倒账通道（定罪仪器不退场）
    assert!(
        APP.contains("e.fed_bytes += bytes.len();"),
        "Feed 臂必须记 fed_bytes（双喂定罪账）"
    );
    assert!(
        GATE.contains("pub fn take_canvas_req(dir: &str) -> bool")
            && APP.contains("self.dump_warm_canvases();"),
        "canvas-req 倒账闸必须接线（gate 取件 + App 落盘）"
    );
    // ⑦ BAR-215③ 断档修复：Tail 臂对快照后输出（Building 相 Pended）
    //    必须 take + 补喂落点画布——%end 后的 %output 恒在快照外，
    //    旧律 pending.clear() = 每档续播丢一批行（5s 档 × 1s 行锁相
    //    实测画布史恒 −1/档）；回潮 clear 即断档复辟
    assert!(
        !APP.contains("e.pending.clear(); // 播种窗输出已在快照内（InCapture 吞咽同规）"),
        "Tail 臂不许再 clear 快照后输出（断档病灶回潮）"
    );
    assert!(
        APP.contains("pend = std::mem::take(&mut e.pending);")
            && APP.contains("canvas.feed_bytes(&pend);")
            && APP.contains("feed_browse(&pend)"),
        "Tail 臂必须把快照后输出补喂落点画布（安装臂同规）"
    );
}
