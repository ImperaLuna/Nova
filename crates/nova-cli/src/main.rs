//! `nova`: the command-line side of Nova.
//!
//! Today: `features` (inspect manifests and the packages they need) and
//! `ping` (talk to a running novad). `setup`, `doctor` and `ipc` come next.

use nova_features::{DISTROS, Gpu, Manifest};
use nova_protocol::{Command, Envelope, Request, encode};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage:
  nova features list
  nova features packages --distro <nix|arch|fedora|debian> [--gpu <nvidia|amd|intel>] [--enable <id>...]
  nova ping [--socket PATH]

environment:
  NOVA_FEATURES_DIR   where feature manifests live (default: the installed set, else ./features)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["features", "list"] => features_list(),
        ["features", "packages", rest @ ..] => features_packages(rest),
        ["ping", rest @ ..] => ping(rest),
        ["-h" | "--help" | "help"] => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(format!("unknown command\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("nova: {message}");
            ExitCode::FAILURE
        }
    }
}

fn manifests() -> Result<Vec<Manifest>, String> {
    nova_features::load_all(&nova_features::default_dir()).map_err(|e| e.to_string())
}

fn features_list() -> Result<(), String> {
    for m in manifests()? {
        let flag = if m.required {
            "required"
        } else if m.default {
            "on"
        } else {
            "off"
        };
        println!("{:<16} {:<9} {}", m.id, flag, m.description);
    }
    Ok(())
}

/// Prints the packages to install, one per line, for the features that would
/// be on. This is the core of `nova setup --dry-run`.
fn features_packages(args: &[&str]) -> Result<(), String> {
    let mut distro = None;
    let mut gpu = None;
    let mut enabled = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().copied().ok_or(format!("{arg} needs a value"));
        match *arg {
            "--distro" => distro = Some(value()?),
            "--gpu" => gpu = Some(parse_gpu(value()?)?),
            "--enable" => enabled.push(value()?.to_owned()),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let distro = distro.ok_or("--distro is required")?;
    if !DISTROS.contains(&distro) {
        return Err(format!("unknown distro {distro} (known: {})", DISTROS.join(", ")));
    }
    let manifests = manifests()?;
    if let Some(bad) = enabled.iter().find(|id| !manifests.iter().any(|m| &m.id == *id)) {
        return Err(format!("no feature called {bad}"));
    }
    let selection = nova_features::Selection { enabled, disabled: vec![] };
    let mut packages: Vec<&String> =
        manifests.iter().filter(|m| selection.is_on(m)).flat_map(|m| m.packages(distro, gpu)).collect();
    packages.sort();
    packages.dedup();
    for p in packages {
        println!("{p}");
    }
    Ok(())
}

fn parse_gpu(value: &str) -> Result<Gpu, String> {
    match value {
        "nvidia" => Ok(Gpu::Nvidia),
        "amd" => Ok(Gpu::Amd),
        "intel" => Ok(Gpu::Intel),
        other => Err(format!("unknown gpu {other} (known: nvidia, amd, intel)")),
    }
}

fn ping(args: &[&str]) -> Result<(), String> {
    let socket = match args {
        [] => nova_paths::socket(),
        ["--socket", path] => PathBuf::from(path),
        _ => return Err(USAGE.into()),
    };
    let stream = UnixStream::connect(&socket).map_err(|e| format!("can't reach novad at {}: {e}", socket.display()))?;
    let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
    let request = encode(Request { id: Some(1), command: Command::Ping }).map_err(|e| e.to_string())?;
    writer.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
    for line in BufReader::new(stream).lines() {
        let line = line.map_err(|e| e.to_string())?;
        let msg: Envelope<serde_json::Value> = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if msg.body.get("id").and_then(|id| id.as_u64()) == Some(1) {
            println!("{}", msg.body.get("result").unwrap_or(&msg.body));
            return Ok(());
        }
    }
    Err("novad closed the connection without replying".into())
}
