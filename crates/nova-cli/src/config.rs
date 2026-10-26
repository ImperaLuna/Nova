//! `nova config`: where Nova's settings live, or config.toml in an editor.

use std::path::Path;
use std::process::Command;

/// Editors to try, in the order shells use them.
const EDITOR_VARS: &[&str] = &["VISUAL", "EDITOR"];

#[derive(clap::Args)]
pub struct ConfigArgs {
    /// Open config.toml in $VISUAL or $EDITOR. novad applies it when you save.
    #[arg(long)]
    edit: bool,
}

pub fn run(args: &ConfigArgs) -> Result<(), String> {
    let dir = nova_paths::config_dir();
    if !args.edit {
        println!("{}", dir.display());
        return Ok(());
    }
    let editor = EDITOR_VARS
        .iter()
        .find_map(|var| std::env::var(var).ok().filter(|editor| !editor.trim().is_empty()))
        .ok_or("neither VISUAL nor EDITOR is set; set one, or open the file yourself")?;
    edit(&editor, &dir.join("config.toml"))
}

/// Runs `editor` on `file`. The editor may carry its own arguments, split
/// like a shell would (`EDITOR="code --wait"`), but nothing in it is run as
/// shell code.
fn edit(editor: &str, file: &Path) -> Result<(), String> {
    let argv =
        shlex::split(editor).filter(|argv| !argv.is_empty()).ok_or(format!("can't read the editor {editor:?}"))?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    }
    // No timeout: this is the user's editor in the foreground, open as long
    // as they edit, and Ctrl-C stops it.
    let status =
        Command::new(&argv[0]).args(&argv[1..]).arg(file).status().map_err(|e| format!("can't run {editor}: {e}"))?;
    match status.success() {
        true => Ok(()),
        false => Err(format!("{editor} exited with {status}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editor_gets_the_file_as_one_argument() {
        let dir = nova_test_support::TempDir::new("config-edit");
        let file = dir.join("with space/config.toml");
        // An "editor" with its own arguments, writing down what it was given.
        edit(&format!("sh -c 'printf \"%s\" \"$0\" > \"{}\"'", dir.join("seen").display()), &file).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("seen")).unwrap(), file.display().to_string());
        assert!(edit("false", &file).is_err());
        // Shell syntax in the editor is an argument, never run.
        let marker = dir.join("ran");
        let _ = edit(&format!("true; touch {}", marker.display()), &file);
        assert!(!marker.exists());
        assert!(edit("'unclosed", &file).is_err());
    }
}
