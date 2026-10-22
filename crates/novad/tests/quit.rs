//! The real novad binary: `quit` over the socket stops it cleanly.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn quit_replies_then_exits_and_removes_the_socket() {
    let dir = std::env::temp_dir().join(format!("novad-quit-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let socket = dir.join("novad.sock");
    let features = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features");
    let mut novad = Command::new(env!("CARGO_BIN_EXE_novad"))
        .arg("--no-ui")
        .arg("--socket")
        .arg(&socket)
        .arg("--log-dir")
        .arg(dir.join("logs"))
        .arg("--features-dir")
        .arg(features)
        .arg("--selection")
        .arg(dir.join("features.toml"))
        .spawn()
        .unwrap();

    let started = Instant::now();
    let stream = loop {
        if let Ok(stream) = UnixStream::connect(&socket) {
            break stream;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "novad never listened");
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut writer = stream.try_clone().unwrap();
    let mut lines = BufReader::new(stream).lines();
    lines.next().unwrap().unwrap(); // hello
    writeln!(writer, r#"{{"v":2,"id":1,"cmd":"quit"}}"#).unwrap();
    assert_eq!(lines.next().unwrap().unwrap(), r#"{"v":2,"id":1,"result":null}"#);

    assert!(novad.wait().unwrap().success());
    assert!(!socket.exists());
    std::fs::remove_dir_all(dir).unwrap();
}
