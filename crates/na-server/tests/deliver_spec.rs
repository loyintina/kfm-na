//! deliver_spec.rs — BAR-252 输入栏投递面考题（追踪信 NA0181；设计楼1，
//! 裁决楼2 白露四点全裁）。
//!
//! 判卷维度：请求解析真值表／登记表解析与核验（拒投 detail 带出路）／
//! 活态核验（pane_pid 子孙树 harness 扫描）／键序构造（queue 降级透出
//! action_effective）／回执体五键／留痕行与时刻／路由臂与闸门腿放行。
//! execute 薄 IO 胶的真机判卷归承影。

use na_server::deliver::{
    Action, Harness, Pane, Plan, Proc, RegRow, TmuxOp, check_alive, descendants, iso8, log_line,
    lookup, parse_panes, parse_ps, parse_registry, parse_req, plan_dispatch, resp_json,
    resp_json_with_screen, run_agent_send, screen_tail,
};

fn kimi_row(name: &str) -> RegRow {
    RegRow {
        name: name.to_string(),
        harness: Harness::Kimi,
    }
}

fn tianshu_row(name: &str) -> RegRow {
    RegRow {
        name: name.to_string(),
        harness: Harness::Tianshu,
    }
}

/// 活态夹具：窗 清和 pane_pid=1000（sh），白露 pane_pid=2000（sh）。
/// 清和树下 1001=kimi；白露树光杆（harness 未跑）；悬铃 pane_pid=3000，
/// 树下 3001=node 且 args 含 tianshu。
fn fixture() -> (Vec<Pane>, Vec<Proc>) {
    let panes = vec![
        Pane {
            session: "清和".into(),
            pane_pid: 1000,
        },
        Pane {
            session: "白露".into(),
            pane_pid: 2000,
        },
        Pane {
            session: "悬铃".into(),
            pane_pid: 3000,
        },
    ];
    let procs = vec![
        Proc {
            pid: 1000,
            ppid: 1,
            comm: "sh".into(),
            args: "-sh".into(),
        },
        Proc {
            pid: 1001,
            ppid: 1000,
            comm: "kimi".into(),
            args: "kimi".into(),
        },
        Proc {
            pid: 2000,
            ppid: 1,
            comm: "sh".into(),
            args: "-sh".into(),
        },
        Proc {
            pid: 3000,
            ppid: 1,
            comm: "sh".into(),
            args: "-sh".into(),
        },
        Proc {
            pid: 3001,
            ppid: 3000,
            comm: "node".into(),
            args: "node /root/60-服务/窗口/起天枢.sh tianshu".into(),
        },
        Proc {
            pid: 9999,
            ppid: 1,
            comm: "kimi".into(),
            args: "kimi".into(),
        }, // 别家树上的不算
    ];
    (panes, procs)
}

#[test]
fn spec_bar252_请求解析_真值表() {
    // 绿：v 缺省 / v=1；text 缺省 = 空串
    let r = parse_req(r#"{"target":"清和","action":"send","text":"你好"}"#).unwrap();
    assert_eq!(r.target, "清和");
    assert_eq!(r.action, Action::Send);
    assert_eq!(r.text, "你好");
    let r = parse_req(r#"{"v":1,"target":"清和","action":"interrupt"}"#).unwrap();
    assert_eq!(r.action, Action::Interrupt);
    assert_eq!(r.text, "");
    // 红：坏 JSON / 版本 / 动作 / 目标
    assert!(parse_req("not json").is_err());
    assert!(parse_req(r#"{"v":2,"target":"清和","action":"send"}"#).is_err());
    assert!(parse_req(r#"{"target":"清和","action":"explode"}"#).is_err());
    assert!(parse_req(r#"{"target":"","action":"send","text":"x"}"#).is_err());
    assert!(parse_req(r#"{"action":"send","text":"x"}"#).is_err());
    // 目标带控制符 / 超长 = fail-closed
    let bad = format!(
        r#"{{"target":"{}","action":"send","text":"x"}}"#,
        "a".repeat(65)
    );
    assert!(parse_req(&bad).is_err());
    assert!(parse_req("{\"target\":\"清\\u0000和\",\"action\":\"send\",\"text\":\"x\"}").is_err());
}

#[test]
fn spec_bar252_登记表解析() {
    let text = "# 注释行\n\n清和\tsession_x\t/root/20-工作区/02-清和\tkimi\n\
        南舟\tsession_y\t/root/20-工作区/04-南舟\n\
        悬铃\t-\t/root/20-工作区/10-悬铃\ttianshu\n\
        proxy\t-\t/root\tservice\n\
        怪窗\t-\t/root\talien\n没有tab的行\n";
    let rows = parse_registry(text);
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0], kimi_row("清和"));
    assert_eq!(rows[1], kimi_row("南舟")); // 旧三列行缺省 kimi
    assert_eq!(rows[2], tianshu_row("悬铃"));
    assert_eq!(
        rows[3],
        RegRow {
            name: "proxy".into(),
            harness: Harness::Service
        }
    );
    assert_eq!(
        rows[4],
        RegRow {
            name: "怪窗".into(),
            harness: Harness::Other("alien".into())
        }
    );
}

#[test]
fn spec_bar252_登记表核验_拒投带出路() {
    let rows = parse_registry("清和\ts\th\tkimi\nproxy\t-\t/root\tservice\n怪\t-\t/r\talien\n");
    // 绿
    assert!(lookup(&rows, "清和").is_ok());
    // 红：三档拒投，detail 全是「原因＋出路」（白露楼2②）
    let d = lookup(&rows, "光杆").unwrap_err();
    assert!(d.contains("未登记") && d.contains("AI 页"), "{d}");
    let d = lookup(&rows, "proxy").unwrap_err();
    assert!(d.contains("服务窗") && d.contains("AI 页"), "{d}");
    let d = lookup(&rows, "怪").unwrap_err();
    assert!(d.contains("不认识") && d.contains("AI 页"), "{d}");
}

#[test]
fn spec_bar252_活态核验() {
    let (panes, procs) = fixture();
    // 绿：kimi 子孙树命中；tianshu node+cmdline 命中
    assert_eq!(check_alive(&kimi_row("清和"), &panes, &procs), Ok(1000));
    assert_eq!(check_alive(&tianshu_row("悬铃"), &panes, &procs), Ok(3000));
    // 红：会话不在 / harness 未跑（白露树光杆；别家树上的 kimi 不算数）
    let d = check_alive(&kimi_row("没这窗"), &panes, &procs).unwrap_err();
    assert!(d.contains("不在") && d.contains("AI 页"), "{d}");
    let d = check_alive(&kimi_row("白露"), &panes, &procs).unwrap_err();
    assert!(d.contains("未跑 harness") && d.contains("AI 页"), "{d}");
    // tianshu 窗里只有 kimi 不算（harness 类与窗对不上 = 空壳同档）
    let d = check_alive(&tianshu_row("清和"), &panes, &procs).unwrap_err();
    assert!(d.contains("未跑 harness"), "{d}");
}

#[test]
fn spec_bar252_现场解析() {
    // parse_panes：tab 分列，坏行（无 tab/非数字 pid）跳过不炸
    let panes = parse_panes("清和\t1000\n坏行没有tab\n白露\tabc\n悬铃\t3000\n");
    assert_eq!(panes.len(), 2);
    assert_eq!(
        panes[0],
        Pane {
            session: "清和".into(),
            pane_pid: 1000
        }
    );
    assert_eq!(panes[1].pane_pid, 3000);
    // parse_ps：whitespace 分列（pid ppid comm args…），表头/短行/非数字跳过
    let procs = parse_ps(
        "  PID  PPID COMMAND ARGS\n1000 1 sh -bash\n短行\n2000 1000 kimi\n\
         3001 3000 node /usr/local/bin/tianshu serve\n",
    );
    assert_eq!(procs.len(), 3);
    assert_eq!(procs[0].comm, "sh");
    assert_eq!(procs[0].args, "-bash");
    assert_eq!(procs[2].comm, "node");
    assert!(procs[2].args.contains("tianshu"));
    // 解析件与直构夹具同效力：过解析器的现场文本喂 check_alive 同判
    assert_eq!(check_alive(&kimi_row("清和"), &panes, &procs), Ok(1000));
}

#[test]
fn spec_bar252_子孙树防环() {
    // ppid 成环的畸形输入不许死循环
    let procs = vec![
        Proc {
            pid: 1,
            ppid: 2,
            comm: "a".into(),
            args: String::new(),
        },
        Proc {
            pid: 2,
            ppid: 1,
            comm: "b".into(),
            args: String::new(),
        },
    ];
    let d = descendants(1, &procs);
    assert_eq!(d.len(), 2);
}

/// 甲（**独立断言**，不是复述实现）：投递真把 target 与正文原文交给了塞话
/// 原语——夹具假脚本把收到的 argv 写进文件，run_agent_send 指向它，
/// 断言 argv 里有目标名与正文原文。咬的是「内容交到原语手里」，
/// 不是「我以为我发对了键」。
#[test]
fn spec_bar252_返工_委派塞话原语() {
    let dir = std::env::temp_dir().join(format!("na-deliver-jia-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fake = dir.join("fake-agent-send.sh");
    let argv_log = dir.join("argv.txt");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/bash\nprintf '%s\\n' \"$@\" > {}\nexit 0\n",
            argv_log.display()
        ),
    )
    .unwrap();
    run_agent_send(fake.to_str().unwrap(), "清和", "你好 世界").expect("假原语应成功");
    let got = std::fs::read_to_string(&argv_log).expect("假脚本应写 argv");
    let lines: Vec<&str> = got.lines().collect();
    assert_eq!(
        lines,
        vec!["清和", "你好 世界"],
        "argv 必须是 <target> <正文原文>"
    );
    // 原语非零退出 = Err（失败不被吞成成功）
    let bad = dir.join("bad-agent-send.sh");
    std::fs::write(&bad, "#!/bin/bash\necho boom >&2\nexit 3\n").unwrap();
    let e = run_agent_send(bad.to_str().unwrap(), "清和", "x").unwrap_err();
    assert!(
        e.contains("boom") && e.contains("3"),
        "失败要带退出码与原语 stderr: {e}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 计划真值表（A 档）：send／queue-kimi 降级 → **委派原语**（不是自拼键序）；
/// interrupt → 直发单个 Escape；queue-tianshu → 文本＋M-Enter；空文本拒。
#[test]
fn spec_bar252_返工_计划真值表() {
    let (plan, eff, note) =
        plan_dispatch(Action::Send, &Harness::Kimi, "清和", "你好 世界").unwrap();
    assert_eq!(eff, Action::Send);
    assert!(note.is_none());
    match plan {
        Plan::AgentSend { target, text } => {
            assert_eq!(target, "清和", "委派用会话名，不是 tmux 目标串");
            assert_eq!(text, "你好 世界");
        }
        other => panic!("send 必须委派塞话原语，实得 {}", plan_name(&other)),
    }
    // interrupt：单 Escape，直发 tmux，不带文本
    let (plan, eff, _) = plan_dispatch(Action::Interrupt, &Harness::Kimi, "清和", "").unwrap();
    assert_eq!(eff, Action::Interrupt);
    match plan {
        Plan::Tmux { target, ops } => {
            assert_eq!(target, "清和");
            assert_eq!(ops.len(), 1);
            assert!(matches!(ops[0], TmuxOp::Key("Escape")));
        }
        other => panic!("interrupt 必须直发单 Escape，实得 {}", plan_name(&other)),
    }
    // queue-tianshu：文本＋M-Enter（排队键，不是塞话序列）
    let (plan, eff, note) =
        plan_dispatch(Action::Queue, &Harness::Tianshu, "悬铃", "排着").unwrap();
    assert_eq!(eff, Action::Queue);
    assert!(note.is_none());
    match plan {
        Plan::Tmux { target, ops } => {
            assert_eq!(target, "悬铃");
            assert_eq!(ops.len(), 2);
            match (&ops[0], &ops[1]) {
                (TmuxOp::Literal(l), TmuxOp::Key(k)) => {
                    assert_eq!(l, "排着");
                    assert_eq!(*k, "M-Enter");
                }
                _ => panic!("tianshu 排队应为 文本＋M-Enter"),
            }
        }
        other => panic!("tianshu 排队必须直发，实得 {}", plan_name(&other)),
    }
    // queue-kimi：降级 send → 委派原语 + note 透出
    let (plan, eff, note) = plan_dispatch(Action::Queue, &Harness::Kimi, "清和", "排着").unwrap();
    assert_eq!(eff, Action::Send);
    assert!(note.unwrap().contains("降级"));
    assert!(matches!(plan, Plan::AgentSend { .. }), "降级臂也必须走委派");
    // 空文本：send/queue 拒
    assert!(plan_dispatch(Action::Send, &Harness::Kimi, "清和", "").is_err());
    assert!(plan_dispatch(Action::Queue, &Harness::Tianshu, "悬铃", "").is_err());
}

fn plan_name(p: &Plan) -> &'static str {
    match p {
        Plan::AgentSend { .. } => "AgentSend",
        Plan::Tmux { .. } => "Tmux",
    }
}

/// 乙（接线守卫，源码锚）：deliver.rs 不得再含键序实现标识与「Esc→文本→
/// Enter」序列字面——两份实现的病；必须含塞话原语路径（委派接线在）。
/// 直发臂的目标串仍须 `=名:`（BAR-152 锚，承影楼6 打回项）。
#[test]
fn spec_bar252_返工_接线守卫() {
    let src = include_str!("../src/deliver.rs");
    for banned in [
        "KeyStep",
        "parse_key_steps",
        "expand_key_steps",
        "StepSpec",
        "kfm-inject-lib",
        "include_str!",
        "KFM_INJECT_STEPS",
    ] {
        assert!(
            !src.contains(banned),
            "键序自实现残留：{banned}（BAR-252 返工：唯一实现者是 agent-send.sh）"
        );
    }
    assert!(src.contains("agent_send_path"), "必须按路径调塞话原语");
    assert!(
        src.contains("run_agent_send(&agent_send_path()"),
        "委派臂必须真调执行函数（摘了它＝自实现回来了）"
    );
    assert!(
        src.contains("NA_AGENT_SEND"),
        "原语路径须 env 可覆盖（夹具口）"
    );
    assert!(
        src.contains("/root/10-项目/kfmv4/scripts/agent-send.sh"),
        "缺省路径指向塞话原语"
    );
    // 直发臂目标串 =名: 锚（BAR-152）
    assert!(
        src.contains("format!(\"={t}:\")"),
        "直发 tmux 的目标串必须 =名: 形态"
    );
    // 唯一实现者仓里：na 仓不该再有键序声明文件
    assert!(
        !std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/kfm-inject-lib.sh"
        ))
        .exists(),
        "kfm-inject-lib.sh 应已删除（键序只许在 agent-send.sh 一份）"
    );
}

#[test]
fn spec_bar252_回执体五键全在() {
    let j: serde_json::Value =
        serde_json::from_str(&resp_json(true, "清和", "queue", "send", "降级了")).unwrap();
    assert_eq!(j["ok"], true);
    assert_eq!(j["target"], "清和");
    assert_eq!(j["action"], "queue");
    assert_eq!(j["action_effective"], "send");
    assert_eq!(j["detail"], "降级了");
    // detail 空串不省略（凭键认形状）；引号/换行转义不破
    let j: serde_json::Value =
        serde_json::from_str(&resp_json(false, "x", "send", "send", "引\"号\n换行")).unwrap();
    assert_eq!(j["detail"], "引\"号\n换行");
    assert!(j.get("action_effective").is_some());
}

#[test]
fn spec_bar252_留痕行与时刻() {
    assert_eq!(
        log_line(0, "清和", "send", "send", true, 6, ""),
        "1970-01-01 08:00:00 +08:00 deliver target=清和 action=send effective=send ok=true bytes=6"
    );
    // 拒投行带 detail
    let l = log_line(0, "光杆", "send", "send", false, 3, "未登记");
    assert!(l.contains("ok=false") && l.contains("detail=未登记"), "{l}");
    // iso8：原点 / 跨日 / 负数（epoch 前）
    assert_eq!(iso8(0), "1970-01-01 08:00:00 +08:00");
    assert_eq!(iso8(86400), "1970-01-02 08:00:00 +08:00");
    assert_eq!(iso8(-1), "1970-01-01 07:59:59 +08:00");
    assert_eq!(iso8(1761000000), "2025-10-21 06:40:00 +08:00");
}

#[test]
fn spec_bar252_路由臂() {
    assert!(matches!(
        na_server::httpd::route("POST", "/api/deliver"),
        na_server::httpd::Route::Deliver
    ));
    assert!(matches!(
        na_server::httpd::route("GET", "/api/deliver"),
        na_server::httpd::Route::NotFound
    ));
}

/// 接线守卫：闸门腿白名单必须放行 Deliver（9 机走 gate_only 腿投递，
/// 摘了就是 404）——源码锚咬 matches! 臂
#[test]
fn spec_bar252_闸门腿放行deliver() {
    let main_rs = include_str!("../src/main.rs");
    let gate_only_seg = main_rs
        .split("if gate_only")
        .nth(1)
        .expect("main.rs 应有 gate_only 闸");
    assert!(
        gate_only_seg[..800].contains("httpd::Route::Deliver"),
        "gate_only 白名单应收 httpd::Route::Deliver"
    );
}

/// 判据③取证钉（白露 MAIN0175 楼23 第③条）：回执带 post_screen＝键序执行
/// 后 600ms 取目标窗尾部三行（剥 ANSI、跳空行、≤200 字）——判卷人据它咬
/// 「按下后对方窗口真进入处理」，不是只看屏上有字。
#[test]
fn spec_bar252_post_screen回执() {
    // 尾部三行 + ANSI 剥净 + 空行跳过
    let raw = "\u{1b}[32m行一\u{1b}[0m\n\n行二\n行三\n行四\n";
    let tail = screen_tail(raw);
    assert!(tail.contains("行二") && tail.contains("行三") && tail.contains("行四"));
    assert!(!tail.contains('\u{1b}'), "ANSI 必须剥净：{tail}");
    assert!(!tail.contains("行一"), "只留尾部三行：{tail}");
    // 200 字截断
    let long = "x".repeat(500);
    assert!(
        screen_tail(&long).chars().count() <= 200,
        "超长必须截到 200 字"
    );
    // 回执六键齐（原五键 + post_screen）
    let j: serde_json::Value = serde_json::from_str(&resp_json_with_screen(
        true,
        "清和",
        "send",
        "send",
        "",
        "行二\n行三",
    ))
    .unwrap();
    for k in [
        "ok",
        "target",
        "action",
        "action_effective",
        "detail",
        "post_screen",
    ] {
        assert!(j.get(k).is_some(), "回执缺键 {k}");
    }
    assert_eq!(j["post_screen"], "行二\n行三");
}
