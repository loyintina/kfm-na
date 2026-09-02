# composer 清单（输入栏状态×转换表 v1.1 首跑草案，living）

> 清单闭环 v1.1 首跑（信箱线程 kfmv4-checklist-loop-finalize-response.md）。
> 组件 = composer（全局输入栏）。本文件是①盘点草案 + ③钉位登记 +
> ②认领进度，结环（⑥→③）追加不重写。
> 样板 v1 十一行源：信箱 kfm-na-frontend-ui-collab-na-response.md；
> 结晶行来源：BAR-041/043/044 首跑实战。

## ① 盘点·states（源=实现扫描，证据 file:line）

| 状态 | 类型 | 赋值/持有证据 |
|---|---|---|
| text | String | src/input_bar.rs:134-151（Inner 全字段） |
| cursor | usize（char 下标） | input_bar.rs:139 |
| focused | bool | input_bar.rs:282/285（focus/unfocus 唯二写点） |
| handle | bool | input_bar.rs:313（tap 定位置 1）；339/372/381/394/414（打字/清空/发送收起） |
| composing | Option<String> | input_bar.rs:240-242（set_composing 唯一写点） |
| scroll_px | i32（raw=距头顶偏移） | input_bar.rs:203-215（scroll_by_px 唯一写点，BAR-043 播种在此） |
| follow | bool | input_bar.rs:205/235（滚动脱锚；任何编辑回锚） |
| lines | u32（渲染量宽写回） | input_bar.rs:275（set_lines）；android_app.rs:796（poll 写回点） |
| 发送钮图标 | 外读 ai_running | ai-presence 状态核（不属本核，只列依赖） |

## ① 盘点·sources（事件源入口，全覆盖自检）

| 入口 | 证据 |
|---|---|
| 通道十一 bar-inject（遥控/AI 驱动轨） | src/gate.rs:1556-1605（十指令） |
| 真触摸（tap 聚焦/拖动滚动/发送钮/Esc 失焦） | src/android_app.rs:570/417/600/678 |
| IME 队列（commit/composing/composing-end） | src/android_app.rs:1407-1412（ime_queue Inject 三态） |
| 行数写回（渲染 poll） | src/android_app.rs:796 |
| 注册（唯一服务句柄） | src/android_app.rs:964 → gate.rs:1460 |

扫描器口径：以上五入口已盖全 grep 面；漏新入口 = 盘点 bug，结环时补。

## ② 清单草案（v1 十一行 + 结晶两行；待用户三选一认领）

| # | 事件 | 迁移 | 禁令 | 时序预算 | 钉/证据 |
|---|---|---|---|---|---|
| 1 | 点文本区（位移<slop） | focused=1；cursor=点按定位；handle=1 | 弹键盘只许壳层做，注入通道禁做 | 定位映射与渲染同帧同尺 | android_app.rs:570；spec_bar_cursor_at |
| 2 | 栏内拖动（≥slop） | follow=0；播种 raw=max_eff 后逐笔 ±1:1，写入即钳 [0,max] | 禁跳过播种直用 raw（BAR-043）；渲染侧禁二次口径 | 1:1 像素 | input_bar.rs:203-215；spec_bar043_* |
| 3 | IME setComposingText(s) | composing=s（空串=清）；follow=1 回锚 | 组合文本禁入 text（虚拟拼接） | 随打随显 | input_bar.rs:234-242 |
| 4 | IME commitText(s) | composing 弃；cursor 处插 s | 禁复读半截拼音 | | android_app.rs:1407 |
| 5 | IME finishComposingText | composing 落真字，光标跟进 | | | android_app.rs:1412 |
| 6 | 退格（组合态） | 组合尾删一字；删空=清组合 | 禁越组合删 text | | input_bar.rs（backspace 分支） |
| 7 | 退格（无组合） | 删 cursor 前一字；handle=0 | 行归属只许 row_of 一把尺，禁第二判据（BAR-041） | | input_bar.rs:372；spec_bar041 |
| 8 | 点发送钮 / Enter | submit：取走 text；focused 保持；handle=0 | 取文禁带组合残片 | | android_app.rs:600；gate.rs:1586 |
| 9 | Esc / 点终端区 | focused=0 | | | android_app.rs:678 |
| 10 | ai_running 翻转 | 发送钮 ▶↔⏸ 硬切 | 禁过渡动画（动效属动画插件包） | 硬切同帧 | ai-presence.md |
| 11 | 清空指令 | text/cursor/handle/composing 全复位；follow=1 | | | input_bar.rs:389 |
| 12 | 结晶：follow=1 态首笔滚动（任意符号） | 先播种 raw=max_eff（当前显示位）再叠加钳制 | 禁把 raw=0 当当前位置（=瞬移头，BAR-043 实锤） | 首笔即 1:1 | spec_bar043_从尾锚续算 |
| 13 | 结晶：值守读 bar-inject 空/纯空白 | 不消费不删文件，留待下轮 | 禁删半截写入（静默吞指令，BAR-044 实锤） | 下一轮读全量 | gate.rs bar_should_consume；spec_bar044 |

时序预算总注记：光标闪烁 530ms 亮/530ms 灭（CARET_BLINK_MS，Android 系统节拍）；
带高实测派生禁读 stale lines（BAR-039）；判卷演示须前台（后台 stale lines 使
闸门侧 max_eff 塌缩——BAR-039 同族，见 state.md 2026-09-02 条）。

## ② 认领进度

- [x] 用户三选一（对 / 不对应是 X / 没想到）×13 行——2026-09-02 用户实机
      复测后口头闭环：「测试了，看不出行为变化。我这边可以闭环了。」
- 「没想到」占比：0/13 = 0%。
- 备注：用户在场窗口同时覆盖三场景装机判卷（真拖 1:1 / 点按柄位 /
  柄稳显）+ BAR-045 panic.log 夜检修复后的复测。

## ③⑤ 钉与判卷

钉已挂 9 颗（spec_bar041×2 / spec_bar043×2 / spec_bar044×1 /
spec_bar_cursor_at / spec_bar_caret_闪烁相位与定位柄 / spec_bar039 /
scroll_拖动脱跟随_编辑回跟随），变异抽检按 v1.1 §⑤ 点名制执行。
缺钉行：1/4/5/6/8/9/10（触摸与 IME 入口为 android 层 C 档——装机实拍
判卷，钉以 cases/ 脚手架形式补，见⑥结环）。

## ⑥ 结晶记录（追加制）

- 2026-09-02 BAR-043：首笔滚动瞬移头 → 行 12 播种契约（首跑实战结晶第 1 条）。
- 2026-09-02 BAR-044：空读竞态吞指令 → 行 13 不消费契约（首跑实战结晶第 2 条）。
- 2026-09-02 BAR-045：prompt_bar.rs 切片倒挂崩溃 → 所有 `items[a..b]`
  强制 `a <= b` + 点按换算与渲染共用 `display_text(snap)` 同源。
