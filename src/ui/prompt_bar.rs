//! ui/prompt_bar.rs — 输入对话栏控件（第 3 层；2026-09-01 自 termview
//! 物理搬移立形：状态核 = input_bar.rs，视图 = 本文件 impl TermView 块，
//! 注入通道 = bar-inject，考题 = input_bar_spec + termview_spec caret/bar
//! 系列，档案 = 插件档案-输入栏.md。搬移为零逻辑变化——逐字节原样）。
use crate::termview::{Frame, GlowSpec, GradSpec, SELECT_BG, VeilSpec, lerp_rgb, wrap_starts};

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
    /// 栏带向上浮盖终端底部行，终端网格几何不动）；文本折行（wrap_starts）
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
        // 量宽折行（画文本也要用，先量一次两头吃）
        let items = self.measure_items(&snap.text, BAR_TEXT_PX);
        let widths: Vec<f32> = items.iter().map(|i| i.2).collect();
        let avail = input_bar::text_avail_w(buf_w).unwrap_or(1.0);
        let starts = wrap_starts(&widths, avail);
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
        frame.fill_rect(0, top, buf_w, bar_h, t.bg);
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
        let text_cw = field_w - 40 - 12;
        // 折行块几何（画字与光标定位同用）：垂直居中成块，超 MAX_LINES
        // 尾锚显最后几行（手动滚动缺口记档案）
        let show = starts.len().min(input_bar::MAX_LINES as usize);
        let vis = &starts[starts.len() - show..];
        let block_h = show as u32 * input_bar::LINE_STEP_PX;
        let block_top = field_top + (field_h.saturating_sub(block_h)) / 2;
        if snap.text.is_empty() {
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
            // textarea 折行（2026-08-31 移动端全量复刻）：用函数头量好的
            // items/starts（同一次量宽，几何与画字同源）。
            // 字号恒定 BAR_TEXT_PX，行高 LINE_STEP_PX
            for (row, &st) in vis.iter().enumerate() {
                let end = if row + 1 < vis.len() {
                    vis[row + 1]
                } else {
                    items.len()
                };
                self.draw_items_left(
                    &mut frame,
                    &items[st..end],
                    text_cx,
                    text_cw,
                    block_top + row as u32 * input_bar::LINE_STEP_PX,
                    input_bar::LINE_STEP_PX,
                    BAR_TEXT_PX,
                    t.text,
                );
            }
        }
        // 光标（2026-08-31 用户指认浏览器控件行为）：聚焦才画，竖线一条，
        // 530ms 相位闪烁——相位由调用方算好传入（caret_on，渲染保持纯函数，
        // dump 快照相位准）；点按定位柄 = 光标线下方蓝色下坠柄，点按定位
        // 才出现，打字/清空/发送收起（状态核管）
        if snap.focused && caret_on {
            let cursor = snap.cursor.min(items.len());
            // cursor 所在行：可见行里最后一个起点 ≤ cursor 的行
            let mut row = 0usize;
            for (k, &st) in vis.iter().enumerate() {
                if st <= cursor {
                    row = k;
                } else {
                    break;
                }
            }
            let row_start = vis[row];
            let row_end = if row + 1 < vis.len() {
                vis[row + 1]
            } else {
                items.len()
            };
            let x_off: f32 = items[row_start..cursor.min(row_end)]
                .iter()
                .map(|i| i.2)
                .sum();
            let row_cy =
                block_top + row as u32 * input_bar::LINE_STEP_PX + input_bar::LINE_STEP_PX / 2;
            let caret_x = text_cx + 18 + x_off as u32;
            frame.fill_round_rect(caret_x, row_cy - 26, 4, 52, 2, t.text);
            if snap.handle {
                // 定位柄：蓝色下坠柄（品牌蓝同选区），悬在文本区下缘
                let hx = (caret_x + 3).saturating_sub(16);
                let hy = field_top + field_h + 1;
                frame.fill_round_rect(hx, hy, 32, 32, 10, SELECT_BG);
                frame.fill_triangle_up(hx + 16, hy - 1, 20, 12, SELECT_BG);
            }
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
    /// dump 读同一份）。buf_w 退化（画不下）按 1 行计
    pub fn bar_text_lines(&self, text: &str, buf_w: u32) -> u32 {
        if text.is_empty() {
            return 1;
        }
        let Some(avail) = crate::input_bar::text_avail_w(buf_w) else {
            return 1;
        };
        let widths: Vec<f32> = self
            .measure_items(text, BAR_TEXT_PX)
            .iter()
            .map(|i| i.2)
            .collect();
        wrap_starts(&widths, avail).len() as u32
    }

    /// 点按定位换算（2026-08-31 浏览器控件行为对齐）：文本区本地坐标
    /// （左上角原点）→ 光标 char 下标。与 render_inputbar 同一套量宽折行
    /// 几何（眼手同尺）；列向「过半归右」就近取字；行向钳在可见尾锚块内。
    /// 注：tofu 字（双字体都缺）不计入 items——定位与渲染同一对齐口径
    pub fn bar_cursor_at(&self, text: &str, buf_w: u32, x_local: f64, y_local: f64) -> usize {
        use crate::input_bar;
        let items = self.measure_items(text, BAR_TEXT_PX);
        if items.is_empty() {
            return 0;
        }
        let widths: Vec<f32> = items.iter().map(|i| i.2).collect();
        let avail = input_bar::text_avail_w(buf_w).unwrap_or(1.0);
        let starts = wrap_starts(&widths, avail);
        let n = starts.len();
        let show = n.min(input_bar::MAX_LINES as usize);
        let bar_h = input_bar::height_for_lines(n as u32);
        let field_h = bar_h - 64;
        let block_h = show as u32 * input_bar::LINE_STEP_PX;
        let block_top = (field_h.saturating_sub(block_h)) / 2;
        // 行：尾锚块内行号 k，全局行号 = 滚出视野的头部分行数 + k
        let k = if y_local >= f64::from(block_top) {
            (((y_local - f64::from(block_top)) / f64::from(input_bar::LINE_STEP_PX)) as usize)
                .min(show - 1)
        } else {
            0
        };
        let grow = n - show;
        let row_start = starts[grow + k];
        let row_end = if grow + k + 1 < n {
            starts[grow + k + 1]
        } else {
            items.len()
        };
        // 列：累计步进宽，过半归右（浏览器 tap 落点就近原则）
        let mut pen = 18.0f32;
        for (i, item) in items[row_start..row_end].iter().enumerate() {
            if x_local < f64::from(pen + item.2 * 0.5) {
                return row_start + i;
            }
            pen += item.2;
        }
        row_end
    }
}
