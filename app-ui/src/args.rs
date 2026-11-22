//! `nova-ui`'s command line. novad starts it with none: the options are for
//! running it by hand.

use std::path::PathBuf;

pub const USAGE: &str = "\
Usage: nova-ui [--probe] [--replay <folder>]...

  --probe            open a window that shows what novad sends
  --replay <folder>  talk to a fake novad that replays the folder's
                     recordings (<topic>.jsonl, as `nova ipc subscribe
                     <topic>` prints them) in place of the real one";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// Open the probe window.
    pub probe: bool,
    /// The folders of recordings a fake novad replays; none means the real
    /// novad.
    pub replay: Vec<PathBuf>,
}

impl Args {
    /// Reads the arguments after the program's name.
    pub fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut parsed = Self::default();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--probe" => parsed.probe = true,
                "--replay" => parsed.replay.push(args.next().ok_or("--replay needs a folder")?.into()),
                other => return Err(format!("unknown argument {other:?}")),
            }
        }
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        Args::parse(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn novad_passes_nothing() {
        assert_eq!(parse(&[]), Ok(Args::default()));
    }

    #[test]
    fn the_options_are_read() {
        let args = parse(&["--replay", "a", "--probe", "--replay", "b"]).unwrap();
        assert_eq!(args, Args { probe: true, replay: vec!["a".into(), "b".into()] });
    }

    #[test]
    fn mistakes_are_named() {
        assert_eq!(parse(&["--replay"]), Err("--replay needs a folder".into()));
        assert_eq!(parse(&["--prob"]), Err("unknown argument \"--prob\"".into()));
    }
}
