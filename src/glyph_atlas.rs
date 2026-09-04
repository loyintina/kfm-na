//! glyph_atlas.rs — 期 1 第 2 层：字形图集 + 网格→GPU 实例转换（A 档，纯逻辑）
//!
//! 分层纪律：本模块零平台依赖（无 glow/winit）——图集装箱、实例数据
//! 布局、网格语义（两遍制/宽字符/spacer/裁剪）全是数据，考题在
//! tests/glyph_atlas_spec.rs（考题先行 + 变异抽检）。GL 上传与绘制
//! 在壳侧（android_app/gles_present），按这里的布局字节直传。
//!
//! 忠实性契约（与 termview::draw_glyph 逐像素对拍为验收）：
//! - 字形放置：left = px + xmin，top = py + off_y（off_y = baseline_off
//!   - ymin - height，由调用方按字体路由算好传入）；
//! - 裁剪：右侧 clip_w（宽字符 2 格）→ 实例四边形宽度钳到
//!   clip_right - left（UV 同步钳）；左/上/下越界交视口裁（等价）；
//! - 两遍制：全部背景实例在前、全部字形实例在后（2026-08-21 两遍制
//!   契约沿用——宽字符 spacer 底色不许盖掉半边字形）；
//! - coverage 8bit 原样进图集（R8 纹理 + NEAREST 采样 1:1 → 覆盖率
//!   逐字节同源，混合差异只剩 CPU 整数舍入 vs GPU 浮点的 ±1/255）。

use std::collections::HashMap;

/// 图集键：字体槽 + 字符（0 = 主字体，1 = CJK 备用；字体路由归调用方）
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct GlyphKey {
    pub font: u8,
    pub c: char,
}

/// 图集中一个字形的落位与相对格原点的偏移
#[derive(Clone, Copy, Debug)]
pub struct GlyphSlot {
    pub page: u16,
    /// 图集页内像素坐标（左上角）
    pub u0: u16,
    pub v0: u16,
    pub w: u16,
    pub h: u16,
    /// 相对格原点的横向偏移（= xmin，可为负：斜体左探）
    pub off_x: i16,
    /// 相对格原点（格顶）的纵向偏移（= baseline_off - ymin - height，
    /// 可为负：高字形上探）
    pub off_y: i16,
}

/// 一页图集：R8 coverage，行架式（shelf）装箱
#[derive(Debug)]
pub struct AtlasPage {
    pub w: u32,
    pub h: u32,
    pub coverage: Vec<u8>,
    /// 当前行架顶（页内 y）
    shelf_y: u32,
    /// 当前行架高（行内最高字形）
    shelf_h: u32,
    /// 当前行架游标（页内 x）
    cursor_x: u32,
}

impl AtlasPage {
    fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            coverage: vec![0; (w * h) as usize],
            shelf_y: 0,
            shelf_h: 0,
            cursor_x: 0,
        }
    }
}

/// 字形图集：同键只光栅化一次（第 2 层的存在意义——现在每帧每字都
/// fontdue.rasterize，AI 流式刷屏时一帧几百次）
#[derive(Debug)]
pub struct GlyphAtlas {
    pages: Vec<AtlasPage>,
    slots: HashMap<GlyphKey, GlyphSlot>,
}

impl GlyphAtlas {
    pub fn new(page_w: u32, page_h: u32) -> Self {
        Self {
            pages: vec![AtlasPage::new(page_w.max(1), page_h.max(1))],
            slots: HashMap::new(),
        }
    }

    pub fn slot(&self, key: &GlyphKey) -> Option<GlyphSlot> {
        self.slots.get(key).copied()
    }

    pub fn pages(&self) -> &[AtlasPage] {
        &self.pages
    }

    /// 装载一个字形（调用方已完成字体路由与光栅化）。同键幂等：已存在
    /// 原样返回，不重占位。w/h 为 0 的空字形调用方自滤（draw_glyph 同规）。
    /// 单字形超过整页尺寸 = 契约违约（终端字形 ≤2 格宽，页 2048 起）。
    pub fn insert(
        &mut self,
        key: GlyphKey,
        w: u32,
        h: u32,
        bitmap: &[u8],
        off_x: i16,
        off_y: i16,
    ) -> GlyphSlot {
        if let Some(s) = self.slots.get(&key) {
            return *s;
        }
        assert!(w >= 1 && h >= 1, "空字形不该进图集（调用方自滤）");
        assert!(
            w <= self.pages[0].w && h <= self.pages[0].h,
            "字形超过整页尺寸"
        );
        assert_eq!(
            bitmap.len(),
            (w * h) as usize,
            "coverage 位图长度与 w*h 不符"
        );
        let (mut page_i, mut cursor_x, mut shelf_y, mut shelf_h) = {
            let p = self.pages.last().unwrap();
            (self.pages.len() - 1, p.cursor_x, p.shelf_y, p.shelf_h)
        };
        // 行架推进：行内放不下 → 换行；列放不下 → 翻页
        if cursor_x + w > self.pages[page_i].w {
            cursor_x = 0;
            shelf_y += shelf_h;
            shelf_h = 0;
        }
        if shelf_y + h > self.pages[page_i].h {
            self.pages
                .push(AtlasPage::new(self.pages[page_i].w, self.pages[page_i].h));
            page_i += 1;
            cursor_x = 0;
            shelf_y = 0;
            shelf_h = 0;
        }
        {
            let page = &mut self.pages[page_i];
            for gy in 0..h {
                let dst = ((shelf_y + gy) * page.w + cursor_x) as usize;
                let src = (gy * w) as usize;
                page.coverage[dst..dst + w as usize]
                    .copy_from_slice(&bitmap[src..src + w as usize]);
            }
            page.cursor_x = cursor_x + w;
            page.shelf_y = shelf_y;
            page.shelf_h = shelf_h.max(h);
        }
        let slot = GlyphSlot {
            page: page_i as u16,
            u0: cursor_x as u16,
            v0: shelf_y as u16,
            w: w as u16,
            h: h as u16,
            off_x,
            off_y,
        };
        self.slots.insert(key, slot);
        slot
    }
}

/// 网格格子的 GPU 中立镜像（render_into 收集段的纯数据版：颜色决策
/// ——INVERSE/选择高亮/光标 swap——归收集方，这里只见结果色）
#[derive(Clone, Copy, Debug)]
pub struct GpuCell {
    pub px: u32,
    pub py: u32,
    pub fg: u32,
    pub bg: u32,
    pub c: char,
    /// 宽字符首格（clip_w = 2 格）
    pub wide: bool,
    /// 宽字符第二格（不落墨）
    pub spacer: bool,
}

/// 背景四边形实例（XRGB 颜色在着色器里展开，省 12 字节/实例）
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BgInstance {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: u32,
}

/// 字形四边形实例（UV 归一化坐标；右/下裁剪已折进 w/h/du/dv；
/// page = 图集页号——每页一次 draw，页内实例连续）
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphInstance {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub u0: f32,
    pub v0: f32,
    pub du: f32,
    pub dv: f32,
    pub fg: u32,
    pub page: u32,
}

/// 实例化产出：两遍制（背景块在前、字形块在后）+ 图集未命中名单
/// （调用方补装载后重生成——流式输出时命中率高，二次生成是零成本路径）
pub struct Instanced {
    pub bg: Vec<BgInstance>,
    pub glyph: Vec<GlyphInstance>,
    pub misses: Vec<GlyphKey>,
}

/// 网格 → GPU 实例（与 render_into 两遍制逐语义对齐）：
/// - 背景：bg != default_bg 才落实例（含 spacer——宽字符底色整字覆盖）；
/// - 字形：paintable && !spacer（空格/控制符不落墨，BAR-015）；
/// - 宽字符 clip_w = 2 格，右侧裁剪折进四边形宽与 UV；
/// - 图集未命中：不落字形实例，记入 misses（背景照出——两遍制下
///   字形层缺席不伤背景）。
pub fn grid_to_instances(
    cells: &[GpuCell],
    atlas: &GlyphAtlas,
    cell_w: u32,
    cell_h: u32,
    default_bg: u32,
    slot_of: impl Fn(char) -> (GlyphKey, Option<GlyphSlot>),
) -> Instanced {
    let mut out = Instanced {
        bg: Vec::new(),
        glyph: Vec::new(),
        misses: Vec::new(),
    };
    // 第一遍：背景
    for cell in cells {
        if cell.bg != default_bg {
            out.bg.push(BgInstance {
                x: cell.px as f32,
                y: cell.py as f32,
                w: cell_w as f32,
                h: cell_h as f32,
                color: cell.bg,
            });
        }
    }
    // 第二遍：字形
    for cell in cells {
        if cell.spacer || !crate::termview::paintable(cell.c) {
            continue;
        }
        // 字体路由（主/CJK）归调用方闭包——prefer_cjk 语义在 termview，
        // 这里只认路由键与图集命中；未命中记键，调用方补装载后重生成
        let (key, slot) = slot_of(cell.c);
        let Some(slot) = slot else {
            out.misses.push(key);
            continue;
        };
        let page = &atlas.pages()[slot.page as usize];
        let clip_right = cell.px as i64
            + if cell.wide {
                i64::from(cell_w) * 2
            } else {
                i64::from(cell_w)
            };
        let left = cell.px as i64 + i64::from(slot.off_x);
        // 四边形宽 = min(字形宽, clip_right - left)，UV 同步钳（CPU 版的
        // 逐像素 x>=clip_right 裁剪等价成几何裁剪）
        let draw_w = ((slot.w as i64).min(clip_right - left)).max(0);
        if draw_w == 0 {
            continue;
        }
        out.glyph.push(GlyphInstance {
            x: left as f32,
            y: (cell.py as i64 + i64::from(slot.off_y)) as f32,
            w: draw_w as f32,
            h: f32::from(slot.h),
            u0: f32::from(slot.u0) / page.w as f32,
            v0: f32::from(slot.v0) / page.h as f32,
            du: draw_w as f32 / page.w as f32,
            dv: f32::from(slot.h) / page.h as f32,
            fg: cell.fg,
            page: u32::from(slot.page),
        });
    }
    out
}
