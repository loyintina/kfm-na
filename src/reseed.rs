//! reseed.rs — BAR-186 臂①：v4 推流播种对账合并（重连快照打底）。
//!
//! 旧路：每次播种（含对账重播，当前会话 5s 一档）都把 capture 全滚动
//! 缓冲（万行级）从头解析建全新画布——相同前缀重放，观感与算力双费。
//! 本件按 immutable-history 模型对账：旧播种文本是新播种的前缀（历史
//! 不可变，只长不改）时，产出「追加 k 行 + 重画 rows 行」的尾块喂给
//! 既有画布，万行前缀一次不重放；对账失败（清史/resize 重排/超一屏
//! 洪峰）回落全量重建（merge_capture 同款判负回落，tmux_ctl.rs:181）。
//!
//! 纯逻辑件（A 档）：进出都是字符串/字节，零平台依赖。尾块的终端语义
//! （LF 顶行入史 / CUP+EL+CNL 逐行重画 / 游标归位+?25l）由等价钉
//! 锁死：reseed 尾块喂旧画布 ≡ 新 capture 全量重建（网格文本+游标
//! 逐格同）。一致性宪法：服务器 capture 永远是真源，本地快照只是
//! 显示缓存——对账只听 capture，不做双向同步。

/// 播种对账裁决
#[derive(Debug, PartialEq, Eq)]
pub enum ReseedPlan {
    /// 对账失败/无旧账 → 全量重建（旧路）
    Rebuild,
    /// 相同前缀不重放：喂给既有画布的 ANSI 尾块
    /// （k 个裸 LF 顶行入史 + 全屏重画 + 游标归位）
    Tail(Vec<u8>),
    /// BAR-215③：canvas_fed 且对账过 → 画布是 %output 活镜像（内容
    /// 领先或齐平快照），调用方一笔不许画。快照后竞速行（capture 执行
    /// 与 %output 入队竞速，窗期照喂进画布）不在快照里——尾块全屏重画
    /// 会把它们整批抹掉（5s 档 × 1s 行锁相实测每档恒丢 1 行，画布史
    /// 差值 50→57 线性漂移、缺号步距恒 5 定罪）。账照归位、pend 照
    /// 补喂，唯有画布不动。
    Skip,
}

/// 对账：旧播种文本 × 新 capture × 屏高 × 画布喂态 → 裁决。
///
/// 判负（Rebuild）全表：屏高为 0 / 任一文本不足一屏 / 历史缩短
/// （对端清史）/ 新历史超旧屏一屏（k>rows，merge_capture 同款回落）/
/// 旧历史段与旧屏顶 k 行在新文本中原样不在位（resize 重排/改写历史）。
///
/// canvas_fed（BAR-215）：Steady/播种窗相 %output 已把新行续喂进画布
/// （画布史 ≠ last_cap 账）时，画布是字节流活镜像——对账过 → Skip
/// 一笔不画：再发 k 个裸 LF = 每档复制 k 行重复带（形态①，94 行 T 行
/// 双份定罪）；全屏重画 = 抹掉快照后竞速行（形态③，每档恒丢 1 行、
/// 缺号步距恒档距定罪）。fed=false（后台会话无 %output / 画布与账
/// 同新）→ 原 k-LF 尾块语义不变。
pub fn plan_reseed(
    old_cap: &str,
    new_cap: &str,
    rows: usize,
    cursor: (u32, u32),
    canvas_fed: bool,
) -> ReseedPlan {
    if rows == 0 {
        return ReseedPlan::Rebuild;
    }
    let old: Vec<&str> = old_cap
        .split('\n')
        .map(|l| l.trim_end_matches('\r'))
        .collect();
    let new: Vec<&str> = new_cap
        .split('\n')
        .map(|l| l.trim_end_matches('\r'))
        .collect();
    if old.len() < rows || new.len() < rows {
        return ReseedPlan::Rebuild;
    }
    let (hist_old, hist_new) = (old.len() - rows, new.len() - rows);
    if hist_new < hist_old {
        return ReseedPlan::Rebuild; // 对端清史
    }
    let k = hist_new - hist_old;
    if k > rows {
        return ReseedPlan::Rebuild; // 超一屏洪峰：被挤进历史的行没全被旧屏抓到过
    }
    // immutable-history 对账：旧历史全段 + 旧屏顶 k 行（挤进历史的那 k 行）
    // 在新文本里必须原样在位
    if old[..hist_old] != new[..hist_old]
        || old[hist_old..hist_old + k] != new[hist_old..hist_old + k]
    {
        return ReseedPlan::Rebuild;
    }
    // BAR-215：画布已被 %output 续喂 = 活镜像，对账过 → 一笔不画。
    // LF 重发 = 重复带；全屏重画 = 抹掉快照后竞速行（capture 执行后
    // 到达的 %output 已照喂进画布，不在快照文本里）。
    if canvas_fed {
        return ReseedPlan::Skip;
    }
    // 尾块拼装：
    // ① 游标先到底行（播种归位后游标在中部，append 必须从底行起）
    // ② k 个裸 LF——底行 LF 滚屏 = 顶行入史，旧屏顶 k 行依次进历史，
    //    内容与次序天然正确（对账已咬死它们 = 新历史的那 k 行），零重写。
    // ③ CUP 回顶 + 逐行 内容+EL（清残）+CNL（下一行行首）全屏重画
    // ④ 游标归位 + 藏光标（Canvas::build 同规）
    let mut out = String::with_capacity(new_cap.len() / 2);
    out.push_str(&format!("\x1b[{rows};1H"));
    for _ in 0..k {
        out.push('\n');
    }
    out.push_str("\x1b[1;1H");
    for (i, line) in new[hist_new..].iter().enumerate() {
        out.push_str(line);
        out.push_str("\x1b[K");
        if i + 1 < rows {
            out.push_str("\x1b[E");
        }
    }
    out.push_str(&format!("\x1b[{};{}H", cursor.1 + 1, cursor.0 + 1));
    out.push_str("\x1b[?25l");
    ReseedPlan::Tail(out.into_bytes())
}
