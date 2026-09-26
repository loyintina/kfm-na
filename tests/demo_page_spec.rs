//! demo_page_spec.rs — md 渲染打样 demo 页布局考题（A 档纯逻辑，答案
//! src/ui/demo_page.rs）。
//!
//! 契约：①行高 = 1.4 倍字号上取整咬半格网（一切行高/间距 = 0.5 格整数倍
//! 的宪法条款在布局层的兑现）；②标题块 = 上垫 0.5 格 + 行带 + 下垫
//! 0.5 格（文字距框缘上 ≥0.5 格），字号阶梯 1.7/1.45/1.2；③块序 =
//! 设计拍板的样品元素清单（H1→正文→H2 节→H2~H6→代码围栏→引用→列表
//! →分隔线→签名）；④代码围栏上下各 0.5 格、分隔线带 2 个半格；
//! ⑤几何单源——块 y 严格累进（上一块底 + 0.5 格隙 = 下一块顶）。
//! 变异抽检：BLOCK_GAP 改 0 → 题⑤红；标题上垫删了 → 题②红；字号阶梯
//! 改平 → 题②红；行高不咬半格 → 题①红；块序换 → 题③红。

use kfm_na::termview::{CELL_H, CELL_W};
use kfm_na::ui::demo_page::{
    BLOCK_GAP, BODY_PX, BlockKind, CODE_LINES, CODE_PX, H1_SCALE, H2_SCALE, H3_SCALE,
    HEAD_TEXT_INSET, HU, INDENT_W, LINE_RATIO, LIST_ITEMS, QUOTE_LINES, layout, line_h,
};

const W: u32 = 1080;

#[test]
fn spec_demo_行高咬半格网() {
    // 行高 = ceil(px×1.4 / 半格) × 半格；且 ≥ 1.4 倍字号（行距律不破）
    for px in [
        BODY_PX,
        BODY_PX * H1_SCALE,
        BODY_PX * H2_SCALE,
        BODY_PX * H3_SCALE,
        CODE_PX,
    ] {
        let lh = line_h(px);
        assert_eq!(lh % HU, 0, "行高必须咬半格网: px={px} lh={lh}");
        assert!(
            lh as f32 >= px * LINE_RATIO,
            "行高不许小于 1.4 倍字号: px={px} lh={lh}"
        );
        assert!(
            (lh as f32) < px * LINE_RATIO + HU as f32,
            "上取整不许超过一个量子: px={px} lh={lh}"
        );
    }
    // 正文行带标定：36×1.4=50.4 → 54 = 1.5 格
    assert_eq!(line_h(BODY_PX), CELL_H * 3 / 2);
}

#[test]
fn spec_demo_标题块容量律与字号阶梯() {
    let lay = layout(W);
    let h1 = &lay.blocks[0];
    assert_eq!(h1.kind, BlockKind::H1);
    // 字号阶梯 1.7/1.45/1.2（变异：阶梯改平即红）
    assert_eq!(h1.px, BODY_PX * H1_SCALE);
    let h2 = lay.blocks.iter().find(|b| b.kind == BlockKind::H2).unwrap();
    let h3 = lay.blocks.iter().find(|b| b.kind == BlockKind::H3).unwrap();
    assert_eq!(h2.px, BODY_PX * H2_SCALE);
    assert_eq!(h3.px, BODY_PX * H3_SCALE);
    assert!(h1.px > h2.px && h2.px > h3.px && h3.px > BODY_PX);
    // ┌ 框容量律：块高 = 上垫 0.5 格 + 行带 + 下垫 0.5 格（变异：删垫即红）
    assert_eq!(h1.h, HU + h1.line_h + HU, "H1 上下垫各 0.5 格");
    assert_eq!(h2.h, HU + h2.line_h + HU);
    assert_eq!(h3.h, HU + h3.line_h + HU);
    // 文字距框左缘 ≥1 格（宪法最小容量律同级条款；const 块 = 编译期钉，
    // 缩进跌破 1 格直接编不过，比运行期断言更早红）
    const {
        assert!(HEAD_TEXT_INSET >= CELL_W, "标题文字左缩进 ≥1 格");
    }
    // H4-H6 不挂框：正文字号、块高 = 单行带
    for k in [BlockKind::H4, BlockKind::H5, BlockKind::H6] {
        let b = lay.blocks.iter().find(|b| b.kind == k).unwrap();
        assert_eq!(b.px, BODY_PX, "{k:?} 正文字号");
        assert_eq!(b.h, b.line_h, "{k:?} 无框块高 = 行带");
    }
}

#[test]
fn spec_demo_块序即样品清单() {
    let lay = layout(W);
    let kinds: Vec<BlockKind> = lay.blocks.iter().map(|b| b.kind).collect();
    assert_eq!(
        kinds,
        [
            BlockKind::H1,
            BlockKind::Body,
            BlockKind::H2, // 「六档标题」节
            BlockKind::H2,
            BlockKind::H3,
            BlockKind::H4,
            BlockKind::H5,
            BlockKind::H6,
            BlockKind::Code,
            BlockKind::Quote,
            BlockKind::List,
            BlockKind::Hr,
            BlockKind::Sign,
        ],
        "块序 = 拍板样品元素清单（变异：换序即红）"
    );
}

#[test]
fn spec_demo_围栏引用列表分隔线几何() {
    let lay = layout(W);
    let code = lay
        .blocks
        .iter()
        .find(|b| b.kind == BlockKind::Code)
        .unwrap();
    // 围栏 = 上 0.5 格 + 三行 + 下 0.5 格（宪法：上下留 0.5 格）
    assert_eq!(
        code.h,
        HU + code.line_h * CODE_LINES.len() as u32 + HU,
        "代码围栏上下各留 0.5 格"
    );
    assert_eq!(code.px, CODE_PX);
    let quote = lay
        .blocks
        .iter()
        .find(|b| b.kind == BlockKind::Quote)
        .unwrap();
    assert_eq!(quote.h, quote.line_h * QUOTE_LINES.len() as u32);
    let list = lay
        .blocks
        .iter()
        .find(|b| b.kind == BlockKind::List)
        .unwrap();
    assert_eq!(list.h, list.line_h * LIST_ITEMS.len() as u32);
    // 缩进 1 格（引用/列表同条款）
    assert_eq!(INDENT_W, CELL_W, "引用/列表缩进 = 1 格");
    // 分隔线：上下各 0.5 格 = 带高 2 个半格
    let hr = lay.blocks.iter().find(|b| b.kind == BlockKind::Hr).unwrap();
    assert_eq!(hr.h, HU * 2, "分隔线上下各 0.5 格");
}

#[test]
fn spec_demo_块几何单源严格累进() {
    let lay = layout(W);
    let (ox, oy) = kfm_na::ui::tab_bar::content_origin();
    assert_eq!((lay.ox, lay.oy), (ox, oy), "内容原点 = 全卡体系同源");
    assert_eq!(lay.cw, W - ox - 37, "内容视口宽 = 屏宽 − 80");
    let mut expect_y = oy;
    for b in &lay.blocks {
        assert_eq!(b.y, expect_y, "{:?} 顶 = 上一块底 + 0.5 格隙", b.kind);
        assert_eq!(b.h % HU, 0, "{:?} 块高咬半格网", b.kind);
        assert_eq!((b.y - oy) % HU, 0, "{:?} 块顶相对原点咬半格网", b.kind);
        expect_y = b.y + b.h + BLOCK_GAP;
    }
    assert_eq!(BLOCK_GAP, HU, "块间留隙 = 0.5 格（变异：改 0 即红）");
}

// ---- ④ 分层判定 / ⑤ chrome 冒烟 / ⑥ icon accent 兜底臂（2026-09-26
// 补：覆盖矩阵棘轮入账钉——termview::demo_split/paint_demo_page_chrome
// 与 gate::ai_presence_handle 的考题引用源）----

// demo_split ≡ cfg_split（右缘家同构同尺——paint_demo_page_chrome 上方
// doc 钉死「与 cfg_split 同构同尺」，分层判定唯一裁决处，softbuffer 与
// GLES 两路径同取）。变异抽检：visible 判定忘 <w（off=w 屏外仍可见）
// → 第 2 条红；grid 判定忘 !=0 → 第 1 条红。
#[test]
fn spec_demo_分层判定右缘家() {
    use kfm_na::termview::{cfg_split, demo_split};
    assert_eq!(demo_split(0, 720), (false, true), "靠泊：页可见网格让位");
    assert_eq!(
        demo_split(720, 720),
        (true, false),
        "屏外右缘：网格露出页隐"
    );
    for off in [1, 359, 719, 1000] {
        assert_eq!(
            demo_split(off, 720),
            cfg_split(off, 720),
            "off={off} demo_split 必须与 cfg_split 同尺"
        );
    }
}

// chrome 冒烟（C 档边角的自动化面）：空缓冲早退不 panic；靠泊页心
// 落墨（渐变暗底内芯不透明——钉零/非零不钉色值，色随 accent 采样）；
// 屏外 off=w 零墨（刚体矩形与屏求交为空）。
// 变异抽检：漏画底色 → 第 1 条红；求交判错（off=w 照画）→ 第 3 条红。
#[test]
fn spec_demo_chrome冒烟_落墨与屏外零墨() {
    use kfm_na::termview::paint_demo_page_chrome;
    use kfm_na::ui::accent::AccentPair;
    let acc = AccentPair {
        c1: 0x112233,
        c2: 0x445566,
    };
    let (w, h) = (400u32, 800u32);
    paint_demo_page_chrome(&mut [], 0, 0, 0, 0, acc); // 空缓冲早退不 panic
    let mut buf = vec![0u32; (w * h) as usize];
    paint_demo_page_chrome(&mut buf, w, h, 0, 0, acc);
    // 页心落墨（页环内芯 = 渐变暗底不透明，非零即墨——色值跟 accent
    // 采样走，钉零/非零不钉色值）；环带区（margin 带）= CARD_PAGE_BG
    // 平色底（先全幅平填再叠环，环外保留平填）
    assert_ne!(
        buf[((h / 2) * w + w / 2) as usize],
        0,
        "靠泊页心必须落墨（渐变暗底内芯）"
    );
    assert_eq!(
        buf[0],
        kfm_na::ui::accent::CARD_PAGE_BG,
        "环带区 = CARD_PAGE_BG 平色底"
    );
    let mut buf2 = vec![0u32; (w * h) as usize];
    paint_demo_page_chrome(&mut buf2, w, h, 0, w as i32, acc);
    assert!(buf2.iter().all(|&p| p == 0), "off=w 完全屏外 = 零墨");
}

// icon accent 读数出口兜底臂：host 考题环境无人登记 presence →
// ai_presence_handle() = None → 终卡壳烧瓶涂装走 FALLBACK（冷启动
// 首帧不死）。登记真身的日子 = 本钉换断言的日子
#[test]
fn spec_demo_icon_accent兜底_未登记即none() {
    assert!(
        kfm_na::gate::ai_presence_handle().is_none(),
        "host 考题环境不许登记 presence（登记了本钉失效，换断言）"
    );
}
