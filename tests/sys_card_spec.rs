//! 环境卡考题（A 档）：三源合成文案映射（后端 × nasup × sys）+
//! sys JSON 解析 + 几何（服务卡下纵排第四张二级卡，恒定高）——
//! 纯逻辑先行钉死，涂装在 termview（眼手同尺：两边吃 sys_card
//! 同一份 layout）。
//!
//! 变异抽检：①INSET_EXTRA 漏加 SYS_GAP（预留量与实高漂移 = 两卡
//! 相叠/底部空洞）必须咬；②compose 内存用量拿 avail 当 used（卡面
//! 显示「剩余」冒充「已用」）必须咬；③parse_sys 缺 load 键不报错
//! （对面不是新版 na-server 静默当零）必须咬；④compose 把 None 当
//! 零值显示（"0.00 0.00 0.00" 冒充数据——Android 拒 loadavg 的
//! 合法常态下卡面造假）必须咬。

use kfm_na::na_server_sup::{SupSnap, SupState};
use kfm_na::settings::Backend;
use kfm_na::svc_health;
use kfm_na::ui::dual_pool::PoolRect;
use kfm_na::ui::sys_card::{self, FIELD_LABELS, N_FIELDS};

fn svc_card() -> PoolRect {
    PoolRect {
        x: 40,
        y: 1500,
        w: 1000,
        h: 600,
    }
}

fn sup() -> SupSnap {
    SupSnap {
        state: SupState::Up,
        target: "root@8.145.46.182:22".into(),
        epoch: 0,
    }
}

fn sysinfo() -> na_sys::SysInfo {
    na_sys::SysInfo {
        load: Some(na_sys::LoadAvg {
            l1: 2.83,
            l5: 2.69,
            l15: 2.22,
        }),
        mem: Some(na_sys::MemInfo {
            total_kb: 15432224,
            avail_kb: 10146796,
        }),
        disk: Some((105_286_258_688, 24_877_244_416)),
    }
}

const SYS_JSON: &str = r#"{"disk_avail_b":24877244416,"disk_total_b":105286258688,"load":[2.83,2.69,2.22],"mem_avail_kb":10146796,"mem_total_kb":15432224}"#;

// ---- sys JSON 解析 ----

#[test]
fn spec_sys解析_全字段() {
    let i = svc_health::parse_sys(SYS_JSON).unwrap();
    let l = i.load.expect("load 有值");
    assert_eq!(l.l1, 2.83);
    assert_eq!(l.l5, 2.69);
    assert_eq!(l.l15, 2.22);
    let m = i.mem.expect("mem 有值");
    assert_eq!(m.total_kb, 15432224);
    assert_eq!(m.avail_kb, 10146796);
    let (dt, da) = i.disk.expect("disk 有值");
    assert_eq!(dt, 105_286_258_688);
    assert_eq!(da, 24_877_244_416);
}

#[test]
fn spec_sys解析_坏件显形() {
    assert!(svc_health::parse_sys("not json").is_err());
    assert!(
        svc_health::parse_sys(r#"{"mem_total_kb":1}"#).is_err(),
        "缺 load 键 = 对面不是新版 na-server，必须报错不许静默当零"
    );
    assert!(
        svc_health::parse_sys(r#"{"load":[1.0,2.0]}"#).is_err(),
        "load 不足三段必须报错"
    );
}

#[test]
fn spec_sys解析_null显形() {
    // 值 null = 该路采不到的合法显形（Android 拒 loadavg）：
    // 键在值空 → None，不许报错不许编造
    let i = svc_health::parse_sys(
        r#"{"load":null,"mem_total_kb":null,"mem_avail_kb":null,"disk_total_b":null,"disk_avail_b":null}"#,
    )
    .expect("null 是合法显形，不许报错");
    assert!(i.load.is_none());
    assert!(i.mem.is_none());
    assert!(i.disk.is_none());
}

// ---- 三源合成文案映射 ----

#[test]
fn spec_合成_kfmv4托管态() {
    let c = sys_card::compose(Backend::Kfmv4, None, None);
    assert_eq!(c.word, "kfmv4 托管");
    assert_eq!(c.load, "—");
    assert_eq!(c.mem, "—");
    assert_eq!(c.disk, "—");
}

#[test]
fn spec_合成_在线有数据() {
    let s = sup();
    let c = sys_card::compose(Backend::NaServer, Some(&s), Some(&sysinfo()));
    assert_eq!(c.word, "root@8.145.46.182:22");
    assert_eq!(c.load, "2.83 2.69 2.22");
    // 内存：已用 = (15432224-10146796)K = 5285428K ≈ 5.0G；总量 ≈ 14.7G
    assert_eq!(c.mem, "5.0G/14.7G 34%");
    // 磁盘：已用 = 105286258688-24877244416 = 80409014272 ≈ 74.9G；总量 98.1G
    assert_eq!(c.disk, "74.9G/98.1G 76%");
}

#[test]
fn spec_合成_待数据占位() {
    let c = sys_card::compose(Backend::NaServer, None, None);
    assert_eq!(c.word, "确认中", "nasup 没起 = 对象词给在途相");
    assert_eq!(c.load, "—", "无数据 = 占位，不编造");
    assert_eq!(c.mem, "—");
    assert_eq!(c.disk, "—");
}

#[test]
fn spec_合成_局部显形() {
    // 单路采不到（Android 拒 loadavg 的合法常态）：该字段占位，
    // 其余路照常显示——显形不连坐
    let s = sup();
    let mut i = sysinfo();
    i.load = None;
    let c = sys_card::compose(Backend::NaServer, Some(&s), Some(&i));
    assert_eq!(c.load, "—", "负载路采不到 = 该字段占位");
    assert_eq!(c.mem, "5.0G/14.7G 34%", "内存路不许被连坐");
    assert_eq!(c.disk, "74.9G/98.1G 76%", "磁盘路不许被连坐");
}

// ---- 几何 ----

#[test]
fn spec_几何_接在服务卡下() {
    let sc = svc_card();
    let l = sys_card::layout(&sc);
    assert_eq!(l.card.x, sc.x, "与服务卡同左右缘（二级卡同池区宽）");
    assert_eq!(l.card.w, sc.w);
    assert_eq!(
        l.card.y,
        sc.y + i64::from(sc.h) + i64::from(sys_card::SYS_GAP),
        "接在服务卡正下方，间距 SYS_GAP"
    );
    assert_eq!(l.card.h, sys_card::CARD_H, "卡高 = 恒定（三字段行）");
    // 纵序：卡头 → 三字段行，逐段相接不重叠、不出卡底
    assert!(l.header.y >= l.card.y);
    for w in l.fields.windows(2) {
        assert_eq!(
            w[1].y,
            w[0].y + i64::from(w[0].h) + i64::from(sys_card::FIELD_GAP),
            "字段行纵序等距相接"
        );
    }
    let last = &l.fields[N_FIELDS - 1];
    assert!(
        last.y + i64::from(last.h) <= l.card.y + i64::from(l.card.h),
        "末字段行不许出卡底"
    );
}

#[test]
fn spec_预留量_与实高同源() {
    assert_eq!(
        sys_card::INSET_EXTRA,
        sys_card::SYS_GAP + sys_card::CARD_H,
        "预留量 = 间距 + 卡实高，漂移即两卡相叠或底部空洞"
    );
}

#[test]
fn spec_字段标签_涂装唯一源() {
    assert_eq!(FIELD_LABELS.len(), N_FIELDS);
    assert_eq!(FIELD_LABELS, ["负载", "内存", "磁盘"]);
}
