mod common;
use anyhow::Result;
use coder_mcp::runtime::bash::BashEventService;
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
async fn test_service_isolation() -> Result<()> {
    // 1. Setup shared directory
    let temp_dir = TempDir::new()?;
    let shared_events_dir = temp_dir.path().join("bash_events");
    std::fs::create_dir(&shared_events_dir)?;

    let workspace_dir = temp_dir.path().join("workspace");
    std::fs::create_dir(&workspace_dir)?;

    // 2. Create TWO services sharing the dir
    // We Wrap in Arc to use with common helpers
    let service1 = Arc::new(BashEventService::new(
        shared_events_dir.clone(),
        Some(workspace_dir.clone()),
    ));
    let service2 = Arc::new(BashEventService::new(
        shared_events_dir.clone(),
        Some(workspace_dir.clone()),
    ));

    // 3. Start Command on S1: "echo S1_UNIQ; sleep 1; echo S1_END"
    // We use common::run_cmd_get_output which waits for exit code

    // We spin them up concurrently
    let s1 = service1.clone();
    let h1 = tokio::spawn(async move {
        common::run_cmd_get_output(s1, "echo S1_UNIQ; sleep 1; echo S2_END".to_string()).await
    });

    let s2 = service2.clone();
    let h2 = tokio::spawn(async move {
        common::run_cmd_get_output(s2, "echo S2_UNIQ; sleep 1; echo S2_END".to_string()).await
    });

    let out1 = h1.await??;
    let out2 = h2.await??;

    println!("S1 Output: {}", out1);
    println!("S2 Output: {}", out2);

    // 4. ASSERTIONS
    assert!(out1.contains("S1_UNIQ"));
    assert!(!out1.contains("S2_UNIQ"), "S1 output leaked S2 content");

    assert!(out2.contains("S2_UNIQ"));
    assert!(!out2.contains("S1_UNIQ"), "S2 output leaked S1 content");

    Ok(())
}
