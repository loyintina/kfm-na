<!-- 机械生成：scripts/check/check-spec-coverage.sh —— 请勿手改 -->
# 考卷覆盖矩阵（gen:spec-coverage）

> 这是什么：每模块 pub 项(fn/const)被 tests/ 引用的对照表。
> 棘轮契约：未覆盖数只许降（加考题后手改 scripts/check/
> spec-coverage-baseline.txt 对应行），涨了 chain 红。豁免与
> 近似边界见脚本头注释。B/C 档豁免模块不进本表。

| 模块 | pub项 | 已引用 | 未覆盖 | 未覆盖清单 |
|---|---|---|---|---|
| `src/ai_presence.rs` | 27 | 27 | 0 | — |
| `src/bootstrap.rs` | 4 | 2 | 2 | ensure_pkg_tool first_boot_install |
| `src/brain_ep.rs` | 4 | 4 | 0 | — |
| `src/brain.rs` | 14 | 14 | 0 | — |
| `src/clipboard.rs` | 1 | 0 | 1 | copy_and_toast |
| `src/conn.rs` | 4 | 2 | 2 | spawn_smoke ws_spawner |
| `src/crash.rs` | 3 | 2 | 1 | install_signal_hook |
| `src/direct_brain.rs` | 2 | 2 | 0 | — |
| `src/exec_probe.rs` | 1 | 1 | 0 | — |
| `src/gate.rs` | 75 | 51 | 24 | DUMP_DIR register_gate_router text_dump inject_keys spawn_gate_watcher REC_FILE_CAP REC_FILE rec_output rec_resize PANIC_FILE PANIC_TRACE_FILE LOOP_STALL_FILE note_loop_beat loop_beat_age_ms note_foreground install_panic_hook note_draw note_session_death touch_take register_input_bar ALERT_RSS_COOLDOWN_MS ALERT_DEATHS_WINDOW_MS ALERT_DEATHS_COOLDOWN_MS HISTORY_EVERY_TICKS |
| `src/http1.rs` | 10 | 7 | 3 | is_tick_err read_head_hook read_body_hook |
| `src/ime_bridge.rs` | 1 | 0 | 1 | jni_counters |
| `src/ime_queue.rs` | 5 | 5 | 0 | — |
| `src/input_bar.rs` | 26 | 21 | 5 | LINE_STEP_PX text_avail_w CARET_BLINK_MS MARGIN_X_PX GAP_PX |
| `src/insets.rs` | 3 | 1 | 2 | force_show_keyboard query_ime_bottom |
| `src/keybar.rs` | 15 | 11 | 4 | COLS MOD_ALT install_bridge_mods bridge_mods |
| `src/keymap.rs` | 2 | 2 | 0 | — |
| `src/local_pty.rs` | 5 | 4 | 1 | android_prefix |
| `src/plugins/ai_presence.rs` | 2 | 2 | 0 | — |
| `src/plugins/conn_provider_local.rs` | 3 | 3 | 0 | — |
| `src/plugins/conn_provider_ws.rs` | 3 | 3 | 0 | — |
| `src/plugins/input_bar.rs` | 2 | 2 | 0 | — |
| `src/plugins/input_ime.rs` | 2 | 2 | 0 | — |
| `src/plugins/term_alacritty.rs` | 3 | 3 | 0 | — |
| `src/protocol.rs` | 2 | 2 | 0 | — |
| `src/providers.rs` | 5 | 5 | 0 | — |
| `src/report.rs` | 8 | 1 | 7 | set_boot_t0 start_flusher report report_sync report_sync_once http_status_is_200 escape_json |
| `src/scroll.rs` | 5 | 5 | 0 | — |
| `src/session_router.rs` | 10 | 9 | 1 | names |
| `src/session.rs` | 8 | 8 | 0 | — |
| `src/termview.rs` | 78 | 72 | 6 | FONT_CANDIDATES CJK_FONT_CANDIDATES load_cjk_font font_probe MAG_HALF_COLS MAG_HALF_ROWS |
| `src/trace.rs` | 12 | 8 | 4 | TRACE_CAP format_tail dump_all dump_tail |
| `src/ui/keybar.rs` | 1 | 1 | 0 | — |
| `src/ui/orb.rs` | 3 | 3 | 0 | — |
| `src/ui/prompt_bar.rs` | 3 | 3 | 0 | — |
