//! input_ime_spec.rs — 输入/IME 插件契约考题（A 档）
//!
//! 约束对象：`src/plugins/input_ime.rs` + `src/keybar.rs::ModifierState` +
//! `src/insets.rs::ImeInsets` trait 层。
//! 依据：设计页 `/root/kfmv4/experiments/dsh-na/na/input-ime.md` §8 考题 4-8
//! + 评审回信（方案 A 批准；形态判别准则 v1.2：Sync 内部可变 → 共享实例直挂）。
//!
//! 判卷维度：注册成功 / ModifierState 语义 / 卸载回滚（句柄存活）/
//! reload 状态清零重来 / 注册冲突失败隔离。
//!
//! 形态注记：ModifierState 是具体类型直挂（Sync 原子态，无需 trait 擦除）；
//! ImeInsets 必须 trait（生产=JniInsets 胶水 / 考题=假实现）。

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use kfm_na::base::Ctx;
use kfm_na::base::{Base, FiberState, GetError, Plugin, ServiceKey};
use kfm_na::insets::ImeInsets;
use kfm_na::keybar::{MOD_CTRL, MOD_SHIFT, ModifierState};
use kfm_na::plugins::input_ime::InputIme;

/// 假 insets：返回固定键盘高度，force_show 计数（零 JNI 零平台依赖）
struct FakeInsets {
    px: u32,
    shown: AtomicU32,
}

impl ImeInsets for FakeInsets {
    fn ime_bottom_px(&self) -> Option<u32> {
        Some(self.px)
    }
    fn force_show(&self) {
        self.shown.fetch_add(1, Ordering::Relaxed);
    }
    fn force_hide(&self) {} // 收键盘本科目用不到（期 0④ 加的 trait 方法）
}

fn load_plugin(base: &Base, insets: Arc<FakeInsets>) {
    base.load(InputIme::new(insets)).expect("装载应成功");
    assert_eq!(
        base.state("input-ime"),
        Some(FiberState::Active),
        "apply 只注册两个共享实例，应瞬时 Active"
    );
}

/// 考题 4：注册成功——两服务键均可取回；假 insets 行为符合接口
#[test]
fn spec_注册成功_两服务键可取回() {
    let base = Base::new(vec![]);
    let fake = Arc::new(FakeInsets {
        px: 300,
        shown: AtomicU32::new(0),
    });
    load_plugin(&base, fake.clone());

    let insets = base
        .ctx()
        .get::<dyn ImeInsets>()
        .expect("ime.insets 应可取回");
    assert_eq!(insets.ime_bottom_px(), Some(300));
    insets.force_show();
    assert_eq!(fake.shown.load(Ordering::Relaxed), 1, "force_show 应透传");

    let mods = base
        .ctx()
        .get::<ModifierState>()
        .expect("input.modifiers 应可取回");
    assert_eq!(mods.peek(), 0, "新实例开考无粘滞");
}

/// 考题 5：ModifierState 语义（经服务实例）——一次性粘滞断言集
/// （= 迁移前 spec_修饰键_一次性粘滞 的原样断言）
#[test]
fn spec_修饰键服务_一次性粘滞() {
    let base = Base::new(vec![]);
    load_plugin(
        &base,
        Arc::new(FakeInsets {
            px: 0,
            shown: AtomicU32::new(0),
        }),
    );
    let mods = base.ctx().get::<ModifierState>().unwrap();

    assert_eq!(mods.peek(), 0, "开考必须无粘滞");
    mods.toggle(MOD_CTRL);
    assert_eq!(mods.peek(), MOD_CTRL, "点亮 Ctrl");
    mods.toggle(MOD_SHIFT);
    assert_eq!(mods.peek(), MOD_CTRL | MOD_SHIFT, "双粘滞并存");
    let taken = mods.take();
    assert_eq!(taken, MOD_CTRL | MOD_SHIFT, "take 读走全部");
    assert_eq!(mods.peek(), 0, "take 后必须清零（联动一次自动灭）");
    mods.toggle(MOD_CTRL);
    mods.toggle(MOD_CTRL);
    assert_eq!(mods.peek(), 0, "再点一次必须灭");
}

/// 考题 6：卸载回滚——两键消失（DeclaredButInactive）；存量 Arc 句柄照常
/// （修饰键状态归调用方持有的 Arc，与前两刀「实例归调用方」同判）
#[test]
fn spec_卸载后_两键消失但句柄存活() {
    let base = Base::new(vec![]);
    load_plugin(
        &base,
        Arc::new(FakeInsets {
            px: 100,
            shown: AtomicU32::new(0),
        }),
    );
    let mods = base.ctx().get::<ModifierState>().unwrap();
    let insets = base.ctx().get::<dyn ImeInsets>().unwrap();
    mods.toggle(MOD_CTRL);

    base.unload("input-ime");
    assert!(
        matches!(
            base.ctx().get::<ModifierState>(),
            Err(GetError::DeclaredButInactive(_))
        ),
        "卸载后 input.modifiers 应取不到"
    );
    assert!(
        matches!(
            base.ctx().get::<dyn ImeInsets>(),
            Err(GetError::DeclaredButInactive(_))
        ),
        "卸载后 ime.insets 应取不到"
    );

    assert_eq!(mods.peek(), MOD_CTRL, "存量句柄状态不随插件死");
    assert_eq!(insets.ime_bottom_px(), Some(100), "存量 insets 句柄照常");
}

/// 考题 7：reload 换新实例——修饰键状态清零重来（明示语义：reload = 复位，
/// 符合一次性粘滞的小状态本质）；旧句柄保留自己的位
#[test]
fn spec_reload_修饰键状态清零重来() {
    let base = Base::new(vec![]);
    load_plugin(
        &base,
        Arc::new(FakeInsets {
            px: 0,
            shown: AtomicU32::new(0),
        }),
    );
    let old = base.ctx().get::<ModifierState>().unwrap();
    old.toggle(MOD_CTRL | MOD_SHIFT);

    base.reload("input-ime");
    assert_eq!(base.state("input-ime"), Some(FiberState::Active));
    let new = base.ctx().get::<ModifierState>().unwrap();
    assert_eq!(new.peek(), 0, "reload 后新实例必须零粘滞（复位语义）");
    assert_eq!(
        old.peek(),
        MOD_CTRL | MOD_SHIFT,
        "旧句柄保留自己的位（互不影响）"
    );
}

/// 考题 8：注册冲突——第二插件同键 → Failed 钉死；先到者 Active 且服务不变
/// （占位者注册带非法位印记的实例：若先到者服务被污染，peek 会非 0）
#[test]
fn spec_注册冲突_后者failed且先到者服务不变() {
    struct Squatter;
    impl Plugin for Squatter {
        fn name(&self) -> &'static str {
            "input-modifiers-squatter"
        }
        fn provides(&self) -> Vec<ServiceKey> {
            vec![ServiceKey::of::<ModifierState>()]
        }
        fn apply(&self, ctx: &mut Ctx) -> Result<(), String> {
            // 印记实例：非法位 0x80（正常路径不会置它）
            let tainted = ModifierState::new();
            tainted.toggle(0x80);
            let undo = ctx
                .provide::<ModifierState>(Arc::new(tainted))
                .map_err(|e| format!("占位者注册失败: {e:?}"))?;
            ctx.effect(undo);
            Ok(())
        }
    }

    let base = Base::new(vec![]);
    load_plugin(
        &base,
        Arc::new(FakeInsets {
            px: 0,
            shown: AtomicU32::new(0),
        }),
    );
    base.load(Squatter).expect("load 本身不报错");

    assert!(
        matches!(
            base.state("input-modifiers-squatter"),
            Some(FiberState::Inactive(kfm_na::base::Idle::Failed(_)))
        ),
        "冲突者应钉死 Failed"
    );
    assert_eq!(
        base.state("input-ime"),
        Some(FiberState::Active),
        "先到者不受传染"
    );
    let mods = base.ctx().get::<ModifierState>().unwrap();
    assert_eq!(mods.peek(), 0, "服务仍是先到者的实例（无占位者印记）");
}
