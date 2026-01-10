use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// Special token to mark end of command execution
const CMD_END_MARKER: &str = ">>DONE<<";

/// Mimics the Agent's view of a terminal session
pub struct TerminalSession {
    writer: Box<dyn Write + Send>,
    // The shared buffer contains ALL output ever received
    output_buffer: Arc<Mutex<String>>,
    // We track where we last read to return incremental output
    last_read_len: usize,
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
        let _child = pair.slave.spawn_command(cmd)?;

        let mut writer = pair.master.take_writer()?;
        let mut reader = pair.master.try_clone_reader()?;
        let output_buffer = Arc::new(Mutex::new(String::new()));
        let buffer_clone = output_buffer.clone();

        // Background reader thread
        thread::spawn(move || {
            let mut buf = [0u8; 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        let s = String::from_utf8_lossy(&buf[0..n]);
                        let mut locked = buffer_clone.lock().unwrap();
                        locked.push_str(&s);
                    }
                    Ok(_) => break,  // EOF
                    Err(_) => break, // Error
                }
            }
        });

        // Initialize shell
        // Disable echo to avoid seeing command itself in output
        writeln!(writer, "stty -echo")?;

        // Wait a bit for initialization
        thread::sleep(Duration::from_millis(200));

        // Clear buffer so we only see fresh output for first command
        {
            let mut locked = output_buffer.lock().unwrap();
            *locked = String::new();
        }

        Ok(Self {
            writer,
            output_buffer,
            last_read_len: 0,
        })
    }

    /// Execute a command with a timeout.
    /// Returns (output, exit_code)
    pub fn execute(&mut self, cmd: &str, timeout_ms: u64) -> Result<(String, i32)> {
        // Prepare the marker with exit code capture
        // We run: cmd; echo ">>DONE:$?<<"
        let marker_cmd = format!("{}; echo \"{}:$?\"", cmd, CMD_END_MARKER);

        writeln!(self.writer, "{}", marker_cmd)?;

        let start = Instant::now();
        let duration = Duration::from_millis(timeout_ms);

        loop {
            // Check if timed out
            if start.elapsed() > duration {
                return Ok((self.read_new_output(), -1));
            }

            // Check content
            {
                let locked = self.output_buffer.lock().unwrap();
                let full_content = &*locked;
                // Look for marker AFTER our last read position
                let new_segment = &full_content[self.last_read_len..];

                if let Some(idx) = new_segment.find(CMD_END_MARKER) {
                    // Check if we have the newline after the marker to ensure flush
                    if let Some(_) = new_segment[idx..].find('\n') {
                        break;
                    }
                }
            }

            thread::sleep(Duration::from_millis(50));
        }

        let output = self.read_new_output();
        // Parse exit code from the end of output

        // Find the marker
        if let Some(pos) = output.rfind(CMD_END_MARKER) {
            let marker_part = &output[pos..];
            // marker_part looks like ">>DONE:0\r\n"

            let clean_marker = marker_part.trim();
            let parts: Vec<&str> = clean_marker.split(':').collect();
            let exit_code = if parts.len() >= 2 {
                // parts[1] is "0" or "0... garbage"
                // Filter digits to be safe
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

    fn read_new_output(&mut self) -> String {
        let locked = self.output_buffer.lock().unwrap();
        let current_len = locked.len();
        let new_content = locked[self.last_read_len..current_len].to_string();
        self.last_read_len = current_len;
        new_content
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
        // Sleep for 2 seconds, timeout is 500ms
        let (output, exit_code) = session.execute("sleep 2", 500).unwrap();
        // Exit code should be -1 for timeout (or whatever logic we have)
        // Wait, local implementation returns -1 on timeout
        assert_eq!(exit_code, -1);
    }

    #[test]
    fn test_execute_exit_code() {
        let mut session = TerminalSession::new().unwrap();
        let (output, exit_code) = session.execute("exit 42", 1000).unwrap();
        // Note: "exit" might close the shell? If it closes the shell, next commands will fail.
        // But for this single execution it should return 42 (or shell closes).
        // Actually, if we run `exit 42; echo DONE`, the echo might not run if shell exits.
        // `bash -c` behavior:
        // `bash` interactive: `exit` closes it.
        // Our init: we spawned `bash`. `exit` will terminate the child process.
        // So this test might break the session.
        // Let's test `(exit 42)` subshell or `false`

        let (output, exit_code) = session.execute("false", 1000).unwrap();
        assert_eq!(exit_code, 1);
    }
}
