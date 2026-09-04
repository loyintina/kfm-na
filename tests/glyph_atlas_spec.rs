//! glyph_atlas_spec.rs — 期 1 第 2 层 A 档考题：图集装箱 + 网格→实例转换
//!
//! 判卷基准 = termview::draw_glyph 的语义逐条翻译（放置/裁剪/两遍制/
//! 宽字符/spacer/paintable），考题先红后绿；变异抽检纪律同 kfmv4。

use kfm_na::glyph_atlas::{GlyphAtlas, GlyphKey, GpuCell, grid_to_instances};

const CW: u32 = 24;
const CH: u32 = 48;
const PAGE_W: u32 = 128;
const PAGE_H: u32 = 128;
const BG: u32 = 0x0000_0000; // DEFAULT_BG

/// 测试假字形：w×h 覆盖全 0xA7（利于逐字节核对）
fn solid(w: u32, h: u32) -> Vec<u8> {
    vec![0xA7; (w * h) as usize]
}

fn key(c: char) -> GlyphKey {
    GlyphKey { font: 0, c }
}

fn cell(c: char, px: u32, py: u32) -> GpuCell {
    GpuCell {
        px,
        py,
        fg: 0x00FF_FF00,
        bg: BG,
        c,
        wide: false,
        spacer: false,
    }
}

/// 统一路由闭包（主字体单槽，无 CJK）
fn router(
    atlas: &GlyphAtlas,
) -> impl Fn(char) -> (GlyphKey, Option<kfm_na::glyph_atlas::GlyphSlot>) + '_ {
    move |c| {
        let k = key(c);
        (k, atlas.slot(&k))
    }
}

// ---------- 图集装箱 ----------

#[test]
fn spec_atlas_装载后槽位与覆盖逐字节() {
    let mut a = GlyphAtlas::new(PAGE_W, PAGE_H);
    let s = a.insert(key('A'), 8, 16, &solid(8, 16), 2, -3);
    assert_eq!((s.page, s.u0, s.v0, s.w, s.h), (0, 0, 0, 8, 16));
    assert_eq!((s.off_x, s.off_y), (2, -3));
    let page = &a.pages()[0];
    // 覆盖逐字节：行架原点起 16 行 × 8 列全 0xA7
    for gy in 0..16u32 {
        for gx in 0..8u32 {
            assert_eq!(page.coverage[(gy * PAGE_W + gx) as usize], 0xA7);
        }
    }
    // 行架右侧未写区域保持 0
    assert_eq!(page.coverage[16], 0);
}

#[test]
fn spec_atlas_同键幂等_不重占位() {
    let mut a = GlyphAtlas::new(PAGE_W, PAGE_H);
    let s1 = a.insert(key('A'), 8, 16, &solid(8, 16), 0, 0);
    let s2 = a.insert(key('A'), 8, 16, &solid(8, 16), 0, 0);
    assert_eq!(s1.u0, s2.u0);
    assert_eq!(s1.v0, s2.v0);
    // 只占一份：'B' 紧跟 'A' 之后（若重占位会跳到更远）
    let s3 = a.insert(key('B'), 8, 16, &solid(8, 16), 0, 0);
    assert_eq!(s3.u0, s1.u0 + 8);
}

#[test]
fn spec_atlas_行内推进_换行_行架高() {
    let mut a = GlyphAtlas::new(PAGE_W, PAGE_H);
    // 行宽 128：3 个 50 宽字形放得下 2 个（100），第 3 个换行
    let s1 = a.insert(key('甲'), 50, 20, &solid(50, 20), 0, 0);
    let s2 = a.insert(key('乙'), 50, 20, &solid(50, 20), 0, 0);
    let s3 = a.insert(key('丙'), 50, 20, &solid(50, 20), 0, 0);
    assert_eq!((s1.u0, s1.v0), (0, 0));
    assert_eq!((s2.u0, s2.v0), (50, 0));
    assert_eq!((s3.u0, s3.v0), (0, 20)); // 换行：v = 前行架高 20
    // 行架高按行内最高者计：矮字形不推高行架，仍在本行架（v=20）
    let s4 = a.insert(key('丁'), 10, 5, &solid(10, 5), 0, 0);
    assert_eq!(s4.v0, 20);
    // 高字形把本行架高推到 30；宽字形逼出换行 → 新行架顶 = 20 + 30
    let s5 = a.insert(key('戊'), 10, 30, &solid(10, 30), 0, 0);
    assert_eq!(s5.v0, 20);
    let s6 = a.insert(key('己'), 120, 10, &solid(120, 10), 0, 0);
    assert_eq!(s6.v0, 50);
}

#[test]
fn spec_atlas_页满翻页_页号递增() {
    let mut a = GlyphAtlas::new(64, 64);
    // 页高 64、行架高 33：第 2 个必换行（33+33=66>64）→ 翻页
    let s1 = a.insert(key('一'), 64, 33, &solid(64, 33), 0, 0);
    let s2 = a.insert(key('二'), 64, 33, &solid(64, 33), 0, 0);
    assert_eq!(s1.page, 0);
    assert_eq!(s2.page, 1);
    assert_eq!((s2.u0, s2.v0), (0, 0)); // 新页从头
    assert_eq!(a.pages().len(), 2);
}

#[test]
#[should_panic(expected = "超过整页")]
fn spec_atlas_超页字形违约即panic() {
    let mut a = GlyphAtlas::new(64, 64);
    a.insert(key('巨'), 128, 10, &solid(128, 10), 0, 0);
}

// ---------- 网格 → 实例 ----------

fn run(cells: &[GpuCell], atlas: &GlyphAtlas) -> kfm_na::glyph_atlas::Instanced {
    grid_to_instances(cells, atlas, CW, CH, BG, router(atlas))
}

fn atlas_with(c: char, w: u32, h: u32, off_x: i16, off_y: i16) -> GlyphAtlas {
    let mut a = GlyphAtlas::new(PAGE_W, PAGE_H);
    a.insert(key(c), w, h, &solid(w, h), off_x, off_y);
    a
}

#[test]
fn spec_inst_两遍制_背景块整体在前() {
    let a = atlas_with('A', 8, 16, 0, 0);
    let mut c1 = cell('A', 0, 0);
    c1.bg = 0x0022_3333;
    let c2 = cell('A', CW, CH);
    let out = run(&[c1, c2], &a);
    // 背景实例全部产出后才有字形实例：bg[0] 是唯一背景，glyph[0] 是唯一字形
    assert_eq!(out.bg.len(), 1);
    assert_eq!(out.bg[0].color, 0x0022_3333);
    assert_eq!(out.glyph.len(), 2);
}

#[test]
fn spec_inst_缺省背景不落实例() {
    let a = atlas_with('A', 8, 16, 0, 0);
    let out = run(&[cell('A', 0, 0)], &a);
    assert!(out.bg.is_empty());
    assert_eq!(out.glyph.len(), 1);
}

#[test]
fn spec_inst_空格与控制符不落墨_背景照出() {
    let a = atlas_with('A', 8, 16, 0, 0);
    let mut sp = cell(' ', 0, 0);
    sp.bg = 0x0044_4444;
    let mut tab = cell('\t', CW, 0);
    tab.bg = 0x0044_4444;
    let out = run(&[sp, tab], &a);
    assert_eq!(out.bg.len(), 2); // 背景照出
    assert!(out.glyph.is_empty()); // BAR-015：不落墨
}

#[test]
fn spec_inst_spacer不落墨_背景照出() {
    let a = atlas_with('中', 16, 16, 0, 0);
    let mut spacer = cell(' ', CW, 0);
    spacer.spacer = true;
    spacer.bg = 0x0011_1111;
    let out = run(&[spacer], &a);
    assert!(out.glyph.is_empty());
    assert_eq!(out.bg.len(), 1);
}

#[test]
fn spec_inst_放置与裁剪_宽字符两格() {
    // 字形 8 宽在格内：left = px + off_x
    let a = atlas_with('A', 8, 16, 2, -3);
    let out = run(&[cell('A', 100, 200)], &a);
    let g = &out.glyph[0];
    assert_eq!((g.x, g.y), (102.0, 197.0)); // px+off_x, py+off_y
    assert_eq!((g.w, g.h), (8.0, 16.0));
    // 宽字符：clip_right = px + 2*CW，四边形宽不超 2 格
    let a2 = atlas_with('中', 16, 16, 0, 0);
    let mut wide = cell('中', 100, 0);
    wide.wide = true;
    let g2 = &run(&[wide], &a2).glyph[0];
    assert_eq!((g2.x, g2.w), (100.0, 16.0)); // 16 宽 < 48 上限，不触裁
    // 窄格字形 20 宽 > 1 格 24？——20 < 24 不触裁；造一个越界的：
    let a3 = atlas_with('W', 30, 16, 0, 0); // 30 宽 > 单格 24
    let g3 = &run(&[cell('W', 100, 0)], &a3).glyph[0];
    assert_eq!(g3.w, 24.0); // 钳到 clip_right - left = 24
    assert!((g3.du - 24.0 / 128.0).abs() < 1e-6); // UV 同步钳
}

#[test]
fn spec_inst_图集未命中_记键且字形缺席背景不伤() {
    let a = GlyphAtlas::new(PAGE_W, PAGE_H); // 空图集
    let mut c = cell('A', 0, 0);
    c.bg = 0x0055_5555;
    let out = run(&[c], &a);
    assert_eq!(out.bg.len(), 1); // 背景不伤
    assert!(out.glyph.is_empty());
    assert_eq!(out.misses, vec![key('A')]);
    // 补装载后重生成：misses 清空
    let mut a = a;
    a.insert(key('A'), 8, 16, &solid(8, 16), 0, 0);
    let out2 = run(&[c], &a);
    assert!(out2.misses.is_empty());
    assert_eq!(out2.glyph.len(), 1);
}

#[test]
fn spec_inst_uv归一化() {
    // 字形放在行架第二位：u0=8 → 8/128 = 0.0625
    let mut a = GlyphAtlas::new(PAGE_W, PAGE_H);
    a.insert(key('X'), 8, 16, &solid(8, 16), 0, 0);
    a.insert(key('Y'), 8, 16, &solid(8, 16), 0, 0);
    let g = &run(&[cell('Y', 0, 0)], &a).glyph[0];
    assert!((g.u0 - 8.0 / 128.0).abs() < 1e-6);
    assert!((g.du - 8.0 / 128.0).abs() < 1e-6);
    assert!(g.v0.abs() < 1e-6);
    assert!((g.dv - 16.0 / 128.0).abs() < 1e-6);
}
