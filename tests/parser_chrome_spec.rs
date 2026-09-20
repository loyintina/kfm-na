//! 解析页壳/内容 inset 契约考题（源码守卫，A 档）——**2026-09-20 视口化
//! 契约取代 BAR-120 同线遮盖契约**。
//!
//! 沿革：BAR-120（同日早）用户实机报「召出键盘后外部框架卡回缩了，但
//! 里面的内容却没有，内容超出了卡片」，当时修约 = 壳与内容同吃栏带高
//! （框底与内容同缘，遮盖线一致）。同日用户拍板视口化（「卡弹小，里面
//! 的内容跟随截断，上下能滑动」）后契约更新为：
//!
//! - **壳吃 bottom_inset（含键盘）**：键盘在场页环弹小到输入栏带以上，
//!   环底缘 = 页面滚动视口底——「卡弹小」的本体；
//! - **内容布局吃栏带高（永不含键盘）**：BAR-119 红线不动——键盘只盖
//!   不重排，卡全价；逾视底归键盘/栏带遮盖，逾视顶归页缘纵裁剪带，
//!   被盖部分经页面滚动（page_scroll）滑出。
//!
//! 本钉 = 源码守卫：android_app.rs 两个产品路（GLES 烘焙/softbuffer
//! 兜底）里——paint_parser_page_chrome 调用必须传 bottom_inset（回潮
//! 成栏带高族 = 键盘在场框不弹小，视口底丢失）；paint_parser_content
//! 调用必须传栏带高且不许吃 bottom_inset（BAR-119 红线回潮 = 键盘
//! 重排塌卡）。
//!
//! 变异抽检：壳调用换回栏带高（视口化丢失）必须咬；内容调用换吃
//! bottom_inset（BAR-119 回潮）必须咬。

/// 产品路（android_app.rs）里：壳调用吃 bottom_inset、内容调用吃栏带高
#[test]
fn spec_视口化_壳吃键盘inset_内容吃栏带高() {
    let src = include_str!("../src/android_app.rs");
    // 实参窗切取：从调用点起逐行扫到闭合「);」（中文注释多字节，按
    // 字节切窗会切穿 char 边界——逐行扫无此坑）
    let args_of = |from: usize| -> String {
        let mut args = String::new();
        for line in src[from..].lines() {
            let closed = line.contains(");");
            args.push_str(line);
            if closed {
                break;
            }
        }
        args
    };
    let mut n_chrome = 0usize;
    for (i, _) in src.match_indices("paint_parser_page_chrome(") {
        let args = args_of(i);
        assert!(
            args.contains("bottom_inset"),
            "视口化契约：解析页壳必须吃 bottom_inset（含键盘）——卡弹小的本体"
        );
        assert!(
            !args.contains("bar_h"),
            "壳回潮成栏带高族 = 键盘在场框不弹小，页面滚动视口底丢失"
        );
        n_chrome += 1;
    }
    assert!(
        n_chrome >= 2,
        "GLES 烘焙与 softbuffer 兜底两个产品路都要钉到"
    );
    let mut n_content = 0usize;
    for (i, _) in src.match_indices("paint_parser_content(") {
        let args = args_of(i);
        assert!(
            args.contains("bar_h"),
            "BAR-119 红线：解析页内容布局必须吃栏带高（键盘只盖不重排）"
        );
        assert!(
            !args.contains("bottom_inset"),
            "BAR-119 回潮：内容吃了 bottom_inset（含键盘）= 键盘重排塌卡"
        );
        n_content += 1;
    }
    assert!(
        n_content >= 2,
        "GLES 烘焙与 softbuffer 兜底两个产品路都要钉到"
    );
}
