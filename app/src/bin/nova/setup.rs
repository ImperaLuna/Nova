use crate::host::{Distro, Host};
use nova_features::{Gpu, Manifest, Selection};
use novad::checks;
use std::fmt;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const UNIT_TEMPLATE: &str = include_str!("../../../../packaging/systemd/nova.service.in");
const UNIT_NOVA_PLACEHOLDER: &str = "@nova@";
const APP_DROP_IN: &str = include_str!("../../../../packaging/systemd/app-.service.d/nova.conf");
const APP_DROP_IN_PATH: &str = "app-.service.d/nova.conf";
const NIRI_RULES: &str = include_str!("../../../../packaging/compositor/niri.kdl");
const NIRI_RULES_FILE: &str = "niri.kdl";
const HYPRLAND_RULES: &str = include_str!("../../../../packaging/compositor/hyprland.conf");
const HYPRLAND_LUA_RULES: &str = include_str!("../../../../packaging/compositor/hyprland.lua");
const HYPRLAND_RULES_FILE: &str = "hyprland.conf";
const HYPRLAND_LUA_RULES_FILE: &str = "hyprland.lua";
const AUR_HELPERS: &[&str] = &["paru", "yay"];
const FLAKE_HINT: &str = "On NixOS, Nova comes from its flake: add `github:ImperaLuna/Nova` to your flake inputs \
     and set `programs.nova.enable = true;` in its home-manager or NixOS module. Turn features on with \
     `programs.nova.features.<id>.enable` (or `programs.nova.featuresFile`) and rebuild, which installs what they \
     need. `nova setup` changes nothing here.";

#[derive(clap::Args)]
pub struct SetupArgs {
    #[arg(long)]
    dry_run: bool,
    #[arg(long, short)]
    yes: bool,
}

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub units_dir: PathBuf,
    pub niri_config: Option<PathBuf>,
    pub hyprland_config: Option<PathBuf>,
    pub nova: PathBuf,
}

impl Paths {
    fn of_this_user() -> Result<Self, String> {
        let config_home = nova_paths::config_home().ok_or("HOME isn't set")?;
        let niri_config = nova_paths::path_var("NIRI_CONFIG").unwrap_or_else(|| config_home.join("niri/config.kdl"));
        let niri_config = niri_config.is_file().then_some(niri_config);
        let hyprland_config = [HYPRLAND_LUA_RULES_FILE, HYPRLAND_RULES_FILE]
            .map(|name| config_home.join("hypr").join(name))
            .into_iter()
            .find(|config| config.is_file());
        Ok(Self {
            config_dir: nova_paths::config_dir(),
            units_dir: config_home.join("systemd/user"),
            niri_config,
            hyprland_config,
            nova: std::env::current_exe().map_err(|e| format!("can't find the nova binary: {e}"))?,
        })
    }
}

pub struct Facts {
    pub host: Host,
    pub manifests: Vec<Manifest>,
    pub selection: Selection,
    pub path_var: String,
    pub paths: Paths,
    pub yes: bool,
}

impl Facts {
    fn gather(yes: bool) -> Result<Self, String> {
        let paths = Paths::of_this_user()?;
        let manifests = nova_features::load_all(&nova_features::default_dir()).map_err(|e| e.to_string())?;
        let selection = checks::read_selection(&[paths.config_dir.join("features.toml")])?;
        Ok(Self {
            host: Host::detect(Path::new("/")),
            manifests,
            selection,
            path_var: checks::session_path(),
            paths,
            yes,
        })
    }

    fn on_path(&self, name: &str) -> Option<PathBuf> {
        nova_paths::find_program(name, &self.path_var)
    }
}

#[derive(Debug, PartialEq)]
pub enum Step {
    Run { why: String, argv: Vec<String> },
    Create { path: PathBuf, contents: String },
    Replace { path: PathBuf, contents: String },
    AppendLine { path: PathBuf, line: String },
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Run { why, argv } => write!(f, "{why}:\n    {}", argv.join(" ")),
            Self::Create { path, .. } => write!(f, "create {}", path.display()),
            Self::Replace { path, .. } => write!(f, "write {}", path.display()),
            Self::AppendLine { path, line } => write!(f, "add to {}:\n    {line}", path.display()),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Plan {
    UseTheFlake,
    Steps(Vec<Step>),
}

pub fn plan(facts: &Facts) -> Result<Plan, String> {
    if facts.host.distro == Distro::NixOs {
        return Ok(Plan::UseTheFlake);
    }
    let Some(distro) = facts.host.distro.packages_key() else {
        return Err(format!(
            "nova setup installs packages on Arch, Fedora and Debian and distros built on them, not on {}. \
             Install the packages in features/*/feature.toml yourself, then run `nova doctor`.",
            facts.host.distro
        ));
    };
    let mut steps = package_steps(facts, distro);
    let paths = &facts.paths;
    steps.push(Step::Create { path: paths.config_dir.join("config.toml"), contents: default_config() });
    steps.push(Step::Create {
        path: paths.config_dir.join("features.toml"),
        contents: default_selection(&facts.manifests),
    });
    steps.push(Step::Replace {
        path: paths.units_dir.join(crate::systemd::UNIT),
        contents: UNIT_TEMPLATE.replace(UNIT_NOVA_PLACEHOLDER, &unit_quoted(&paths.nova)),
    });
    steps.push(Step::Replace { path: paths.units_dir.join(APP_DROP_IN_PATH), contents: APP_DROP_IN.into() });
    steps.push(Step::Run {
        why: "reload systemd's units".into(),
        argv: words(&["systemctl", "--user", "daemon-reload"]),
    });
    steps.push(Step::Run {
        why: "start novad with the graphical session".into(),
        argv: words(&["systemctl", "--user", "enable", crate::systemd::UNIT]),
    });
    if let Some(niri_config) = &paths.niri_config {
        let rules = paths.config_dir.join(NIRI_RULES_FILE);
        steps.push(Step::Replace { path: rules.clone(), contents: NIRI_RULES.into() });
        steps.push(Step::AppendLine { path: niri_config.clone(), line: format!("include {}", quoted(&rules)) });
    }
    if let Some(hyprland_config) = &paths.hyprland_config {
        steps.extend(hyprland_steps(hyprland_config, &paths.config_dir));
    }
    Ok(Plan::Steps(steps))
}

fn hyprland_steps(hyprland_config: &Path, config_dir: &Path) -> [Step; 2] {
    let lua = hyprland_config.extension().is_some_and(|extension| extension == "lua");
    let (file, contents) = match lua {
        true => (HYPRLAND_LUA_RULES_FILE, HYPRLAND_LUA_RULES),
        false => (HYPRLAND_RULES_FILE, HYPRLAND_RULES),
    };
    let rules = config_dir.join(file);
    let line = match lua {
        true => format!("require({})", quoted(&rules)),
        false => format!("source = {}", rules.display()),
    };
    [
        Step::Replace { path: rules, contents: contents.into() },
        Step::AppendLine { path: hyprland_config.to_owned(), line },
    ]
}

fn package_steps(facts: &Facts, distro: &str) -> Vec<Step> {
    let gpu = gpu_for_packages(&facts.host);
    let needed: Vec<&Manifest> = facts
        .manifests
        .iter()
        .filter(|manifest| facts.selection.is_on(manifest))
        .filter(|manifest| manifest.check.binaries.is_empty() || !manifest.missing_binaries(&facts.path_var).is_empty())
        .filter(|manifest| !manifest.packages(distro, gpu).is_empty())
        .collect();
    let mut packages: Vec<String> = Vec::new();
    for manifest in &needed {
        for package in manifest.packages(distro, gpu) {
            if !packages.contains(package) {
                packages.push(package.clone());
            }
        }
    }
    if packages.is_empty() {
        return vec![];
    }
    let yes = facts.yes;
    let ids = needed.iter().map(|manifest| manifest.id.as_str()).collect::<Vec<_>>().join(", ");
    let mut why = format!("install packages for {ids}");
    let install = match distro {
        "arch" => match AUR_HELPERS.iter().find(|helper| facts.on_path(helper).is_some()) {
            Some(helper) => [words(&[helper, "-S", "--needed"]), flag(yes, "--noconfirm")].concat(),
            None => {
                why += " (packages only in the AUR need paru or yay installed first)";
                [words(&["sudo", "pacman", "-S", "--needed"]), flag(yes, "--noconfirm")].concat()
            }
        },
        "fedora" => [words(&["sudo", "dnf", "install"]), flag(yes, "-y")].concat(),
        _ => [words(&["sudo", "apt-get", "install"]), flag(yes, "-y")].concat(),
    };
    vec![Step::Run { why, argv: [install, packages].concat() }]
}

fn gpu_for_packages(host: &Host) -> Option<Gpu> {
    if host.nvidia_driver && host.gpus.contains(&Gpu::Nvidia) {
        return Some(Gpu::Nvidia);
    }
    host.gpus.iter().copied().find(|gpu| *gpu != Gpu::Nvidia)
}

fn quoted(path: &Path) -> String {
    let escaped = path.display().to_string().replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn unit_quoted(path: &Path) -> String {
    // systemd reads `%` in a command line as a specifier:
    // https://www.freedesktop.org/software/systemd/man/latest/systemd.unit.html#Specifiers
    quoted(path).replace('%', "%%")
}

fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

fn flag(on: bool, flag: &str) -> Vec<String> {
    if on { vec![flag.to_owned()] } else { vec![] }
}

fn default_config() -> String {
    format!(
        "# Nova's settings. novad applies a change as soon as you save.\n\
         \n\
         # [theme]\n\
         # name = \"{}\"  # a file in Nova's themes folder, without .toml\n\
         # mode = \"dark\"  # or \"light\"\n\
         \n\
         # [wallpaper]  # or `nova wallpaper <file>`\n\
         # path = \"~/Pictures/Wallpapers/dusk.jpg\"  # on every screen\n\
         # (a video, a Wallpaper Engine folder or a Workshop ID moves, with the\n\
         #  wallpaper-engine feature on)\n\
         # folder = \"~/Pictures/Wallpapers\"  # what `nova toggle wallpaper` offers\n\
         # [wallpaper.outputs]\n\
         # DP-2 = \"~/Pictures/Wallpapers/tide.jpg\"  # one screen's own\n",
        checks::DEFAULT_THEME_ID
    )
}

fn default_selection(manifests: &[Manifest]) -> String {
    let listed = |default: bool| {
        manifests
            .iter()
            .filter(|manifest| manifest.default == default)
            .map(|manifest| match &manifest.default_mode {
                Some(default) => format!(
                    "#   {}: {} (modes {}; {default} unless picked below)\n",
                    manifest.id,
                    manifest.name,
                    manifest.modes.join(", ")
                ),
                None => format!("#   {}: {}\n", manifest.id, manifest.name),
            })
            .collect::<String>()
    };
    format!(
        "# Which of Nova's features are on. After turning one on, run `nova setup`\n\
         # again so it installs what that feature needs.\n\
         #\n\
         # On unless listed in `disabled`:\n{}\
         # Off unless listed in `enabled`:\n{}\
         enabled = []\n\
         disabled = []\n\
         \n\
         # A feature with modes runs in the one picked here:\n\
         # [modes]\n\
         # nova-files = \"full\"\n",
        listed(true),
        listed(false)
    )
}

pub fn run(args: &SetupArgs) -> Result<(), String> {
    let facts = Facts::gather(args.yes)?;
    let steps = match plan(&facts)? {
        Plan::UseTheFlake => {
            println!("{FLAKE_HINT}");
            return Ok(());
        }
        Plan::Steps(steps) => steps,
    };
    println!("Setting up Nova for {}:\n", facts.host.distro);
    for (number, step) in steps.iter().enumerate() {
        println!("{}. {step}", number + 1);
    }
    println!();
    if args.dry_run {
        println!("Dry run: nothing changed.");
        return Ok(());
    }
    if !installed() {
        return Err(NOT_INSTALLED.into());
    }
    if !args.yes && !confirmed()? {
        return Err("stopped; nothing changed.".into());
    }
    for step in &steps {
        execute(step, &facts.path_var)?;
    }
    finish(&facts.path_var)
}

const NOT_INSTALLED: &str = "this nova runs from a source checkout (no share/nova next to its binary), \
     and the service it would install couldn't find Nova's features, themes and UI. \
     Install Nova first (install.sh or a package), then run that nova's setup.";

fn installed() -> bool {
    nova_paths::installed_or_checkout("features").is_absolute()
}

fn confirmed() -> Result<bool, String> {
    if !std::io::stdin().is_terminal() {
        return Err("setup asks before it changes anything; run it in a terminal, or with --yes".into());
    }
    print!("Go ahead? [y/N] ");
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).map_err(|e| e.to_string())?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}

pub fn execute(step: &Step, path_var: &str) -> Result<(), String> {
    match step {
        Step::Run { argv, .. } => {
            let (program, args) = argv.split_first().ok_or("empty command")?;
            let status = Command::new(program)
                .args(args)
                .env("PATH", path_var)
                .status()
                .map_err(|e| format!("can't run {program}: {e}"))?;
            if !status.success() {
                return Err(format!("`{}` failed ({status})", argv.join(" ")));
            }
        }
        Step::Create { path, contents } => {
            if !path.exists() {
                write(path, contents)?;
            }
        }
        Step::Replace { path, contents } => write(path, contents)?,
        Step::AppendLine { path, line } => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
            if !text.lines().any(|existing| existing.trim() == line) {
                let separator = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
                let mut file = std::fs::OpenOptions::new()
                    .append(true)
                    .open(path)
                    .map_err(|e| format!("can't open {}: {e}", path.display()))?;
                writeln!(file, "{separator}{line}").map_err(|e| format!("can't write {}: {e}", path.display()))?;
            }
        }
    }
    Ok(())
}

fn write(path: &Path, contents: &str) -> Result<(), String> {
    create_parent(path)?;
    let name = path.file_name().ok_or_else(|| format!("{} isn't a file", path.display()))?;
    let partial = path.with_file_name(format!(".{}.{}.partial", name.to_string_lossy(), std::process::id()));
    let written = std::fs::write(&partial, contents).and_then(|()| std::fs::rename(&partial, path));
    written.map_err(|e| {
        let _ = std::fs::remove_file(&partial);
        format!("can't write {}: {e}", path.display())
    })
}

fn create_parent(path: &Path) -> Result<(), String> {
    match path.parent() {
        Some(dir) => std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display())),
        None => Ok(()),
    }
}

fn finish(path_var: &str) -> Result<(), String> {
    if nova_paths::find_program(checks::GPUI_UI, path_var).is_none() {
        return Err(format!(
            "everything else is set up, but {} isn't on PATH, so Nova has no UI: so far only Nova's Nix flake \
             ships it. Put it on PATH, then run `nova doctor`.",
            checks::GPUI_UI
        ));
    }
    println!("Nova is set up. Start it with `nova`; `nova doctor` checks the rest.");
    println!("On Hyprland without uwsm, also add `exec-once = nova` to hyprland.conf so novad gets the session.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifests() -> Vec<Manifest> {
        nova_features::load_all(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../features")).unwrap()
    }

    fn paths(home: &Path) -> Paths {
        Paths {
            config_dir: home.join(".config/nova"),
            units_dir: home.join(".config/systemd/user"),
            niri_config: None,
            hyprland_config: None,
            nova: home.join(".local/bin/nova"),
        }
    }

    fn facts(distro: Distro, gpus: Vec<Gpu>, nvidia_driver: bool) -> Facts {
        Facts {
            host: Host { distro, gpus, nvidia_driver },
            manifests: manifests(),
            selection: Selection { enabled: vec!["recording".into()], ..Selection::default() },
            path_var: String::new(),
            paths: paths(Path::new("/home/me")),
            yes: false,
        }
    }

    fn shown(facts: &Facts) -> String {
        match plan(facts).unwrap() {
            Plan::UseTheFlake => "use the flake".into(),
            Plan::Steps(steps) => steps.iter().map(|step| format!("{step}\n")).collect(),
        }
    }

    #[test]
    fn arch_with_nvidia_on_niri() {
        let mut facts = facts(Distro::Arch, vec![Gpu::Intel, Gpu::Nvidia], true);
        facts.paths.niri_config = Some("/home/me/.config/niri/config.kdl".into());
        insta::assert_snapshot!(shown(&facts));
    }

    #[test]
    fn arch_with_amd() {
        insta::assert_snapshot!(shown(&facts(Distro::Arch, vec![Gpu::Amd], false)));
    }

    #[test]
    fn fedora() {
        insta::assert_snapshot!(shown(&facts(Distro::Fedora, vec![Gpu::Amd], false)));
    }

    #[test]
    fn debian() {
        insta::assert_snapshot!(shown(&facts(Distro::Debian, vec![Gpu::Intel], false)));
    }

    #[test]
    fn nixos_uses_the_flake_and_unknown_distros_are_refused() {
        assert_eq!(plan(&facts(Distro::NixOs, vec![], false)), Ok(Plan::UseTheFlake));
        let refused = plan(&facts(Distro::Other("gentoo".into()), vec![], false)).unwrap_err();
        assert!(refused.contains("not on gentoo"), "{refused}");
    }

    #[test]
    fn nouveau_gets_the_other_gpus_packages() {
        let host = Host { distro: Distro::Arch, gpus: vec![Gpu::Nvidia], nvidia_driver: false };
        assert_eq!(gpu_for_packages(&host), None);
        let hybrid = Host { gpus: vec![Gpu::Intel, Gpu::Nvidia], ..host };
        assert_eq!(gpu_for_packages(&hybrid), Some(Gpu::Intel));
    }

    struct Home(nova_test_support::TempDir);

    impl Home {
        fn new(name: &str) -> Self {
            let home = nova_test_support::TempDir::new(&format!("setup-{name}"));
            std::fs::create_dir_all(home.join("usr/bin")).unwrap();
            for bin in
                ["wf-recorder", "grim", "wl-copy", "xdg-open", "awww", "awww-daemon", "upower", "bluetoothctl", "paru"]
            {
                nova_test_support::write_program(&home.join("usr/bin"), bin, "exit 0");
            }
            Self(home)
        }

        fn path_var(&self) -> String {
            self.0.join("usr/bin").display().to_string()
        }
    }

    #[test]
    fn installed_features_are_skipped_and_the_aur_helper_is_used() {
        let home = Home::new("installed");
        let facts = Facts { path_var: home.path_var(), ..facts(Distro::Arch, vec![Gpu::Amd], false) };
        let Plan::Steps(steps) = plan(&facts).unwrap() else { panic!() };
        assert!(!steps.iter().any(|step| matches!(step, Step::Run { why, .. } if why.starts_with("install"))));
        std::fs::remove_file(home.0.join("usr/bin/wf-recorder")).unwrap();
        let Plan::Steps(steps) = plan(&facts).unwrap() else { panic!() };
        let Step::Run { why, argv } = &steps[0] else { panic!("{:?}", steps[0]) };
        assert_eq!(why, "install packages for recording");
        assert_eq!(argv, &words(&["paru", "-S", "--needed", "wf-recorder"]));
    }

    #[test]
    fn yes_answers_the_package_manager_too() {
        let facts = Facts { yes: true, ..facts(Distro::Debian, vec![], false) };
        let Plan::Steps(steps) = plan(&facts).unwrap() else { panic!() };
        assert_eq!(
            steps[0],
            Step::Run {
                why: "install packages for bluetooth, peripherals, recording, screenshot".into(),
                argv: words(&[
                    "sudo",
                    "apt-get",
                    "install",
                    "-y",
                    "bluez",
                    "upower",
                    "wf-recorder",
                    "grim",
                    "wl-clipboard",
                    "xdg-utils"
                ]),
            }
        );
    }

    #[test]
    fn running_the_file_steps_twice_is_safe() {
        let home = Home::new("files");
        let niri_config = home.0.join(".config/niri/config.kdl");
        std::fs::create_dir_all(niri_config.parent().unwrap()).unwrap();
        std::fs::write(&niri_config, "input {\n}").unwrap();
        let paths = Paths { niri_config: Some(niri_config.clone()), ..paths(&home.0) };
        let facts = Facts { path_var: home.path_var(), paths: paths.clone(), ..facts(Distro::Arch, vec![], false) };
        let Plan::Steps(steps) = plan(&facts).unwrap() else { panic!() };
        let file_steps: Vec<&Step> = steps.iter().filter(|step| !matches!(step, Step::Run { .. })).collect();
        for _ in 0..2 {
            for step in &file_steps {
                execute(step, &facts.path_var).unwrap();
            }
            std::fs::write(paths.config_dir.join("config.toml"), "# mine\n").unwrap();
        }
        assert_eq!(std::fs::read_to_string(paths.config_dir.join("config.toml")).unwrap(), "# mine\n");
        let niri = std::fs::read_to_string(&niri_config).unwrap();
        let include = format!("include \"{}\"", paths.config_dir.join(NIRI_RULES_FILE).display());
        assert_eq!(niri, format!("input {{\n}}\n{include}\n"));
        assert_eq!(std::fs::read_to_string(paths.config_dir.join(NIRI_RULES_FILE)).unwrap(), NIRI_RULES);
        let unit = std::fs::read_to_string(paths.units_dir.join("nova.service")).unwrap();
        assert!(unit.contains(&format!("ExecStart=@\"{}\" Nova daemon", paths.nova.display())), "{unit}");
    }

    #[test]
    fn the_niri_include_escapes_the_path() {
        let mut facts = facts(Distro::Arch, vec![], false);
        facts.paths.config_dir = r#"/home/a "b"\c/.config/nova"#.into();
        facts.paths.niri_config = Some("/home/me/.config/niri/config.kdl".into());
        let Plan::Steps(steps) = plan(&facts).unwrap() else { panic!() };
        let Some(Step::AppendLine { line, .. }) = steps.iter().find(|step| matches!(step, Step::AppendLine { .. }))
        else {
            panic!("{steps:?}")
        };
        assert_eq!(line, r#"include "/home/a \"b\"\\c/.config/nova/niri.kdl""#);
    }

    #[test]
    fn hyprland_sources_the_rules() {
        let mut facts = facts(Distro::Fedora, vec![], false);
        facts.paths.hyprland_config = Some("/home/me/.config/hypr/hyprland.conf".into());
        let Plan::Steps(steps) = plan(&facts).unwrap() else { panic!() };
        let Some(Step::AppendLine { path, line }) = steps.iter().find(|step| matches!(step, Step::AppendLine { .. }))
        else {
            panic!("{steps:?}")
        };
        assert_eq!(path, Path::new("/home/me/.config/hypr/hyprland.conf"));
        assert_eq!(line, "source = /home/me/.config/nova/hyprland.conf");
    }

    #[test]
    fn a_lua_hyprland_config_requires_the_lua_rules() {
        let config_dir = Path::new(r#"/home/a "b"/.config/nova"#);
        let [Step::Replace { path: rules, contents }, Step::AppendLine { path, line }] =
            hyprland_steps(Path::new("/home/me/.config/hypr/hyprland.lua"), config_dir)
        else {
            panic!()
        };
        assert_eq!(rules, config_dir.join("hyprland.lua"));
        assert!(contents.contains("allow_session_lock_restore = true"), "{contents}");
        assert_eq!(path, Path::new("/home/me/.config/hypr/hyprland.lua"));
        assert_eq!(line, r#"require("/home/a \"b\"/.config/nova/hyprland.lua")"#);
    }

    #[test]
    fn the_unit_keeps_a_path_with_spaces_as_one_word() {
        assert_eq!(unit_quoted(Path::new("/home/me/my apps/50%/nova")), r#""/home/me/my apps/50%%/nova""#);
    }

    #[test]
    fn commands_run_with_the_path_they_were_found_on() {
        let home = Home::new("run");
        nova_test_support::write_program(&home.0.join("usr/bin"), "nova-setup-test-helper", "exit 0");
        let step = Step::Run { why: "test".into(), argv: words(&["nova-setup-test-helper"]) };
        let path_var = format!("{}:/bin:/usr/bin", home.path_var());
        assert_eq!(execute(&step, &path_var), Ok(()));
    }

    #[test]
    fn the_default_files_read_as_defaults() {
        let home = Home::new("defaults");
        let config = home.0.join("config.toml");
        std::fs::write(&config, default_config()).unwrap();
        let themes = Path::new(env!("CARGO_MANIFEST_DIR")).join("../themes");
        assert_eq!(checks::read_config(&config, &themes), Ok(vec![]));
        let selection = home.0.join("features.toml");
        std::fs::write(&selection, default_selection(&manifests())).unwrap();
        assert_eq!(checks::read_selection(&[selection]), Ok(Selection::default()));
    }
}
