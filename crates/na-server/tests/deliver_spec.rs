//! deliver_spec.rs — BAR-252 输入栏投递面考题（追踪信 NA0181；设计楼1，
//! 裁决楼2 白露四点全裁）。
//!
//! 判卷维度：请求解析真值表／登记表解析与核验（拒投 detail 带出路）／
//! 活态核验（pane_pid 子孙树 harness 扫描）／键序构造（queue 降级透出
//! action_effective）／回执体五键／留痕行与时刻／路由臂与闸门腿放行。
//! execute 薄 IO 胶的真机判卷归承影。

use na_server::deliver::{
    Action, Harness, Pane, Proc, RegRow, build_keys, check_alive, descendants, iso8, log_line,
    lookup, parse_panes, parse_ps, parse_registry, parse_req, resp_json,
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

#[test]
fn spec_bar252_键序构造() {
    // send：字面文本 + Enter，目标 =名 精确匹配，-l -- 不解析键名
    let (cmds, eff, note) = build_keys(Action::Send, &Harness::Kimi, "清和", "你好 世界").unwrap();
    assert_eq!(eff, Action::Send);
    assert!(note.is_none());
    assert_eq!(
        cmds,
        vec![
            vec!["send-keys", "-t", "=清和", "-l", "--", "你好 世界"],
            vec!["send-keys", "-t", "=清和", "Enter"],
        ]
    );
    // interrupt：只发 Escape，不带文本
    let (cmds, eff, _) = build_keys(Action::Interrupt, &Harness::Kimi, "清和", "").unwrap();
    assert_eq!(eff, Action::Interrupt);
    assert_eq!(cmds, vec![vec!["send-keys", "-t", "=清和", "Escape"]]);
    // queue：tianshu 发 M-Enter
    let (cmds, eff, note) = build_keys(Action::Queue, &Harness::Tianshu, "悬铃", "排着").unwrap();
    assert_eq!(eff, Action::Queue);
    assert!(note.is_none());
    assert_eq!(cmds[1], vec!["send-keys", "-t", "=悬铃", "M-Enter"]);
    // queue：kimi 无排队键 → 降级 send，action_effective=send + note 透出
    let (cmds, eff, note) = build_keys(Action::Queue, &Harness::Kimi, "清和", "排着").unwrap();
    assert_eq!(eff, Action::Send);
    assert!(note.unwrap().contains("降级"));
    assert_eq!(cmds[1], vec!["send-keys", "-t", "=清和", "Enter"]);
    // 空文本：send/queue 拒，interrupt 放行
    assert!(build_keys(Action::Send, &Harness::Kimi, "清和", "").is_err());
    assert!(build_keys(Action::Queue, &Harness::Tianshu, "悬铃", "").is_err());
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
