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

// ---- cmd_capture（2026-09-25 外置视口快照：tmux 像素级滚动的内容源）----

#[test]
fn spec_cmd_capture_精确匹配带色全史() {
    // '=name:' = 精确匹配会话 + 活动窗格（BAR-152 实机定罪：'=name' 裸用
    // 在 pane 目标上不成立，tmux 报 can't find pane）；-e 保色；-S - 全
    // 滚动缓冲；头行 history_size（v3 增量合并的锚）；尾带成功标记
    // （exit 码经 sh -c 传不回，标记是唯一验收）
    assert_eq!(
        tmux_ctl::cmd_capture("amp"),
        "tmux display-message -p -t '=amp:' '#{history_size} #{history_limit}' && tmux capture-pane -p -e -S - -t '=amp:' && echo KFM_CAP_OK; exit"
    );
    assert_eq!(
        tmux_ctl::cmd_capture("my srv"),
        "tmux display-message -p -t '=my srv:' '#{history_size} #{history_limit}' && tmux capture-pane -p -e -S - -t '=my srv:' && echo KFM_CAP_OK; exit"
    );
    // 增量小抓：同头行同尾标，但只抓当前屏（无 -S -）
    assert_eq!(
        tmux_ctl::cmd_capture_screen("amp"),
        "tmux display-message -p -t '=amp:' '#{history_size} #{history_limit}' && tmux capture-pane -p -e -t '=amp:' && echo KFM_CAP_OK; exit"
    );
}

// ---- capture_strip_marker（BAR-152：报错文本与快照同走 stdout，
// 无标记 = 抓取失败不许进浏览态）----

#[test]
fn spec_bar152_快照验收_标记判卷() {
    // 真快照：标记在尾 → 剥标记还净内容（pty \r\n 尾巴照剥）
    assert_eq!(
        tmux_ctl::capture_strip_marker("line1\r\nline2\r\nKFM_CAP_OK\r\n"),
        Some("line1\r\nline2".to_string())
    );
    // 抓取失败：报错文本无标记 → None（「can't find pane 当快照」的
    // 整页消失病灶在此闸死）
    assert_eq!(
        tmux_ctl::capture_strip_marker("can't find pane: =kfm-na\r\n"),
        None
    );
    // 空输出/半截输出同样 None
    assert_eq!(tmux_ctl::capture_strip_marker(""), None);
    assert_eq!(tmux_ctl::capture_strip_marker("KFM_CAP_O"), None);
}

// ---- respawn_attach_cmd（BAR-144：重孵按附着账裁决，不许一刀切默认）----

#[test]
fn spec_bar144_重孵_附着账在_附回账上会话() {
    // 用户 attach nz 后隧道抖动重孵：命令必须附回 nz，不是设置里的
    // 默认 kfm-na——一刀切 default_config 就是「切 nz 被拽回」的病灶
    let default = Some("tmux new-session -A -s 'kfm-na'");
    assert_eq!(
        tmux_ctl::respawn_attach_cmd(Some("nz"), default),
        Some("tmux new-session -A -s 'nz'".to_string())
    );
}

#[test]
fn spec_bar144_重孵_账空_原命令一字不动() {
    // 账空两臂：默认 attach 命令原样还回（首次进默认会话的重连语义）；
    // None 进 None 出（脱离 tmux 后的裸 shell 重孵语义）
    let default = Some("tmux new-session -A -s 'kfm-na'");
    assert_eq!(
        tmux_ctl::respawn_attach_cmd(None, default),
        Some("tmux new-session -A -s 'kfm-na'".to_string())
    );
    assert_eq!(tmux_ctl::respawn_attach_cmd(None, None), None);
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

// ---- v3 增量合并（immutable-history 模型：历史不可变，新输出只把屏顶
// 行挤进历史尾；2026-09-25 用户拍板「视口在中央也该跟贴底一样活」）----

#[test]
fn spec_capture_parse_史量与正文() {
    // 正常：头行史量 + 正文还净
    let (hist, limit, text) = tmux_ctl::capture_parse("2979 10000\r\nL1\r\nL2\r\nKFM_CAP_OK\r\n")
        .expect("真快照必须过闸");
    assert_eq!((hist, limit), (2979, 10000));
    assert_eq!(
        text, "L1\r\nL2",
        "pty 的 \\r\\n 行尾原样保留（Term 解析同尺）"
    );
    // 无标记/头行非双数/无正文 = None（垃圾永许不进浏览态）
    assert_eq!(tmux_ctl::capture_parse("L1\r\nL2\r\n"), None);
    assert_eq!(tmux_ctl::capture_parse("abc def\nL1\nKFM_CAP_OK\n"), None);
    assert_eq!(tmux_ctl::capture_parse("2979\nL1\nKFM_CAP_OK\n"), None);
    assert_eq!(tmux_ctl::capture_parse("2979 10000KFM_CAP_OK"), None);
}

#[test]
fn spec_增量合并_屏顶行挤进历史尾() {
    // 旧全文 = 史 3 行 + 屏 4 行；新输出 2 行把旧屏顶 2 行（S0/S1）挤进
    // 历史尾：hist 3→5，新全文 = 旧前 5 行 + 新屏 4 行
    let old = "H0\nH1\nH2\nS0\nS1\nS2\nS3";
    let screen = "S2\nS3\nN0\nN1";
    let merged = tmux_ctl::merge_capture(old, 3, 4, 5, screen).expect("合法合并必须成");
    assert_eq!(merged, "H0\nH1\nH2\nS0\nS1\nS2\nS3\nN0\nN1");
    // k=0（alt 屏 TUI 原地重绘，史不动）：历史照抄 + 屏整换
    let merged0 = tmux_ctl::merge_capture(old, 3, 4, 3, "A\nB\nC\nD").unwrap();
    assert_eq!(merged0, "H0\nH1\nH2\nA\nB\nC\nD");
}

#[test]
fn spec_增量合并_判负回落全量() {
    let old = "H0\nH1\nH2\nS0\nS1\nS2\nS3";
    let screen = "S2\nS3\nN0\nN1";
    // 清史（hist 缩水）→ None
    assert_eq!(tmux_ctl::merge_capture(old, 3, 4, 2, screen), None);
    // k > rows（刷新间隔 scrolled 超一屏，旧屏不够抄）→ None
    assert_eq!(tmux_ctl::merge_capture(old, 3, 4, 8, screen), None);
    // 旧全文行数与账不符（resize 过境）→ None
    assert_eq!(tmux_ctl::merge_capture("X\nY", 3, 4, 5, screen), None);
    // 新屏行数与账不符（对端重排）→ None
    assert_eq!(tmux_ctl::merge_capture(old, 3, 4, 5, "A\nB"), None);
}
