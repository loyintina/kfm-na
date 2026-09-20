//! 环境卡考题（A 档）：三源合成文案映射（后端 × nasup × sys）+
//! sys JSON 解析 + 几何（服务卡下纵排第四张二级卡，恒定高，六字段
//! 两竖列）——纯逻辑先行钉死，涂装在 termview（眼手同尺：两边吃
//! sys_card 同一份 layout）。
//!
//! 变异抽检：①预留带漏加前距/本卡高（预留量与实高漂移 = 两卡
//! 相叠/底部空洞）必须咬；②compose 内存用量拿 avail 当 used（卡面
//! 显示「剩余」冒充「已用」）必须咬；③parse_sys 缺 load 键不报错
//! （对面不是新版 na-server 静默当零）必须咬；④compose 把 None 当
//! 零值显示（"0.00 0.00 0.00" 冒充数据——Android 拒 loadavg 的
//! 合法常态下卡面造假）必须咬；⑤compose 交换路拿 SwapFree 当已用
//! 必须咬（同②，交换路的镜像病）；⑥六字段几何列序错乱（行主序
//! 负载/进程 | 内存/交换 | 磁盘/在线——左右列换位 = 语义串行）必须咬。

use kfm_na::na_server_sup::{SupSnap, SupState};
use kfm_na::settings::Backend;
use kfm_na::svc_health;
use kfm_na::termview::CELL_H;
use kfm_na::ui::dual_pool::PoolRect;
use kfm_na::ui::parser_chain::{self, ChainCardId};
use kfm_na::ui::sys_card::{self, FIELD_LABELS, N_FIELDS};

/// 链几何夹具：tmux 卡 + 字面高度表（link=600 是虚构卡高——本考题
/// 只钉「接在谁下面、多大间距、字段怎么排」，不钉卡高账本身）
fn chain_fixture() -> (PoolRect, parser_chain::ChainHeights) {
    let tmux = PoolRect {
        x: 40,
        y: 1500 - 800 - i64::from(CELL_H),
        w: 1000,
        h: 800,
    };
    let h = parser_chain::ChainHeights {
        tmux: 800,
        link: 600,
        sys: sys_card::CARD_H,
    };
    (tmux, h)
}

/// 链上的合并卡席位（Link 槽外框）：由 tmux 卡 + 排布器配给——
/// 与生产侧同路径（slot_rect → layout_in）
fn svc_card() -> PoolRect {
    let (tmux, h) = chain_fixture();
    parser_chain::slot_rect(ChainCardId::Link, &tmux, &h)
}

/// 环境卡 layout（排布器配给制）
fn sys_lay() -> sys_card::SysLayout {
    let (tmux, h) = chain_fixture();
    sys_card::layout_in(parser_chain::slot_rect(ChainCardId::Sys, &tmux, &h))
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
            procs: Some((2, 123)),
        }),
        mem: Some(na_sys::MemInfo {
            total_kb: 15432224,
            avail_kb: 10146796,
            swap: Some((4096000, 1024000)),
        }),
        disk: Some((105_286_258_688, 24_877_244_416)),
        uptime_s: Some(7849375),
    }
}

const SYS_JSON: &str = r#"{"disk_avail_b":24877244416,"disk_total_b":105286258688,"load":[2.83,2.69,2.22],"procs":[2,123],"mem_avail_kb":10146796,"mem_total_kb":15432224,"swap_total_kb":4096000,"swap_free_kb":1024000,"uptime_s":7849375}"#;

// ---- sys JSON 解析 ----

#[test]
fn spec_sys解析_全字段() {
    let i = svc_health::parse_sys(SYS_JSON).unwrap();
    let l = i.load.expect("load 有值");
    assert_eq!(l.l1, 2.83);
    assert_eq!(l.l5, 2.69);
    assert_eq!(l.l15, 2.22);
    assert_eq!(l.procs, Some((2, 123)));
    let m = i.mem.expect("mem 有值");
    assert_eq!(m.total_kb, 15432224);
    assert_eq!(m.avail_kb, 10146796);
    assert_eq!(m.swap, Some((4096000, 1024000)));
    let (dt, da) = i.disk.expect("disk 有值");
    assert_eq!(dt, 105_286_258_688);
    assert_eq!(da, 24_877_244_416);
    assert_eq!(i.uptime_s, Some(7849375));
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
        r#"{"load":null,"procs":null,"mem_total_kb":null,"mem_avail_kb":null,"swap_total_kb":null,"swap_free_kb":null,"disk_total_b":null,"disk_avail_b":null,"uptime_s":null}"#,
    )
    .expect("null 是合法显形，不许报错");
    assert!(i.load.is_none());
    assert!(i.mem.is_none());
    assert!(i.disk.is_none());
    assert!(i.uptime_s.is_none());
}

#[test]
fn spec_sys解析_旧版缺新键() {
    // 旧版 na-server 不认 procs/swap_*/uptime_s：缺键 = None 显形
    // 「—」，契约向旧兼容不破（不许报错不许连坐老键）
    let i = svc_health::parse_sys(
        r#"{"load":[2.83,2.69,2.22],"mem_total_kb":15432224,"mem_avail_kb":10146796,"disk_total_b":105286258688,"disk_avail_b":24877244416}"#,
    )
    .expect("旧版缺新键必须照常解析");
    let l = i.load.expect("load 有值");
    assert_eq!(l.procs, None, "旧版缺 procs = None 不连坐 load");
    let m = i.mem.expect("mem 有值");
    assert_eq!(m.swap, None, "旧版缺 swap 键 = None 不连坐 mem");
    assert_eq!(i.uptime_s, None);
}

// ---- 三源合成文案映射 ----

#[test]
fn spec_合成_kfmv4托管态() {
    let c = sys_card::compose(Backend::Kfmv4, None, None);
    assert_eq!(c.word, "kfmv4 托管");
    assert_eq!(c.load, "—");
    assert_eq!(c.procs, "—");
    assert_eq!(c.mem, "—");
    assert_eq!(c.swap, "—");
    assert_eq!(c.disk, "—");
    assert_eq!(c.uptime, "—");
}

#[test]
fn spec_合成_在线有数据() {
    let s = sup();
    let c = sys_card::compose(Backend::NaServer, Some(&s), Some(&sysinfo()));
    assert_eq!(c.word, "root@8.145.46.182:22");
    assert_eq!(c.load, "2.83 2.69 2.22");
    assert_eq!(c.procs, "2/123");
    // 内存：已用 = (15432224-10146796)K = 5285428K ≈ 5.0G；总量 ≈ 14.7G
    assert_eq!(c.mem, "5.0G/14.7G 34%");
    // 交换：已用 = (4096000-1024000)K = 3072000K ≈ 2.9G；总量 ≈ 3.9G
    assert_eq!(c.swap, "2.9G/3.9G 75%");
    // 磁盘：已用 = 105286258688-24877244416 = 80409014272 ≈ 74.9G；总量 98.1G
    assert_eq!(c.disk, "74.9G/98.1G 76%");
    // 在线：7849375s = 90 天 20 时
    assert_eq!(c.uptime, "90天20时");
}

#[test]
fn spec_合成_待数据占位() {
    let c = sys_card::compose(Backend::NaServer, None, None);
    assert_eq!(c.word, "确认中", "nasup 没起 = 对象词给在途相");
    assert_eq!(c.load, "—", "无数据 = 占位，不编造");
    assert_eq!(c.procs, "—");
    assert_eq!(c.mem, "—");
    assert_eq!(c.swap, "—");
    assert_eq!(c.disk, "—");
    assert_eq!(c.uptime, "—");
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
    assert_eq!(c.procs, "—", "procs 挂在 load 路，同路显形");
    assert_eq!(c.mem, "5.0G/14.7G 34%", "内存路不许被连坐");
    assert_eq!(c.swap, "2.9G/3.9G 75%", "交换路不许被连坐");
    assert_eq!(c.disk, "74.9G/98.1G 76%", "磁盘路不许被连坐");
    assert_eq!(c.uptime, "90天20时", "在线路不许被连坐");
    // load 在但 procs 缺（第 4 段坏件）：负载照常，进程占位
    let mut i2 = sysinfo();
    i2.load.as_mut().unwrap().procs = None;
    let c2 = sys_card::compose(Backend::NaServer, Some(&s), Some(&i2));
    assert_eq!(c2.load, "2.83 2.69 2.22", "procs 缺不许连坐负载");
    assert_eq!(c2.procs, "—");
}

// ---- 几何 ----

#[test]
fn spec_几何_接在服务卡下() {
    let sc = svc_card();
    let l = sys_lay();
    assert_eq!(l.card.x, sc.x, "与服务卡同左右缘（二级卡同池区宽）");
    assert_eq!(l.card.w, sc.w);
    assert_eq!(
        l.card.y,
        sc.y + i64::from(sc.h) + i64::from(CELL_H),
        "接在服务卡正下方，间距一格（排布器注册槽 gap_before）"
    );
    assert_eq!(
        l.card.h,
        sys_card::CARD_H,
        "卡高 = 恒定（三行六字段两竖列）"
    );
    // 六字段行主序两竖列：0 负载/1 进程 | 2 内存/3 交换 | 4 磁盘/5 在线
    let pp = kfm_na::ui::parser_page::COL_GAP;
    let cx = l.card.x + i64::from(kfm_na::ui::parser_page::CARD_PAD_H);
    let cw = l.card.w - kfm_na::ui::parser_page::CARD_PAD_H * 2;
    let col_w = (cw - pp) / 2;
    for (i, f) in l.fields.iter().enumerate() {
        let (row, col) = (i / 2, i % 2);
        let want_x = if col == 0 {
            cx
        } else {
            cx + i64::from(col_w + pp)
        };
        assert_eq!(f.x, want_x, "字段 {i} 列位错（行主序两竖列）");
        assert_eq!(f.w, col_w, "字段 {i} 列宽 = (内容宽−列距)/2");
        if i >= 2 {
            assert_eq!(
                f.y,
                l.fields[i - 2].y + i64::from(sys_card::FIELD_H + sys_card::FIELD_GAP),
                "行间纵序等距相接"
            );
        }
        let _ = row;
    }
    assert_eq!(l.fields[0].y, l.fields[1].y, "同一行两列同高");
    let last = &l.fields[N_FIELDS - 1];
    assert!(
        last.y + i64::from(last.h) <= l.card.y + i64::from(l.card.h),
        "末字段行不许出卡底"
    );
}

#[test]
fn spec_预留量_与实高同源() {
    // Link 槽之后的全部占位 = 本卡前距 + 本卡实高（排布器账——
    // 变异①：漏 CELL_H 或漏 CARD_H 必须咬）
    for n in [0usize, 1, 3, 6] {
        assert_eq!(
            parser_chain::reserved_below(ChainCardId::Link, &parser_chain::heights(0, n)),
            CELL_H + sys_card::CARD_H,
            "n={n} Link 后预留量漂移 = 两卡相叠或底部空洞"
        );
    }
}

#[test]
fn spec_字段标签_涂装唯一源() {
    assert_eq!(FIELD_LABELS.len(), N_FIELDS);
    assert_eq!(
        FIELD_LABELS,
        ["负载", "进程", "内存", "交换", "磁盘", "在线"]
    );
}
