//! 连接卡文案考题（A 档）：隧道快照 → 卡文案 的映射——纯逻辑先行
//! 钉死。2026-09-20 晚合并裁决：连接卡 + 服务卡合并成 link_card 一张
//! 二级卡两竖列，几何/命中考卷归 link_card_spec，本卷只留文案面。
//!
//! 变异抽检：①from_tunnel 把 attempts>0 的重拉账显成「—」（退避账
//! 不背）必须咬；②错误文在线相不掩「—」（旧错带进在线卡面）必须咬。

use kfm_na::tunnel::{TunnelSnap, TunnelState};
use kfm_na::ui::conn_card;

fn snap_of(state: TunnelState) -> TunnelSnap {
    TunnelSnap {
        state,
        local_port: 9021,
        target: "root@8.145.46.182:22".into(),
        epoch: 0,
        leg: None,
        port_open: false,
        reverse_up: false,
        quic_fails: 0,
        quic_configured: false,
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
fn spec_字段标签_涂装命中唯一源() {
    assert_eq!(conn_card::FIELD_LABELS.len(), conn_card::N_FIELDS);
    assert_eq!(conn_card::FIELD_LABELS, ["目标", "本地口", "重拉", "错误"]);
}
