use crate::client::Client;
use nova_wallpaper_api::{Command, Wallpapers};
use std::path::{Path, PathBuf};

#[derive(clap::Args)]
pub struct WallpaperArgs {
    file: Option<PathBuf>,
    #[arg(long, requires = "file")]
    output: Option<String>,
}

pub fn run(args: &WallpaperArgs, socket: &Path) -> Result<(), String> {
    let mut client = Client::connect(socket)?;
    let Some(file) = &args.file else {
        let answer = client.request(Command::Wallpapers)?;
        let wallpapers: Wallpapers =
            serde_json::from_value(answer).map_err(|e| format!("novad's answer didn't make sense: {e}"))?;
        print!("{}", describe(&wallpapers));
        return Ok(());
    };
    let path = match workshop_id(file) {
        Some(id) => id.to_owned(),
        None => absolute(file)?,
    };
    client.request(Command::SetWallpaper { path, output: args.output.clone() })?;
    Ok(())
}

fn workshop_id(file: &Path) -> Option<&str> {
    let text = file.to_str()?;
    (nova_wallpaper_api::is_workshop_id(text) && !file.exists()).then_some(text)
}

fn absolute(file: &Path) -> Result<String, String> {
    let path = std::fs::metadata(file)
        .and_then(|_| std::path::absolute(file))
        .map_err(|e| format!("can't find {}: {e}", file.display()))?;
    path.to_str().map(str::to_owned).ok_or_else(|| format!("{} isn't valid UTF-8", path.display()))
}

fn describe(wallpapers: &Wallpapers) -> String {
    let mut text = String::new();
    match &wallpapers.path {
        Some(path) => text.push_str(&format!("every output: {path}\n")),
        None if wallpapers.outputs.is_empty() => text.push_str("no wallpaper set\n"),
        None => {}
    }
    for (output, path) in &wallpapers.outputs {
        text.push_str(&format!("{output}: {path}\n"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_every_output_then_each_of_its_own() {
        assert_eq!(describe(&Wallpapers::default()), "no wallpaper set\n");
        let wallpapers = Wallpapers { path: Some("/a.jpg".into()), outputs: [("DP-2".into(), "/b.png".into())].into() };
        assert_eq!(describe(&wallpapers), "every output: /a.jpg\nDP-2: /b.png\n");
        let only_one = Wallpapers { path: None, outputs: [("DP-2".into(), "/b.png".into())].into() };
        assert_eq!(describe(&only_one), "DP-2: /b.png\n");
    }
}
