//! tests/viewer_fling_wiring_spec.rs — BAR-167 ① 查看器滚动接线守卫
//! （源码钉，仿 ftree_wiring_spec 症 6 同款）：android_app 是
//! `#[cfg(target_os = "android")]` 宿主编不到，壳接线断了宿主测试
//! 全绿也照烂——故用 include_str! 把关键接线字面量钉死。惯性甩尾 =
//! scroll.rs Fling 同一物理机（用户工单原话「不许手写增量」），涂装
//! 裁剪/滚动几何钉在 termview_spec/modal_spec，本册只管壳接线。

/// 壳源（android-only，include_str 读字面）
const APP: &str = include_str!("../src/android_app.rs");
const FX: &str = include_str!("../src/ui/fx_spring.rs");

#[test]
fn spec_bar167_viewer_滚动接线守卫() {
    // ① 滚动状态住核心（眼手同尺单源）：涂装/命中/壳手势同读
    // cfg_page viewer 态的 scroll 维，壳不许另起第二份账
    assert!(
        APP.contains("pg.scroll_viewer_by("),
        "壳滚动必须走 cfg_page::scroll_viewer_by（眼手同尺单源）"
    );
    // ② 接力件建机（两处起手路由：配置页 / 文件树页同族仲裁）
    assert!(
        APP.matches("self.modal_scroll = Some(crate::scroll::TouchScroll::new(")
            .count()
            >= 2,
        "两处跳框起手路由都必须建滚动接力件（配置页/文件树页）"
    );
    // ③ 拖动逐事件喂 moved_px_at（惯性速度采样唯一入口——喂无时钟的
    // moved_px = 抬手速度恒 0，甩尾哑掉）
    assert!(
        APP.contains("ms.moved_px_at(y, crate::report::boot_ms() as f64)"),
        "拖动必须喂 moved_px_at（速度采样唯一入口）"
    );
    // ④ 抬手交接甩尾 = scroll.rs Fling 物理机（用户工单「不许手写增量」）
    assert!(
        APP.contains(".and_then(|ms| ms.fling_on_release())"),
        "抬手必须从接力件交接 fling_on_release（Fling 物理机）"
    );
    // ⑤ 帧泵推进（真实间隔折帧）+ 燃尽报账 + 清点（起手取消/燃尽离场）
    assert!(
        APP.contains("self.viewer_fling.as_mut().and_then(|f| f.step(dt as f64))"),
        "帧泵必须逐圈推进 Fling::step"
    );
    assert!(
        APP.contains("查看器甩尾尽: {why}"),
        "燃尽必须报账（速度燃尽/触底/顶/车道翻牌）"
    );
    assert!(
        APP.matches("self.viewer_fling = None;").count() >= 2,
        "清点两处：新触摸落地取消 + 帧泵燃尽离场"
    );
    // ⑥ 滚动上限按实时屏尺寸现算现喂（2026-09-27 BAR-169 换芯：md
    // 排版 total_h 直喂版 viewer_scroll_max_h——旧字段版随 md 渲染器
    // 退役，涂装裁剪/滚动上限/钳制同尺纪律不变）
    assert!(
        APP.matches("md::viewer_scroll_max_h(sw, sh, lay.total_h)")
            .count()
            >= 2,
        "拖动臂与甩尾泵都必须现算 viewer_scroll_max_h（同一份几何）"
    );
    // ⑦ 活性探针第八路（页停住甩尾在飞 = 不入表即甩尾零帧）
    assert!(
        FX.contains("pub fn viewer_fling_live()")
            && FX.contains("let viewer_fling = viewer_fling_live();"),
        "查看器甩尾活性旗必须在"
    );
    assert!(
        FX.contains("|| viewer_fling"),
        "查看器甩尾活性必须接进 fx_frame_due 的 active 表"
    );
    assert!(
        APP.contains(
            "crate::ui::fx_spring::note_viewer_fling_live(\n                self.viewer_fling.is_some() || self.mail_fling.is_some(),"
        ),
        "壳每圈帧泵末尾必须同步活性旗（BAR-212：查看器与信件列表卡甩尾共用一路，OR 同旗）"
    );
    // 活性旗的真行为（不是 grep）：默认假、置位读得回、清位读得回
    use kfm_na::ui::fx_spring::{note_viewer_fling_live, viewer_fling_live};
    assert!(!viewer_fling_live(), "默认该是假（无甩尾不产帧）");
    note_viewer_fling_live(true);
    assert!(viewer_fling_live());
    note_viewer_fling_live(false);
    assert!(!viewer_fling_live());
}

// ---- BAR-169 md 渲染器接线守卫（源码钉，同律：壳断了宿主全绿也照烂）----

const TV: &str = include_str!("../src/termview.rs");

#[test]
fn spec_bar169_md_渲染接线守卫() {
    // ① 查看器涂装换芯：paint_viewer_card 走 md 排版（layout_md）+
    // 实例格两维（BAR-204：md_style 全局口废除，pinch 实例格一统）+
    // 内容高直喂卡几何
    assert!(
        TV.contains("layout_md(&v.content, md::viewer_content_w(w)"),
        "查看器涂装必须吃 md 排版（layout_md + viewer_content_w 同尺）"
    );
    let call_at = TV
        .find("layout_md(&v.content, md::viewer_content_w(w)")
        .expect("查看器 layout_md 调用点必须在");
    assert!(
        TV[call_at..call_at + 200].contains("self.cell_size()"),
        "查看器排版必须吃实例格两维（pinch 联动；全局样式口已废）"
    );
    assert!(
        TV.contains("md::viewer_card_rect_h(w, h, lay.total_h)"),
        "卡高必须吃 md 排版 total_h（内容高直喂版）"
    );
    // ② 壳三处接线全走内容高直喂版（拖动/甩尾滚 max + 抬手命中卡几何）
    assert!(
        APP.matches("viewer_scroll_max_h(sw, sh, lay.total_h)")
            .count()
            >= 2,
        "拖动与甩尾两臂的滚动上限都必须走 md total_h"
    );
    assert!(
        APP.contains("md::viewer_card_rect_h(sw, sh, lay.total_h)"),
        "抬手命中卡几何必须走 md total_h"
    );
    // ③ 旧字段版不再被壳调用（残留 = 两份几何账分叉）
    assert!(
        !APP.contains("md::viewer_fields("),
        "壳不许再调旧字段版 viewer_fields（分叉账）"
    );
    // ④ VeilSig 补维：滚动 + 实例格两维（BAR-204 换约：md_style 全局态
    // 废除——pinch 变格 = 版面变必须重烘；漏维 = 鬼影同律）
    assert!(
        APP.contains("viewer_scroll: cs.viewer.as_ref().map_or(0, |v| v.scroll)"),
        "VeilSig 必须含 scroll 维（GLES 烘焙滚动重烘）"
    );
    assert!(
        APP.contains("cell: (u32, u32)"),
        "VeilSig 必须含实例格两维（BAR-204 换约）"
    );
    assert!(
        !APP.contains("md_style"),
        "md_style 全局口已废（BAR-204）——VeilSig/壳全册零残留"
    );
    // ⑤ 量宽同尺：壳排版走 viewer_md_layout（格步进尺 = 涂装同一把
    // char_cells×实例格宽），三处接线同读这一个 helper
    assert!(
        APP.matches("self.viewer_md_layout(").count() >= 3,
        "三处接线必须同读 viewer_md_layout（量宽单源）"
    );
    // ⑥ 渲染设置卡整族废除（BAR-204 a 案：pinch 实例格一统——render.json
    // 写盘/全局样式口灌/档位换选全退役，壳册零残留钉）
    assert!(
        !APP.contains("render_to_json") && !APP.contains("render_cfg"),
        "渲染设置卡已废（render.json 写盘链零残留）"
    );
    assert!(
        !APP.contains("set_md_style"),
        "全局样式口已废（set_md_style 零残留）"
    );
}

// ---- BAR-174 信件本地缓存接线守卫（源码钉，同律：壳断了宿主全绿也照烂）----

const SESS: &str = include_str!("../src/sess_pool.rs");

#[test]
fn spec_bar174_缓存接线守卫() {
    // ① 壳必须喂缓存根（configure 旁同款静态配置）：没喂 = 缓存层静默
    // 关闭，信号差首屏干等
    assert!(
        APP.contains("crate::sess_pool::set_cache_root("),
        "壳必须调 sess_pool::set_cache_root（信件本地缓存根）"
    );
    assert!(
        APP.contains(r#".join("cache/letters")"#),
        "缓存根必须落 <私有目录>/cache/letters"
    );
    // ② request_entries 信箱分支必须先灌缓存（stale 行）再起后台线程——
    // 顺序反了 = 首屏即时承诺失效
    let req = SESS
        .find("pub fn request_entries")
        .expect("request_entries");
    let stale = SESS
        .find("stale_letter_entries(&cached)")
        .expect("先灌缓存调用");
    let spawn = SESS[req..]
        .find("std::thread::spawn(move || {")
        .map(|i| req + i)
        .expect("request_entries 后台线程");
    assert!(
        req < stale && stale < spawn,
        "request_entries 必须先灌缓存行再 spawn 后台 GET（首屏即时）"
    );
    // ③ request_content 失败必须回退缓存副本（引用块头声明），缓存也没有
    // 才落「（取数失败：…）」
    assert!(
        SESS.contains("> （本地缓存副本，联网后自动刷新）\\n\\n"),
        "request_content GET 失败必须回退缓存副本并带引用块头"
    );
    assert!(
        SESS.contains("read_body(&root.join(inbox), &name)"),
        "request_content 失败回退必须读缓存正文文件"
    );
}

// ---- BAR-187 文件树本地缓存接线守卫（源码钉，模板 = spec_bar174_缓存接线守卫）----

const FS_FETCH: &str = include_str!("../src/fs_fetch.rs");

#[test]
fn spec_bar187_缓存接线守卫() {
    // ① 壳必须喂缓存根（BAR-174 同位同律）：没喂 = 缓存层静默关闭
    assert!(
        APP.contains("crate::fs_fetch::set_cache_root("),
        "壳必须调 fs_fetch::set_cache_root（文件树本地缓存根）"
    );
    assert!(
        APP.contains(r#".join("cache/fs")"#),
        "缓存根必须落 <私有目录>/cache/fs"
    );
    // ② list 必须先灌缓存再 spawn 后台 GET（顺序反了 = 弱网秒开承诺失效）
    let req = FS_FETCH.find("pub fn request_list(").expect("request_list");
    let serve = FS_FETCH
        .find("serve_list_cache(&dir, false)")
        .expect("缓存先灌调用");
    let spawn = FS_FETCH[req..]
        .find("spawn_list(dir, false, had_cache)")
        .map(|i| req + i)
        .expect("request_list 起 spawn_list");
    assert!(
        req < serve && serve < spawn,
        "request_list 必须先 serve_list_cache 再 spawn_list"
    );
    // ③ 失败决策必过 list_fail_goes_old 闸（有缓存不走 fail）
    assert!(
        FS_FETCH.contains("if list_fail_goes_old(had_cache)"),
        "list 失败必须过 list_fail_goes_old 闸"
    );
    // ④ 阅读页缓存先画 + 换芯三件套（核方法全被取数面消费）
    assert!(
        FS_FETCH.contains("serve_read_cache(&path)"),
        "首块必须先 serve_read_cache"
    );
    assert!(
        FS_FETCH.contains("st.apply_refresh0(next, trunc, text)"),
        "缓存先画后 refresh 首块必须走 apply_refresh0 换芯"
    );
    assert!(
        FS_FETCH.contains("st.refresh_failed()"),
        "缓存先画后 refresh 失败必须 refresh_failed 摘账"
    );
    // ⑤ 写透只在真源完整读完（trunc=false 才进写透段）
    assert!(
        FS_FETCH.contains("read_cache_json(&path, &t)"),
        "真源 eof 才 read_cache_json 写透"
    );
}
