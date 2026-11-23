use std::path::Path;
use std::process::Command;

const EDITOR_VARS: &[&str] = &["VISUAL", "EDITOR"];

#[derive(clap::Args)]
pub struct ConfigArgs {
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

fn edit(editor: &str, file: &Path) -> Result<(), String> {
    let argv =
        shlex::split(editor).filter(|argv| !argv.is_empty()).ok_or(format!("can't read the editor {editor:?}"))?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    }
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
        edit(&format!("sh -c 'printf \"%s\" \"$0\" > \"{}\"'", dir.join("seen").display()), &file).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("seen")).unwrap(), file.display().to_string());
        assert!(edit("false", &file).is_err());
        let marker = dir.join("ran");
        let _ = edit(&format!("true; touch {}", marker.display()), &file);
        assert!(!marker.exists());
        assert!(edit("'unclosed", &file).is_err());
    }
}
