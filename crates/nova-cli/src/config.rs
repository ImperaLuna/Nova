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

/// Runs `editor` on `file`. Like git, the editor is a shell command line, so
/// `EDITOR="code --wait"` works.
fn edit(editor: &str, file: &Path) -> Result<(), String> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    }
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$1\""))
        .arg(editor)
        .arg(file)
        .status()
        .map_err(|e| format!("can't run {editor}: {e}"))?;
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
        let dir = std::env::temp_dir().join(format!("nova-config-edit-{}", std::process::id()));
        let file = dir.join("with space/config.toml");
        // An "editor" with its own arguments, writing down what it was given.
        edit(&format!("sh -c 'printf \"%s\" \"$0\" > \"{}\"'", dir.join("seen").display()), &file).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("seen")).unwrap(), file.display().to_string());
        assert!(edit("false", &file).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
