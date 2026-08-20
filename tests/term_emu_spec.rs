//! term_emu_spec.rs — 终端模拟器插件契约考题（A 档）
//!
//! 约束对象：`src/plugins/term_alacritty.rs` + `src/termview.rs` 的 trait 层
//! （`TermEmu`/`TermEmuFactory`/`AlacrittyEmuFactory`）。
//! 依据：设计页 `/root/kfmv4/experiments/dsh-na/na/terminal-emulator.md` §8
//! 考题 4-8 + 评审回信（考题先红、trait 演化纪律注释）。
//!
//! 判卷维度：注册成功 / trait 对象冒烟 / 卸载回滚（实例存活）/ reload 换工厂 /
//! 注册冲突失败隔离。渲染质量判卷不在这里（termview_spec 33 道盯着本体）。
//!
//! 字体夹具：双环境解析同 termview_spec（服务器 /usr/share/fonts，
//! 手机 Termux $PREFIX/share/fonts，同名 DejaVu 文件度量一致）。

use std::sync::Arc;

use kfm_na::base::Ctx;
use kfm_na::base::{Base, FiberState, GetError, Plugin, ServiceKey};
use kfm_na::plugins::term_alacritty::TermAlacritty;
use kfm_na::termview::{self, BuiltTerm, TermEmuFactory};

/// 测试字体夹具（双环境）：两个候选路径喂给工厂，load_font 自取存在的那个
const FIXTURE_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/data/data/com.termux/files/usr/share/fonts/TTF/DejaVuSansMono.ttf",
];

fn load_plugin(base: &Base) {
    base.load(TermAlacritty::with_candidates(FIXTURE_CANDIDATES))
        .expect("装载应成功");
    assert_eq!(
        base.state("term-alacritty"),
        Some(FiberState::Active),
        "apply 只注册不建终端，应瞬时 Active"
    );
}

/// 考题 4：注册成功——ctx 可取回工厂；build 得 Ok 并带回主字体来源名
/// （build 失败=字体全灭走 Err 不算插件失败，评审裁决 3；host 夹具下应 Ok）
#[test]
fn spec_注册成功_工厂可取回且build成() {
    let base = Base::new(vec![]);
    load_plugin(&base);
    let factory = base
        .ctx()
        .get::<dyn TermEmuFactory>()
        .expect("注册表式服务键应可取回");
    let (emu, main_font, _cjk) = factory.build().expect("夹具字体在，build 应成");
    assert!(main_font.contains("DejaVuSansMono"), "主字体来源应上报");
    // 默认占位网格 80x24（build_from_candidates 现状行为锚）
    assert_eq!(emu.cell_size(), (termview::CELL_W, termview::CELL_H));
}

/// 考题 5：trait 对象冒烟——经 Box<dyn TermEmu> feed 文字后 render_into
/// 帧缓冲有非背景像素（对象面全通，不是虚注册）
#[test]
fn spec_trait对象面_feed后渲染出墨() {
    let base = Base::new(vec![]);
    load_plugin(&base);
    let factory = base.ctx().get::<dyn TermEmuFactory>().unwrap();
    let (mut emu, _, _) = factory.build().unwrap();

    emu.resize_cells(5, 2);
    emu.feed(b"hi");
    let (cw, ch) = emu.cell_size();
    let (w, h) = ((cw * 5) as usize, (ch * 2) as usize);
    let mut buf = vec![0u32; w * h];
    emu.render_into(&mut buf, w as u32, h as u32);
    let inked = buf.iter().filter(|&&p| p != termview::DEFAULT_BG).count();
    assert!(inked > 0, "feed 'hi' 后应有字形像素上屏");
}

/// 考题 6：卸载回滚 + 实例存活——unload 后工厂取回失败；
/// 卸载前 build 的终端实例照常 feed/render（终端不随插件死，
/// 状态存活 §7 + 连接 provider 考题 7 先例）
#[test]
fn spec_卸载后_工厂消失但实例存活() {
    let base = Base::new(vec![]);
    load_plugin(&base);
    let factory = base.ctx().get::<dyn TermEmuFactory>().unwrap();
    let (mut emu, _, _) = factory.build().unwrap();

    base.unload("term-alacritty");
    assert!(
        matches!(
            base.ctx().get::<dyn TermEmuFactory>(),
            Err(GetError::DeclaredButInactive(_))
        ),
        "卸载后新调用方应取不到工厂"
    );

    // 存量实例：终端是调用方持有的长寿命 mutable 状态
    emu.feed(b"still alive");
    let (cw, ch) = emu.cell_size();
    let mut buf = vec![0u32; (cw * 80 * ch * 24) as usize];
    emu.render_into(&mut buf, cw * 80, ch * 24);
    assert!(
        buf.iter().any(|&p| p != termview::DEFAULT_BG),
        "卸载后已建终端照常渲染"
    );
}

/// 考题 7：reload 换新工厂——新工厂 build 可用，旧实例不受影响
#[test]
fn spec_reload_新工厂可用_旧实例不受影响() {
    let base = Base::new(vec![]);
    load_plugin(&base);
    let factory_v1 = base.ctx().get::<dyn TermEmuFactory>().unwrap();
    let (mut old, _, _) = factory_v1.build().unwrap();
    old.feed(b"old");

    base.reload("term-alacritty");
    assert_eq!(
        base.state("term-alacritty"),
        Some(FiberState::Active),
        "reload 后应重新 Active"
    );
    let factory_v2 = base.ctx().get::<dyn TermEmuFactory>().unwrap();
    let (mut new, _, _) = factory_v2.build().expect("新工厂应能 build");
    new.feed(b"new");

    // 旧实例照常（独占持有，不依赖工厂闭包）
    old.feed(b" again");
    old.scroll_to_bottom();
    assert!(!old.mouse_report_active(), "普通 shell 不该开鼠标上报");
}

/// 考题 8：注册冲突——第二个同键插件 apply Err → Failed 钉死；
/// 先到者 Active 且服务不变（评审附带发现 1：断言比连接 provider 强一档，
/// 明确「先到者服务不被污染」）
#[test]
fn spec_注册冲突_后者failed且先到者服务不变() {
    struct Squatter;
    impl Plugin for Squatter {
        fn name(&self) -> &'static str {
            "term-emu-squatter"
        }
        fn provides(&self) -> Vec<ServiceKey> {
            vec![ServiceKey::of::<dyn TermEmuFactory>()]
        }
        fn apply(&self, ctx: &mut Ctx) -> Result<(), String> {
            let undo = ctx
                .provide::<dyn TermEmuFactory>(Arc::new(DummyFactory))
                .map_err(|e| format!("占位者注册失败: {e:?}"))?;
            ctx.effect(undo);
            Ok(())
        }
    }
    struct DummyFactory;
    impl TermEmuFactory for DummyFactory {
        fn build(&self) -> Result<BuiltTerm, String> {
            panic!("占位者不该被调用")
        }
    }

    let base = Base::new(vec![]);
    load_plugin(&base);
    base.load(Squatter).expect("load 本身不报错");

    assert!(
        matches!(
            base.state("term-emu-squatter"),
            Some(FiberState::Inactive(kfm_na::base::Idle::Failed(_)))
        ),
        "冲突者应钉死 Failed"
    );
    assert_eq!(
        base.state("term-alacritty"),
        Some(FiberState::Active),
        "先到者不受传染"
    );
    // 先到者服务不变（squatter 的 DummyFactory 一调就 panic）
    let factory = base.ctx().get::<dyn TermEmuFactory>().unwrap();
    let (mut emu, _, _) = factory.build().expect("应仍是先到者的工厂");
    emu.feed(b"proof");
}
