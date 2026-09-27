//! md_parse.rs A 档考题（BAR-169 md 渲染器一期·解析核心层）：
//! 六样子集边界全钉——标题层级/嵌套强调/围栏内字面/未闭合容错/
//! 分隔线与列表优先级/段落成块律。变异抽检记录见 bugs.md BAR-169 行。

use kfm_na::ui::demo_page::SegStyle;
use kfm_na::ui::md_parse::{MdSink, Span, parse_inline, parse_md};

/// 事件记录夹具：回调序 = 文档序（一句一事件，可读对账）
#[derive(Default)]
struct Rec {
    events: Vec<String>,
}

fn spans_txt(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|(st, t)| {
            let tag = match st {
                SegStyle::Normal => "N",
                SegStyle::Bold => "B",
                SegStyle::Code => "C",
            };
            format!("[{tag}]{t}")
        })
        .collect::<Vec<_>>()
        .join("|")
}

impl MdSink for Rec {
    fn heading(&mut self, level: u8, spans: Vec<Span>) {
        self.events.push(format!("H{level}:{}", spans_txt(&spans)));
    }
    fn paragraph(&mut self, lines: Vec<Vec<Span>>) {
        let body = lines
            .iter()
            .map(|l| spans_txt(l))
            .collect::<Vec<_>>()
            .join("⏎");
        self.events.push(format!("P:{body}"));
    }
    fn code_block(&mut self, lines: Vec<String>) {
        self.events.push(format!("CODE:{}", lines.join("⏎")));
    }
    fn quote(&mut self, lines: Vec<Vec<Span>>) {
        let body = lines
            .iter()
            .map(|l| spans_txt(l))
            .collect::<Vec<_>>()
            .join("⏎");
        self.events.push(format!("Q:{body}"));
    }
    fn list(&mut self, items: Vec<Vec<Span>>) {
        let body = items
            .iter()
            .map(|l| spans_txt(l))
            .collect::<Vec<_>>()
            .join("⏎");
        self.events.push(format!("L:{body}"));
    }
    fn hr(&mut self) {
        self.events.push("HR".to_string());
    }
}

fn rec(md: &str) -> Vec<String> {
    let mut r = Rec::default();
    parse_md(md, &mut r);
    r.events
}

#[test]
fn spec_bar169_01_标题六档层级() {
    let ev = rec("# 一\n## 二\n### 三\n#### 四\n##### 五\n###### 六");
    assert_eq!(
        ev,
        vec![
            "H1:[N]一",
            "H2:[N]二",
            "H3:[N]三",
            "H4:[N]四",
            "H5:[N]五",
            "H6:[N]六"
        ]
    );
}

#[test]
fn spec_bar169_02_标题边界_七井与无空格皆正文() {
    // ####### = 正文（CommonMark 同律）；#后无空格 = 正文；孤 # = H1 空题
    let ev = rec("####### 七井不是标题\n#无空格不是\n#");
    assert_eq!(
        ev,
        vec!["P:[N]####### 七井不是标题⏎[N]#无空格不是", "H1:[N]"]
    );
}

#[test]
fn spec_bar169_03_行内粗体行内码混排() {
    let ev = rec("见字**如面**与`code`同排");
    assert_eq!(ev, vec!["P:[N]见字|[B]如面|[N]与|[C]code|[N]同排"]);
}

#[test]
fn spec_bar169_04_围栏内一切字面() {
    // 围栏里的 ** # ` > - 全不解析（变异点：围栏内若走行内解析即咬）
    let ev = rec("```\n**not bold** # not heading `x`\n> not quote\n```\n后文");
    assert_eq!(
        ev,
        vec![
            "CODE:**not bold** # not heading `x`⏎> not quote",
            "P:[N]后文"
        ]
    );
}

#[test]
fn spec_bar169_05_未闭合围栏_余下全文归代码() {
    let ev = rec("前文\n```rust\nfn a() {}\n# 被吞\n**也被吞**");
    assert_eq!(ev, vec!["P:[N]前文", "CODE:fn a() {}⏎# 被吞⏎**也被吞**"]);
}

#[test]
fn spec_bar169_06_引用连续成块行内照解析() {
    let ev = rec("> 第一行**粗**\n> 第二行`码`\n隔断\n> 新块");
    assert_eq!(
        ev,
        vec![
            "Q:[N]第一行|[B]粗⏎[N]第二行|[C]码",
            "P:[N]隔断",
            "Q:[N]新块"
        ]
    );
}

#[test]
fn spec_bar169_07_列表连续成块孤行横杠不算() {
    let ev = rec("- 甲\n- 乙**重**\n-\n- 丙");
    // 「-」孤行无空格不是列表项 → 断块归正文
    assert_eq!(ev, vec!["L:[N]甲⏎[N]乙|[B]重", "P:[N]-", "L:[N]丙"]);
}

#[test]
fn spec_bar169_08_分隔线判定先于列表() {
    // ---/----/- - - = 分隔线；- x = 列表（先判 hr 再判列表，顺序反 = 咬）
    let ev = rec("---\n----\n- - -\n- 列表项");
    assert_eq!(ev, vec!["HR", "HR", "HR", "L:[N]列表项"]);
}

#[test]
fn spec_bar169_09_行内未闭合与空对皆字面() {
    assert_eq!(
        parse_inline("未闭合 **粗体"),
        vec![(SegStyle::Normal, "未闭合 **粗体".to_string())]
    );
    assert_eq!(
        parse_inline("未闭合 `码"),
        vec![(SegStyle::Normal, "未闭合 `码".to_string())]
    );
    // 空粗体对 **** = 字面（不吞文）
    assert_eq!(
        parse_inline("a****b"),
        vec![(SegStyle::Normal, "a****b".to_string())]
    );
}

#[test]
fn spec_bar169_10_标记不嵌套() {
    // 粗体内的 ` 是字面；码内的 ** 是字面；反引号优先于星号
    assert_eq!(
        parse_inline("**粗`内`体**"),
        vec![(SegStyle::Bold, "粗`内`体".to_string())]
    );
    assert_eq!(
        parse_inline("`码**内**码`"),
        vec![(SegStyle::Code, "码**内**码".to_string())]
    );
}

#[test]
fn spec_bar169_11_段落连续成块空行分块() {
    let ev = rec("第一段甲\n第一段乙\n\n第二段");
    assert_eq!(ev, vec!["P:[N]第一段甲⏎[N]第一段乙", "P:[N]第二段"]);
}

#[test]
fn spec_bar169_12_文档序即回调序() {
    let ev = rec("# 题\n正文\n```\nx\n```\n> 引\n- 项\n---\n尾");
    assert_eq!(
        ev,
        vec![
            "H1:[N]题",
            "P:[N]正文",
            "CODE:x",
            "Q:[N]引",
            "L:[N]项",
            "HR",
            "P:[N]尾"
        ]
    );
}

#[test]
fn spec_bar169_13_空文档零事件_空行占位不塌() {
    assert_eq!(rec(""), Vec::<String>::new());
    assert_eq!(rec("\n\n  \n"), Vec::<String>::new());
    assert_eq!(parse_inline(""), vec![(SegStyle::Normal, String::new())]);
}

#[test]
fn spec_bar169_14_段落遇块起手即断() {
    // 段落中行出现块起手（标题/引用/列表/围栏/分隔线）= 段落断、新块起
    let ev = rec("正文行\n## 插题\n续文");
    assert_eq!(ev, vec!["P:[N]正文行", "H2:[N]插题", "P:[N]续文"]);
}
