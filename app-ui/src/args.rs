//! `nova-ui`'s command line. novad starts it with none: the options are for
//! running it by hand.

use std::path::PathBuf;

pub const USAGE: &str = "\
Usage: nova-ui [--replay <folder>]... [--demo] [--demo-toggles <times>]
               [--demo-panel <name>] [--frames]

  --replay <folder>       talk to a fake novad that replays the folder's
                          recordings (<topic>.jsonl, as `nova ipc subscribe
                          <topic>` prints them) in place of the real one
  --demo                  put test pills on the stage that open test panels:
                          `demo` and `demo-right` on the top strip,
                          `demo-edge` against the right edge and
                          `demo-whole` over the whole screen
  --demo-toggles <times>  with --demo: toggle a panel <times> times (two
                          for one open and close), screen after screen,
                          then quit
  --demo-panel <name>     the panel --demo-toggles toggles; `demo` if not
                          given
  --frames                log each motion's frames and how many came late,
                          and the frames each stage draws at rest";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// The folders of recordings a fake novad replays; none means the real
    /// novad.
    pub replay: Vec<PathBuf>,
    /// Show the test pills and panels.
    pub demo: bool,
    /// How often the demo opens and closes its panel by itself before the
    /// program quits; none leaves it to clicks.
    pub demo_toggles: Option<u32>,
    /// The panel it opens and closes then; none means the `demo` panel.
    pub demo_panel: Option<String>,
    /// Log the stage's frames.
    pub frames: bool,
}

impl Args {
    /// Reads the arguments after the program's name.
    pub fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut parsed = Self::default();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--replay" => parsed.replay.push(args.next().ok_or("--replay needs a folder")?.into()),
                "--demo" => parsed.demo = true,
                "--demo-toggles" => {
                    let times = args.next().ok_or("--demo-toggles needs a number")?;
                    parsed.demo_toggles =
                        Some(times.parse().map_err(|error| format!("--demo-toggles {times:?}: {error}"))?);
                    parsed.demo = true;
                }
                "--demo-panel" => parsed.demo_panel = Some(args.next().ok_or("--demo-panel needs a panel's name")?),
                "--frames" => parsed.frames = true,
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
        let args = parse(&["--replay", "a", "--demo", "--replay", "b", "--frames"]).unwrap();
        let replay = vec!["a".into(), "b".into()];
        assert_eq!(args, Args { replay, demo: true, demo_toggles: None, demo_panel: None, frames: true });
    }

    #[test]
    fn toggling_by_itself_turns_the_demo_on() {
        let args = parse(&["--demo-toggles", "200"]).unwrap();
        assert_eq!(args, Args { demo: true, demo_toggles: Some(200), ..Args::default() });
        let edge = parse(&["--demo-toggles", "2", "--demo-panel", "demo-edge"]).unwrap();
        assert_eq!(edge.demo_panel.as_deref(), Some("demo-edge"));
    }

    #[test]
    fn mistakes_are_named() {
        assert_eq!(parse(&["--replay"]), Err("--replay needs a folder".into()));
        assert_eq!(parse(&["--demo-panel"]), Err("--demo-panel needs a panel's name".into()));
        assert_eq!(parse(&["--prob"]), Err("unknown argument \"--prob\"".into()));
        assert!(parse(&["--demo-toggles", "often"]).unwrap_err().starts_with("--demo-toggles \"often\""));
    }
}
