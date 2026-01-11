mod common;
use anyhow::Result;
use coder_mcp::runtime::bash::BashEventService;
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
async fn test_service_robustness_timeout() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let service = Arc::new(BashEventService::new(
        temp_dir.path().join("events"),
        Some(temp_dir.path().join("ws")),
    ));

    // Run sleep 5 with timeout 2
    // TerminalSession returns -1 on timeout
    let cmd = "sleep 5; echo SHOULD_NOT_SEE".to_string();
    // We expect timeout loop to finish?
    // Wait, common::run_cmd_get_output waits for exit code.
    // TerminalSession returns exit code -1 on timeout.
    // So we should get output (empty or partial) and exit code -1.

    // We can't use run_cmd_get_output because it returns string.
    // We need to inspect exit code.
    // But failing that, we can just check if output contains "SHOULD_NOT_SEE" (it shouldn't)
    // and if it returns relatively quickly (2s + overhead) vs 5s.
    // Actually, start_bash_command takes timeout param.

    let start = std::time::Instant::now();

    // Manual execution with timeout param 2
    let req = coder_mcp::models::ExecuteBashRequest {
        command: cmd.clone(),
        cwd: None,
        timeout: Some(2),
    };
    let bash_cmd = service.start_bash_command(req);

    // Poll for exit
    let mut finished = false;
    for _ in 0..50 {
        // 5s max polling
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let page = service.search_bash_events(Some(bash_cmd.id));
        for item in page.items {
            if let coder_mcp::models::BashEvent::BashOutput(out) = item {
                if out.exit_code.is_some() {
                    assert_eq!(out.exit_code, Some(-1), "Expected exit code -1 for timeout");
                    finished = true;
                }
                if let Some(s) = out.stdout {
                    assert!(
                        !s.contains("SHOULD_NOT_SEE"),
                        "Command should have been killed"
                    );
                }
            }
        }
        if finished {
            break;
        }
    }

    assert!(finished, "Command did not finish (timeout logic failed)");
    assert!(
        start.elapsed().as_secs() < 5,
        "Command took too long (real sleep executed?)"
    );

    Ok(())
}

#[tokio::test]
async fn test_service_robustness_failure_exit_code() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let service = Arc::new(BashEventService::new(
        temp_dir.path().join("events"),
        Some(temp_dir.path().join("ws")),
    ));

    // Run false
    let req = coder_mcp::models::ExecuteBashRequest {
        command: "false".to_string(),
        cwd: None,
        timeout: Some(5),
    };
    let bash_cmd = service.start_bash_command(req);

    let mut found_exit = false;
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let page = service.search_bash_events(Some(bash_cmd.id));
        for item in page.items {
            if let coder_mcp::models::BashEvent::BashOutput(out) = item {
                if out.exit_code.is_some() {
                    assert_eq!(out.exit_code, Some(1), "Expected exit code 1 for 'false'");
                    found_exit = true;
                }
            }
        }
        if found_exit {
            break;
        }
    }
    assert!(found_exit, "Did not find exit code");

    Ok(())
}

#[tokio::test]
async fn test_service_robustness_large_output() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let service = Arc::new(BashEventService::new(
        temp_dir.path().join("events"),
        Some(temp_dir.path().join("ws")),
    ));

    // Generate ~50KB output
    // seq 10000 -> "1\n2\n...10000\n"
    let output = common::run_cmd_get_output(service, "seq 10000".to_string()).await?;

    // Verify length roughly (chars)
    assert!(
        output.len() > 40000,
        "Output too short, got {} bytes",
        output.len()
    );
    assert!(output.contains("10000"), "Output truncated at end?");

    Ok(())
}
