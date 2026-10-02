//! reader_wiring_spec.rs — 阅读页**接线**考题（BAR-170，2026-09-27）
//!
//! 分工说明：纯逻辑核的考卷在 `tests/reader_page_spec.rs`（状态核 A 档
//! 带变异）；缝的占槽/直通/重播踢在 `tests/fx_ease_spec.rs` 末（第六道
//! 缝入账钉；2026-09-30 BAR-207 Demo 缝退役顺位）；终端钮几何钉在
//! `tests/term_btn_spec.rs`。
//! 本册管**涂装与壳核之间的机械接线**——漏一处 = 点文件没反应 /
//! 阅读页零帧 / 终端钮点不中 / 甩尾不取消，实拍前就能钉死的缺口类错误
//! （同 ftree_wiring_spec 的立法）。
//!
//! 2026-10-01 BAR-208 改版：返回钮退役（退出归终端钮），排版走缓存
//! （text_epoch 钉在 reader_page_spec，缓存钉在 md_layout_spec），
//! 拖拽推回臂死透反向钉在壳手势册。
//!
//! 变异方向：改道点缺 summon（阅读页不进场）/ 缺 dismiss FileTree（树
//! 不关压在页上）/ 活性链摘 reader 那路（甩尾零帧）/ gles 槽号错（画到
//! 别人的槽）/ 链尾字面量被吞（viewer/ft 两路被挤掉）/ 排版绕开缓存
//! （每帧全文重排回潮）/ 拖拽推回臂复活（退出路径双源）——本册必红。

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
        gles.contains("Reader = 17"),
        "阅读页 GLES 槽号必须 17（BAR-207 Demo 槽退役顺位，前 17 槽各有其主）"
    );
    assert!(
        gles.contains("layers: [ChromeLayer; 19]"),
        "槽层数组必须 19 件（17 旧槽 + Reader + Mail 信箱页槽 BAR-214 末尾入列）"
    );
    // 构造器槽数钉（BAR-207 cfg 盲区实录：类型改后构造器字面量
    // 漏删一件，宿主 check 全绿、android check 才咬——mk_layer 调用
    // 计数 = 槽数的机械对表，变异：删/加一件即红）
    assert_eq!(
        gles.matches("mk_layer(&gl),").count(),
        19,
        "构造器 mk_layer 必须 19 件与槽层数组同数（BAR-207 cfg 盲区钉，BAR-214 升 19）"
    );
    assert!(
        gles.contains("z_order: [crate::ai_presence::Panel; 6]"),
        "z_order 必须六公民（BAR-207 Demo 退役，BAR-214 信箱页入列）"
    );
    assert!(
        gles.contains("Mail = 18"),
        "信箱页 GLES 槽号必须 18（BAR-214 末尾入列，前 18 槽各有其主）"
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
    // BAR-208：正文排版走排版缓存（同代同宽零重排——滚动卡帧根修）；
    // 返回钮已退役（退出归终端钮 ui/term_btn.rs），命中几何随葬
    assert!(
        tv.contains("layout_md_cached("),
        "正文涂装必须走排版缓存（每帧全文重排 = BAR-208 病灶回潮）"
    );
    assert!(
        !tv.contains("rd_return_hit"),
        "返回钮命中几何必须随葬（BAR-208 退出归终端钮）"
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
    // 横向占优让回（redroid 判卷咬出：无让回 = 事件全被滚动槽吞，
    // 右缘推回永远够不到面板拖拽）——文件树同款让回律
    assert!(
        app.contains("阅读页手势让回面板页"),
        "横向占优必须整槽让回面板页（推回/关页才轮得到面板拖拽）"
    );
    // 抬手两分流（BAR-208）：终端钮点按 → 出栈回终端；拖过 → 交接甩尾。
    // 起手仲裁钮分流先于正文槽：终端钮 > 设置钮 > 滚动接力件
    assert!(
        app.contains("crate::ui::term_btn::hit(x, y, sw)"),
        "阅读页仲裁必须先分流终端钮命中（BAR-208 唯一退出路径）"
    );
    assert!(
        app.contains("self.term_btn_touch = Some((id, x, y, false))"),
        "终端钮命中必须建槽（点按抬手才触发，拖过 slop 不触发）"
    );
    assert!(
        app.contains("终端钮点按: 栈顶"),
        "终端钮点按必须留栈痕（栈操作日志可见条款）"
    );
    assert!(
        app.contains("dismiss_top(crate::ai_presence::Panel::Reader)"),
        "终端钮/推回必须出栈阅读页"
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
    // BAR-208：排版帮手换芯——缓存 peek 命中零克隆，未中才排版入柜
    // （锁序红线 term→reader 不倒持）
    assert!(
        app.contains("fn reader_layout_arc("),
        "md 排版帮手必须走缓存版（reader_layout_arc）"
    );
    assert!(
        app.contains("layout_md_peek("),
        "帧泵/拖拽必须走 peek 零克隆快路（每帧重排 = 病灶回潮）"
    );
    // BAR-208：拖拽推回臂退役（阅读页退出归终端钮，不占拖拽滑槽）——
    // 反向钉：两变体必须死透，复活 = 滑槽语义双源
    let pd = include_str!("../src/ui/panel_drag.rs");
    assert!(
        !pd.contains("DismissReader") && !pd.contains("DragTop::Reader"),
        "panel_drag 阅读页推回两臂必须死透（BAR-208 退出归终端钮）"
    );
    let seam = include_str!("../src/ui/seam.rs");
    assert!(
        seam.contains("pub fn occupy_reader_panel_offset_x(")
            && seam.contains("pub fn replay_reader_panel_offset_x("),
        "第六道缝整族必须在（BAR-207 Demo 缝退役顺位）"
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

#[test]
fn spec_bar220_阅读信箱两缝入帧时钟与帧账() {
    // BAR-220（2026-10-02 承影真机「抬手即卡」定罪）：阅读/信箱两缝落地
    // 时漏接 fx_frame_due 活性表（收场动画零泵 = 抬手即卡），信箱缝还漏
    // 接 note_anim_frame 帧账表（panel-anim 收场轮无账可查）。行为钉在
    // fx_spring_spec「spec_bar220_帧时钟_*缝活跃也产帧」；本钉守接线本
    // 体——摘臂/摘账变异必红。
    let fx = include_str!("../src/ui/fx_spring.rs");
    assert!(
        fx.contains("|| crate::ui::seam::reader_panel_offset_x_active()"),
        "阅读缝活性必须进 fx_frame_due 的 active 表（摘掉 = 收场动画零泵）"
    );
    assert!(
        fx.contains("|| crate::ui::seam::mail_panel_offset_x_active()"),
        "信箱缝活性必须进 fx_frame_due 的 active 表（摘掉 = 收场动画零泵）"
    );
    let app = include_str!("../src/android_app.rs");
    assert!(
        app.contains("|| crate::ui::seam::mail_panel_offset_x_active()"),
        "信箱缝活性必须进 note_anim_frame 帧账表（摘掉 = 收场轮 panel-anim 无账）"
    );
}
