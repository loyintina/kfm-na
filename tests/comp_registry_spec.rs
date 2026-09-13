//! tests/comp_registry_spec.rs — A 档考题：组件注册表（src/ui/comp_registry.rs）
//!
//! 契约真相源：docs/active/theme.md §五 目录语义 7「组件池页」（2026-09-13
//! 九修：唯一信息源 = comp_registry 常量表，页面直接读表渲染）+ §六
//! 「样式唯一来源」。本文件是考题，生成器不许改。
//!
//! 核心棘轮：**每个条目的 (symbol, file) 必须真实**——symbol 字符串
//! 出现在 file 文本里。表与代码漂移（重命名/删函数忘改表）= 考题红，
//! 防止花名册念出不存在的东西（kfmv4 教训：无模板必漂移）。

use kfm_na::ui::comp_registry::{CATEGORIES, COMPONENTS, CompStatus, count_of, entries_of};

#[test]
fn categories_unique_and_nonempty() {
    let mut seen = std::collections::HashSet::new();
    for c in CATEGORIES {
        assert!(!c.is_empty(), "大类名不许空");
        assert!(seen.insert(c), "大类 {c} 重复登记");
    }
}

#[test]
fn every_category_has_at_least_one_entry() {
    for c in CATEGORIES {
        assert!(
            count_of(c) >= 1,
            "大类 {c} 零条目——空类不许挂在下池（占位的类先别登记）"
        );
    }
}

#[test]
fn entries_fields_nonempty_and_cat_known() {
    for (i, e) in COMPONENTS.iter().enumerate() {
        assert!(!e.name.is_empty(), "条目 #{i} 名空");
        assert!(!e.symbol.is_empty(), "条目 #{i}（{}）symbol 空", e.name);
        assert!(!e.file.is_empty(), "条目 #{i}（{}）file 空", e.name);
        assert!(!e.spec.is_empty(), "条目 #{i}（{}）spec 空", e.name);
        assert!(!e.tests.is_empty(), "条目 #{i}（{}）tests 空", e.name);
        assert!(!e.desc.is_empty(), "条目 #{i}（{}）desc 空", e.name);
        assert!(
            CATEGORIES.contains(&e.cat),
            "条目 #{}（{}）的 cat「{}」不在大类表里",
            i,
            e.name,
            e.cat
        );
    }
}

#[test]
fn names_unique() {
    let mut seen = std::collections::HashSet::new();
    for e in COMPONENTS {
        assert!(
            seen.insert(e.name),
            "组件名 {} 重复（跳框标题会撞车）",
            e.name
        );
    }
}

/// 棘轮本体：symbol 字符串必须真实出现在 file 里（crate 根为基准）
#[test]
fn symbol_actually_exists_in_file() {
    let root = env!("CARGO_MANIFEST_DIR");
    for e in COMPONENTS {
        let path = format!("{root}/{}", e.file);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("条目 {} 的 file {} 读不到: {err}", e.name, e.file));
        assert!(
            text.contains(e.symbol),
            "条目 {} 的 symbol「{}」在 {} 里不存在——表与代码漂移",
            e.name,
            e.symbol,
            e.file
        );
    }
}

#[test]
fn entries_of_matches_count_of_and_cat() {
    for c in CATEGORIES {
        let idx = entries_of(c);
        assert_eq!(idx.len(), count_of(c));
        for &i in &idx {
            assert_eq!(COMPONENTS[i].cat, c, "entries_of 返回的条目 cat 不符");
        }
    }
    // 并集 = 全集（每个条目恰好归一个大类）
    let total: usize = CATEGORIES.iter().map(|c| count_of(c)).sum();
    assert_eq!(total, COMPONENTS.len());
}

#[test]
fn status_labels_nonempty() {
    for s in [
        CompStatus::Active,
        CompStatus::Mothballed,
        CompStatus::Planned,
    ] {
        assert!(!s.label().is_empty());
    }
}
