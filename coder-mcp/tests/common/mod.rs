use anyhow::Result;
use coder_mcp::models::{BashEvent, ExecuteBashRequest};
use coder_mcp::runtime::bash::BashEventService;
use std::sync::Arc;
use std::time::Duration;

/// Helper to run a command and wait for specific output
#[allow(dead_code)]
pub async fn run_cmd_and_wait(
    service: Arc<BashEventService>,
    cmd_str: String,
    expect_out: String,
) -> Result<()> {
    let req = ExecuteBashRequest {
        command: cmd_str.clone(),
        cwd: None,
        timeout: Some(30),
    };
    let cmd = service.start_bash_command(req);

    // Poll for output
    let mut collected_output = String::new();
    let mut attempts = 0;
    while attempts < 300 {
        // 10s max
        tokio::time::sleep(Duration::from_millis(100)).await;
        let page = service.search_bash_events(Some(cmd.id));
        for item in page.items {
            if let BashEvent::BashOutput(out) = item {
                if let Some(s) = out.stdout {
                    collected_output.push_str(&s);
                }
                if let Some(e) = out.stderr {
                    collected_output.push_str(&e);
                }
            }
        }
        if collected_output.contains(&expect_out) {
            return Ok(());
        }
        attempts += 1;
    }
    Err(anyhow::anyhow!(
        "Timeout waiting for output '{}' from command '{}'. Got: {}",
        expect_out,
        cmd_str,
        collected_output
    ))
}

#[allow(dead_code)]
pub async fn run_cmd_get_output(service: Arc<BashEventService>, cmd_str: String) -> Result<String> {
    let req = ExecuteBashRequest {
        command: cmd_str.clone(),
        cwd: None,
        timeout: Some(10),
    };
    let cmd = service.start_bash_command(req);

    // Poll for output
    let mut collected_output = String::new();
    let mut attempts = 0;

    // We wait a bit to ensure command finished.
    // Ideally we check for exit code, but BashEventService doesn't explicitly signal "DONE" in search results easily
    // without parsing ALL events. But we can check if we got an exit code event.
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let page = service.search_bash_events(Some(cmd.id));

        let mut finished = false;
        collected_output.clear(); // Rebuild from scratch to ensure correct order/no dupes if we were paging (we aren't really paging here yet)

        for item in page.items {
            if let BashEvent::BashOutput(out) = item {
                if let Some(s) = out.stdout {
                    collected_output.push_str(&s);
                }
                if let Some(e) = out.stderr {
                    collected_output.push_str(&e);
                }
                if out.exit_code.is_some() {
                    finished = true;
                }
            }
        }

        if finished {
            return Ok(collected_output);
        }

        attempts += 1;
        if attempts > 50 {
            return Err(anyhow::anyhow!("Timeout waiting for command '{}'", cmd_str));
        }
    }
}
