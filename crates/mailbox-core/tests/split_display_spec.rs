//! BAR-254（NA0183／MAIN0173 白露楼2/楼4/楼6 判据）信头人名切分 A 档考题：
//! 两级切分纯函数 split_display_items 真值表——
//!   一级按 、，,；; 及 等 分项；二级项内名册职能池最长前缀匹配；
//!   广播项（全体／职能+全体）不切；unknown 原样不猜不截；
//!   及式裸名第二项继承前项职能（解析层归属，显示层可省）。
//! 变异三刀必咬：最长匹配摘除／两字闸摘除／及式继承摘除。

use mailbox_core::roster::{DispItem, DispKind, Roster, render_display, split_display_items};

/// 主册 roster.json 同构缩录：职能 13 键（含 legacy 评审/研究/开发/测试/
/// 运维/调度部——keys 天然覆盖存量旧名），名字池取在役 13 人
const ROSTER_JSON: &str = r#"{
  "projects": ["na", "kfmv4"],
  "functions": {
    "委员会": {}, "乾部": {}, "乾部一组": {}, "乾部二组": {},
    "坤部": {}, "坤部一组": {}, "坤部二组": {},
    "评审部": {}, "研究部": {}, "开发部": {}, "测试部": {}, "运维部": {}, "调度部": {}
  },
  "names": {
    "白露": { "functions": ["乾部一组"], "legacy_functions": ["评审部"] },
    "清和": { "functions": ["坤部一组"], "legacy_functions": ["研究部"] },
    "观澜": { "functions": ["坤部一组"], "legacy_functions": ["开发部"] },
    "闻灯": { "functions": ["坤部一组"], "legacy_functions": ["开发部"] },
    "承影": { "functions": ["坤部一组"], "legacy_functions": ["测试部"] },
    "卡萝": { "functions": ["坤部二组"], "legacy_functions": ["运维部"] },
    "蔚然": { "functions": ["坤部二组"], "legacy_functions": ["运维部"] },
    "小满": { "functions": ["乾部一组"], "legacy_functions": ["研究部", "开发部", "测试部"] },
    "南舟": { "functions": ["坤部一组"], "legacy_functions": ["研究部"] },
    "空谷": { "functions": ["乾部二组"] },
    "空山": { "functions": ["乾部二组"] },
    "悬铃": { "functions": ["乾部二组"] },
    "茉莉": { "functions": ["乾部一组"] }
  }
}"#;

fn roster() -> Roster {
    Roster::from_json_str(ROSTER_JSON).expect("夹具名册可解析")
}

fn person(func: &str, name: &str) -> DispItem {
    DispItem {
        kind: DispKind::Person,
        func: Some(func.to_string()),
        name: name.to_string(),
        inherited: false,
    }
}

fn inherited(func: &str, name: &str) -> DispItem {
    DispItem {
        inherited: true,
        ..person(func, name)
    }
}

fn broadcast(raw: &str) -> DispItem {
    DispItem {
        kind: DispKind::Broadcast,
        func: None,
        name: raw.to_string(),
        inherited: false,
    }
}

fn unknown(raw: &str) -> DispItem {
    DispItem {
        kind: DispKind::Unknown,
        func: None,
        name: raw.to_string(),
        inherited: false,
    }
}

fn split(field: &str) -> Vec<DispItem> {
    split_display_items(Some(&roster()), field)
}

// ---- MAIN0173 §三 四样本（用户报病那条在首）----

#[test]
fn spec_bar254_four_samples() {
    // 样本①：用户报病「乾部一组白露」——池里 乾部 与 乾部一组 同在，
    // 必须取最长（取短则余「一组白露」四字落 unknown，正是线上病象）
    assert_eq!(split("乾部一组白露"), vec![person("乾部一组", "白露")]);
    // 样本②
    assert_eq!(split("坤部一组清和"), vec![person("坤部一组", "清和")]);
    // 样本③：存量 legacy 职能名（functions 键集天然覆盖）
    assert_eq!(split("研究部清和"), vec![person("研究部", "清和")]);
    // 样本④：广播不切
    assert_eq!(split("全体"), vec![broadcast("全体")]);
}

// ---- 广播族：职能+全体 原样（南舟楼3 §四 四条）----

#[test]
fn spec_bar254_func_all_broadcast() {
    assert_eq!(split("研究部全体"), vec![broadcast("研究部全体")]);
    assert_eq!(split("乾部一组全体"), vec![broadcast("乾部一组全体")]);
    assert_eq!(split("开发部全体"), vec![broadcast("开发部全体")]);
}

// ---- 及式：第二项裸名继承前项职能（白露楼4 唯一真漏面）----

#[test]
fn spec_bar254_ji_inheritance() {
    // NA0004/0010/0020/0022/0039/0083 同形
    assert_eq!(
        split("开发部闻灯及观澜"),
        vec![person("开发部", "闻灯"), inherited("开发部", "观澜")]
    );
    // 主册 0112：及式双全形（第二项自带职能不继承）
    assert_eq!(
        split("研究部清和及运维部卡萝"),
        vec![person("研究部", "清和"), person("运维部", "卡萝")]
    );
    // 三项链：末项裸名继承「前一项」的职能（0020/0022/0039 原形）
    assert_eq!(
        split("研究部清和及开发部闻灯及观澜"),
        vec![
            person("研究部", "清和"),
            person("开发部", "闻灯"),
            inherited("开发部", "观澜"),
        ]
    );
    // 人＋广播混合：两段都在（南舟楼1 样本②）
    assert_eq!(
        split("评审部白露及全体"),
        vec![person("评审部", "白露"), broadcast("全体")]
    );
    // 及式裸名 ∉ 名字池 ⇒ 不继承，unknown 原样
    assert_eq!(
        split("开发部闻灯及张仨"),
        vec![person("开发部", "闻灯"), unknown("张仨")]
    );
}

// ---- 顿号式裸名【不】继承（判据只许及式；显示原样同名但不许归属）----

#[test]
fn spec_bar254_dun_no_inheritance() {
    assert_eq!(
        split("乾部一组白露、悬铃"),
        vec![person("乾部一组", "白露"), unknown("悬铃")]
    );
}

// ---- unknown 三边界：不猜不截 ----

#[test]
fn spec_bar254_unknown_boundaries() {
    // 职能不在池
    assert_eq!(split("星际舰队张三"), vec![unknown("星际舰队张三")]);
    // 职能命中但余 ≠ 两字
    assert_eq!(split("乾部一组白露雪"), vec![unknown("乾部一组白露雪")]);
    // 余两字但 ∉ 名字池
    assert_eq!(split("乾部一组张仨"), vec![unknown("乾部一组张仨")]);
}

// ---- 一级分隔符全集：、，,；; 及 等；空项滤除 ----

#[test]
fn spec_bar254_separator_full_set() {
    assert_eq!(
        split("坤部一组清和，乾部一组白露,评审部承影；测试部卡萝;运维部蔚然"),
        vec![
            person("坤部一组", "清和"),
            person("乾部一组", "白露"),
            person("评审部", "承影"),
            person("测试部", "卡萝"),
            person("运维部", "蔚然"),
        ]
    );
    // 契约「等」：分项且尾空项滤除
    assert_eq!(split("研究部空谷等"), vec![person("研究部", "空谷")]);
    // 连续分隔符／首尾空白
    assert_eq!(
        split("  乾部一组白露 、、 坤部一组清和 "),
        vec![person("乾部一组", "白露"), person("坤部一组", "清和")]
    );
}

// ---- 空池（roster 读不到）＝ 全部原样照抄，一项不死 ----

#[test]
fn spec_bar254_empty_pool_passthrough() {
    assert_eq!(
        split_display_items(None, "坤部一组清和、全体"),
        vec![unknown("坤部一组清和"), broadcast("全体")]
    );
}

// ---- 显示串渲染：person 取名、broadcast/unknown 取原样，「、」相连 ----

#[test]
fn spec_bar254_render_display() {
    let items = split("开发部闻灯及观澜、研究部全体、星际舰队张三");
    assert_eq!(
        render_display(&items),
        "闻灯、观澜、研究部全体、星际舰队张三"
    );
    assert_eq!(render_display(&split("全体")), "全体");
    assert_eq!(render_display(&[]), "");
}
