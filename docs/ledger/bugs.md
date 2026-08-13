# bugs.md — KFM-NA BAR 账本

> 每条 bug 修复登记一行。钉 = `#[test]` 名带 BAR 编号，位置列在右列。
> 纪律见 AGENTS.md 门 2（fix 提交不带钉 = 提交不了）。
>
> 分级：I = 事故级（用户实拍踩到/数据损坏）· L = 逻辑错（测试/审计发现）· V = 观感类

| 编号 | 模块 | 病灶与契约 | 级 | 状态 | 钉位置 |
|------|------|-----------|----|------|--------|
| BAR-001 | termview | 字形竖直居中 → 高矮字母底边错位（里倒歪斜）。契约：同基线字母底边对齐、高字母顶边更高、下伸字母探过基线 | V | 已修 | tests/termview_spec.rs `spec_bar001_基线对齐_同基线字母底边对齐` |
| BAR-002 | termview | 真机 NotoSansCJK-Regular.ttc 在 fontdue 0.9 下光栅全空（能载不能画）→ 只见光标不见字。契约：load_font 必须跳过空光栅字体（探针字符 'M' 无墨即弃） | I | 已修（跳过 → Roboto 兜底，11:30 实拍字已出） | tests/termview_spec.rs `spec_字体_空光栅判不合格` / `spec_字体_真字形判合格` |
| BAR-003 | termview | 真机落到 Roboto（比例字体）→ 定宽格摆比例字形，间距忽近忽远。契约：'i'/'M' 步进宽不等即弃；路径候选全灭落内嵌 DejaVuSansMono | I | 已修（等宽判定 + 内嵌兜底） | tests/termview_spec.rs `spec_字体_等宽判定` / `spec_字体_加载跳过比例字体` / `spec_字体_内嵌字节可直接用` / `spec_字体_候选全灭落内嵌等宽` |
| BAR-004 | android_app | 切后台再进页面消失：Android 退后台销毁 native 表面，壳不弃窗不弃 softbuffer 表面，回前台对着死柄画。契约：suspended 弃窗弃表面，resumed 重建；Term/会话还活着就不重开会话 | I | 已修（B 档生命周期胶水，C 档实拍判卷，无输入输出可出考题） | —（tests:na，平台胶水） |
| BAR-005 | termview | 网格从 (0,0) 画起，边缘字符被屏幕圆角/曲面切半。契约：帧缓冲四周 MARGIN 带内必须是纯背景，墨水全在带内之后 | V | 已修（MARGIN 12px 黑带，不画框） | tests/termview_spec.rs `spec_边距_首格不贴边` |
| BAR-006 | android_app | 软键盘盖住底部内容不上滚：cargo-apk 0.10/ndk-build 无 windowSoftInputMode 字段，android-activity 0.6 无 insets API。契约：Ime::Enabled 网格让出底部 42% 估计高度，Disabled 收回（C 档实拍调参） | V | 已修（估计式避让，真实高度待 JNI） | —（tests:na，平台胶水；避让几何挂在 apply_window_size 单点） |
