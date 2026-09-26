//! crates/na-agent/tests/fence_spec.rs — BAR-161 钉②：run_command 围栏三言。
//!
//! ①cwd 锁配置根（cd 逃逸拒 + 实证 pwd == 配置根）；②禁 sudo/su；
//! ③命令与 stdout/stderr 全量进 wire。
//! 答案区：crates/na-agent/src/tools.rs（check_command）+ agent.rs（入账）。
//! 考题不许改。

use std::collections::VecDeque;
use std::sync::Mutex;

use na_agent::agent::run_turn;
use na_agent::dialect::{ChatClient, ChatReply, FunctionCall, Message, ToolCall, ToolSpec};
use na_agent::host::StdHost;
use na_agent::session::SessionWriter;
use na_agent::tools::{check_command, execute};

#[test]
fn spec_bar161_围栏_cwd逃逸拒() {
    for bad in [
        "cd /etc",
        "cd ..",
        "ls; cd /",
        "ls && cd /tmp",
        "ls | cd /x",
        "(cd /)",
        "ls\ncd /",
    ] {
        assert!(check_command(bad).is_err(), "应拒: {bad:?}");
    }
    // 参数位/同名词不是调用，放行
    for ok in ["ls", "pwd", "echo cd", "cat cdrom.txt", "echo 'cd /'"] {
        assert!(check_command(ok).is_ok(), "应放行: {ok:?}");
    }
}

#[test]
fn spec_bar161_围栏_sudo_su拒() {
    for bad in [
        "sudo ls",
        "su -",
        "echo ok && sudo id",
        "ls;su root",
        "x | sudo tee /a",
    ] {
        assert!(check_command(bad).is_err(), "应拒: {bad:?}");
    }
    // 参数位同名词放行（拦的是调用不是字符串）
    for ok in ["echo sudoers", "cat sudo.log", "grep su /etc/passwd"] {
        assert!(check_command(ok).is_ok(), "应放行: {ok:?}");
    }
}

/// 围栏①宿主半实证：子进程 cwd 恒为配置根（真 sh 真临时目录）
#[test]
fn spec_bar161_围栏_cwd锁配置根_实证() {
    let tmp = tempfile::tempdir().expect("临时目录");
    let host = StdHost::new(tmp.path().to_path_buf());
    let out = execute(&host, "run_command", "{\"command\":\"pwd\"}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("合法 JSON");
    assert_eq!(v["exit_code"], 0);
    // canonicalize 对表（/tmp 可能有符号链接层）
    let want = tmp
        .path()
        .canonicalize()
        .expect("canonicalize")
        .to_string_lossy()
        .into_owned();
    let got = v["stdout"].as_str().expect("stdout").trim();
    assert_eq!(got, want, "子进程 cwd 恒为配置根");
}

/// 围栏②执行面实证：被拒的命令根本到不了宿主（execute 层即拦）
#[test]
fn spec_bar161_围栏_被拒命令不到宿主() {
    let tmp = tempfile::tempdir().expect("临时目录");
    let host = StdHost::new(tmp.path().to_path_buf());
    let out = execute(&host, "run_command", "{\"command\":\"sudo id\"}");
    assert!(out.contains("工具错误"), "拒在执行前: {out}");
    assert!(out.contains("围栏②"), "报围栏②: {out}");
    let out = execute(&host, "run_command", "{\"command\":\"cd /etc && ls\"}");
    assert!(out.contains("围栏①"), "报围栏①: {out}");
}

struct StubClient {
    replies: Mutex<VecDeque<ChatReply>>,
}

impl ChatClient for StubClient {
    fn chat(&self, _m: &str, _ms: &[Message], _t: &[ToolSpec]) -> Result<ChatReply, String> {
        self.replies
            .lock()
            .expect("锁")
            .pop_front()
            .ok_or_else(|| "剧本耗尽".into())
    }
}

/// 围栏③：命令文本与 stdout/stderr 全量进 wire（真 sh + 真会话文件）
#[test]
fn spec_bar161_围栏_命令与输出全量进wire() {
    let tmp = tempfile::tempdir().expect("临时目录");
    let workdir = tmp.path().join("work");
    std::fs::create_dir_all(&workdir).expect("建工作区");
    let host = StdHost::new(workdir);
    let brain = StubClient {
        replies: Mutex::new(
            vec![
                ChatReply {
                    content: None,
                    tool_calls: vec![ToolCall {
                        id: "c1".into(),
                        kind: "function".into(),
                        function: FunctionCall {
                            name: "run_command".into(),
                            arguments: "{\"command\":\"echo BAR161-WIRE; echo BAR161-ERR 1>&2\"}"
                                .into(),
                        },
                    }],
                    usage: None,
                },
                ChatReply {
                    content: Some("收工".into()),
                    tool_calls: vec![],
                    usage: None,
                },
            ]
            .into(),
        ),
    };
    let sess = tmp.path().join("0001-会话.jsonl");
    let sess = sess.to_string_lossy().into_owned();
    let mut writer = SessionWriter::open(&host, &sess);
    let mut messages = vec![Message::user("跑")];
    run_turn(&host, &brain, &mut writer, "m", &mut messages, 4).expect("跑通");

    let text = std::fs::read_to_string(&sess).expect("会话文件在");
    // tool_call 事件带完整命令文本
    let call = text
        .lines()
        .find_map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).ok()?;
            (v["type"] == "tool_call").then_some(v)
        })
        .expect("tool_call 事件在");
    assert!(
        call["arguments"]
            .as_str()
            .expect("arguments")
            .contains("echo BAR161-WIRE; echo BAR161-ERR 1>&2"),
        "命令全量进 wire"
    );
    // tool_result 事件带 stdout 与 stderr 全量
    let result = text
        .lines()
        .find_map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).ok()?;
            (v["type"] == "tool_result").then_some(v)
        })
        .expect("tool_result 事件在");
    let out = result["output"].as_str().expect("output");
    assert!(out.contains("BAR161-WIRE"), "stdout 进 wire: {out}");
    assert!(out.contains("BAR161-ERR"), "stderr 进 wire: {out}");
    assert!(out.contains("\"exit_code\":0"), "exit_code 进 wire: {out}");
}
