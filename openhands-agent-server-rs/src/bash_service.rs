use crate::models::{BashCommand, BashEvent, BashEventPage, BashOutput, ExecuteBashRequest};
use chrono::Utc;
use glob::glob;
use std::fs;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;
use uuid::Uuid;

#[derive(Clone)]
pub struct BashEventService {
    pub bash_events_dir: PathBuf,
}

impl BashEventService {
    pub fn new(bash_events_dir: PathBuf) -> Self {
        fs::create_dir_all(&bash_events_dir).expect("Failed to create bash events dir");
        Self { bash_events_dir }
    }

    fn save_event(&self, event: &BashEvent) {
        let timestamp_str = event.timestamp().format("%Y%m%d%H%M%S");
        let kind = match event {
            BashEvent::BashCommand(_) => "BashCommand",
            BashEvent::BashOutput(_) => "BashOutput",
        };

        let filename = match event {
            BashEvent::BashCommand(c) => format!("{}_{}_{}", timestamp_str, kind, c.id.simple()),
            BashEvent::BashOutput(o) => format!(
                "{}_{}_{}_{}",
                timestamp_str,
                kind,
                o.command_id.simple(),
                o.id.simple()
            ),
        };

        let path = self.bash_events_dir.join(filename);
        let json = serde_json::to_string_pretty(event).expect("Failed to serialize event");
        fs::write(path, json).expect("Failed to write event file");
    }

    fn load_event(path: PathBuf) -> Option<BashEvent> {
        let content = fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    pub fn start_bash_command(&self, req: ExecuteBashRequest) -> BashCommand {
        let command_id = Uuid::new_v4();
        let bash_command = BashCommand {
            id: command_id,
            timestamp: Utc::now(),
            command: req.command.clone(),
            cwd: req.cwd.clone(),
            timeout: req.timeout.unwrap_or(300),
        };

        // Save initial command event synchronously
        self.save_event(&BashEvent::BashCommand(bash_command.clone()));

        let service = self.clone();
        let cmd_clone = bash_command.clone();

        // Spawn background task
        tokio::spawn(async move {
            service.execute_bash_command_background(cmd_clone).await;
        });

        bash_command
    }

    async fn execute_bash_command_background(&self, command: BashCommand) {
        let mut cmd = Command::new("bash");
        cmd.arg("-c").arg(&command.command);
        if let Some(cwd) = &command.cwd {
            cmd.current_dir(cwd);
        }
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let timeout_duration = Duration::from_secs(command.timeout);

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let out = BashOutput {
                    id: Uuid::new_v4(),
                    timestamp: Utc::now(),
                    command_id: command.id,
                    order: 0,
                    exit_code: Some(-1),
                    stdout: None,
                    stderr: Some(format!("Failed to spawn: {}", e)),
                };
                self.save_event(&BashEvent::BashOutput(out));
                return;
            }
        };

        // For simplicity, we read everything at end for now, or minimal chunking.
        // Implementing full stream chunking like Python requires more complex async loop.
        // Let's stick to reading complete output for first pass of parity to match `execute_bash_command` reliability,
        // but since this is background, we can just wait.

        let wait_output = async {
            let mut stdout = String::new();
            let mut stderr = String::new();
            if let Some(mut out) = child.stdout.take() {
                let _ = out.read_to_string(&mut stdout).await;
            }
            if let Some(mut err) = child.stderr.take() {
                let _ = err.read_to_string(&mut stderr).await;
            }
            let status = child.wait().await;
            (status, stdout, stderr)
        };

        match timeout(timeout_duration, wait_output).await {
            Ok((status_res, stdout, stderr)) => {
                let exit_code = status_res.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
                let out = BashOutput {
                    id: Uuid::new_v4(),
                    timestamp: Utc::now(),
                    command_id: command.id,
                    order: 0, // Simplified single output event
                    exit_code: Some(exit_code),
                    stdout: if stdout.is_empty() {
                        None
                    } else {
                        Some(stdout)
                    },
                    stderr: if stderr.is_empty() {
                        None
                    } else {
                        Some(stderr)
                    },
                };
                self.save_event(&BashEvent::BashOutput(out));
            }
            Err(_) => {
                let _ = child.kill().await;
                let out = BashOutput {
                    id: Uuid::new_v4(),
                    timestamp: Utc::now(),
                    command_id: command.id,
                    order: 0,
                    exit_code: Some(-1),
                    stdout: None,
                    stderr: Some("Command timed out".to_string()),
                };
                self.save_event(&BashEvent::BashOutput(out));
            }
        }
    }

    pub fn get_bash_event(&self, id: Uuid) -> Option<BashEvent> {
        let pattern = self.bash_events_dir.join(format!("*_{}", id.simple()));
        let paths: Vec<_> = glob(pattern.to_str()?)
            .ok()?
            .filter_map(Result::ok)
            .collect();

        if let Some(path) = paths.first() {
            Self::load_event(path.clone())
        } else {
            None
        }
    }

    pub fn search_bash_events(&self, command_id: Option<Uuid>) -> BashEventPage {
        let pattern = if let Some(_cid) = command_id {
            // Find all events with this command id in name
            // Filename formats:
            // Command: TIMESTAMP_BashCommand_CMDID_CMDID (since id=command_id) -- Wait, format is TIMESTAMP_KIND_ID.
            // But for BashCommand ID is CMDID. So TIMESTAMP_BashCommand_CMDID.
            // Output: TIMESTAMP_BashOutput_CMDID_OUTPUTID.
            // So we can glob for *_{cid.simple()}* potentially?
            // Actually Python implementation does: *_{cid.simple()}_* OR *_{cid.simple()} depending on structure.
            // Let's scan all and filter for correctness and simplicity.
            "*"
        } else {
            "*"
        };

        let mut events = Vec::new();
        let full_pattern = self.bash_events_dir.join(pattern);

        if let Ok(entries) = glob(full_pattern.to_str().unwrap_or("")) {
            for entry in entries.filter_map(Result::ok) {
                if let Some(event) = Self::load_event(entry) {
                    let match_cmd = match command_id {
                        Some(cid) => match &event {
                            BashEvent::BashCommand(c) => c.id == cid,
                            BashEvent::BashOutput(o) => o.command_id == cid,
                        },
                        None => true,
                    };

                    if match_cmd {
                        events.push(event);
                    }
                }
            }
        }

        // Sort by timestamp aka filename usually works, or sort explicitly
        events.sort_by_key(|e| e.timestamp());

        BashEventPage {
            items: events,
            next_page_id: None, // No pagination implemented yet
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    async fn wait_for_output(service: &BashEventService, cmd_id: Uuid) -> Option<BashOutput> {
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let page = service.search_bash_events(Some(cmd_id));
            if let Some(event) = page.items.last() {
                if let BashEvent::BashOutput(out) = event {
                    return Some(out.clone());
                }
            }
        }
        None
    }

    #[tokio::test]
    async fn test_run_bash_command_success() {
        let dir = tempdir().unwrap();
        let service = BashEventService::new(dir.path().to_path_buf());

        let req = ExecuteBashRequest {
            command: "echo hello".to_string(),
            cwd: None,
            timeout: Some(5),
        };

        let cmd = service.start_bash_command(req);
        let out = wait_for_output(&service, cmd.id).await.expect("No output");

        assert_eq!(out.exit_code, Some(0));
        assert_eq!(out.stdout, Some("hello\n".to_string()));
    }

    #[tokio::test]
    async fn test_run_bash_command_failure() {
        let dir = tempdir().unwrap();
        let service = BashEventService::new(dir.path().to_path_buf());

        let req = ExecuteBashRequest {
            command: "exit 1".to_string(),
            cwd: None,
            timeout: Some(5),
        };

        let cmd = service.start_bash_command(req);
        let out = wait_for_output(&service, cmd.id).await.expect("No output");

        assert_eq!(out.exit_code, Some(1));
    }

    #[tokio::test]
    async fn test_run_bash_command_timeout() {
        let dir = tempdir().unwrap();
        let service = BashEventService::new(dir.path().to_path_buf());

        let req = ExecuteBashRequest {
            command: "sleep 2".to_string(),
            cwd: None,
            timeout: Some(1),
        };

        let cmd = service.start_bash_command(req);
        let out = wait_for_output(&service, cmd.id).await.expect("No output");

        assert_eq!(out.exit_code, Some(-1));
        assert!(out.stderr.unwrap_or_default().contains("timed out"));
    }

    #[tokio::test]
    async fn test_run_bash_command_cwd() {
        let dir = tempdir().unwrap();
        let service = BashEventService::new(dir.path().to_path_buf());

        let req = ExecuteBashRequest {
            command: "pwd".to_string(),
            cwd: Some("/".to_string()),
            timeout: Some(5),
        };

        let cmd = service.start_bash_command(req);
        let out = wait_for_output(&service, cmd.id).await.expect("No output");

        assert_eq!(out.exit_code, Some(0));
        assert_eq!(
            out.stdout.map(|s| s.trim().to_string()),
            Some("/".to_string())
        );
    }

    #[tokio::test]
    async fn test_search_bash_events() {
        let dir = tempdir().unwrap();
        let service = BashEventService::new(dir.path().to_path_buf());

        // Run first command
        let req1 = ExecuteBashRequest {
            command: "echo cmd1".to_string(),
            cwd: None,
            timeout: Some(5),
        };
        let cmd1 = service.start_bash_command(req1);
        wait_for_output(&service, cmd1.id).await;

        // Run second command
        let req2 = ExecuteBashRequest {
            command: "echo cmd2".to_string(),
            cwd: None,
            timeout: Some(5),
        };
        let cmd2 = service.start_bash_command(req2);
        wait_for_output(&service, cmd2.id).await;

        // Search for cmd1
        let page1 = service.search_bash_events(Some(cmd1.id));
        assert!(page1.items.iter().all(|e| match e {
            BashEvent::BashCommand(c) => c.id == cmd1.id,
            BashEvent::BashOutput(o) => o.command_id == cmd1.id,
        }));

        // Search all
        let page_all = service.search_bash_events(None);
        assert!(page_all.items.len() >= 4); // 2 commands + 2 outputs
    }
}
