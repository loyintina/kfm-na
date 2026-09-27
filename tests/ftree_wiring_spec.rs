//! ftree_wiring_spec.rs — 文件树页**接线**考题（BAR-165，2026-09-26）
//!
//! 分工说明：纯逻辑核的考卷在 `tests/filetree_spec.rs`（52 钉）、涂装的
//! 落位/不炸在同文件 `src/termview.rs` 的 `mod ft_paint_smoke`。本册管
//! **两件涂装与壳核之间的事**：
//! ① 底栏三盒命中（纯几何，与涂装同源一份——眼手同尺的物质保证）；
//! ② 接线源码守卫——三处涂装入口、脏帧 sig 十维、活性探针第六路、手势
//!    三臂、取数编码、查看器补画。这些是「漏一处 = 判卷空白/动画零帧/
//!    点文件没反应」的机械钉（B 档胶水的判卷人本来是实拍，但缺口类错误
//!    实拍前就能钉死，别等上机）。
//! 打回两症（2026-09-27）：症 5 交互口径（目录行整行 = 开合）与症 6 行表
//! 滚动接惯性甩尾（现役复用件 `src/scroll.rs`：TouchScroll + Fling）——
//! 后者的接线守卫 + 数值行为两钉在本册末（源码守卫与纯逻辑两条腿）。
//!
//! 变异方向：三盒命中不吃垂直留白 / × 盒不吃右缘镜像 / sig 退回六维 /
//! 活性探针摘文件树那路 / 行表甩尾：摘交接、起手不取消、燃尽不清空、
//! 活性摘甩尾那路、拖动丢计时采样（`moved_px_at` → `moved_px`）——本册必红。

use kfm_na::termview::{FtBarHit, ft_bar_hit, ft_geom};

/// 真机几何锚点（1260×2800，无键盘）：43/1223/55/2781/2671
fn geom_1260x2800() -> kfm_na::termview::FtGeom {
    ft_geom(1260, 2800, 0)
}

#[test]
fn spec_bar165_底栏三盒命中() {
    let g = geom_1260x2800();
    assert_eq!(
        (g.x0, g.x1, g.list_y0, g.bar_y0, g.bar_y1),
        (43, 1223, 55, 2671, 2781)
    );
    // 三盒高 66、上下各留 22（盒带 = [2693, 2759)）
    let y = g.bar_y0 + 55;
    assert_eq!(ft_bar_hit(&g, g.x0 + 1, y), Some(FtBarHit::Eye));
    assert_eq!(ft_bar_hit(&g, g.x0 + 125 / 2, y), Some(FtBarHit::Eye));
    // × 盒贴右缘（镜像），根名盒居中于「眼右 + 8」到「×左 − 8」之间
    assert_eq!(ft_bar_hit(&g, g.x1 - 1, y), Some(FtBarHit::Close));
    assert_eq!(ft_bar_hit(&g, g.x1 - 130, y), Some(FtBarHit::Close));
    let mid = (g.x0 + 125 + 8 + (g.x1 - 130 - 8)) / 2;
    assert_eq!(ft_bar_hit(&g, mid, y), Some(FtBarHit::Root));
    // 盒与盒之间的缝是空的（不是「整条栏都算命中」）
    assert_eq!(ft_bar_hit(&g, g.x0 + 125 + 2, y), None, "眼盒右缝");
    assert_eq!(ft_bar_hit(&g, g.x1 - 130 - 2, y), None, "×盒左缝");
    // 垂直：栏内但在盒带之外（上下留白）不命中
    assert_eq!(ft_bar_hit(&g, g.x0 + 10, g.bar_y0), None, "栏顶留白");
    assert_eq!(ft_bar_hit(&g, g.x0 + 10, g.bar_y1 - 1), None, "栏底留白");
    // 栏外/行表区不命中
    assert_eq!(ft_bar_hit(&g, g.x0 + 10, g.bar_y0 - 1), None);
    assert_eq!(ft_bar_hit(&g, g.x0 + 10, g.list_y0 + 10), None);
    // 窄屏：三盒不许互相压（余量收窄后仍各归其位）
    let n = ft_geom(700, 2000, 0);
    assert!(n.x1 > n.x0);
    let ny = n.bar_y0 + (n.bar_y1 - n.bar_y0) / 2;
    assert_eq!(ft_bar_hit(&n, n.x0 + 2, ny), Some(FtBarHit::Eye));
    assert_eq!(ft_bar_hit(&n, n.x1 - 2, ny), Some(FtBarHit::Close));
}

#[test]
fn spec_bar165_三处涂装与接线源码守卫() {
    let app = include_str!("../src/android_app.rs");
    // 三处涂装：GLES 槽烘焙 + 软渲染兜底在同一个文件里各一次
    assert_eq!(
        app.matches(".paint_ft_content(").count(),
        2,
        "GLES 槽烘焙 + 软渲染兜底两条路必须各画一次内容"
    );
    assert!(
        app.contains("crate::ui::filetree::note_baked_snap"),
        "烘焙后必须落屏代快照（命中吃账不吃活体，BAR-145 判例）"
    );
    let gate = include_str!("../src/gate.rs");
    assert!(
        gate.contains("t.paint_ft_content("),
        "值守倒帧（na-shot 判卷那条路）不画内容 = 判卷截图一片空白"
    );
    // 脏帧 sig 十维：漏维 = 确定的鬼影
    assert!(
        app.contains("DirtyGuard<(u32, u32, u32, u32, u32, u32, u32, u32, u32, u32)>"),
        "文件树槽 sig 必须含帧账四维（代/滚动/选中/动画簇）"
    );
    assert!(
        app.contains("let ft_frame = crate::ui::filetree::frame("),
        "涂装与 sig 必须同吃一份帧账（各算一遍 = 尺不同）"
    );
    // 活性探针第六路（页停住而内容在动时若不入表 = 动画零帧）
    let fx = include_str!("../src/ui/fx_spring.rs");
    // 判据要吃「定义 + 接进 active 表」两件：只钉定义会被「留着定义摘掉
    // 或项」的变异逃逸（本册变异 2 实咬过这一口）。症 6 起甩尾那路接在它
    // 后面，本项不再是表尾——只钉项不钉分号（钉分号会被「后面接新路」
    // 误伤，钉项照样咬得住「摘掉这一路」）
    assert!(
        fx.contains("crate::ui::filetree::anim_active") && fx.contains("|| ft_anim"),
        "文件树页内动画必须进 fx_frame_due 活性表（定义与接线两件都要在）"
    );
    // 手势三臂（症 6 改装后：起手建接力件、拖动吃件、抬手两分流）
    assert!(
        app.contains("self.ft_touch = Some(FtTouch {") && app.contains("start_x: x,"),
        "起手建手势槽（接力件在位）"
    );
    assert!(
        app.contains("文件树行表手势让回面板页"),
        "横向占优整槽让回面板页"
    );
    assert!(
        app.contains("FileTreeState::hit_snap"),
        "抬手命中吃屏代快照"
    );
    assert!(
        app.contains("let x_local = ft.start_x as i64 - g.x0;"),
        "命中 x 必须折成列表相对（状态核 x 尺 = 行左缘起算）"
    );
    assert!(
        app.contains("want_toggle && row.kind.is_dir()"),
        "目录行整行=开合 / 文件行=选中的切分（2026-09-27 用户裁决回原版语义：\
         真机 logcat 20+ 次点按零 toggle = 用户点目录行没反应）"
    );
    assert!(
        app.contains("state.lock().unwrap().scroll_by(-(d as i64), view_h)"),
        "行表滚动吃接力件像素位移、上界吃当下可视窗"
    );
    // 取数：召唤沿拉根层 + 展开拉子层；查询值必须编码
    assert!(
        app.contains("crate::fs_fetch::request_root()"),
        "召唤沿必须拉根层"
    );
    assert!(
        app.contains("crate::fs_fetch::request_list(path)"),
        "展开必须拉子层"
    );
    let fetch = include_str!("../src/fs_fetch.rs");
    assert!(
        fetch.contains("fsapi::pct_encode"),
        "查询值必须百分号编码（中文目录名不编码 = 服务端拆错参数）"
    );
    assert!(
        fetch.contains("crate::endpoint::EndpointKind::Local"),
        "本地相降级判在客户端（服务端保持相无关）"
    );
    // 查看器补画（配置槽在文件树页在顶时不烘焙，漏了 = 点文件没反应）
    let tv = include_str!("../src/termview.rs");
    assert!(
        tv.contains("viewer_snap_global"),
        "文件树槽必须补画 BAR-163 查看器"
    );
    // 契约文档与注册表
    let md = include_str!("../docs/active/文件树.md");
    assert!(
        md.contains("FT_CSS") && md.contains("/api/fs/list"),
        "契约文档缺参数/端点"
    );
    let reg = include_str!("../src/ui/registry.md");
    assert!(reg.contains("filetree 文件树页"), "控件未入册");
    let agents = include_str!("../AGENTS.md");
    assert!(
        agents.contains("docs/active/文件树.md"),
        "AGENTS 文档地图未登记"
    );
}

/// BAR-165 症 6（2026-09-27 用户拍板「滚动接惯性甩尾；废弃手写增量跟手」）：
/// 文件树行表滚动接现役复用件（`src/scroll.rs` TouchScroll + Fling），
/// 壳里不再有手写「上次 y」增量。源码守卫四件 + 活性探针一路。
#[test]
fn spec_bar165_症6_行表滚动接惯性甩尾接线守卫() {
    let app = include_str!("../src/android_app.rs");
    // ① 起手：建接力件（slop 门/速度采样在件里）+ 新触摸落地即取消在飞甩尾
    assert!(
        app.contains("crate::scroll::TouchScroll::new(")
            && app.contains("crate::ui::filetree::ROW_H as f64"),
        "行表起手必须建接力件，行高喂行表 ROW_H（不是终端 cell_h）"
    );
    //（`ft_fling_last_ms` 只在起手取消那一处写 None——燃尽离场那只清 fling，
    // 故这一串是「起手取消」的专属指纹）
    assert!(
        app.contains("self.ft_fling_last_ms = None;"),
        "新触摸落地必须取消在飞甩尾（comp_registry「惯性甩尾」条契约）"
    );
    // ② 拖动期吃接力件的像素位移（速度采样的唯一入口）
    assert!(
        app.contains("ft.scroll.moved_px_at(y, crate::report::boot_ms() as f64)"),
        "拖动期必须吃 moved_px_at（自带 slop 门 + 甩尾速度采样）"
    );
    assert!(
        !app.contains("let inc = y - ft.3"),
        "手写增量跟手必须废（症 6 用户原话「废弃手写增量跟手」）"
    );
    // ③ 抬手交接甩尾
    assert!(
        app.contains("ft.scroll.fling_on_release()"),
        "拖动过抬手必须交接甩尾（fling_on_release）"
    );
    assert!(
        app.contains("if phase == TouchPhase::Ended && !ft.scroll.was_tap()"),
        "抬手两分流：was_tap 假 = 拖动过 → 交接甩尾"
    );
    // ④ 帧泵推进 + 燃尽清空（三处清点：起手取消 / 帧泵燃尽 / 首帧账）
    assert!(
        app.contains("self.ft_fling.as_mut().and_then(|f| f.step(dt as f64))"),
        "帧泵必须逐圈推进 Fling::step（真实间隔折帧）"
    );
    assert!(
        app.contains("文件树甩尾尽: {why}"),
        "燃尽必须报账（速度燃尽/触底/触史顶）"
    );
    assert!(
        app.matches("self.ft_fling = None;").count() >= 2,
        "清点两处起步：起手落地取消 + 帧泵燃尽离场（少一处 = 甩尾不取消/空转脏帧）"
    );
    assert!(
        app.contains("if after - before != req {"),
        "本笔位移被钳（实际滚动量 ≠ 请求量 = 触底/触史顶）必须燃尽"
    );
    // ⑤ 活性探针第七路（页停住而甩尾在飞时若不入表 = 甩尾零帧）
    let fx = include_str!("../src/ui/fx_spring.rs");
    assert!(
        fx.contains("pub fn ft_fling_live()") && fx.contains("let ft_fling = ft_fling_live();"),
        "甩尾活性旗必须在（定义一项）"
    );
    assert!(
        fx.contains("|| ft_fling;"),
        "甩尾活性必须接进 fx_frame_due 的 active 表（摘掉 = 甩尾零帧）"
    );
    assert!(
        app.contains("crate::ui::fx_spring::note_ft_fling_live(self.ft_fling.is_some())"),
        "壳每圈帧泵末尾必须同步活性旗"
    );
    // 活性旗的真行为（不是 grep）：置位读得回、清位读得回、默认假
    assert!(
        !kfm_na::ui::fx_spring::ft_fling_live(),
        "默认该是假（无甩尾不产帧）"
    );
    kfm_na::ui::fx_spring::note_ft_fling_live(true);
    assert!(kfm_na::ui::fx_spring::ft_fling_live());
    kfm_na::ui::fx_spring::note_ft_fling_live(false);
    assert!(!kfm_na::ui::fx_spring::ft_fling_live());
}

/// 同一症的**纯逻辑**腿（不靠源码 grep）：甩尾三件数值行为——
/// 初速 = 拖动期采样末速、位移单调递减到燃尽、停滞松手不甩。
/// 复用件出处 = `src/scroll.rs` 惯性甩尾段（kfmv4 canvas-scroll 直译）
#[test]
fn spec_bar165_症6_甩尾数值行为() {
    use kfm_na::scroll::{
        DRAG_GAIN, FLING_BOOST, FLING_FRAME_MS, FLING_STOP_MIN, Fling, TouchScroll,
    };
    // ① 一次快拖后抬手：初速 = 采样速（件自己算，壳不掺和）
    let mut t = TouchScroll::new(500.0, kfm_na::ui::filetree::ROW_H as f64);
    t.moved_px_at(560.0, 1000.0); // 越阈笔（dt 无账不采样）
    t.moved_px_at(600.0, 1000.0 + FLING_FRAME_MS); // 一笔 40 手指 px / 一帧
    let f = t.fling_on_release().expect("快拖抬手必须出甩尾");
    let expect = 40.0 * DRAG_GAIN * FLING_BOOST; // 位移 40×增益2 / 一帧 × 1.7
    assert!(
        (f.velocity() - expect).abs() < 1e-6,
        "初速该是采样速 {expect}，实际 {}",
        f.velocity()
    );
    // ② 逐帧位移单调递减、尾巴不吞、到燃尽阈停下（None 出场）
    let mut f = Fling::new(60.0);
    let mut prev = f64::INFINITY;
    let mut steps = 0;
    while let Some(d) = f.step(FLING_FRAME_MS) {
        assert!(
            d > 0.0 && d < prev,
            "第 {steps} 步位移 {d} 不递减（prev={prev}）"
        );
        prev = d;
        steps += 1;
    }
    assert!(steps > 5, "60px/帧起步该走十帧以上，实际 {steps}");
    assert!(
        f.velocity().abs() < FLING_STOP_MIN,
        "燃尽后残速 {} 该低于停止阈 {FLING_STOP_MIN}",
        f.velocity()
    );
    // ③ 停住再松手不甩（速度采样跟着时间戳走，按住不动 = 杀速）
    let mut u = TouchScroll::new(500.0, kfm_na::ui::filetree::ROW_H as f64);
    u.moved_px_at(560.0, 1000.0);
    u.moved_px_at(600.0, 1000.0 + FLING_FRAME_MS); // 快甩一笔
    u.moved_px_at(600.0, 1300.0); // 按住不动 ≈283ms
    assert!(
        u.fling_on_release().is_none(),
        "停住了才松手不许甩（kfmv4 同款）"
    );
}
