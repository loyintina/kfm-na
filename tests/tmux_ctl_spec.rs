//! tmux_ctl_spec.rs — tmux 控制核考题（A 档纯逻辑：解析/命令构造/名字清洗）
//!
//! 对象模型（2026-09-19 用户拍板，nz tmux-tabs v2.1 会话版同构）：
//! **标签 = 服务器全部 tmux 会话**。列 `tmux list-sessions`、点行切换 =
//! 重开远程连接 attach（`new-session -A`，壳负责）、＋输入名字新建
//! （`new-session -d`）、×确认关闭（`kill-session`）、[重排] =
//! `resize-window -x/-y` 钉 na 网格（manual 即生效；largest/latest 下
//! tmux 自动翻 manual——2026-09-19 服务器实证 rc=0）。
//!
//! 判卷维度：
//! - parse_session_list：pty 脏文本（\r\n、ANSI、空行、噪声）→ 干净
//!   会话表；attached 是计数（多客户端 >1 也算附着）；名字可含竖线
//! - 命令构造：list/new/kill/reflow/attach 精确字串——-t 目标一律
//!   `='名'` 精确匹配（tmux 默认前缀模糊匹配，杀错会话不可挽回）；
//!   attach 是常驻命令**不带 exit**（带了就附着即退）
//! - sanitize_name：新建名清洗——空/纯空白 = None；引号/竖线/分号/
//!   控制字符 = None（注入面）；tmux 自动编号 = None 走无参新建
//!
//! 变异抽检方向：-t 漏 '=' 前缀、attach 尾加 exit、attached 判据
//! 改 ==1（多客户端漏判）、sanitize 放行引号——本文件必须红。

use kfm_na::tmux_ctl::{
    self, TmuxSession, cmd_attach, cmd_kill, cmd_list, cmd_new, cmd_reflow, parse_session_list,
    sanitize_name, session_name_of,
};

// ---- parse_session_list ----

#[test]
fn spec_parse_干净三行() {
    let out = "kfm-na|1|1\namp|2|0\ndsh|3|2\n";
    let ss = parse_session_list(out);
    assert_eq!(
        ss,
        vec![
            TmuxSession {
                name: "kfm-na".into(),
                windows: 1,
                attached: true
            },
            TmuxSession {
                name: "amp".into(),
                windows: 2,
                attached: false
            },
            TmuxSession {
                name: "dsh".into(),
                windows: 3,
                attached: true
            },
        ]
    );
    // dsh 两个客户端附着（计数 2）也算附着——多客户端不许漏判
}

#[test]
fn spec_parse_crlf与噪声全滤() {
    let out = "$ tmux list-sessions\r\nkfm-na|1|1\r\n\r\nno server running on /tmp\r\namp|2|0\r\n$ exit\r\n";
    let ss = parse_session_list(out);
    assert_eq!(ss.len(), 2);
    assert_eq!(ss[0].name, "kfm-na");
    assert!(ss[0].attached);
    assert_eq!(ss[1].name, "amp");
    assert!(!ss[1].attached);
}

#[test]
fn spec_parse_ansi转义剥离() {
    let out = "\u{1b}[32mkfm-na|1|1\u{1b}[0m\n";
    let ss = parse_session_list(out);
    assert_eq!(ss.len(), 1);
    assert_eq!(ss[0].name, "kfm-na");
}

#[test]
fn spec_parse_名字含竖线() {
    // 末两段是窗数/附着计数，其余全归名字
    let out = "a|b c|4|0\n";
    let ss = parse_session_list(out);
    assert_eq!(ss.len(), 1);
    assert_eq!(ss[0].name, "a|b c");
    assert_eq!(ss[0].windows, 4);
    assert!(!ss[0].attached);
}

#[test]
fn spec_parse_坏行作废() {
    // 窗数非数字 / 附着段非数字 / 段数不够 = 整行作废
    let out = "x|NaN|1\ny|2|yes\nonlyname\nz|3|1\n";
    let ss = parse_session_list(out);
    assert_eq!(ss.len(), 1);
    assert_eq!(ss[0].name, "z");
}

#[test]
fn spec_parse_全噪声得空表() {
    assert!(parse_session_list("no server running\r\n").is_empty());
    assert!(parse_session_list("").is_empty());
}

// ---- 命令构造（字面量钉死）----

#[test]
fn spec_cmd_list_精确字串() {
    assert_eq!(
        cmd_list(),
        "tmux list-sessions -F '#{session_name}|#{session_windows}|#{session_attached}'; exit"
    );
}

#[test]
fn spec_cmd_new_有名() {
    // -P -F 回打印新会话名（壳要知道 attach 谁）
    assert_eq!(
        cmd_new(Some("work")),
        "tmux new-session -d -P -F '#{session_name}' -s 'work'; exit"
    );
}

#[test]
fn spec_cmd_new_无名自动编号() {
    assert_eq!(
        cmd_new(None),
        "tmux new-session -d -P -F '#{session_name}'; exit"
    );
}

#[test]
fn spec_cmd_kill_精确匹配() {
    // '=' 前缀 = 精确匹配：tmux 默认前缀模糊，杀会话不许模糊
    assert_eq!(cmd_kill("amp"), "tmux kill-session -t '=amp'; exit");
}

#[test]
fn spec_cmd_reflow_精确字串() {
    assert_eq!(
        cmd_reflow("kfm-na", 48, 47),
        "tmux resize-window -t '=kfm-na' -x 48 -y 47; exit"
    );
}

#[test]
fn spec_cmd_attach_常驻无exit() {
    // attach 是重开远程连接的启动命令——带 exit 会附着即退
    assert_eq!(cmd_attach("amp"), "tmux new-session -A -s 'amp'");
    assert!(!cmd_attach("amp").contains("exit"));
}

#[test]
fn spec_cmd_名字含空格照引() {
    assert_eq!(cmd_kill("my srv"), "tmux kill-session -t '=my srv'; exit");
    assert_eq!(cmd_attach("my srv"), "tmux new-session -A -s 'my srv'");
}

// ---- session_name_of（现有启动命令 → 附着会话名）----

#[test]
fn spec_session_name_开屏直连命令() {
    assert_eq!(
        session_name_of("tmux new-session -A -s kfm-na"),
        Some("kfm-na".into())
    );
}

#[test]
fn spec_session_name_attach命令() {
    assert_eq!(session_name_of("tmux attach -t work"), Some("work".into()));
    // 带引号的目标也认得
    assert_eq!(
        session_name_of("tmux new-session -A -s 'my srv'"),
        Some("my srv".into())
    );
}

#[test]
fn spec_session_name_无会话名() {
    assert_eq!(session_name_of("bash"), None);
    assert_eq!(session_name_of("tmux new-session"), None);
    assert_eq!(session_name_of("tmux new-session -s"), None); // 缺值不 panic
}

// ---- sanitize_name ----

#[test]
fn spec_sanitize_正常名() {
    assert_eq!(sanitize_name("work"), Some("work".into()));
    assert_eq!(sanitize_name("  my srv 2 "), Some("my srv 2".into()));
    assert_eq!(sanitize_name("项目甲"), Some("项目甲".into()));
}

#[test]
fn spec_sanitize_空与纯空白() {
    assert_eq!(sanitize_name(""), None);
    assert_eq!(sanitize_name("   "), None);
}

#[test]
fn spec_sanitize_注入面全拒() {
    // 单引号（破引用）/ 竖线（碎解析）/ 冒号（tmux 目标语法）/ 分号
    // （串命令）/ 控制字符（含换行）——一个都不许过
    for bad in ["a'b", "a|b", "a:b", "a;b", "a\nb", "a\rb", "a\u{1b}b"] {
        assert_eq!(sanitize_name(bad), None, "应拒: {bad:?}");
    }
}

#[test]
fn spec_sanitize_超长截断() {
    let long = "x".repeat(100);
    let s = sanitize_name(&long).unwrap();
    assert!(s.chars().count() <= 32);
}

// ---- 模块级常量契约 ----

#[test]
fn spec_list_format_与cmd_list同源() {
    assert!(cmd_list().contains(tmux_ctl::LIST_FORMAT));
}
