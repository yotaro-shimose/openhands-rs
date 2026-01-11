mod common;
use anyhow::Result;
use coder_mcp::runtime::bash::BashEventService;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::task::JoinHandle;

#[tokio::test]
async fn test_service_concurrency() -> Result<()> {
    // Shared resources
    let temp_root = TempDir::new()?;
    let shared_events_dir = temp_root.path().join("bash_events");
    std::fs::create_dir(&shared_events_dir)?;

    // Scenario 1: 5 Sessions sharing exact same workspace
    let shared_workspace = temp_root.path().join("shared_ws");
    std::fs::create_dir(&shared_workspace)?;

    // Scenario 2: 5 Sessions with unique workspaces
    let unique_ws_root = temp_root.path().join("unique_ws");
    std::fs::create_dir(&unique_ws_root)?;

    let mut handles: Vec<JoinHandle<Result<()>>> = Vec::new();

    // Spawn Group 1 (Shared Workspace)
    for i in 0..5 {
        let events_dir = shared_events_dir.clone();
        let ws_dir = shared_workspace.clone();

        handles.push(tokio::spawn(async move {
            let session_id = format!("SHARED_{}", i);
            let service = Arc::new(BashEventService::new(events_dir, Some(ws_dir)));

            // 1. Export unique var
            common::run_cmd_and_wait(
                service.clone(),
                format!("export MY_ID={}", session_id),
                "".to_string(),
            )
            .await?;

            // 2. Create unique dir and cd into it
            common::run_cmd_and_wait(
                service.clone(),
                format!("mkdir -p dir_{} && cd dir_{}", i, i),
                "".to_string(),
            )
            .await?;

            // 3. Verify PWD is correct
            common::run_cmd_and_wait(service.clone(), "pwd".to_string(), format!("dir_{}", i))
                .await?;

            // 4. Verify ENV is isolated (and persistent for self)
            common::run_cmd_and_wait(
                service.clone(),
                "echo $MY_ID".to_string(),
                session_id.clone(),
            )
            .await?;

            // 5. Heavy concurrency check: sleep + echo
            common::run_cmd_and_wait(
                service.clone(),
                format!("sleep 1; echo FINISHED_{}", session_id),
                format!("FINISHED_{}", session_id),
            )
            .await?;

            Ok(())
        }));
    }

    // Spawn Group 2 (Unique Workspaces)
    for i in 0..5 {
        let events_dir = shared_events_dir.clone(); // Still sharing events dir!
        let ws_dir = unique_ws_root.join(format!("ws_{}", i));
        std::fs::create_dir(&ws_dir)?;

        handles.push(tokio::spawn(async move {
            let session_id = format!("UNIQUE_{}", i);
            let service = Arc::new(BashEventService::new(events_dir, Some(ws_dir)));

            // 1. Export unique var
            common::run_cmd_and_wait(
                service.clone(),
                format!("export MY_ID={}", session_id),
                "".to_string(),
            )
            .await?;

            // 2. Verify ENV
            common::run_cmd_and_wait(
                service.clone(),
                "echo $MY_ID".to_string(),
                session_id.clone(),
            )
            .await?;

            // 3. Verify PWD (should be root of its unique ws initially)
            common::run_cmd_and_wait(
                service.clone(),
                "mkdir sub && cd sub".to_string(),
                "".to_string(),
            )
            .await?;
            common::run_cmd_and_wait(service.clone(), "pwd".to_string(), "/sub".to_string())
                .await?;

            // 4. Heavy concurrency check
            common::run_cmd_and_wait(
                service.clone(),
                format!("sleep 1; echo FINISHED_{}", session_id),
                format!("FINISHED_{}", session_id),
            )
            .await?;

            Ok(())
        }));
    }

    // Await all
    for h in handles {
        h.await??;
    }

    Ok(())
}
