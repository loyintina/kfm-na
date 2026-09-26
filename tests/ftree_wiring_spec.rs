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
//!
//! 变异方向：三盒命中不吃垂直留白 / × 盒不吃右缘镜像 / sig 退回六维 /
//! 活性探针摘文件树那路——本册必红。

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
    // 或项」的变异逃逸（本册变异 2 实咬过这一口）
    assert!(
        fx.contains("crate::ui::filetree::anim_active") && fx.contains("|| ft_anim;"),
        "文件树页内动画必须进 fx_frame_due 活性表（定义与接线两件都要在）"
    );
    // 手势三臂
    assert!(
        app.contains("ft_touch = Some((x, y, false, y))"),
        "起手建槽（含上次 y）"
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
        app.contains("let x_local = ft.0 as i64 - g.x0;"),
        "命中 x 必须折成列表相对（传屏幕 x 就差一个内容左缘——实拍逮到过：三角点不中）"
    );
    assert!(
        app.contains("want_toggle && row.kind.is_dir()"),
        "三角带=开合 / 名字区=选中 的切分（原版截屏实拍：选中行是目录且未展开）"
    );
    assert!(
        app.contains("state.lock().unwrap().scroll_by(-(inc as i64), view_h)"),
        "行表滚动吃增量、上界吃当下可视窗"
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
