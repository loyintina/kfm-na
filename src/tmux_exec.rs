//! tmux_exec.rs — 短命 ws 执行通道（B 档胶水：线程 + runtime + 超时）
//!
//! 解析页 tmux 插件的执行腿：起一条短命 ws 会话跑 `tmux …; exit`，
//! 收全部输出直到 exit，结果进 mpsc——壳在事件循环里 try_recv 排水
//! （零新事件源，50ms 唤醒锤自然捎带）。服务端一行不动：复用
//! conn::echo_roundtrip（kfmv4 terminal-pty 协议，`sh -c` 执行）。
//!
//! 纪律：纯逻辑（命令构造/输出解析）全在 tmux_ctl（A 档钉着），本册
//! 只做「送出去、收回来」——出现「该做个决定了」一律下沉 tmux_ctl。

use std::sync::mpsc::Receiver;

/// 执行超时（tmux 命令全是亚秒级；10s = 网络病态兜底）
const EXEC_TIMEOUT_S: u64 = 10;

/// 起一条短命 ws 会话执行 command，输出（含 \r\n 原文）或错误进返回的
/// Receiver。调用方节奏 = 人点按钮频率，一线程一执行不池化
pub fn exec(url: String, command: String) -> Receiver<Result<String, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let send = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt.block_on(async move {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(EXEC_TIMEOUT_S),
                    crate::conn::echo_roundtrip(&url, &command, &mut |_| {}),
                )
                .await
                {
                    Ok(Ok(run)) => Ok(run.outputs.concat()),
                    Ok(Err(e)) => Err(e),
                    Err(_) => Err(format!("执行超时（{EXEC_TIMEOUT_S}s）")),
                }
            }),
            Err(e) => Err(format!("建 runtime 失败: {e}")),
        };
        // 主循环死了发送失败：吞掉——执行线程绝不为上报陪葬（ws 线程同规）
        let _ = tx.send(send);
    });
    rx
}
