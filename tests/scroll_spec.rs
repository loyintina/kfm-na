//! scroll_spec.rs — 触摸滚动手势状态机考题（A 档纯逻辑，答案 src/scroll.rs）
//!
//! 契约（手机终端的自然手感）：
//! - 位移 < TAP_SLOP_PX 松开 = 点按（唤软键盘），期间一行都不许滚
//! - 越过阈值进入滚动：手指向下拖 = 看更老的历史 = 行数为正
//! - 像素→行换算带余数挂账：半行半行慢拖必须累计成行（取整吞余数则慢滚哑）
//! - 拖下去再拖回来：行数可逆（净位移为零 → 净滚动为零）
//! - 拖滚增益 DRAG_GAIN=2（2026-09-26 用户拍板「移 10px 滚 20px」）：
//!   只乘手指位移，slop 门不动；三通道共用，甩尾初速同倍跟随

use kfm_na::scroll::{DRAG_GAIN, TAP_SLOP_PX, TouchScroll};

const CELL: f64 = 30.0; // 格高 30px（与真机尖刺常量同量级）

#[test]
fn spec_轻点是点按不是滚动() {
    // 全程位移都在阈值内：不许出一行滚动，松手算点按
    let mut t = TouchScroll::new(500.0, CELL);
    assert_eq!(t.moved(505.0), 0);
    assert_eq!(t.moved(500.0 + TAP_SLOP_PX - 1.0), 0);
    // 边界钉（2026-08-27 变异抽检存活体：恰好到阈值=仍算轻点，
    // AOSP touchSlop 含边惯例——此前 < vs <= 无人判卷）
    assert_eq!(t.moved(500.0 + TAP_SLOP_PX), 0, "恰好到位仍是点按期");
    assert_eq!(t.moved(490.0), 0);
    assert!(t.was_tap(), "没过阈值必须是点按");
}

#[test]
fn spec_越阈拖动出滚动不算点按() {
    // 一口气拖过阈值：进入滚动模式，松手不许当点按（不弹键盘）
    let mut t = TouchScroll::new(500.0, CELL);
    t.moved(500.0 + TAP_SLOP_PX + 1.0);
    assert!(!t.was_tap(), "越过阈值后松手不许当点按");
}

#[test]
fn spec_方向_向下拖看历史() {
    // 自然滚动：手指向下（y 增大）= 看更老的输出 = 行数为正
    // （alacritty Scroll::Delta 正数 = display_offset 增大）
    let mut t = TouchScroll::new(500.0, CELL);
    let lines = t.moved(500.0 + CELL * 3.0);
    assert_eq!(lines, 6, "向下拖三格高×增益2 = +6 行");
    let mut u = TouchScroll::new(500.0, CELL);
    let lines = u.moved(500.0 - CELL * 2.0);
    assert_eq!(lines, -4, "向上拖两格高×增益2 = -4 行");
}

#[test]
fn spec_余数挂账_慢拖累计成行() {
    // 病灶候选：每次 moved 各自取整会吞掉半行零头，慢速滚动永远出不了行。
    // 契约：三次 0.5 行（15px）的下拖 = 1 行 + 余数继续挂
    let mut t = TouchScroll::new(500.0, CELL);
    let half = CELL / 2.0; // 15px < TAP_SLOP？不——阈值内不滚！先一把越阈
    // 先越阈进入滚动模式（越阈那一下的位移也计入；增益 2：指 25px = 页 50px）
    let l0 = t.moved(500.0 + TAP_SLOP_PX + 1.0);
    assert_eq!(l0, 1, "指 25px×2=50px → 1 行（余数挂账 20px）");
    assert_eq!(
        t.moved(500.0 + TAP_SLOP_PX + 1.0 + half),
        1,
        "再拖指 15px（页 30px）：20+30=50 → 1 行（余 20）"
    );
    assert_eq!(
        t.moved(500.0 + TAP_SLOP_PX + 1.0 + half + half),
        1,
        "再 15px：20+30=50 → 1 行（余 20）"
    );
    assert_eq!(
        t.moved(500.0 + TAP_SLOP_PX + 1.0 + half + half + half),
        1,
        "再 15px：同上 → 1 行（余 20 不漂）"
    );
}

#[test]
fn spec_拖下去再拖回来净滚动为零() {
    // 下拉 3 行再上拉 3 行：净位移 0，累计滚动也必须回到 0（余数符号不漂）
    let mut t = TouchScroll::new(500.0, CELL);
    let down = t.moved(500.0 + CELL * 3.0);
    let up = t.moved(500.0);
    assert_eq!(down, 6);
    assert_eq!(up, -6, "回原位必须是 -6 行，净滚动归零");
}

#[test]
fn spec_越阈当刻的位移也计入滚动() {
    // 契约细节：越阈那一下不许白吞——25px 越阈位移要挂进余数，
    // 否则「刚越阈就松手再慢拖」的手感会缺一段
    let mut t = TouchScroll::new(500.0, CELL);
    assert_eq!(
        t.moved(500.0 + TAP_SLOP_PX + CELL),
        3,
        "越阈位移必须计入：指 55px×2=110px → 3 行"
    );
}

#[test]
fn spec_滚轮序列_sgr编码() {
    // 鼠标上报模式下滚屏翻成 SGR 1006 滚轮事件发 PTY：
    // 看历史（手指下拖）= wheel up = button 64；看最新 = wheel down = 65。
    // 格式 ESC [ < btn ; col ; row M，坐标 1-based（终端协议惯例）
    use kfm_na::scroll::wheel_seq;
    assert_eq!(wheel_seq(true, 1, 1), "\x1b[<64;1;1M");
    assert_eq!(wheel_seq(false, 1, 1), "\x1b[<65;1;1M");
    assert_eq!(wheel_seq(true, 72, 40), "\x1b[<64;72;40M");
}

// ---------- 像素级滚动（2026-09-24 用户拍板「下面的方向就是滚动的像素级」） ----------
// 契约：moved_px 与 moved 同一只 slop 门，但不取整不挂账——位移原样
// 出 px（带符号）。旧行级通道 moved 一行不动（降级保底，设置里可切回）

#[test]
fn spec_像素滚动_moved_px零头原样不取整() {
    let mut t = TouchScroll::new(500.0, CELL);
    // slop 门同尺：阈值内零位移
    assert_eq!(t.moved_px(505.0), 0.0);
    assert_eq!(t.moved_px(500.0 + TAP_SLOP_PX), 0.0, "恰好到位仍是点按期");
    assert!(t.was_tap());
    // 越阈后：位移原样出——半行零头不取整不挂账（moved 在这里出 0 行）
    let d = t.moved_px(500.0 + TAP_SLOP_PX + 15.5);
    assert_eq!(
        d,
        15.5 * DRAG_GAIN,
        "越阈后第一笔位移×增益出（slop 段不计入，同 moved）"
    );
    assert!(!t.was_tap());
    // 连续小步：每次出当次位移×增益，无累计无吞零
    assert_eq!(
        t.moved_px(500.0 + TAP_SLOP_PX + 15.5 + 3.25),
        3.25 * DRAG_GAIN
    );
    // 反向：负位移×增益出
    assert_eq!(t.moved_px(500.0), -(TAP_SLOP_PX + 18.75) * DRAG_GAIN);
}

#[test]
fn spec_像素滚动_双通道互不污染() {
    // 同一只状态机的两条通道：slop/last_y 共享——行级保底与像素级
    // 切换发生在两次触摸之间（设置页里点开关），一次触摸内不换道
    let mut t = TouchScroll::new(500.0, CELL);
    assert_eq!(
        t.moved(500.0 + TAP_SLOP_PX + CELL),
        3,
        "行级通道照旧（增益 2：54px→108→3 行）"
    );
    let mut u = TouchScroll::new(500.0, CELL);
    let d = u.moved_px(500.0 + TAP_SLOP_PX + CELL);
    assert_eq!(d, CELL * DRAG_GAIN, "像素通道同位移×增益出 px");
}

// ---------- BAR-151 滚轮路余数挂账（2026-09-24 仪器定罪：tmux 滚动 ticks 恒 0） ----------
// 病灶：像素通道滚轮换算逐事件 trunc——慢拖每笔位移 <cell_h 恒 0 tick，
// 余数全吞（真机实录 d=16/20/20 ticks=0，tmux 滚动整只哑掉）。
// 契约：滚轮 tick 换算必须带余数挂账——与行级 moved 同一把尺：
// 半行半行慢拖必须累计成 tick；往返净位移为零 → 净 tick 为零。

#[test]
fn spec_bar151_滚轮挂账_慢拖累计成tick() {
    let mut t = TouchScroll::new(500.0, CELL);
    // 越阈：指 30px（slop 24 不计入，第一笔有效 6px×增益2=12px 页位移）
    assert_eq!(t.wheel_ticks(530.0, CELL), 0, "12px 不足一行不许出 tick");
    // 每笔指 +10px（页 +20px）：12→32→22→42→32——过一行即出 tick
    assert_eq!(
        t.wheel_ticks(540.0, CELL),
        1,
        "累计 32px 过一行出 1 tick（余 2）"
    );
    assert_eq!(t.wheel_ticks(550.0, CELL), 0);
    assert_eq!(t.wheel_ticks(560.0, CELL), 1);
    assert_eq!(t.wheel_ticks(570.0, CELL), 1);
    assert_eq!(t.wheel_pending(), 2.0, "零头挂账不吞（12+20×4−90=2）");
}

#[test]
fn spec_bar151_滚轮挂账_往返净tick为零() {
    let mut t = TouchScroll::new(500.0, CELL);
    let mut net = 0;
    // 下去指 99px（slop 24 不计 → 有效 75px × 增益 2 = 页 150px = 恰好
    // 5 tick 零头归零）
    for y in [530.0, 560.0, 599.0] {
        net += t.wheel_ticks(y, CELL);
    }
    // 回拖指 75px（页 −150px，与下去的挂账借还对称）：净计零 = 恰好
    // 净 0 tick 且零头归零——吞零头/造 tick 都会让净账或零头对不上
    for y in [564.0, 534.0, 524.0] {
        net += t.wheel_ticks(y, CELL);
    }
    assert_eq!(net, 0, "往返借还对称必须恰好净 0 tick（挂账造假即破）");
    assert_eq!(t.wheel_pending().abs(), 0.0, "往返后零头必须归零");
}

#[test]
fn spec_bar151_滚轮挂账_slop段不计入() {
    // 点按嫌疑期（slop 内）的位移一滴都不许进滚轮挂账——
    // 否则轻点带微抖也攒零头，下一次真拖起步就白送 tick
    let mut t = TouchScroll::new(500.0, CELL);
    assert_eq!(t.wheel_ticks(510.0, CELL), 0);
    assert_eq!(t.wheel_ticks(520.0, CELL), 0);
    assert_eq!(t.wheel_pending(), 0.0, "slop 段位移不许进挂账");
    assert!(t.was_tap(), "全程没过阈值仍是点按");
}

// ---------- 惯性甩尾（2026-09-25「工业级滑动」，参数直译 kfmv4
// canvas-scroll.ts 实测手感；答案 src/scroll.rs Fling + moved_px_at）----------

use kfm_na::scroll::{
    FLING_BOOST, FLING_DECAY, FLING_FRAME_MS, FLING_START_MIN, FLING_STOP_MIN, Fling,
};

#[test]
fn spec_惯性甩尾_速度采样公式() {
    // kfmv4 直译：vel = 事件位移/事件间隔 × 帧尺 × 增益（逐事件瞬时采样）
    let mut t = TouchScroll::new(500.0, CELL);
    // 越阈第一笔：off=30 超 slop 24，d=6，dt=16.667ms
    let d0 = t.moved_px_at(530.0, FLING_FRAME_MS);
    assert_eq!(d0, 6.0 * DRAG_GAIN, "slop 段不计入位移（有效位移吃增益）");
    // 第二笔：指 30px（页 60），dt=16.667ms → vel = 60×BOOST
    //（甩尾初速跟随拖滚增益——同一根手指同一种快）
    t.moved_px_at(560.0, FLING_FRAME_MS * 2.0);
    let f = t.fling_on_release().expect("快甩必须出甩尾");
    assert!(
        (f.velocity() - 60.0 * FLING_BOOST).abs() < 1e-9,
        "采样公式必须是 d/dt×帧尺×增益，实得 {}",
        f.velocity()
    );
}

#[test]
fn spec_惯性甩尾_点按与低速不甩() {
    // 点按（未越阈）：None
    let mut t = TouchScroll::new(500.0, CELL);
    t.moved_px_at(505.0, 10.0);
    assert!(t.fling_on_release().is_none(), "点按不许甩");
    // 低速爬行（指 1px/200ms，页 2px/200ms ≈ 0.28px/帧 < 启动阈 0.5）：None
    let mut u = TouchScroll::new(500.0, CELL);
    u.moved_px_at(530.0, 0.0); // 越阈笔（dt=0 不采样）
    u.moved_px_at(531.0, 200.0);
    let v = 2.0 / 200.0 * FLING_FRAME_MS * FLING_BOOST;
    assert!(v < FLING_START_MIN, "考题前提：本速必须低于启动阈");
    assert!(u.fling_on_release().is_none(), "低速松手不许甩");
}

#[test]
fn spec_惯性甩尾_停滞杀速() {
    // 真实手感死穴：快速拖后按住停顿再松手 = 不许甩（末笔 d=0 把速度
    // 采样清零）。旧式「速度窗平均」机在这里会误判出甩尾
    let mut t = TouchScroll::new(500.0, CELL);
    t.moved_px_at(530.0, 0.0);
    t.moved_px_at(560.0, FLING_FRAME_MS); // 快甩一笔 30px/帧
    t.moved_px_at(560.0, 300.0); // 按住不动 ~283ms
    assert!(
        t.fling_on_release().is_none(),
        "按住停顿后松手必须不甩（停滞杀速）"
    );
}

#[test]
fn spec_惯性甩尾_衰减燃尽与位移账() {
    // v=10px/帧出发：逐帧 ×FLING_DECAY，燃尽阈 0.3。总位移账 = 等比级数
    // ≈ v/(1-DECAY)=250px（±一帧尾），且 |v| 全程单调降
    let expect_total = 10.0 / (1.0 - FLING_DECAY);
    let mut f = Fling::new(10.0);
    let mut total = 0.0;
    let mut last_v = f.velocity().abs();
    let mut frames = 0;
    loop {
        frames += 1;
        assert!(frames < 1000, "衰减机必须燃尽不许永动");
        match f.step(FLING_FRAME_MS) {
            Some(d) => {
                total += d;
                let v = f.velocity().abs();
                assert!(v < last_v, "速度必须单调衰减");
                last_v = v;
            }
            None => {
                assert!(
                    f.velocity().abs() < FLING_STOP_MIN,
                    "燃尽时速度必须低于阈，实得 {}",
                    f.velocity()
                );
                break;
            }
        }
    }
    assert!(
        (total - expect_total).abs() < 15.0,
        "等比位移账必须 ≈ v/(1-DECAY)={expect_total:.0}px，实得 {total}"
    );
    // dt≤0 = 零位移零衰减（同帧多圈防御）
    let mut g = Fling::new(5.0);
    assert_eq!(g.step(0.0), Some(0.0));
    assert_eq!(g.velocity(), 5.0);
}

#[test]
fn spec_惯性甩尾_时间切片等比折帧() {
    // 4ms 降频泵一圈 ≠ 16.667ms 一帧：step(2 帧) 的位移+衰减必须
    // ≈ 两次 step(1 帧) 的合成（等比折帧，不许按调用次数衰减）
    let mut a = Fling::new(10.0);
    let da = a.step(FLING_FRAME_MS * 2.0).unwrap();
    let mut b = Fling::new(10.0);
    let db = b.step(FLING_FRAME_MS).unwrap() + b.step(FLING_FRAME_MS).unwrap();
    assert!(
        (da - db).abs() < 1e-9,
        "双帧一步({da}) 必须等于单帧两步({db})"
    );
    assert!(
        (a.velocity() - b.velocity()).abs() < 1e-9,
        "折帧后速度必须一致"
    );
}

// ---------- BAR-158 切入方向闸（2026-09-26 field-reports 实录定罪：
// 追底态「切入(推流画布) 补滚 -167.1px → 触底自动回 live」成对连发 =
// 追底朝 live 方向拖动误入浏览态，切入即闪退 = 页面闪一下）----------
// 契约：浏览挂账只许朝历史方向（d>0）净积压；朝 live 方向的位移钳到 0，
// 调用方只在挂账 >0 时才许切入浏览。答案 src/scroll.rs browse_pending_gate

#[test]
fn spec_bar158_追底朝live位移不挂账() {
    use kfm_na::scroll::browse_pending_gate;
    // 追底态（挂账 0）朝 live 方向的任意位移：挂账恒 0，永不转正
    assert_eq!(browse_pending_gate(0.0, -1.0), 0.0);
    assert_eq!(
        browse_pending_gate(0.0, -11322.6),
        0.0,
        "实录同款大负位移也不许挂账"
    );
    // 零位移不动账
    assert_eq!(browse_pending_gate(0.0, 0.0), 0.0);
}

#[test]
fn spec_bar158_朝历史净积压转正才切入() {
    use kfm_na::scroll::browse_pending_gate;
    // 朝历史方向正常挂账（切入补滚语义不动）
    assert_eq!(browse_pending_gate(0.0, 42.0), 42.0);
    // 已积压后反向：净额照扣，扣穿 0 钳底不翻负
    assert_eq!(browse_pending_gate(42.0, -10.0), 32.0);
    assert_eq!(
        browse_pending_gate(42.0, -100.0),
        0.0,
        "反向扣穿钳 0，不许留负账诱捕切入"
    );
    // 混合手势：先朝 live 后朝历史——负段被钳后正段从 0 起算
    let p = browse_pending_gate(0.0, -30.0);
    assert_eq!(browse_pending_gate(p, 15.0), 15.0);
}
