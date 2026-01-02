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
                    order: 0,
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
        let paths: Vec<_> = glob(pattern.to_str().unwrap_or(""))
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .collect();

        if let Some(path) = paths.first() {
            Self::load_event(path.clone())
        } else {
            None
        }
    }

    pub fn search_bash_events(&self, command_id: Option<Uuid>) -> BashEventPage {
        let mut events = Vec::new();
        let full_pattern = self.bash_events_dir.join("*");

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

        events.sort_by_key(|e| e.timestamp());

        BashEventPage {
            items: events,
            next_page_id: None,
        }
    }
}
