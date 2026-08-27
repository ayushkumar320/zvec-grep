use std::{error::Error, fs, process::Command};

use tempfile::TempDir;

#[test]
fn direct_and_server_modes_share_embedded_lexical_behavior() -> Result<(), Box<dyn Error>> {
    let root = TempDir::new()?;
    fs::write(
        root.path().join("sample.txt"),
        "before\nresident keyword search\nafter\n",
    )?;
    let binary = env!("CARGO_BIN_EXE_zg");
    let run = |mode: &str| {
        Command::new(binary)
            .current_dir(root.path())
            .args([
                "query",
                "--mode",
                mode,
                "--rg",
                "-F",
                "resident keyword",
                ".",
            ])
            .output()
    };

    let direct = run("direct")?;
    let server = run("server")?;
    assert!(
        direct.status.success(),
        "direct stderr: {}",
        String::from_utf8_lossy(&direct.stderr)
    );
    assert!(
        server.status.success(),
        "server stderr: {}",
        String::from_utf8_lossy(&server.stderr)
    );
    assert_eq!(direct.stdout, server.stdout);
    assert_eq!(
        String::from_utf8(direct.stdout)?,
        "sample.txt\n  2: resident keyword search\n"
    );
    Ok(())
}
