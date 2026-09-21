//! 服务卡文案考题（A 档）：三源合成文案映射（后端 × nasup × health）、
//! health JSON 解析、时长格式——纯逻辑先行钉死。2026-09-20 晚
//! 合并裁决：连接卡 + 服务卡合并成 link_card 一张二级卡两竖列，
//! 几何考卷归 link_card_spec，本卷只留文案面。
//!
//! 变异抽检：①compose 把 Phase::Error 的旧数据清空（闪断清卡面）
//! 必须咬；②会话行 alive=false 不标死（死会话冒充活的）必须咬。

use kfm_na::na_server_sup::{SupSnap, SupState};
use kfm_na::settings::Backend;
use kfm_na::svc_health::{self, HealthSnap, Phase};
use kfm_na::ui::svc_card::{self, FIELD_LABELS, N_FIELDS};

fn sup_of(state: SupState) -> SupSnap {
    SupSnap {
        mode: kfm_na::na_server_sup::SupMode::Systemd,
        state,
        target: "root@8.145.46.182:22".into(),
        epoch: 0,
    }
}

fn hs(phase: Phase, info: Option<svc_health::HealthInfo>) -> HealthSnap {
    HealthSnap {
        phase,
        info,
        epoch: 0,
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

// ---- 三源合成文案映射 ----

#[test]
fn spec_合成_kfmv4托管态() {
    let c = svc_card::compose(Backend::Kfmv4, None, &hs(Phase::Kfmv4, None));
    assert_eq!(c.word, "kfmv4 托管");
    assert_eq!(c.backend, "kfmv4");
    assert_eq!(c.uptime, "—");
    assert_eq!(c.sess_n, "—");
    assert_eq!(c.error, "—");
    assert!(c.lines.is_empty(), "托管态不许有会话行");
}

#[test]
fn spec_合成_在线有数据() {
    let info = svc_health::parse_health(HEALTH_JSON).unwrap();
    let sup = sup_of(SupState::Up);
    let c = svc_card::compose(Backend::NaServer, Some(&sup), &hs(Phase::Ready, Some(info)));
    assert_eq!(c.word, "自持在线");
    assert_eq!(c.backend, "na-server");
    assert_eq!(c.uptime, "1h");
    assert_eq!(c.sess_n, "2");
    assert_eq!(c.lines.len(), 2);
    assert_eq!(c.lines[0], "s6 · 80×24 · 闲 5m");
    assert_eq!(c.error, "—");
}

#[test]
fn spec_合成_外部借用与待数据() {
    let sup = sup_of(SupState::ExternalUp);
    let c = svc_card::compose(Backend::NaServer, Some(&sup), &hs(Phase::Loading, None));
    assert_eq!(c.word, "外部借用");
    assert_eq!(c.uptime, "—", "health 未到位 = 字段占位，不编造");
    assert!(c.lines.is_empty());
    // 看门狗没起（后端刚切来）= 未启动
    let c2 = svc_card::compose(Backend::NaServer, None, &hs(Phase::Pending, None));
    assert_eq!(c2.word, "未启动");
}

#[test]
fn spec_合成_轮询错留旧账() {
    // 闪断：Error 相保留上一份数据（卡面不清空），错误上字段
    let info = svc_health::parse_health(HEALTH_JSON).unwrap();
    let sup = sup_of(SupState::Up);
    let c = svc_card::compose(
        Backend::NaServer,
        Some(&sup),
        &hs(Phase::Error("连接失败: refused".into()), Some(info)),
    );
    assert_eq!(c.sess_n, "2", "Error 相必须保留旧数据，闪断不清卡面");
    assert_eq!(c.lines.len(), 2);
    assert_eq!(c.error, "连接失败: refused");
}

#[test]
fn spec_合成_退避相错误来自nasup() {
    let sup = sup_of(SupState::Down {
        attempts: 2,
        last_error: "exec 超时".into(),
    });
    let c = svc_card::compose(Backend::NaServer, Some(&sup), &hs(Phase::Pending, None));
    assert_eq!(c.word, "退避 ×2");
    assert_eq!(c.error, "exec 超时", "health 无错时错误字段落 nasup 旧账");
}

#[test]
fn spec_字段标签_涂装唯一源() {
    assert_eq!(FIELD_LABELS.len(), N_FIELDS);
    assert_eq!(FIELD_LABELS, ["后端", "在线", "会话", "错误"]);
}
