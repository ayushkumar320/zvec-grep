use std::{
    error::Error,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Output, Stdio},
    sync::mpsc::{self, Receiver},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use serde_json::json;
use tempfile::TempDir;

struct ServerGuard {
    binary: PathBuf,
    home: PathBuf,
    active: bool,
}

impl ServerGuard {
    fn stop(&mut self) -> Result<Output, std::io::Error> {
        let output = Command::new(&self.binary)
            .args(["server", "off", "--home"])
            .arg(&self.home)
            .output()?;
        if output.status.success() {
            self.active = false;
        }
        Ok(output)
    }
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = Command::new(&self.binary)
                .args(["server", "off", "--home"])
                .arg(&self.home)
                .output();
        }
    }
}

struct StdioBridge {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    reader: Option<JoinHandle<()>>,
}

impl StdioBridge {
    fn spawn(binary: &Path, home: &Path, listen: &str) -> Result<Self, Box<dyn Error>> {
        let mut child = Command::new(binary)
            .args([
                "server",
                "--stdio",
                "--home",
                path_text(home)?,
                "--listen",
                listen,
                "--mcp-toolset",
                "full",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child.stdin.take().ok_or("stdio bridge has no stdin")?;
        let stdout = child.stdout.take().ok_or("stdio bridge has no stdout")?;
        let (sender, lines) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            stdin: Some(stdin),
            lines,
            reader: Some(reader),
        })
    }

    fn request(
        &mut self,
        request: &serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn Error>> {
        let id = request.get("id").cloned().ok_or("request has no id")?;
        let stdin = self.stdin.as_mut().ok_or("stdio bridge is closed")?;
        writeln!(stdin, "{request}")?;
        stdin.flush()?;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let line = self.lines.recv_timeout(remaining)?;
            let response = serde_json::from_str::<serde_json::Value>(&line)?;
            if response.get("id") == Some(&id) {
                return Ok(response);
            }
        }
    }

    fn notify(&mut self, notification: &serde_json::Value) -> Result<(), Box<dyn Error>> {
        let stdin = self.stdin.as_mut().ok_or("stdio bridge is closed")?;
        writeln!(stdin, "{notification}")?;
        stdin.flush()?;
        Ok(())
    }

    fn close(mut self) -> Result<(), Box<dyn Error>> {
        drop(self.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait()? {
                if let Some(reader) = self.reader.take() {
                    let _ = reader.join();
                }
                if status.success() {
                    return Ok(());
                }
                return Err(format!("stdio bridge exited with {status}").into());
            }
            if Instant::now() >= deadline {
                self.child.kill()?;
                let _ = self.child.wait();
                return Err("stdio bridge did not exit after stdin closed".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for StdioBridge {
    fn drop(&mut self) {
        drop(self.stdin.take());
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[test]
fn server_on_exposes_only_agent_search_and_off_stops_it() -> Result<(), Box<dyn Error>> {
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_zg"));
    let home = TempDir::new()?;
    let port = available_port()?;
    let listen = format!("127.0.0.1:{port}");
    let output = Command::new(&binary)
        .args([
            "server",
            "on",
            "--home",
            path_text(home.path())?,
            "--listen",
            &listen,
            "--mcp-toolset",
            "agent",
        ])
        .output()?;
    assert_command_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Server: ready"));
    assert!(stdout.contains("MCP toolset: agent"));
    let mut guard = ServerGuard {
        binary,
        home: home.path().to_owned(),
        active: true,
    };

    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "zg-test", "version": "1" }
        }
    });
    let response = post_json(port, None, &initialize.to_string())?;
    assert!(response.contains("\"name\":\"zvec-grep\""));
    let session = response
        .lines()
        .find_map(|line| {
            line.strip_prefix("mcp-session-id:")
                .map(str::trim)
                .map(str::to_owned)
        })
        .ok_or("initialize response did not contain mcp-session-id")?;

    let initialized = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    });
    let _ = post_json(port, Some(&session), &initialized.to_string())?;
    let list = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    let response = post_json(port, Some(&session), &list.to_string())?;
    assert!(response.contains("zvec_grep_search"));
    assert!(!response.contains("zvec_grep_index"));
    assert!(!response.contains("zvec_grep_rg"));
    assert!(response.contains("\"maximum\":50"));

    let call = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "zvec_grep_search",
            "arguments": {
                "root": home.path(),
                "query": "daemon lifecycle"
            }
        }
    });
    let response = post_json(port, Some(&session), &call.to_string())?;
    assert!(response.contains("error[ZG.ENGINE.NOT_FOUND]"));
    assert!(response.contains("workspace index at"));
    assert!(response.contains("no workspace manifest was found"));
    assert!(response.contains("\"isError\":true"));

    // CLI administration uses the typed daemon protocol rather than the
    // public MCP toolset, so status remains available with the agent profile.
    let cli_status = Command::new(&guard.binary)
        .args(["status", "--mode", "server", "--home"])
        .arg(&guard.home)
        .arg(home.path())
        .output()?;
    assert_command_success(&cli_status);
    let cli_stdout = String::from_utf8_lossy(&cli_status.stdout);
    assert!(cli_stdout.contains("Workspace index: missing"));
    assert!(cli_stdout.contains(&format!("Root: {}", home.path().display())));

    let output = guard.stop()?;
    assert_command_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("Server: stopped"));
    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)]
fn full_toolset_exposes_lifecycle_tools_and_runs_managed_rg() -> Result<(), Box<dyn Error>> {
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_zg"));
    let home = TempDir::new()?;
    std::fs::write(
        home.path().join("sample.txt"),
        "resident workspace manager\n",
    )?;
    let port = available_port()?;
    let listen = format!("127.0.0.1:{port}");
    let output = Command::new(&binary)
        .args([
            "server",
            "on",
            "--home",
            path_text(home.path())?,
            "--listen",
            &listen,
            "--mcp-toolset",
            "full",
        ])
        .output()?;
    assert_command_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("MCP toolset: full"));
    let mut guard = ServerGuard {
        binary,
        home: home.path().to_owned(),
        active: true,
    };

    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "zg-full-test", "version": "1" }
        }
    });
    let response = post_json(port, None, &initialize.to_string())?;
    let session = response
        .lines()
        .find_map(|line| {
            line.strip_prefix("mcp-session-id:")
                .map(str::trim)
                .map(str::to_owned)
        })
        .ok_or("initialize response did not contain mcp-session-id")?;
    let initialized = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    });
    let _ = post_json(port, Some(&session), &initialized.to_string())?;

    let list = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    let response = post_json(port, Some(&session), &list.to_string())?;
    for name in [
        "zvec_grep_search",
        "zvec_grep_index",
        "zvec_grep_index_drop",
        "zvec_grep_rg",
        "zvec_grep_index_status",
        "zvec_grep_server_status",
    ] {
        assert!(response.contains(name), "full toolset is missing {name}");
    }

    let rg = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "zvec_grep_rg",
            "arguments": {
                "root": home.path(),
                "command": "rg -F resident sample.txt"
            }
        }
    });
    let response = post_json(port, Some(&session), &rg.to_string())?;
    assert!(response.contains("matchedBy=lexical sample.txt:1"));
    assert!(response.contains("resident workspace manager"));
    assert!(response.contains("\"isError\":false"));

    let index = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "zvec_grep_index",
            "arguments": { "root": home.path(), "wait": false }
        }
    });
    let response = post_json(port, Some(&session), &index.to_string())?;
    assert!(response.contains("\"state\":\"queued\""));
    assert!(response.contains("\"job_id\":"));
    assert!(!response.contains("generation-"));
    assert!(response.contains("\"isError\":false"));

    let status = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {
            "name": "zvec_grep_server_status",
            "arguments": {}
        }
    });
    let response = post_json(port, Some(&session), &status.to_string())?;
    assert!(response.contains("active_runtimes"));
    assert!(response.contains("\"active_runtimes\":1"));
    assert!(response.contains("structuredContent"));
    assert!(response.contains("\"isError\":false"));

    let index_status = json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "tools/call",
        "params": {
            "name": "zvec_grep_index_status",
            "arguments": { "root": home.path() }
        }
    });
    let response = post_json(port, Some(&session), &index_status.to_string())?;
    assert!(response.contains("\"indexed\":false"));
    assert!(response.contains("\"index_policy\":\"undecided\""));
    assert!(response.contains("\"source\":\"unindexed\""));
    assert!(response.contains("\"runtime\":"));
    assert!(response.contains("\"job_state\":\"failed\""));
    assert!(response.contains("\"isError\":false"));

    let output = guard.stop()?;
    assert_command_success(&output);
    Ok(())
}

#[test]
fn concurrent_stdio_bootstraps_share_one_resident_daemon() -> Result<(), Box<dyn Error>> {
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_zg"));
    let home = TempDir::new()?;
    let port = available_port()?;
    let listen = format!("127.0.0.1:{port}");
    let mut guard = ServerGuard {
        binary: binary.clone(),
        home: home.path().to_owned(),
        active: true,
    };
    let mut bridges = (0..4)
        .map(|_| StdioBridge::spawn(&binary, home.path(), &listen))
        .collect::<Result<Vec<_>, _>>()?;

    for (index, bridge) in bridges.iter_mut().enumerate() {
        let initialize = json!({
            "jsonrpc": "2.0",
            "id": index + 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "zg-stdio-test", "version": "1" }
            }
        });
        let response = bridge.request(&initialize)?;
        assert_eq!(response["result"]["serverInfo"]["name"], "zvec-grep");
        bridge.notify(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }))?;
    }

    let list = bridges[0].request(&json!({
        "jsonrpc": "2.0",
        "id": 100,
        "method": "tools/list",
        "params": {}
    }))?;
    let tools = list["result"]["tools"]
        .as_array()
        .ok_or("tools/list did not return an array")?;
    assert_eq!(tools.len(), 6);

    for bridge in bridges {
        bridge.close()?;
    }

    let status = Command::new(&binary)
        .args(["server", "status", "--home"])
        .arg(home.path())
        .output()?;
    assert_command_success(&status);
    let stdout = String::from_utf8_lossy(&status.stdout);
    assert!(stdout.contains("Server: ready"));
    assert!(stdout.contains("MCP toolset: full"));

    let output = guard.stop()?;
    assert_command_success(&output);
    Ok(())
}

fn available_port() -> Result<u16, std::io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

fn post_json(port: u16, session: Option<&str>, body: &str) -> Result<String, Box<dyn Error>> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let session_header = session.map_or_else(String::new, |value| {
        format!("Mcp-Session-Id: {value}\r\nMCP-Protocol-Version: 2025-11-25\r\n")
    });
    write!(
        stream,
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\n{session_header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

fn path_text(path: &Path) -> Result<&str, Box<dyn Error>> {
    path.to_str()
        .ok_or_else(|| "temporary path is not valid UTF-8".into())
}

fn assert_command_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
