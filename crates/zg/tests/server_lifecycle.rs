use std::{
    error::Error,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::Duration,
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
    assert!(response.contains("error_code: capability_unavailable"));
    assert!(response.contains("capability unavailable: query"));
    assert!(response.contains("\"isError\":true"));

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

    let status = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "zvec_grep_server_status",
            "arguments": {}
        }
    });
    let response = post_json(port, Some(&session), &status.to_string())?;
    assert!(response.contains("active_runtimes"));
    assert!(response.contains("structuredContent"));
    assert!(response.contains("\"isError\":false"));

    let index_status = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {
            "name": "zvec_grep_index_status",
            "arguments": { "root": home.path() }
        }
    });
    let response = post_json(port, Some(&session), &index_status.to_string())?;
    assert!(response.contains("error_code: capability_unavailable"));
    assert!(response.contains("capability unavailable: inspect"));
    assert!(response.contains("\"isError\":true"));

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
