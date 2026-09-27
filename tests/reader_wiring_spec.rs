//! reader_wiring_spec.rs — 阅读页**接线**考题（BAR-170，2026-09-27）
//!
//! 分工说明：纯逻辑核的考卷在 `tests/reader_page_spec.rs`（状态核 A 档
//! 带变异）；缝的占槽/直通/重播踢在 `tests/fx_ease_spec.rs` 末（第七道
//! 缝入账钉，与 Demo 缝同款五件）；几何命中钉在同册 `rd_return_hit`
//! 用例。本册管**涂装与壳核之间的机械接线**——漏一处 = 点文件没反应 /
//! 阅读页零帧 / 返回钮点不中 / 甩尾不取消，实拍前就能钉死的缺口类错误
//! （同 ftree_wiring_spec 的立法）。
//!
//! 变异方向：改道点缺 summon（阅读页不进场）/ 缺 dismiss FileTree（树
//! 不关压在页上）/ 活性链摘 reader 那路（甩尾零帧）/ gles 槽号错（画到
//! 别人的槽）/ 链尾字面量被吞（viewer/ft 两路被挤掉）——本册必红。

#[test]
fn spec_bar170_文件点击改道阅读页() {
    let app = include_str!("../src/android_app.rs");
    // 改道三件套必须在：关树 → 召阅读页 → 起首块拉取（会话池信件仍走旧
    // request_read，不走此道——见改道点注释）
    assert!(
        app.contains("点文件→阅读页"),
        "改道点必须报账（ftree 频道）"
    );
    assert!(
        app.contains("ai.dismiss_top(\n                                                crate::ai_presence::Panel::FileTree,\n                                            );")
            || app.contains("dismiss_top(crate::ai_presence::Panel::FileTree)"),
        "点文件必须关文件树（契约：树关页进同拍）"
    );
    assert!(
        app.contains("summon_panel(crate::ai_presence::Panel::Reader)"),
        "点文件必须召唤阅读页面板"
    );
    assert!(
        app.contains("crate::fs_fetch::request_read_chunk(0)"),
        "改道必须起首块（offset=0）拉取"
    );
    assert!(
        app.contains("crate::ui::reader_page::reader_handle()"),
        "改道必须喂状态核 open(path, name)"
    );
}

#[test]
fn spec_bar170_活性链第九路与缝() {
    let fx = include_str!("../src/ui/fx_spring.rs");
    // 活性旗定义 + 接进 active 表（定义与接线两件都要在——只钉定义会被
    // 「留着定义摘掉或项」的变异逃逸，ftree_wiring_spec 已咬过这一口）
    assert!(
        fx.contains("pub fn reader_fling_live()")
            && fx.contains("let reader_fling = reader_fling_live();"),
        "阅读页甩尾活性旗必须在（定义一项）"
    );
    assert!(
        fx.contains("|| reader_fling"),
        "阅读页甩尾活性必须进 fx_frame_due 的 active 表（摘掉 = 甩尾零帧）"
    );
    // 链尾字面量必须仍在（第八路插法规：新旧钉同吃不破——reader 插在
    // viewer 前，不许把 viewer/ft 两路挤掉）
    assert!(
        fx.contains("|| viewer_fling || ft_fling;"),
        "活性链尾「|| viewer_fling || ft_fling;」字面量必须仍在"
    );
    let app = include_str!("../src/android_app.rs");
    assert!(
        app.contains("crate::ui::fx_spring::note_reader_fling_live(self.reader_fling.is_some())"),
        "壳每圈帧泵末尾必须同步活性旗"
    );
    // 出栈清点：阅读页出栈必须 close + 清甩尾（漏 = 页没了甩尾还在空转）
    assert!(
        app.matches("note_reader_fling_live(false)").count() >= 2,
        "活性旗清位至少两处（出栈清点 + 起手/燃尽路）"
    );
    // 活性旗的真行为（不是 grep）：置位读得回、清位读得回、默认假
    assert!(
        !kfm_na::ui::fx_spring::reader_fling_live(),
        "默认该是假（无甩尾不产帧）"
    );
    kfm_na::ui::fx_spring::note_reader_fling_live(true);
    assert!(kfm_na::ui::fx_spring::reader_fling_live());
    kfm_na::ui::fx_spring::note_reader_fling_live(false);
    assert!(!kfm_na::ui::fx_spring::reader_fling_live());
}

#[test]
fn spec_bar170_涂装三路与槽号() {
    let gles = include_str!("../src/gles_present.rs");
    assert!(
        gles.contains("Reader = 18"),
        "阅读页 GLES 槽号必须 18（前 18 槽各有其主）"
    );
    assert!(
        gles.contains("layers: [ChromeLayer; 19]"),
        "槽层数组必须 19 件（18 旧槽 + Reader）"
    );
    assert!(
        gles.contains("z_order: [crate::ai_presence::Panel; 6]"),
        "z_order 必须六公民（阅读页入垫底序）"
    );
    let gate = include_str!("../src/gate.rs");
    assert!(
        gate.contains("t.paint_reader_content("),
        "值守倒帧（na-shot 判卷那条路）不画阅读页 = 判卷截图一片空白"
    );
    let tv = include_str!("../src/termview.rs");
    assert!(
        tv.contains("pub fn paint_reader_page_chrome("),
        "页框涂装（顶栏文件名/返回钮/底缘线/进度线）必须在"
    );
    assert!(
        tv.contains("fn paint_reader_content_impl("),
        "正文涂装实现必须在（md 管线喂 paint_md_body）"
    );
    assert!(
        tv.contains("pub fn rd_return_hit("),
        "返回钮命中几何必须与涂装同源一份"
    );
    assert!(
        tv.contains("pub fn reader_geom("),
        "阅读页整页几何必须同源一份（命中与涂装同吃）"
    );
    assert!(
        tv.contains("pub fn rd_split("),
        "阅读页裂口几何必须在（面板页推移吃它）"
    );
}

#[test]
fn spec_bar170_壳手势与帧泵接线() {
    let app = include_str!("../src/android_app.rs");
    // 起手：阅读页在顶且缝采样归零 → 整页建滚动接力件
    assert!(
        app.contains("self.reader_scroll = Some(crate::scroll::TouchScroll::new("),
        "阅读页起手必须建 TouchScroll 接力件"
    );
    // 抬手两分流：点按返回钮 → 出栈；拖过 → 交接甩尾
    assert!(
        app.contains("crate::termview::rd_return_hit(&g, rt.0 as i64, rt.1 as i64)"),
        "返回钮点按必须吃同源命中几何"
    );
    assert!(
        app.contains("dismiss_top(crate::ai_presence::Panel::Reader)"),
        "返回钮/推回必须出栈阅读页"
    );
    // 帧泵：poll_reader 每圈一问 tick_restore + need_prefetch
    assert!(
        app.contains("fn poll_reader(&mut self)"),
        "阅读页帧泵必须在"
    );
    assert!(
        app.contains("pg.tick_restore(total_h, view_h)"),
        "帧泵必须走恢复值落地（内容长到包得住才落）"
    );
    assert!(
        app.contains("pg.need_prefetch(total_h, view_h)"),
        "帧泵必须走预取判（滚近内容尾拉下一块）"
    );
    assert!(
        app.contains("fn reader_md_layout(&self, text: &str, cw: u32)"),
        "md 排版帮手必须在（锁序：克隆出锁后排版，不嵌锁）"
    );
    // 拖拽推回：GLES + 软路两臂映射 DismissReader
    let pd = include_str!("../src/ui/panel_drag.rs");
    assert!(
        pd.contains("DismissReader") && pd.contains("DragTop::Reader"),
        "panel_drag 必须有阅读页推回两臂"
    );
    let seam = include_str!("../src/ui/seam.rs");
    assert!(
        seam.contains("pub fn occupy_reader_panel_offset_x(")
            && seam.contains("pub fn replay_reader_panel_offset_x("),
        "第七道缝整族必须在"
    );
}

#[test]
fn spec_bar170_对象轴文件席与文档() {
    let ep = include_str!("../src/endpoint.rs");
    assert!(
        ep.contains("kind: EndpointKind::File,"),
        "对象轴必须空注册文件席（阅读页挂对象轴席位）"
    );
    assert!(
        ep.contains("EndpointKind::File => ExecPlan::NoServer"),
        "文件席无 exec 通道（数据面走 fs_fetch 分块拉取）"
    );
    // 数据面：fs_fetch 分块拉取 + 防迟到块喂错文件
    let fetch = include_str!("../src/fs_fetch.rs");
    assert!(
        fetch.contains("pub fn request_read_chunk("),
        "分块拉取入口必须在"
    );
    assert!(
        fetch.contains("read_range_json_in") || fetch.contains("offset"),
        "服务端必须支持 offset 分块（na-protocol read_range_json_in）"
    );
    // 注册表与文档地图
    let md = include_str!("../docs/active/阅读页.md");
    assert!(
        md.contains("BAR-170") && md.contains("CHUNK_MAX"),
        "契约文档缺编号/分块参数"
    );
    let agents = include_str!("../AGENTS.md");
    assert!(
        agents.contains("docs/active/阅读页.md"),
        "AGENTS 文档地图未登记阅读页"
    );
}
