# gate.rs 变异 triage 表 r1（71 存活逐条三层标注）

> 这是什么：gate.rs 变异首批 71 存活的三层判卷——**host 已盖**（host
> 考卷其实能抓,变异逃逸=漏网,复核考题）/**设备已盖**（设备考官每天
> 在真机上映,登记豁免-设备层）/**真空白**（双层都没盖,host 补题清
> 单）。依据:变异只跑 host 层,「设备已盖」为按考官覆盖图的判定
> （na-regress 七卷+夜班冒烟对每函数的实际映射）。
> 日期: 2026-08-28。配套: kfm-na-gate-mutants-report.md。

## 一、真空白（host 补题清单,按优先级）

### 高（会话语义核心）
1. **pump_once 按名路由臂删除 ×2**（delete match arm "local"/"remote"）——
   按名路由是会话输出投递的核心语义,臂删=该会话输出整路丢失;
   session_pump_spec 未做逐名投递断言。**补题:双会话灌注断言各名
   输出各归各的 sink。**
2. **SessionPump::register `!=`→`==`**（换心脏逻辑反转）——同名重挂
   反转后新旧心脏错位,断线重孵场景必炸;spec 未压此边。**补题:
   同名重挂断言（旧通道遗物不喂）。**
3. **pump_once bool 返回 ×2 + 累加翻转 ×2**——fed 返回值驱动重绘;
   累加是泵字节账。**补题:fed 语义断言+字节账断言。**

### 中（辅助链路）
4. **pump_take_replay ×3**（切会话补屏数据源）——spec 未断言取走
   即清/内容正确。**补题:replay 取走断言。**
5. **rec_decode_all 边界 ×3 + plen guard**——飞行记录仪解析边界,
   na-replay 判卷的底层。**补题:rec_spec 边界三针。**
6. **install_panic_hook ()**——panic 钩子安装被空转=坠机诊断全盲,
   潜伏型缺口。**补题:安装后触发断言 panic.log 增行（参考 crash
   探针判卷法）。**

### 低（人工工具域,可登记后延）
7. loop_beat_age_ms ×4 + note_loop_beat——看门狗心跳,判卷人=
   na-ping（人工工具,不在自动套件）。
8. 436 行 replay 字节账 ×4——帽边界,低危。
9. note_loop_beat——心跳写入停摆,na-ping 人工工具可判,低优先。

## 二、设备已盖（豁免-设备层,登记不补题）

watch_loop 全家 ×4（值守死=全通道死,任何 PIN 卷即红）;
spawn_gate_watcher ×3（同上）;text_dump ×3（BAR-040 读屏判卷）;
inject_keys ×1（na-type 全依赖）;restart_check ×2（BAR-040 重启
闭环）;stats_answer ×2（na-stats 全依赖）;trace_dump ×2（PIN-boot）;
touch_check ×6 + touch_take ×1（PIN-touch 全依赖）;
register_gate_router ×1（PIN-switch 依赖 active 读数）;
pump_register ×1（PIN-rehatch 依赖输出泵）;
pump_take_control ×1（PIN-rehatch 依赖 Exited 事件）;
history_tick ×2（PIN-pump 依赖）;
note_session_death ×1（PIN-rehatch/PIN-standby 依赖 deaths 计数）;
alert_tick ×2、note_draw ×1、rec_output/rec_resize/rec_compact/
rec_ts/start_recorder/note_frame_size（诊断设施/统计口径/等价
变异,登记低危豁免）。

**小计:真空白 24（高 7/中 8/低 9）;设备已盖+低危豁免 ≈ 47。**

## 三、变异常识（本表沉淀）

- **变异只审 host 层——双层判卷线要做「双清单对照」才知道真空白**:
  本次 71 存活中约 52 条由设备层覆盖,真空白 19 条。若不做此 triage,
  「71 个缺口」会既吓退行动、又埋掉真缺口。
- 胶水函数变异存活是**常态不是病**（它们的价值由设备层兑付）;纯
  函数变异存活才是病（scroll/keymap 0 存活对照）。
- 下一批扫描前先清高优先真空白（7 条）,否则同区再扫只会复制噪声。

——kfm-na(Kimi Code) · 2026-08-28 · 补题清单执行随下批通报
