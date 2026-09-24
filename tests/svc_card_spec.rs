//! 通道卡文案考题（A 档，2026-09-24 通道段改造：原「服务」段退役，
//! 原位换四口连接状况 + 调试钮）：隧道快照 → 四口状态词映射、调试钮
//! 裁决——纯逻辑先行钉死。几何考卷归 link_card_spec，本卷只留文案面。
//! （svc_health 解析/时长/会话行三钉保留——那些是 svc_health 的钉，
//! 与本卡文案无关。）
//!
//! 变异抽检：①quic_row 把「跳闸降级」判成「待起」（降级相不显形 =
//! 用户以为 QUIC 还在扛）必须咬；②reverse_row 把「断」判成「重拉中」
//! （死透了还装活）必须咬；③toggle_verdict 未配置也给钮（点了没反应
//! = 死钮）必须咬。

use kfm_na::svc_health;
use kfm_na::tunnel::{Leg, QUIC_FAIL_TRIP, TunnelSnap, TunnelState};
use kfm_na::ui::svc_card::{self, FIELD_LABELS, N_FIELDS, QuicToggle};

/// 隧道快照夹具（只读字段全 pub，直构造）
fn tsnap(
    state: TunnelState,
    leg: Option<Leg>,
    reverse_up: bool,
    quic_fails: u32,
    quic_configured: bool,
) -> TunnelSnap {
    TunnelSnap {
        state,
        local_port: 9021,
        target: "root@8.145.46.182:22".into(),
        epoch: 0,
        leg,
        port_open: false,
        reverse_up,
        quic_fails,
        quic_configured,
    }
}

const HEALTH_JSON: &str = r#"{"sessions":[{"alive":true,"cmd":"tmux new-session -A -s kfm-na","cols":80,"id":"s6","idle_s":359,"rows":24},{"alive":false,"cmd":"bash","cols":120,"id":"s7","idle_s":5,"rows":40}],"started_epoch_s":1789871037,"uptime_s":4173}"#;

// ---- health 解析 ----

#[test]
fn spec_health解析_全字段() {
    let info = svc_health::parse_health(HEALTH_JSON).unwrap();
    assert_eq!(info.uptime_s, 4173);
    assert_eq!(info.sessions.len(), 2);
    let s0 = &info.sessions[0];
    assert_eq!(s0.id, "s6");
    assert_eq!(s0.cols, 80);
    assert_eq!(s0.rows, 24);
    assert!(s0.alive);
    assert_eq!(s0.idle_s, 359);
    assert!(!info.sessions[1].alive, "死会话的 alive 必须保真");
}

#[test]
fn spec_health解析_坏件显形() {
    assert!(svc_health::parse_health("not json").is_err());
    assert!(
        svc_health::parse_health(r#"{"sessions":[]}"#).is_err(),
        "缺 uptime_s = 对面不是 na-server，必须报错不许静默当零会话"
    );
    // 缺 sessions = 零会话（宽容缺省），不报错
    let info = svc_health::parse_health(r#"{"uptime_s":1}"#).unwrap();
    assert!(info.sessions.is_empty());
}

#[test]
fn spec_时长格式_档位() {
    assert_eq!(svc_health::fmt_duration(0), "0s");
    assert_eq!(svc_health::fmt_duration(59), "59s");
    assert_eq!(svc_health::fmt_duration(60), "1m");
    assert_eq!(svc_health::fmt_duration(3599), "59m");
    assert_eq!(svc_health::fmt_duration(3600), "1h");
    assert_eq!(svc_health::fmt_duration(86399), "23h");
    assert_eq!(svc_health::fmt_duration(86400), "1d");
}

#[test]
fn spec_会话行_死活两相() {
    let info = svc_health::parse_health(HEALTH_JSON).unwrap();
    assert_eq!(
        svc_health::session_line(&info.sessions[0]),
        "s6 · 80×24 · 闲 5m"
    );
    assert_eq!(
        svc_health::session_line(&info.sessions[1]),
        "s7 · 120×40 · 闲 5s（死）",
        "死会话必须标死，不许悄悄当活的画"
    );
}

// ---- 通道卡文案映射（隧道快照 → 四口状态词） ----

#[test]
fn spec_合成_看门狗没起() {
    let c = svc_card::compose(None);
    assert_eq!(c.word, "未启动");
    assert_eq!(c.vals[3], "预留 M4", "62694 恒预留");
    assert_eq!(c.vals[0], "—");
}

#[test]
fn spec_合成_quic在线全相() {
    let s = tsnap(TunnelState::QuicUp, Some(Leg::Quic), true, 0, true);
    let c = svc_card::compose(Some(&s));
    assert_eq!(c.word, "QUIC 在线");
    assert_eq!(c.vals, ["QUIC 桥在线", "在线", "在线", "预留 M4"]);
}

#[test]
fn spec_数据行_五相() {
    assert_eq!(svc_card::data_row(&TunnelState::QuicUp), "QUIC 桥在线");
    assert_eq!(svc_card::data_row(&TunnelState::Up), "ssh 正连在线");
    assert_eq!(svc_card::data_row(&TunnelState::ExternalUp), "外部借用");
    assert_eq!(svc_card::data_row(&TunnelState::Starting), "起手中");
    assert_eq!(
        svc_card::data_row(&TunnelState::Down {
            attempts: 1,
            last_error: String::new()
        }),
        "断"
    );
}

#[test]
fn spec_反连行_断不许装重拉() {
    // 变异②：数据路断了反连必断（同一条 ssh）——「断」不许写成「重拉中」
    let down = TunnelState::Down {
        attempts: 0,
        last_error: String::new(),
    };
    assert_eq!(
        svc_card::reverse_row(&tsnap(down, None, false, 0, true)),
        "断"
    );
    assert_eq!(
        svc_card::reverse_row(&tsnap(TunnelState::QuicUp, Some(Leg::Quic), true, 0, true)),
        "在线"
    );
    assert_eq!(
        svc_card::reverse_row(&tsnap(TunnelState::QuicUp, Some(Leg::Quic), false, 0, true)),
        "重拉中",
        "腿在伴生死 = 重拉窗口（封锁闸/死亡审理中）"
    );
}

#[test]
fn spec_quic行_六相真值表() {
    // 变异①：跳闸降级必须显形——降级了还说「待起」= 用户以为 QUIC 在扛
    let up = TunnelState::QuicUp;
    assert_eq!(
        svc_card::quic_row(&tsnap(up.clone(), Some(Leg::Quic), true, 0, false)),
        "未配置"
    );
    assert_eq!(
        svc_card::quic_row(&tsnap(
            TunnelState::Up,
            Some(Leg::Ssh),
            true,
            QUIC_FAIL_TRIP,
            true
        )),
        "跳闸降级 ssh"
    );
    assert_eq!(
        svc_card::quic_row(&tsnap(up.clone(), Some(Leg::Quic), true, 0, true)),
        "在线"
    );
    assert_eq!(
        svc_card::quic_row(&tsnap(
            TunnelState::Starting,
            Some(Leg::Quic),
            true,
            0,
            true
        )),
        "握手中"
    );
    assert_eq!(
        svc_card::quic_row(&tsnap(TunnelState::Starting, None, false, 2, true)),
        "挂×2"
    );
    assert_eq!(
        svc_card::quic_row(&tsnap(TunnelState::Starting, None, false, 0, true)),
        "待起"
    );
}

#[test]
fn spec_调试钮裁决_三相() {
    // 变异③：未配置 = None（死钮不许活）；未满 = 跳闸；已满 = 投票
    assert_eq!(
        svc_card::toggle_verdict(Some(&tsnap(
            TunnelState::QuicUp,
            Some(Leg::Quic),
            true,
            0,
            false
        ))),
        None,
        "未配置 = 钮不动作"
    );
    assert_eq!(
        svc_card::toggle_verdict(None),
        None,
        "看门狗没起 = 钮不动作"
    );
    assert_eq!(
        svc_card::toggle_verdict(Some(&tsnap(
            TunnelState::QuicUp,
            Some(Leg::Quic),
            true,
            0,
            true
        ))),
        Some(QuicToggle::Trip),
        "跳闸账未满 = 跳闸 QUIC"
    );
    assert_eq!(
        svc_card::toggle_verdict(Some(&tsnap(
            TunnelState::Up,
            Some(Leg::Ssh),
            true,
            QUIC_FAIL_TRIP,
            true
        ))),
        Some(QuicToggle::Heal),
        "跳闸账满 = 投 QUIC"
    );
    // 钮面与裁决同一份账
    assert_eq!(
        svc_card::toggle_label(Some(&tsnap(
            TunnelState::QuicUp,
            Some(Leg::Quic),
            true,
            0,
            true
        ))),
        "跳闸 QUIC"
    );
    assert_eq!(svc_card::toggle_label(None), "QUIC 未配置");
}

#[test]
fn spec_字段标签_涂装唯一源() {
    assert_eq!(FIELD_LABELS.len(), N_FIELDS);
    assert_eq!(
        FIELD_LABELS,
        ["数据 9021", "反连 9022", "QUIC 62633", "QUIC 62694"]
    );
}
