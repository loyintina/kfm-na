//! ws_live_spec.rs — ws 连接层 B 档钉（live）：真连本机 kfmv4 8021 /ws 闭环
//!
//! 默认 #[ignore]——依赖运行中的 kfmv4 服务器。
//! 手动跑：cargo test --test ws_live_spec -- --ignored
//!
//! 判卷：open(echo) → opened → output 含印记 → exited(0)，一步不许缺。

#[test]
#[ignore]
fn live_echo_闭环() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let mut events = Vec::new();
        let run = kfm_na::conn::echo_roundtrip(
            "ws://127.0.0.1:8021/ws",
            "echo KFM-NA-LIVE-OK",
            &mut |stage| events.push(stage.to_string()),
        )
        .await
        .expect("闭环失败");
        // 事件序列：connected → opened → output(≥1) → exited
        assert_eq!(events.first().map(String::as_str), Some("connected"));
        assert_eq!(events.get(1).map(String::as_str), Some("opened"));
        assert!(events.iter().any(|s| s == "output"));
        assert_eq!(events.last().map(String::as_str), Some("exited"));
        assert!(run.outputs.concat().contains("KFM-NA-LIVE-OK"));
        assert_eq!(run.exit_code, 0);
        assert!(!run.session_id.is_empty());
    });
}
