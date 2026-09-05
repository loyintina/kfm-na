//! ui/prompt_bar.rs — 输入对话栏控件（第 3 层；2026-09-01 自 termview
//! 物理搬移立形：状态核 = input_bar.rs，视图 = 本文件 impl TermView 块，
//! 注入通道 = bar-inject，考题 = input_bar_spec + termview_spec caret/bar
//! 系列，档案 = 插件档案-输入栏.md。搬移为零逻辑变化——逐字节原样）。
use crate::termview::{Frame, GlowSpec, GradSpec, SELECT_BG, VeilSpec, lerp_rgb};

/// 行归属纯函数（BAR-041）：字符位 idx 落在 starts 的第几行——最后一个
/// 起点 ≤ idx 的行（idx=文末 items.len() 自动归末行）。2026-09-01 闪退案
/// 根因修复：原内联双分支把「行数 n」错当「字符数」做边界判定，多行文本
/// 光标位 > n 即切片倒挂 panic
pub fn row_of(starts: &[usize], idx: usize) -> usize {
    let mut k = 0;
    if std::env::var("KFM_DEBUG_BAR").is_ok() {
        eprintln!("[dbg row_of] starts={:?} idx={} → 计算", starts, idx);
    }
    for (kk, &st) in starts.iter().enumerate() {
        if st <= idx {
            k = kk;
        } else {
            break;
        }
    }
    k
}

/// 输入栏正文字号（px，物理像素）= 单行文本区高 156 × 0.26（单行时的
/// 历史配比）。textarea 多行后字号不随行高缩——量宽/折行/画字同用这一把尺
/// 输入栏正文字号（px，物理像素）= 单行文本区高 156 × 0.26（单行时的
/// 历史配比）。textarea 多行后字号不随行高缩——量宽/折行/画字同用这一把尺
/// （模块内私有：调用方只管传 snap.lines，字号是渲染内部事）
const BAR_TEXT_PX: f32 = 156.0 * 0.26;
impl crate::termview::TermView {
    /// 全局输入栏（ai-presence 期 0 组件三，§二 常驻 chrome 一）：
    /// 压底紧贴键盘（keybar 在其上一层——调用方几何保证）。样式 = kfmv4
    /// base.css 逐项复刻（2026-08-31 v2 质感版，不再是截图取色近似——
    /// 直接读 .ai-input-bar/.ai-input/.ai-send-btn 的 CSS 配方）：
    /// 栏带顶部渐变发丝线（紫→青→紫 α0.4）；文本区 135° 渐变描边
    /// （1px 物理 3，左缘 3 倍粗）+ 近黑底 + 顶部内阴影，聚焦 = 紫外
    /// 发光（0 0 20px α0.35）+ 描边提点亮度硬切（零动画帧）；发送钮 =
    /// 135° 渐变 + 顶部玻璃高光（inset 白 0.15）+ 紫色投影
    /// （0 4px 12px α0.3）+ 白 ▶（sending = ⏸ 双竖条，跟 AI 运行态硬切）。
    /// 全部图元 SDF 抗锯齿。
    /// 光标（2026-08-31 浏览器控件行为对齐）：聚焦画竖线光标，caret_on =
    /// 闪烁相位（调用方按 CARET_BLINK_MS 算好传入，渲染纯函数）；定位柄
    /// 蓝色下坠柄跟 snap.handle 走。
    /// textarea（2026-08-31 移动端全量复刻）：带高随行数长（覆盖式悬浮——
    /// 栏带向上浮盖终端底部行，终端网格几何不动）；文本折行（2026-09-04
    /// 起 multiline_starts：'\n' 硬换行 + 行内软折同尺）
    /// 多行绘制，超 MAX_LINES 尾锚显最后几行。**折行数由本函数从
    /// snap.text 实测量出**（渲染几何与所画文本同源——后台 dump 无 poll
    /// 写回也不会带高/文本两张皮，2026-08-31 实拍定罪）；snap.lines 只
    /// 服务触摸命中（前台 poll 写回，眼手同尺）
    #[allow(clippy::too_many_arguments)]
    pub fn render_inputbar(
        &self,
        buf: &mut [u32],
        buf_w: u32,
        buf_h: u32,
        ime_bottom: u32,
        snap: &crate::input_bar::BarSnap,
        sending: bool,
        caret_on: bool,
    ) {
        use crate::input_bar;
        // token 读取（theme.rs 第 2 层）：本函数不许出现字面颜色
        let t = &self.theme.bar;
        let min_w = 2 * input_bar::MARGIN_X_PX + input_bar::GAP_PX + input_bar::SEND_W_PX + 40;
        if buf_w < min_w {
            return; // 窗太窄画不下，保命要紧
        }
        // 量宽折行（画文本也要用，先量一次两头吃）。显示文本 = text 在
        // 光标处拼入组合态（input_bar::display_text 单源，「所见」定义）。
        // 2026-09-04 Enter 换行：items 走 measure_bar_items（'\n' 零宽条目
        // 保 1:1），折行走 multiline_starts（硬换行 + 软折同尺）
        let display = crate::input_bar::InputBarState::display_text(snap);
        let items = self.measure_bar_items(&display, BAR_TEXT_PX);
        let widths: Vec<f32> = items.iter().map(|i| i.2).collect();
        let avail = input_bar::text_avail_w(buf_w).unwrap_or(1.0);
        let chars: Vec<char> = display.chars().collect();
        let starts = input_bar::multiline_starts(&chars, &widths, avail);
        let n_lines = if snap.text.is_empty() {
            1
        } else {
            starts.len() as u32
        };
        let bar_h = input_bar::height_for_lines(n_lines);
        let Some(top) = buf_h
            .checked_sub(ime_bottom)
            .and_then(|b| b.checked_sub(bar_h))
        else {
            return;
        };
        let mut frame = Frame {
            buf,
            w: buf_w,
            h: buf_h,
        };
        // 栏带底 = 半透（BAR-067）：kfmv4 rgba(18,18,26,.85) 还原——
        // 高字节携带 α，GLES 条件 alpha 直通（mark_chrome_alpha），
        // 终端内容 15% 透出；后续装饰 blend_px 保 α 不掉
        frame.fill_rect(
            0,
            top,
            buf_w,
            bar_h,
            (crate::theme::CHROME_BAND_ALPHA << 24) | t.bg,
        );
        // 带顶渐变发丝线（kfmv4 border-image：紫→青→紫 α0.4，3px 物理）
        for py in 0..3u32 {
            for px in 0..buf_w {
                let c = if px < buf_w / 2 {
                    lerp_rgb(t.border_l, t.accent, px * 255 / (buf_w / 2).max(1))
                } else {
                    lerp_rgb(
                        t.accent,
                        t.border_l,
                        (px - buf_w / 2) * 255 / (buf_w / 2).max(1),
                    )
                };
                frame.blend_px(px, top + py, c, 102);
            }
        }
        // 文本区：带内上下各留 32，高随行数长（单行 156）
        let field_h = bar_h - 64;
        let field_top = top + 32;
        let send_left = buf_w - input_bar::MARGIN_X_PX - input_bar::SEND_W_PX;
        let field_left = input_bar::MARGIN_X_PX;
        let field_w = send_left - input_bar::GAP_PX - field_left;
        // 文本区内芯底色 = 横向暗色渐变（2026-08-31 用户实拍指正：kfmv4
        // 内芯不是纯黑——左紫调 (29,23,57) → 右青调 (12,40,54)，是半透明
        // 底叠 backdrop blur 把描边环境色晕进来的效果；取稍沉一档防塑料蓝）
        let (field_bg_l, field_bg_r) = if snap.focused {
            (t.field_focus_bg_l, t.field_focus_bg_r)
        } else {
            (t.field_bg_l, t.field_bg_r)
        };
        // 聚焦 = 紫外发光（kfmv4 focus box-shadow 0 0 20px α0.35）
        if snap.focused {
            frame.glow_round_rect(
                field_left,
                field_top,
                field_w,
                field_h,
                40,
                GlowSpec {
                    color: t.glow,
                    alpha: 89,
                    spread: 24,
                    y_off: 0,
                },
            );
        }
        // 描边：135° 对角渐变（kfmv4 #7c3aed → rgba(0,212,255,0.8)）
        frame.fill_round_rect_grad(
            field_left,
            field_top,
            field_w,
            field_h,
            40,
            GradSpec {
                c1: t.border_l,
                c2: t.border_r,
                diag: true,
            },
        );
        // 内芯：横向暗色渐变底，左缘让 9（3px CSS 加粗描边）其余让 3
        let core_x = field_left + 9;
        let core_y = field_top + 3;
        let core_w = field_w - 12;
        let core_h = field_h - 6;
        frame.fill_round_rect_grad(
            core_x,
            core_y,
            core_w,
            core_h,
            36,
            GradSpec {
                c1: field_bg_l,
                c2: field_bg_r,
                diag: false,
            },
        );
        // 顶部内阴影（kfmv4 inset 0 1px 2px 黑 0.2）
        frame.inner_top_veil(
            core_x,
            core_y,
            core_w,
            core_h,
            36,
            VeilSpec {
                color: 0,
                alpha: 51,
                rows: 4,
            },
        );
        // 文字左内缩 ~58（draw 族自带 18 起笔）
        let text_cx = field_left + 40;
        // 视口（2026-09-01 像素级滚动，BAR-042 跟手拍板）：文本条带在
        // field 内垂直偏移 eff px，内容 1:1 跟手。follow=尾锚（条带底贴
        // 视口底，打字态）；拖动后 scroll_px 钳制固定；条带不足一屏 =
        // 居中不可滚。几何单源 = input_bar::viewport_geometry（点按换算
        // 同吃此函数——眼手同尺）。
        // BAR-049：视口 = field 上下各收 TEXT_PAD_Y 内衬——文字/高亮不贴
        // field 边框（kfmv4 padding 14px CSS ≈ 40 物理），滚到边界的行在
        // 内衬带里被裁掉。field_y0/y1 以下一律指「文本视口」边界。
        let n = starts.len();
        let view_h = input_bar::text_view_h(field_h);
        let (_, eff, top_off) =
            input_bar::viewport_geometry(n as u32, view_h, snap.follow, snap.scroll_px);
        let field_y0 = field_top as i32 + input_bar::TEXT_PAD_Y as i32;
        let field_y1 = (field_top + field_h) as i32 - input_bar::TEXT_PAD_Y as i32;
        let text_top = field_y0 + top_off as i32 - eff;
        let line_y = |k: usize| -> i32 { text_top + k as i32 * input_bar::LINE_STEP_PX as i32 };
        let text_cw = field_w - 40 - 12;
        if display.is_empty() {
            self.draw_text_left(
                &mut frame,
                "输入消息…",
                text_cx,
                text_cw,
                field_top,
                field_h,
                BAR_TEXT_PX,
                t.placeholder,
            );
        } else {
            // textarea 折行：只画与 field 有交集的行，行内逐像素垂直裁剪
            let first = ((field_y0 - text_top).max(0) / input_bar::LINE_STEP_PX as i32) as usize;
            let last = (((field_y1 - text_top) + input_bar::LINE_STEP_PX as i32 - 1)
                / input_bar::LINE_STEP_PX as i32)
                .min(n as i32) as usize;
            for k in first..last {
                let st = starts[k];
                let end = if k + 1 < n {
                    starts[k + 1]
                } else {
                    items.len()
                };
                // 选区高亮（BAR-046）：画在文字底层，只处理 committed text 空间。
                // MVP 假设 item 下标与 char 下标 1:1（无 tofu），与光标/定位柄
                // 现有口径一致；tofu 字场景记诚实边界。
                // BAR-047：高亮矩形裁进文本可视区——文字有 per-pixel 垂直裁剪，
                // 高亮原来什么都没有：半滚出栏顶/底的行拿满行高矩形画出栏框
                // （实拍：长文全选高亮盖穿圆角框），横向也钳到文本区右缘。
                if snap.selecting && snap.selection_start < snap.selection_end {
                    let sel_s = snap.selection_start.max(st);
                    let sel_e = snap.selection_end.min(end).max(sel_s);
                    if sel_s < sel_e {
                        let x0 = items[st..sel_s].iter().map(|i| i.2).sum::<f32>();
                        let x1 = items[st..sel_e].iter().map(|i| i.2).sum::<f32>();
                        let clip_x1 = text_cx + text_cw;
                        let sx0 = (text_cx + 18 + x0 as u32).min(clip_x1);
                        let sx1 = (text_cx + 18 + x1 as u32).min(clip_x1);
                        let ry0 = line_y(k).max(field_y0);
                        let ry1 = (line_y(k) + input_bar::LINE_STEP_PX as i32).min(field_y1);
                        if sx1 > sx0 && ry1 > ry0 {
                            frame.fill_rect(
                                sx0,
                                ry0 as u32,
                                sx1 - sx0,
                                (ry1 - ry0) as u32,
                                t.select_bg,
                            );
                        }
                    }
                }
                self.draw_items_left(
                    &mut frame,
                    &items[st..end.max(st)],
                    text_cx,
                    text_cw,
                    line_y(k) as u32,
                    input_bar::LINE_STEP_PX,
                    BAR_TEXT_PX,
                    t.text,
                    Some((field_y0, field_y1)),
                );
            }
        }
        // 光标（2026-08-31 浏览器控件行为）：聚焦才画，竖线一条，530ms
        // 相位闪烁（caret_on 由调用方算好传入）；点按定位柄 = 蓝色下坠柄
        // （加大版：正方形承载、上尖三角尖对光标行底），稳显不随闪烁。
        // 行滚出视口则光标/柄都不画（不冒充在窗内）
        let comp_len = snap.composing.chars().count();
        let caret_idx = (snap.cursor + comp_len).min(items.len());
        let caret_row_all = row_of(&starts, caret_idx);
        let caret_y = line_y(caret_row_all);
        let caret_fully_visible =
            caret_y >= field_y0 && caret_y + input_bar::LINE_STEP_PX as i32 <= field_y1;
        if snap.focused && caret_fully_visible {
            let row_start = starts[caret_row_all];
            let row_end = if caret_row_all + 1 < n {
                starts[caret_row_all + 1]
            } else {
                items.len()
            };
            let caret_slice_end = caret_idx.min(row_end).max(row_start);
            let x_off: f32 = items[row_start..caret_slice_end].iter().map(|i| i.2).sum();
            let row_cy = caret_y + input_bar::LINE_STEP_PX as i32 / 2;
            let caret_x = text_cx + 18 + x_off as u32;
            if caret_on {
                frame.fill_round_rect(caret_x, (row_cy - 26) as u32, 4, 52, 2, t.text);
            }
            if snap.handle {
                // 定位柄(BAR-042 用户复测版):加大——正方形承载、上尖三角
                // 尖对光标行底;稳显不随闪烁
                // BAR-050 平顶 → BAR-052 一体图钉光栅：斜边(m=21/17)经
                // r=10 肩部切弧过渡立边（钝角圆角，成熟输入法柄同形），
                // 平顶拼接的 3px 接缝台阶消除。外沿/轴不变：尖轴=块轴=
                // caret_x+2，跨 [tip_y-1, tip_y+58]
                let hx = (caret_x + 2).saturating_sub(22);
                let tip_y = (caret_y + input_bar::LINE_STEP_PX as i32 - 2) as u32;
                frame.fill_pin_handle(hx + 22, tip_y - 1, 22, 18, 43, 10, 12, SELECT_BG);
            }
        }
        // 组合态下划线（浏览器 preedit 视觉对齐）：拼音区字底一道品牌青。
        // 稳显不随光标闪烁；组合行滚出视口则不画
        let comp_start = snap.cursor.min(items.len());
        let comp_row_all = row_of(&starts, comp_start);
        let comp_y = line_y(comp_row_all);
        if snap.focused
            && !snap.composing.is_empty()
            && comp_y >= field_y0
            && comp_y + input_bar::LINE_STEP_PX as i32 <= field_y1
        {
            let comp_end = (snap.cursor + comp_len).min(items.len());
            let row_start = starts[comp_row_all];
            let comp_start_clamped = comp_start.max(row_start);
            let comp_end_clamped = comp_end.max(row_start);
            let ux0 = (text_cx + 18) as f32
                + items[row_start..comp_start_clamped]
                    .iter()
                    .map(|i| i.2)
                    .sum::<f32>();
            let ux1 = (text_cx + 18) as f32
                + items[row_start..comp_end_clamped]
                    .iter()
                    .map(|i| i.2)
                    .sum::<f32>();
            let urow_cy = comp_y + input_bar::LINE_STEP_PX as i32 / 2;
            frame.fill_rect(
                ux0 as u32,
                (urow_cy + 22) as u32,
                (ux1 - ux0).max(4.0) as u32,
                4,
                t.accent,
            );
        }
        // 选择锚点柄（BAR-046）：选择模式下画在文字之上；行滚出视口不画。
        // 锚点位置用同一套 row_of/starts/items，与光标/点按换算同源。
        if snap.selecting {
            self.draw_selection_anchor(
                &mut frame,
                snap.selection_start,
                text_cx,
                &starts,
                &items,
                &line_y,
                field_y0,
                field_y1,
                t.select_handle,
                true, // 左锚点尖朝上
            );
            self.draw_selection_anchor(
                &mut frame,
                snap.selection_end,
                text_cx,
                &starts,
                &items,
                &line_y,
                field_y0,
                field_y1,
                t.select_handle,
                false, // 右锚点尖朝上
            );
        }
        // 操作菜单（BAR-046）：选择模式下弹出气泡条；MVP 自绘在栏带内。
        // BAR-049：菜单可见锚也吃文本视口（上下收 TEXT_PAD_Y 内衬后的边界）
        if snap.selecting {
            self.draw_selection_menu(
                &mut frame,
                snap,
                &starts,
                &items,
                text_cx,
                &line_y,
                field_y0 as u32,
                view_h,
                bar_h,
                top,
                buf_w,
            );
        }
        // 发送钮：kfmv4 42×42 方钮 align-self:center——定尺居中，不随行数
        // 拉长（2026-08-31 用户指认「内容多的情况下按钮应该保持不动或者
        // 居中」）。先紫色投影（kfmv4 0 4px 12px α0.3），再 135° 渐变本体，
        // 再顶部玻璃高光（inset 白 0.15），最后白 ▶
        let send_h = 140u32;
        let send_top = top + (bar_h - send_h) / 2;
        frame.glow_round_rect(
            send_left,
            send_top,
            input_bar::SEND_W_PX,
            send_h,
            36,
            GlowSpec {
                color: t.glow,
                alpha: 77,
                spread: 14,
                y_off: 6,
            },
        );
        frame.fill_round_rect_grad(
            send_left,
            send_top,
            input_bar::SEND_W_PX,
            send_h,
            36,
            GradSpec {
                c1: t.send_tl,
                c2: t.send_br,
                diag: true,
            },
        );
        frame.inner_top_veil(
            send_left,
            send_top,
            input_bar::SEND_W_PX,
            send_h,
            36,
            VeilSpec {
                color: 0x00FF_FFFF,
                alpha: 38,
                rows: 3,
            },
        );
        // 发送钮图标二态硬切（kfmv4 .ai-send-btn.sending：▶ ↔ ⏸，
        // 跟 AI 运行态走，零动画帧）
        let icon_cx = send_left + input_bar::SEND_W_PX / 2;
        let icon_cy = send_top + send_h / 2;
        if sending {
            // ⏸：两条竖圆角矩形（与 ▶ 同视觉重心同白）
            frame.fill_round_rect(icon_cx - 23, icon_cy - 27, 15, 54, 7, t.send_tri);
            frame.fill_round_rect(icon_cx + 8, icon_cy - 27, 15, 54, 7, t.send_tri);
        } else {
            frame.fill_triangle_right(icon_cx, icon_cy, 54, t.send_tri);
        }
    }

    /// 量输入栏文本折行数（眼手同尺单源的量宽端：渲染层有字体，android_app
    /// 每帧文本/宽度变化时调用 → InputBarState::set_lines 写回，触摸命中与
    /// dump 读同一份）。buf_w 退化（画不下）按 1 行计。
    /// 2026-09-04 Enter 换行：硬换行计入行数（multiline_starts）
    pub fn bar_text_lines(&self, text: &str, buf_w: u32) -> u32 {
        // 调用方传 display_text(组合态拼入);此处只管量
        if text.is_empty() {
            return 1;
        }
        let Some(avail) = crate::input_bar::text_avail_w(buf_w) else {
            return 1;
        };
        let widths: Vec<f32> = self
            .measure_bar_items(text, BAR_TEXT_PX)
            .iter()
            .map(|i| i.2)
            .collect();
        let chars: Vec<char> = text.chars().collect();
        crate::input_bar::multiline_starts(&chars, &widths, avail).len() as u32
    }

    /// 点按定位换算（2026-08-31 浏览器控件行为对齐；BAR-042 像素滚动
    /// 修订）：文本区本地坐标（左上角原点）→ 光标 char 下标。几何经
    /// viewport_geometry 单源（眼手同尺——旧版按尾窗假设映射，滚离尾锚
    /// 后点按会映射到滚出行，光标不可见）。列向「过半归右」就近取字
    pub fn bar_cursor_at(
        &self,
        snap: &crate::input_bar::BarSnap,
        buf_w: u32,
        x_local: f64,
        y_local: f64,
    ) -> usize {
        use crate::input_bar;
        // 眼手同尺：点按换算与渲染共用 display_text（含组合态），避免
        // 组合态下布局与光标不同源导致命中错位 / 切片倒挂
        let display = crate::input_bar::InputBarState::display_text(snap);
        let items = self.measure_bar_items(&display, BAR_TEXT_PX);
        if items.is_empty() {
            return 0;
        }
        let widths: Vec<f32> = items.iter().map(|i| i.2).collect();
        let avail = input_bar::text_avail_w(buf_w).unwrap_or(1.0);
        let chars: Vec<char> = display.chars().collect();
        let starts = input_bar::multiline_starts(&chars, &widths, avail);
        let n = starts.len();
        let bar_h = input_bar::height_for_lines(n as u32);
        // BAR-049：视口高与渲染同尺（field 上下收 TEXT_PAD_Y 内衬）
        let (_, eff, top_off) = input_bar::viewport_geometry(
            n as u32,
            input_bar::text_view_h(bar_h - 64),
            snap.follow,
            snap.scroll_px,
        );
        // 行:strip 坐标 = field 内 y - 内衬 - 顶留白 + 滚动偏移;行 = strip/行高,
        // 钳 [0, 行数-1](与渲染视口窗同源:viewport_geometry)
        let strip_y = y_local - f64::from(input_bar::TEXT_PAD_Y) - top_off as f64 + eff as f64;
        let k = ((strip_y / f64::from(input_bar::LINE_STEP_PX))
            .floor()
            .max(0.0) as usize)
            .min(n.saturating_sub(1));
        let row_start = starts[k];
        let row_end = if k + 1 < n {
            starts[k + 1]
        } else {
            items.len()
        };
        // 2026-09-04 Enter 换行：行末跟 '\n' 时，点该行右侧空白 = 行尾
        // （'\n' 之前的位置），不是 row_end（下一行首）
        let content_end = if row_end > row_start && items[row_end - 1].1 == '\n' {
            row_end - 1
        } else {
            row_end
        };
        // 列：累计步进宽，过半归右（浏览器 tap 落点就近原则）
        let mut pen = 18.0f32;
        for (i, item) in items[row_start..row_end.max(row_start)].iter().enumerate() {
            if x_local < f64::from(pen + item.2 * 0.5) {
                return row_start + i;
            }
            pen += item.2;
        }
        content_end
    }

    /// 画单个选择锚点柄（BAR-046）：尖朝上，底边贴对应行底。
    /// idx=char 下标；left=true 时尖在水平中心，false 同理。
    #[allow(clippy::too_many_arguments)]
    fn draw_selection_anchor(
        &self,
        frame: &mut Frame,
        idx: usize,
        text_cx: u32,
        starts: &[usize],
        items: &[(&fontdue::Font, char, f32)],
        line_y: &dyn Fn(usize) -> i32,
        field_y0: i32,
        field_y1: i32,
        color: u32,
        _left: bool,
    ) {
        use crate::input_bar;
        let row = row_of(starts, idx.min(items.len()));
        let row_start = starts[row];
        let row_y = line_y(row);
        if row_y + input_bar::LINE_STEP_PX as i32 <= field_y0 || row_y >= field_y1 {
            return; // 行滚出视口不画
        }
        let x_off: f32 = items[row_start..idx.min(items.len()).max(row_start)]
            .iter()
            .map(|i| i.2)
            .sum();
        let ax = (text_cx + 18 + x_off as u32) as i32;
        let tip_y = (row_y + input_bar::LINE_STEP_PX as i32 - 2) as u32;
        // 上尖三角 + 正方承载（与定位柄同族，缩小版）。
        // 2026-09-03 ①号迭代：两图元水平中心统一锚到 ax（原三角 ax+half、
        // 矩形 ax-half+4，静态错位 10px，实拍可见尖与块不对齐）。
        // BAR-051：三角改左缘锚定（ax-half 起恰好 28px），与下方块
        // fill_rect 同跨——旧 cx 闭区间语义底行 29px 恒偏右半像素。
        // BAR-052：三图元拼接 → fill_pin_handle 一体图钉光栅——45° 斜边
        // 经 r=7 肩部切弧过渡立边（钝角圆角，成熟输入法柄同形），平顶
        // 拼接的 2px 接缝台阶消除。外沿/轴不变：柄形合 span
        // y ∈ [tip-1, tip+38]，中心 ≈ tip+18——几何
        // （bar_selection_geometry.anchor_at）以 (ax, tip+18) 为柄中心，
        // 热区同尺，眼手同源
        frame.fill_pin_handle(ax as u32, tip_y.saturating_sub(1), 14, 14, 27, 7, 8, color);
    }

    /// 选区的可见锚 y（BAR-048）：菜单只锚「看得见的选区」——长文全选时
    /// 选区首行早滚出栏顶，菜单追隐藏首行会浮出输入栏往屏顶爬（用户实拍：
    /// 上滚内容菜单自己往页面上方走）。返回 None = 选区整段滚出视口，
    /// 菜单不画、几何不返（眼手同尺：看不见的点不得有触摸热区）。
    /// 钉死钳制（BAR-048 复测防抖）：锚 y 钳进视口边界——选区起点滚在视口
    /// 之上时菜单钉栏顶固定位，不追部分可见行的连续 y（追了就是锯齿：
    /// 行内连续移 63px、跨行跳回 63px，实拍「上下抖动」）。
    fn visible_sel_anchor_y(
        line_y0: i32,
        n_rows: usize,
        field_y0: i32,
        field_y1: i32,
        row_s: usize,
        row_e: usize,
    ) -> Option<(i32, i32)> {
        let step = crate::input_bar::LINE_STEP_PX as i32;
        if n_rows == 0 || line_y0 >= field_y1 {
            return None;
        }
        let first_vis = ((field_y0 - line_y0).max(0) / step) as usize;
        let last_vis = (((field_y1 - 1 - line_y0).max(0) / step) as usize).min(n_rows - 1);
        let s = row_s.max(first_vis);
        let e = row_e.min(last_vis);
        if s > e {
            return None;
        }
        let y_s = (line_y0 + s as i32 * step).max(field_y0);
        let y_e = (line_y0 + e as i32 * step).min(field_y1 - step);
        Some((y_s, y_e))
    }

    /// 选择菜单气泡矩形（BAR-046）：渲染（draw_selection_menu）与触摸几何
    /// （bar_selection_geometry）共用这一把尺——眼手同尺单源，位置规则改了
    /// 只改这里。2026-09-03 ⑤号迭代：弃「选区垂直中心-20」改为贴选区——
    /// 首选首行上方 12px（Android 语境菜单惯例）。气泡是浮层，不受栏带
    /// 上缘钳制（可浮出栏带盖在终端区上），只守屏顶 8px；上方真放不下
    /// （选区滚到屏顶）才翻末行下方 12px，再不行贴栏底。
    fn selection_menu_rect(
        y_s: i32,
        y_e: i32,
        buf_w: u32,
        bar_top: u32,
        bar_h: u32,
    ) -> (u32, u32, u32, u32) {
        let menu_h = crate::input_bar::MENU_H;
        // 窄窗（含 600 宽考题夹具）内缩守边 8px，防菜单越出屏幕
        let menu_w = crate::input_bar::MENU_W.min(buf_w.saturating_sub(16));
        let menu_x = (buf_w.saturating_sub(menu_w)) / 2;
        const GAP: i32 = 12;
        let mut menu_y = y_s - menu_h as i32 - GAP;
        if menu_y < 8 {
            menu_y = y_e + crate::input_bar::LINE_STEP_PX as i32 + GAP;
            if menu_y + menu_h as i32 > (bar_top + bar_h) as i32 {
                menu_y = (bar_top + bar_h) as i32 - menu_h as i32 - 8; // 兜底：贴栏底
            }
        }
        (menu_x, menu_y.max(8) as u32, menu_w, menu_h)
    }

    /// 画选择操作菜单（BAR-046）：气泡条「全选 | 复制 | 剪切 | 粘贴」，
    /// 按钮标签居中自绘（2026-09-03 ④号迭代终结 MVP 空色块）。
    /// 位置：贴选区首行上方；空间不足翻末行下方（规则见 selection_menu_rect）。
    #[allow(clippy::too_many_arguments)]
    fn draw_selection_menu(
        &self,
        frame: &mut Frame,
        snap: &crate::input_bar::BarSnap,
        starts: &[usize],
        items: &[(&fontdue::Font, char, f32)],
        _text_cx: u32,
        line_y: &dyn Fn(usize) -> i32,
        field_top: u32,
        field_h: u32,
        bar_h: u32,
        bar_top: u32,
        buf_w: u32,
    ) {
        if snap.selection_start >= snap.selection_end {
            return;
        }
        let row_s = row_of(starts, snap.selection_start.min(items.len()));
        let row_e = row_of(starts, snap.selection_end.min(items.len()));
        // BAR-048：只锚可见选区+钉死钳制——选区首行滚出栏顶时菜单钉栏顶
        // 固定位（不追部分可见行的连续 y，防锯齿抖动）；整段滚出视口不画
        let Some((y_s, y_e)) = Self::visible_sel_anchor_y(
            line_y(0),
            starts.len(),
            field_top as i32,
            (field_top + field_h) as i32,
            row_s,
            row_e,
        ) else {
            return;
        };
        let (menu_x, menu_y, menu_w, menu_h) =
            Self::selection_menu_rect(y_s, y_e, buf_w, bar_top, bar_h);
        // 气泡底
        let t = &self.theme.bar;
        frame.fill_round_rect(menu_x, menu_y, menu_w, menu_h, 16, t.menu_bg);
        // 投影：简化版底边 4px 黑 α0.25
        for py in 0..4u32 {
            frame.blend_px(menu_x + 20, menu_y + menu_h + py, 0, 64);
        }
        // 分隔线（3 条竖线分四格）
        let btn_w = menu_w / 4;
        for i in 1..4 {
            let x = menu_x + btn_w * i;
            frame.fill_rect(x, menu_y + 12, 2, menu_h - 24, t.menu_disabled);
        }
        // 按钮文字（2026-09-03 ④号迭代：居中自绘，终结 MVP 空色块——
        // 实拍菜单条只有分隔线没有标签，用户不知道哪格是什么）
        const LABELS: [&str; 4] = ["全选", "复制", "剪切", "粘贴"];
        for (i, label) in LABELS.iter().enumerate() {
            self.draw_text_centered(
                frame,
                label,
                menu_x + btn_w * i as u32,
                menu_y,
                btn_w,
                menu_h,
                crate::input_bar::MENU_TEXT_PX,
                t.menu_text,
            );
        }
    }

    /// 选择态屏幕几何（BAR-046）：锚点柄视觉中心 + 菜单气泡边界。
    /// 触摸命中与渲染同源，眼手同尺。
    pub fn bar_selection_geometry(
        &self,
        snap: &crate::input_bar::BarSnap,
        buf_w: u32,
        buf_h: u32,
        ime_bottom: u32,
    ) -> Option<crate::input_bar::BarSelectionGeometry> {
        use crate::input_bar;
        if !snap.selecting || snap.selection_start >= snap.selection_end {
            return None;
        }
        let min_w = 2 * input_bar::MARGIN_X_PX + input_bar::GAP_PX + input_bar::SEND_W_PX + 40;
        if buf_w < min_w {
            return None;
        }
        let display = crate::input_bar::InputBarState::display_text(snap);
        let items = self.measure_bar_items(&display, BAR_TEXT_PX);
        if items.is_empty() {
            return None;
        }
        let widths: Vec<f32> = items.iter().map(|i| i.2).collect();
        let avail = input_bar::text_avail_w(buf_w)?;
        let chars: Vec<char> = display.chars().collect();
        let starts = input_bar::multiline_starts(&chars, &widths, avail);
        let n_lines = starts.len();
        let bar_h = input_bar::height_for_lines(n_lines as u32);
        let top = buf_h.checked_sub(ime_bottom)?.checked_sub(bar_h)?;
        let field_h = bar_h - 64;
        let field_top = top + 32;
        let field_left = input_bar::MARGIN_X_PX;
        let text_cx = field_left + 40;
        // BAR-049：文本视口 = field 上下收 TEXT_PAD_Y 内衬（与渲染同尺）
        let view_h = input_bar::text_view_h(field_h);
        let (vy0, vy1) = (
            field_top as i32 + input_bar::TEXT_PAD_Y as i32,
            (field_top + field_h) as i32 - input_bar::TEXT_PAD_Y as i32,
        );
        let (_, eff, top_off) =
            input_bar::viewport_geometry(n_lines as u32, view_h, snap.follow, snap.scroll_px);
        let text_top = vy0 + top_off as i32 - eff;
        let line_y = |k: usize| -> i32 { text_top + k as i32 * input_bar::LINE_STEP_PX as i32 };
        // 锚点柄视觉中心（2026-09-03 ②号迭代）：柄形 = 上尖三角（tip-1 起
        // 高 14）+ 正方承载（tip+11 起高 28），合 span y∈[tip-1, tip+38]，
        // 中心 tip+18；水平中心 ax（两图元 2026-09-03 ①号迭代后同锚 ax）。
        // 热区以此为中心（原返回柄左缘 tip 点，与视觉中心错位 14/18px，
        // 指按在看得见的柄上却落在热区外——锚点拖不动的根因）
        let anchor_at = |idx: usize| -> (f64, f64) {
            let row = row_of(&starts, idx.min(items.len()));
            let row_start = starts[row];
            let x_off: f32 = items[row_start..idx.min(items.len()).max(row_start)]
                .iter()
                .map(|i| i.2)
                .sum();
            let ax = f64::from(text_cx + 18) + f64::from(x_off);
            let ay = f64::from(line_y(row) + input_bar::LINE_STEP_PX as i32 - 2 + 18);
            (ax, ay)
        };
        let left = anchor_at(snap.selection_start);
        let right = anchor_at(snap.selection_end);
        // 菜单位置与 draw_selection_menu 同源（单尺 selection_menu_rect +
        // visible_sel_anchor_y 可见段钳制+钉死钳制，BAR-048）——选区整段
        // 滚出视口时菜单不画，几何也同步 None（看不见的点不得有触摸热区）
        let row_s = row_of(&starts, snap.selection_start.min(items.len()));
        let row_e = row_of(&starts, snap.selection_end.min(items.len()));
        let (y_s, y_e) =
            Self::visible_sel_anchor_y(line_y(0), starts.len(), vy0, vy1, row_s, row_e)?;
        let (menu_x, menu_y, menu_w, menu_h) =
            Self::selection_menu_rect(y_s, y_e, buf_w, top, bar_h);
        Some(crate::input_bar::BarSelectionGeometry {
            left_anchor: left,
            right_anchor: right,
            menu_x,
            menu_y,
            menu_w,
            menu_h,
        })
    }
}
