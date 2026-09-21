//! 环境卡考题（A 档）：三源合成文案映射（后端 × nasup × sys）+
//! sys JSON 解析 + 几何（左滚动区二级卡，恒定高，**单竖列**：卡头 →
//! 四轨【文字行 + 柱轨】→ 两尾部行）——纯逻辑先行钉死，涂装在 termview
//! （眼手同尺：两边吃 sys_card 同一份 layout）。
//!
//! 变异抽检：①左区槽位漏减滚动/脏滚动不钳（卡不随账走 = 三区 v2
//! 排布器账漂移）必须咬；②compose 内存用量拿 avail 当 used（卡面
//! 显示「剩余」冒充「已用」）必须咬；③parse_sys 缺 load 键不报错
//! （对面不是新版 na-server 静默当零）必须咬；④compose 把 None 当
//! 零值显示（"0.00 0.00 0.00" 冒充数据——Android 拒 loadavg 的
//! 合法常态下卡面造假）必须咬；⑤compose 交换路拿 SwapFree 当已用
//! 必须咬（同②，交换路的镜像病）；⑥单竖列轨序错乱（柱轨与标签错位
//! = 数值和曲线讲两件事）必须咬；⑦柱轨与文字行不咬合（柱画在别人
//! 行下）必须咬；⑧柱层画布宽漏加一柱距（稳态位右缘取到画布外 =
//! 进口柱塌成一条）必须咬。

use kfm_na::na_server_sup::{SupSnap, SupState};
use kfm_na::settings::Backend;
use kfm_na::svc_health;
use kfm_na::sys_hist;
use kfm_na::ui::parser_chain::{self, ChainCardId};
use kfm_na::ui::sys_card::{self, METRIC_LABELS, N_METRICS, N_TAILS, TAIL_LABELS, TRACK_H};

/// 三区几何夹具（2026-09-21 三区排布 v2：环境卡归左滚动区）：屏
/// 1080×2280、可视底 2000、tmux 卡高 300——本考题只钉「环境卡在左区
/// 怎么排、字段怎么分列」，不钉三区几何本身（那在 parser_chain_spec）
const W: u32 = 1080;
const H: u32 = 2280;
const VB: i64 = 2000;

fn regs() -> parser_chain::Regions {
    parser_chain::regions(W, H, 0, VB, 300)
}

fn heights() -> parser_chain::ChainHeights {
    parser_chain::heights(300, 2)
}

/// 环境卡 layout（排布器配给制——与生产侧同路径 slot_rect → layout_in）
fn sys_lay() -> sys_card::SysLayout {
    let r = regs();
    sys_card::layout_in(parser_chain::slot_rect(
        ChainCardId::Sys,
        &r,
        &heights(),
        &parser_chain::Scrolls { left: 0, right: 0 },
    ))
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
    let c = sys_card::compose(
        kfm_na::endpoint::EndpointKind::Server,
        Backend::Kfmv4,
        None,
        None,
    );
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
    let c = sys_card::compose(
        kfm_na::endpoint::EndpointKind::Server,
        Backend::NaServer,
        Some(&s),
        Some(&sysinfo()),
    );
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
    let c = sys_card::compose(
        kfm_na::endpoint::EndpointKind::Server,
        Backend::NaServer,
        None,
        None,
    );
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
    let c = sys_card::compose(
        kfm_na::endpoint::EndpointKind::Server,
        Backend::NaServer,
        Some(&s),
        Some(&i),
    );
    assert_eq!(c.load, "—", "负载路采不到 = 该字段占位");
    assert_eq!(c.procs, "—", "procs 挂在 load 路，同路显形");
    assert_eq!(c.mem, "5.0G/14.7G 34%", "内存路不许被连坐");
    assert_eq!(c.swap, "2.9G/3.9G 75%", "交换路不许被连坐");
    assert_eq!(c.disk, "74.9G/98.1G 76%", "磁盘路不许被连坐");
    assert_eq!(c.uptime, "90天20时", "在线路不许被连坐");
    // load 在但 procs 缺（第 4 段坏件）：负载照常，进程占位
    let mut i2 = sysinfo();
    i2.load.as_mut().unwrap().procs = None;
    let c2 = sys_card::compose(
        kfm_na::endpoint::EndpointKind::Server,
        Backend::NaServer,
        Some(&s),
        Some(&i2),
    );
    assert_eq!(c2.load, "2.83 2.69 2.22", "procs 缺不许连坐负载");
    assert_eq!(c2.procs, "—");
}

#[test]
fn spec_合成_本地相() {
    use kfm_na::endpoint::EndpointKind;
    // 本地相（两轴第 6 步③）：对象词 = 注册表静态词「本地」，字段
    // 吃本地直读 sys——后端相不掺和（Kfmv4 后端也照样出本机体征，
    // 本机不需要任何服务器）
    let c = sys_card::compose(EndpointKind::Local, Backend::Kfmv4, None, Some(&sysinfo()));
    assert_eq!(c.word, "本地", "本地相对象词 = 注册表静态词");
    assert_eq!(c.load, "2.83 2.69 2.22", "本地直读字段照常出");
    assert_eq!(c.mem, "5.0G/14.7G 34%");
    assert_eq!(c.uptime, "90天20时");
    // 本地相无数据 = 字段占位不编造（直读路塌 = 合法显形）
    let c2 = sys_card::compose(EndpointKind::Local, Backend::NaServer, None, None);
    assert_eq!(c2.word, "本地");
    assert_eq!(c2.load, "—");
    assert_eq!(c2.disk, "—");
}

// ---- 几何 ----

#[test]
fn spec_几何_左区贴窗顶单竖列() {
    let r = regs();
    let l = sys_lay();
    assert_eq!(l.card.x, r.left.x, "环境卡在左滚动区（三区 v2）");
    assert_eq!(l.card.w, r.left.w);
    assert_eq!(l.card.y, r.left.y, "scroll=0 贴左区窗顶");
    assert_eq!(l.card.h, sys_card::CARD_H, "卡高 = 恒定（四轨 + 两尾部行）");
    use kfm_na::ui::parser_page as pk;
    let cx = l.card.x + i64::from(pk::CARD_PAD_H);
    let cw = l.card.w - pk::CARD_PAD_H * 2;
    // 卡头贴卡顶内缘
    assert_eq!(l.header.x, cx);
    assert_eq!(l.header.w, cw);
    assert_eq!(l.header.y, l.card.y + i64::from(pk::CARD_PAD_V));
    // 四轨单竖列：全宽同 x，文字行 + 其下柱轨，轨序递进
    for (i, md) in l.metrics.iter().enumerate() {
        assert_eq!(md.row.x, cx, "轨 {i} 文字行左锚 = 卡内缘（单竖列）");
        assert_eq!(md.row.w, cw, "轨 {i} 文字行全宽（无并排——变异⑥）");
        assert_eq!(md.track.x, cx, "轨 {i} 柱轨与文字行同 x");
        assert_eq!(md.track.w, cw, "柱轨全宽（柱列按轨宽取件）");
        assert_eq!(md.track.h, TRACK_H);
        assert_eq!(
            md.track.y,
            md.row.y + i64::from(sys_card::FIELD_H),
            "柱轨必须紧咬本轨文字行下缘（变异⑦：错行 = 数值与曲线讲两件事）"
        );
        if i > 0 {
            assert_eq!(
                md.row.y,
                l.metrics[i - 1].track.y + i64::from(TRACK_H + pk::ROW_GAP),
                "轨间纵序等距相接"
            );
        }
    }
    // 尾部两行接在末轨之下
    assert_eq!(
        l.tails[0].y,
        l.metrics[N_METRICS - 1].track.y + i64::from(TRACK_H + pk::ROW_GAP),
        "尾部行紧跟末轨（行距同池）"
    );
    assert_eq!(
        l.tails[1].y,
        l.tails[0].y + i64::from(sys_card::FIELD_H + sys_card::FIELD_GAP)
    );
    let last = &l.tails[N_TAILS - 1];
    assert_eq!(
        last.y + i64::from(last.h),
        l.card.y + i64::from(l.card.h) - i64::from(pk::CARD_PAD_V),
        "末行底 = 卡底内缘（卡高账与布局逐值咬合）"
    );
}

#[test]
fn spec_柱层_画布尺与轨几何() {
    let l = sys_lay();
    let band = sys_card::band_of(&l);
    let t0 = &band.tracks[0];
    assert_eq!(
        band.canvas_w,
        t0.w + sys_hist::STEP,
        "变异⑧：层画布宽 = 轨宽 + 一柱距（稳态位右缘要取到轨宽+柱距）"
    );
    assert_eq!(band.canvas_h(), TRACK_H * N_METRICS as u32);
    assert_eq!(band.track_h, TRACK_H);
    assert_eq!(
        t0.bars,
        sys_hist::bars_for(t0.w, sys_hist::STEP),
        "可见柱数 = 轨宽/柱距（单源）"
    );
    assert!(t0.bars >= 1);
    assert_eq!(t0.max_h, sys_hist::BAR_MAX_H.min(TRACK_H));
    assert_eq!(t0.step, sys_hist::STEP);
    // 四轨 x/宽同尺（柱层紧凑叠放的前提）
    for t in band.tracks.iter() {
        assert_eq!(t.x, t0.x);
        assert_eq!(t.w, t0.w);
    }
    // 层内 v 段按轨序齐分
    assert!((band.uv_v(1).0 - 0.25).abs() < 1e-6);
    assert!((band.uv_v(3).1 - 1.0).abs() < 1e-6);
}

#[test]
fn spec_槽位_随左区滚动账平移() {
    // 左区槽位 = 区窗 − eff_scroll（三区 v2 排布器账——变异①：漏减
    // 滚动或脏滚动不钳必须咬；更多区几何钉在 parser_chain_spec）
    let r = regs();
    let h = heights();
    let s0 = parser_chain::Scrolls { left: 0, right: 0 };
    let base = parser_chain::slot_rect(ChainCardId::Sys, &r, &h, &s0);
    assert_eq!(base.y, r.left.y);
    // 短内容 max=0：任何脏滚动都钳回区顶（环境卡恒高 < 左区窗）
    assert_eq!(
        parser_chain::scroll_max(ChainCardId::Sys, &r, &h),
        0,
        "本组参数左账必须不可滚"
    );
    let dirty = parser_chain::Scrolls {
        left: 500,
        right: 0,
    };
    let clamped = parser_chain::slot_rect(ChainCardId::Sys, &r, &h, &dirty);
    assert_eq!(clamped.y, r.left.y, "max=0 的区脏滚动必须钳回区顶");
}

#[test]
fn spec_字段标签_涂装唯一源() {
    // 单竖列（2026-09-21 环境卡重做）：四轨标签与 sys_hist 轨序同长同序，
    // 尾部两行各归各位——涂装/命中/取件三处同吃这两张表
    assert_eq!(METRIC_LABELS.len(), N_METRICS);
    assert_eq!(METRIC_LABELS, ["负载", "内存", "交换", "磁盘"]);
    assert_eq!(TAIL_LABELS.len(), N_TAILS);
    assert_eq!(TAIL_LABELS, ["进程", "在线"]);
    assert_eq!(N_METRICS, sys_hist::METRICS.len(), "轨序同长");
}

#[test]
fn spec_值文案_取件同序() {
    let s = sup();
    let c = sys_card::compose(
        kfm_na::endpoint::EndpointKind::Server,
        Backend::NaServer,
        Some(&s),
        Some(&sysinfo()),
    );
    let mv = sys_card::metric_values(&c);
    assert_eq!(mv[0], c.load, "轨 0 = 负载（与 METRIC_LABELS 同序）");
    assert_eq!(mv[1], c.mem);
    assert_eq!(mv[2], c.swap);
    assert_eq!(mv[3], c.disk);
    let tv = sys_card::tail_values(&c);
    assert_eq!(tv[0], c.procs, "尾部 0 = 进程（与 TAIL_LABELS 同序）");
    assert_eq!(tv[1], c.uptime);
}
