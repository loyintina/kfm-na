# md 渲染器契约（BAR-169，2026-09-27 研究线工单一期）

> 首个消费者 = **会话池查看器正文**（用户痛点原话：会话池的会话、信箱
> 的显示文字不好读、行距小）。demo 页改造排二期（本单一行不动 demo
> 页）。**动渲染器前必读本文 + theme.md §2.5/§三。**

## 三层形制（分层纪律：①②核心纯逻辑零平台依赖，③壳）

1. **解析核心层** `src/ui/md_parse.rs`（A 档）——六样子集手写解析，
   **零新依赖红线**（不许引 pulldown 类 crate）。产出走 `MdSink`
   事件口（**渲染目标藏 trait**：排版层是消费端①，demo 页二期是②，
   不许为某个消费者写死）。块型语义对齐 `demo_page::BlockKind`。
2. **排版层** `src/ui/md_layout.rs`（A 档）——块流 → 折行几何。
   **尺子单源对齐 demo_page**：行高 = ratio 倍字号上取整咬半格网
   （`line_h_styled` ≡ `demo_page::line_h` 缺参逐值相等有钉）、块隙
   BLOCK_GAP、缩进档/框量全读 demo_page 常量表。**折行条款**：正文/
   标题按内容宽（px）折行，量宽走 `MdMeasure` 通路（壳 =
   `TermView::text_width` 真字尺，含 `Box<dyn TermEmu>` dyn 转发——
   cfg 盲区件）；H1 横带随每行字宽（行宽实量存 `MdLine.w`，涂装
   不重算）；代码围栏不折行（涂装右缘断墨）。
3. **绘制层** `src/ui/md_paint.rs`（壳，`impl TermView`）——涂装
   规格 = theme.md §2.5 淡彩六色家族 + §三 md 条款：H1 [ 半包框
   （逐行横带随字宽，首行横带贴块顶）+ 淡彩 slot0 双绘 / H2H3 淡彩
   slot4/5 / H4 淡彩 slot1 双绘、H5 白 0.75、H6 白 0.5 / 代码围栏
   值框（paint_thin_frame 共享件）/ 引用左竖线 2px / 列表 ▪ 8px
   方块只挂项首行（`MdLine.item_start`）/ 分隔线 3px。**demo 页
   涂装即打样规格**，本册是同配方的数据驱动版。

## 解析取舍定案（考题钉死，改动 = 修宪）

- 段落 = 连续非空行一块，**每源行独立折行**（不跨行拼接——CJK
  信件换行即语义换行）；空行分块；空文档 = 一行空正文占位不塌
- 围栏 ``` 开闭成块，**围栏内一切字面**（星号/井号/反引号/引用/
  列表全不解析）；未闭合围栏 = 余下全文归代码块（信件容错不炸）
- 分隔线 = 整行 ≥3 个 `-`（先判分隔线再判列表——`---` 不是列表项）
- ATX：`#`×1-6 + 空格/行尾 = 标题；`#`×7+ / `#` 后无空格 = 正文
- 行内：`**粗体**` 与 `` `码` `` 左到右扫描、反引号优先；未闭合
  标记 = 字面；空对 `****` = 字面；**标记内不嵌套解析**
- 段落中行出现块起手（标题/引用/列表/围栏/分隔线）= 段落断、新块起

## 查看器换芯（眼手同尺链）

- 正文 = md 文档，题注「内容」带退役；卡高/滚动上限吃 md 排版
  `total_h`：modal.rs 新增内容高直喂版 `viewer_card_rect_h` /
  `viewer_scroll_max_h` / `viewer_content_w`（旧字段版委托等价，
  构造保证，钉两版不漂）
- 壳四处同读一份排版：涂装（paint_viewer_card）/ 拖动滚动 / 甩尾
  帧泵 / 抬手命中——汇 `viewer_md_layout` helper（**锁序红线
  term→cfg_page**：先取正文快照放锁，排版完回锁；倒持 = 死锁）
- 滚动 = BAR-167 ①不变（文档原点 = 字段区顶 − scroll，纵裁剪
  [vp_top, ink_bottom) 双裁纪律）；惯性甩尾 scroll.rs 物理机不动
- **VeilSig 补两维**（GLES 烘焙）：viewer_scroll（BAR-167 漏维补——
  redroid 兜底路逐帧原位涂装掩盖了它，GLES 路径漏维 = 滚动不重烘
  鬼影）+ md_style（样式变 = 版面变必须重烘）

## 渲染设置卡（设置页「渲染字号」「渲染行距」两行）

- 两旋钮：字号基准档位 32/36/44px、行距档位 1.30/1.40/1.65
  （档位表单源 settings.rs `MD_FONT_STOPS`/`MD_RATIO_STOPS`）
- 持久化 = 私有目录 `settings/render.json`（**各卡各文件**，写坏
  不连坐 terminal.json）；缺/坏 → 宪法缺省（36/1.40 = 打样规格，
  行为零变化锚）+ 上报不炸
- 生效链：点选 → 写盘 + `md_layout::set_md_style` 全局口即时灌 →
  涂装/滚动上限/命中同读 `md_style()`（VeilSig md_style 维触发重烘）
- 一行一下拉取舍：下拉面版几何只认上池首行（trigger_rect/
  cfg_dropdown_panel_geom 同律），双下拉位是二期活

## 判卷

- 钉 31 枚：md_parse 14（六样边界）+ md_layout 11（尺子咬合/折行/
  样式）+ md_paint 烟雾 3（六样出墨/滚动平移双裁/病态尺寸）+
  modal 2（_h 版等价/content_w）+ settings 1（render.json）+
  接线守卫 1（viewer_fling_wiring_spec spec_bar169）
- 变异九咬全中（解析四 + 排版五；**两例漏杀实录**：H1 框量钉敏感
  带初版选窄被 M7 逃逸 → 补宽钉咬住；BAR-167 已有同款教训）
- redroid 副口实拍：长信滚动 + 六样子集渲染（证据见 bugs.md
  BAR-169 行判卷列）
