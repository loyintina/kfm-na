//! 连接/服务卡考题（A 档）：几何（tmux 卡下纵排第二张二级卡）+
//! 文案映射（隧道快照 → 卡行）+ 命中（[重连] 唯一可点）——纯逻辑先行
//! 钉死，涂装在 termview（眼手同尺：两边吃 conn_card 同一份 layout）。
//!
//! 变异抽检：①INSET_EXTRA 漏加 CONN_GAP（tmux 卡预留量与实高漂移 =
//! 两卡相叠/底部空洞）必须咬；②字段行数 4→3（卡高少一行 = 钉的几何
//! 全错）必须咬。

use kfm_na::tunnel::{TunnelSnap, TunnelState};
use kfm_na::ui::conn_card::{self, ConnHit};
use kfm_na::ui::dual_pool::PoolRect;
use kfm_na::ui::parser_page as pp;

fn tmux_card() -> PoolRect {
    PoolRect {
        x: 40,
        y: 100,
        w: 1000,
        h: 700,
    }
}

fn snap_of(state: TunnelState) -> TunnelSnap {
    TunnelSnap {
        state,
        local_port: 9021,
        target: "root@8.145.46.182:22".into(),
        epoch: 0,
    }
}

#[test]
fn spec_文案_快照四相() {
    // 无隧道（L3 未装/无服务器条目）= 未启动全占位
    let c = conn_card::from_tunnel(None);
    assert_eq!(c.word, "未启动");
    assert_eq!(c.target, "—");
    assert_eq!(c.local, "—");
    assert_eq!(c.attempts, "—");
    assert_eq!(c.error, "—");

    let up = conn_card::from_tunnel(Some(&snap_of(TunnelState::Up)));
    assert_eq!(up.word, "自持在线");
    assert_eq!(up.target, "root@8.145.46.182:22");
    assert_eq!(up.local, "127.0.0.1:9021");
    assert_eq!(up.attempts, "—", "在线时不背旧账次数");
    assert_eq!(up.error, "—");

    let ext = conn_card::from_tunnel(Some(&snap_of(TunnelState::ExternalUp)));
    assert_eq!(ext.word, "外部借用");

    let down = conn_card::from_tunnel(Some(&snap_of(TunnelState::Down {
        attempts: 3,
        last_error: "ssh 退出 Some(1)".into(),
    })));
    assert_eq!(down.word, "退避 ×3");
    assert_eq!(
        down.attempts, "×3",
        "退避中次数上卡——用户要知道在敲第几次门"
    );
    assert_eq!(down.error, "ssh 退出 Some(1)");
}

#[test]
fn spec_几何_接在tmux卡下() {
    let tc = tmux_card();
    let l = conn_card::layout(&tc);
    assert_eq!(l.card.x, tc.x, "与 tmux 卡同左右缘（二级卡同池区宽）");
    assert_eq!(l.card.w, tc.w);
    assert_eq!(
        l.card.y,
        tc.y + i64::from(tc.h) + i64::from(conn_card::CONN_GAP),
        "接在 tmux 卡正下方，间距 CONN_GAP"
    );
    assert_eq!(l.card.h, conn_card::CARD_H, "卡高 = 内容定（恒定）");
    // 纵序：卡头 → 四字段行 → 分隔线 → 重连钮，逐段相接不重叠
    assert!(l.header.y >= l.card.y);
    for w in l.fields.windows(2) {
        assert_eq!(
            w[1].y,
            w[0].y + i64::from(w[0].h) + i64::from(conn_card::FIELD_GAP),
            "字段行纵序等距相接"
        );
    }
    assert!(l.divider.y >= l.fields[3].y + i64::from(l.fields[3].h));
    assert!(l.button.y >= l.divider.y);
    assert!(
        l.button.y + i64::from(l.button.h) <= l.card.y + i64::from(l.card.h),
        "重连钮不许出卡底"
    );
}

#[test]
fn spec_预留量_与实高同源() {
    // INSET_EXTRA = tmux 卡为该让出的底部带——与 conn 卡实高 + 间距同
    // 源钉死：两处各写一份必漂移（相叠/空洞鬼影）
    assert_eq!(
        conn_card::INSET_EXTRA,
        conn_card::CONN_GAP + conn_card::CARD_H,
        "预留量 = 间距 + 卡实高，漂移即两卡相叠或底部空洞"
    );
}

#[test]
fn spec_命中_只有重连可点() {
    let l = conn_card::layout(&tmux_card());
    let bx = l.button.x + i64::from(l.button.w) / 2;
    let by = l.button.y + i64::from(l.button.h) / 2;
    assert_eq!(conn_card::hit(&l, bx, by), Some(ConnHit::Reconnect));
    // 字段行/卡头/分隔线 = 展示件，不可点
    let fx = l.fields[0].x + i64::from(l.fields[0].w) / 2;
    let fy = l.fields[0].y + i64::from(l.fields[0].h) / 2;
    assert_eq!(conn_card::hit(&l, fx, fy), None, "字段行是纯展示");
    assert_eq!(conn_card::hit(&l, l.card.x - 5, by), None, "卡外不命中");
}

#[test]
fn spec_字段标签_涂装命中唯一源() {
    assert_eq!(conn_card::FIELD_LABELS.len(), conn_card::N_FIELDS);
    assert_eq!(conn_card::FIELD_LABELS, ["目标", "本地口", "重拉", "错误"]);
    // 卡高账：PAD_V·2 + 卡头 + 行距 + 四字段行 + 三行距 + 分隔线带 + 钮
    let expect = pp::CARD_PAD_V * 2
        + pp::ROW_H
        + pp::ROW_GAP
        + conn_card::N_FIELDS as u32 * conn_card::FIELD_H
        + (conn_card::N_FIELDS as u32 - 1) * conn_card::FIELD_GAP
        + pp::DIVIDER_ZONE
        + pp::BTN_H;
    assert_eq!(conn_card::CARD_H, expect, "卡高账与逐项常量和一致");
}
