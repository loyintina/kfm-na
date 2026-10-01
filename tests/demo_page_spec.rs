//! demo_page_spec.rs — md 引擎宪法 token 考题（A 档纯逻辑，答案
//! src/ui/demo_page.rs）。
//!
//! BAR-207（2026-09-30）：md 打样 demo 页退役，本卷同步瘦身——打样期
//! 的块序/整页几何/chrome 冒烟五钉随页同焚，留下的只钉 md 排版/涂装
//! 引擎（md_layout/md_paint）还在吃的尺子：
//! ①行高 = 1.4 倍字号上取整咬半格网（一切行高/间距 = 0.5 格整数倍
//! 的宪法条款在布局层的兑现）；②宪法 token 编译期钉——标题文字左缩进
//! ≥1 格 / [ 框横带收尾 0.5 格 / 圆角 R=半格 / 分隔线 3px / 缩进 1 格 /
//! 淡彩槽位角色位序；③BLOCK_GAP = 0.5 格。
//! 变异抽检：LINE_RATIO 改 → 题①红；HEAD_TEXT_INSET 跌破 1 格 →
//! 题②编译红；pastel_role 位序换 → 题②红；BLOCK_GAP 改 0 → 题③红。

use kfm_na::termview::{CELL_H, CELL_W};
use kfm_na::ui::demo_page::{
    BLOCK_GAP, BODY_PX, CODE_PX, H1_SCALE, H2_SCALE, H3_SCALE, HEAD_CORNER_R, HEAD_TEXT_INSET,
    HEAD_TOP_TAIL, HR_THICK, HU, INDENT_W, LINE_RATIO, TABLE_COL_GAP_CELLS, TABLE_COL_MIN_CELLS,
    TABLE_HEAD_UNDER, TABLE_ROW_PAD, line_h, pastel_role,
};

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
fn spec_demo_宪法token编译期钉() {
    // const 块 = 编译期钉，跌破红线直接编不过，比运行期断言更早红：
    // 文字距框左缘 ≥1 格（宪法最小容量律同级条款）；[ 框横带收尾 = 0.5 格
    // （横带宽 = 1 格缩进 + 文字行宽 + 0.5 格收尾，随字长不吃满）、圆角
    // R = 半格、分隔线 5px（2026-09-27 三修 1→3；2026-10-01 BAR-218 用户
    // 再判太细 3→5）；引用/列表缩进
    // 1 格；淡彩槽位角色表（宪法 §2.5 角色映射单源，编译期咬死位序）
    const {
        assert!(HEAD_TEXT_INSET >= CELL_W, "标题文字左缩进 ≥1 格");
        assert!(HEAD_TOP_TAIL == CELL_W / 2, "[ 框横带收尾 = 0.5 格");
        assert!(HEAD_CORNER_R == CELL_H / 2, "[ 框圆角 R = 半格");
        assert!(HR_THICK == 5, "分隔线 = 5px（BAR-218 再加粗）");
        assert!(
            TABLE_COL_MIN_CELLS == 10,
            "表格列宽下限 = 10 字符格（BAR-218 用户拍板）"
        );
        assert!(TABLE_COL_GAP_CELLS == 2, "表格列隙 = 2 字符格（BAR-218）");
        assert!(TABLE_ROW_PAD * 2 == HU, "表格行垫 = 0.25 格（BAR-218）");
        assert!(
            TABLE_HEAD_UNDER == 3,
            "表格表头下划线 = 3px 与框厚同尺（BAR-218）"
        );
        assert!(INDENT_W == CELL_W, "引用/列表缩进 = 1 格");
        assert!(BLOCK_GAP == HU, "块间留隙 = 0.5 格（变异：改 0 即红）");
        assert!(pastel_role::BOLD == 0);
        assert!(pastel_role::H4 == 1);
        assert!(pastel_role::RESERVED == 2);
        assert!(pastel_role::INLINE_CODE == 3);
        assert!(pastel_role::H2 == 4);
        assert!(pastel_role::H3 == 5);
        // 字号阶梯 1.7/1.45/1.2（变异：阶梯改平即红）
        assert!(H1_SCALE > H2_SCALE && H2_SCALE > H3_SCALE && H3_SCALE > 1.0);
    }
}
