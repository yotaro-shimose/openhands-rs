use anyhow::Result;
use portable_pty::{Child, CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const CMD_END_MARKER: &str = ">>DONE<<";
const INIT_MARKER: &str = ">>INIT_DONE<<";

/// Mimics the Agent's view of a terminal session
pub struct TerminalSession {
    writer: Box<dyn Write + Send>,
    // The shared buffer contains output since last read
    output_buffer: Arc<Mutex<String>>,
    // Keep child process to kill it on drop
    child: Box<dyn Child + Send>,
    // Status of the background reader
    is_alive: Arc<AtomicBool>,
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

impl TerminalSession {
    pub fn new() -> Result<Self> {
        let pty_system = NativePtySystem::default();
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let cmd = CommandBuilder::new("bash");
        let child = pair.slave.spawn_command(cmd)?;

        let mut writer = pair.master.take_writer()?;
        let mut reader = pair.master.try_clone_reader()?;
        let output_buffer = Arc::new(Mutex::new(String::new()));
        let buffer_clone = output_buffer.clone();
        let is_alive = Arc::new(AtomicBool::new(true));
        let is_alive_clone = is_alive.clone();

        thread::spawn(move || {
            let mut buf = [0u8; 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        let s = String::from_utf8_lossy(&buf[0..n]);
                        let mut locked = buffer_clone.lock().unwrap();
                        locked.push_str(&s);
                    }
                    Ok(_) => {
                        // EOF
                        is_alive_clone.store(false, Ordering::Relaxed);
                        break;
                    }
                    Err(_) => {
                        // Error
                        is_alive_clone.store(false, Ordering::Relaxed);
                        break;
                    }
                }
            }
        });

        writeln!(writer, "stty -echo")?;
        // Handshake to ensure shell is ready
        writeln!(writer, "echo \"{}\"", INIT_MARKER)?;

        // Wait for handshake
        let start = Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(5) {
                return Err(anyhow::anyhow!(
                    "Failed to initialize terminal: timeout waiting for handshake"
                ));
            }
            if !is_alive.load(Ordering::Relaxed) {
                return Err(anyhow::anyhow!(
                    "Failed to initialize terminal: background thread exited"
                ));
            }
            {
                let mut locked = output_buffer.lock().unwrap();
                if let Some(idx) = locked.find(INIT_MARKER) {
                    if locked[idx..].contains('\n') {
                        *locked = String::new();
                        break;
                    }
                }
            }
            thread::sleep(Duration::from_millis(10));
        }

        Ok(Self {
            writer,
            output_buffer,
            child,
            is_alive,
        })
    }

    pub fn execute(&mut self, cmd: &str, timeout_ms: u64) -> Result<(String, i32)> {
        // Check health
        if !self.is_alive.load(Ordering::Relaxed) {
            return Err(anyhow::anyhow!("Terminal session is dead"));
        }

        let marker_cmd = format!("{}; echo \"{}:$?\"", cmd, CMD_END_MARKER);

        writeln!(self.writer, "{}", marker_cmd)?;

        let start = Instant::now();
        let duration = Duration::from_millis(timeout_ms);

        loop {
            if start.elapsed() > duration {
                return Ok((self.drain_output(), -1));
            }
            if !self.is_alive.load(Ordering::Relaxed) {
                return Err(anyhow::anyhow!(
                    "Terminal background thread died during execution"
                ));
            }

            let mut found = false;
            {
                let locked = self.output_buffer.lock().unwrap();
                let full_content = &*locked;
                // Scan all occurrences of marker
                for (idx, _) in full_content.match_indices(CMD_END_MARKER) {
                    let after_marker = &full_content[idx + CMD_END_MARKER.len()..];
                    // Expect immediate ':'
                    if after_marker.starts_with(':') {
                        let after_colon = &after_marker[1..];
                        // Check if it starts with digit or -
                        if let Some(first_char) = after_colon.trim_start().chars().next() {
                            if first_char.is_digit(10) || first_char == '-' {
                                // Has digits. Check if we have a newline after digits to ensure parsing is safe
                                if after_colon.contains('\n') {
                                    found = true;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            if found {
                break;
            }

            thread::sleep(Duration::from_millis(50));
        }

        let output = self.drain_output();

        // DEBUG PRINT
        // println!("DEBUG: cmd='{}' output='{:?}'", cmd, output);

        if let Some(pos) = output.rfind(CMD_END_MARKER) {
            let marker_part = &output[pos..];
            let clean_marker = marker_part.trim();
            let parts: Vec<&str> = clean_marker.split(':').collect();
            let exit_code = if parts.len() >= 2 {
                let dig: String = parts[1]
                    .chars()
                    .take_while(|c| c.is_digit(10) || *c == '-')
                    .collect();
                dig.parse().unwrap_or(-1)
            } else {
                -1
            };

            let actual_output = &output[..pos];
            return Ok((actual_output.trim_end().to_string(), exit_code));
        }

        Ok((output, -1))
    }

    fn drain_output(&mut self) -> String {
        let mut locked = self.output_buffer.lock().unwrap();
        let current_content = locked.clone();
        *locked = String::new();
        current_content
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_simple_command() {
        let mut session = TerminalSession::new().unwrap();
        let (output, exit_code) = session.execute("echo hello", 1000).unwrap();
        assert_eq!(exit_code, 0);
        assert!(output.contains("hello"));
    }

    #[test]
    fn test_execute_state_persistence() {
        let mut session = TerminalSession::new().unwrap();
        session.execute("export MY_VAR=123", 1000).unwrap();

        let (output, exit_code) = session.execute("echo $MY_VAR", 1000).unwrap();
        assert_eq!(exit_code, 0);
        assert!(output.contains("123"));
    }

    #[test]
    fn test_execute_directory_persistence() {
        let mut session = TerminalSession::new().unwrap();
        session.execute("mkdir -p /tmp/test_dir", 1000).unwrap();
        session.execute("cd /tmp/test_dir", 1000).unwrap();

        let (output, exit_code) = session.execute("pwd", 1000).unwrap();
        assert_eq!(exit_code, 0);
        assert!(output.contains("/tmp/test_dir"));
    }

    #[test]
    fn test_execute_timeout() {
        let mut session = TerminalSession::new().unwrap();
        let (_output, exit_code) = session.execute("sleep 2", 500).unwrap();
        assert_eq!(exit_code, -1);
    }

    #[test]
    fn test_execute_exit_code() {
        let mut session = TerminalSession::new().unwrap();

        let (_output, exit_code) = session.execute("false", 1000).unwrap();
        assert_eq!(exit_code, 1);
    }
}
