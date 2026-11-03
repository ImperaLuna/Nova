//! `nova setup`: installs what Nova needs on Arch, Fedora and Debian (and
//! distros built on them), then sets it up for this user. NixOS uses the
//! flake instead.
//!
//! Setup first makes a [`Plan`] from facts about the machine, prints it, and
//! runs it after a yes. Making the plan changes nothing, so `--dry-run` and
//! the tests see exactly what would run. Running it again is safe: files
//! that are the user's (config.toml, features.toml) are only written when
//! missing, and everything else is replaced or skipped when it's already
//! right.
//!
//! The steps:
//! - Packages: each feature that's on lists its distro's packages in
//!   `features/<id>/feature.toml`, by GPU where that matters. A feature whose
//!   `check.binaries` are all there already is skipped.
//! - config.toml and features.toml, commented, when missing.
//! - The systemd user unit, from `packaging/systemd/nova.service.in`, and
//!   its drop-in that starts login apps after it (`app-.service.d`).
//! - A `NovaUI` link to Quickshell in `~/.local/bin`, which novad runs so
//!   the UI shows as NovaUI in ps and btop (the Nix package has a launcher).
//! - niri's blur behind Nova's glass, a rule hiding xembedsniproxy's
//!   window and Nova Files' corners (`packaging/compositor/niri.kdl`),
//!   included from the user's niri config; Hyprland's rules
//!   (`packaging/compositor/hyprland.conf`), sourced from its config.
//!   Hyprland blurs what Quickshell asks for without a rule.
//! - While the `files` feature is on: Nova Files' desktop entry and icon in
//!   the user's data folder, and Nova Files as their default for folders
//!   (the Nix modules do the same); and Nova's Open and Save dialogs: the
//!   portal backend file (`features/files/nova.portal`) in the user's data
//!   folder and, for niri and Hyprland, the portal's config choosing it
//!   (`novad::prefer_nova_file_chooser`), then the portal restarted.

use crate::host::{Distro, Host};
use nova_features::{Gpu, Manifest, Selection};
use nova_quickshell::Range;
use novad::checks;
use std::fmt;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const UNIT_TEMPLATE: &str = include_str!("../../../packaging/systemd/nova.service.in");
const UNIT_NOVA_PLACEHOLDER: &str = "@nova@";
/// Orders login apps after novad (IMP-107), as `<units>/app-.service.d/nova.conf`.
const APP_DROP_IN: &str = include_str!("../../../packaging/systemd/app-.service.d/nova.conf");
const APP_DROP_IN_PATH: &str = "app-.service.d/nova.conf";
const NIRI_RULES: &str = include_str!("../../../packaging/compositor/niri.kdl");
/// Where the niri rules go, in Nova's config folder.
const NIRI_RULES_FILE: &str = "niri.kdl";
const HYPRLAND_RULES: &str = include_str!("../../../packaging/compositor/hyprland.conf");
/// Where the Hyprland rules go, in Nova's config folder.
const HYPRLAND_RULES_FILE: &str = "hyprland.conf";
/// Nova Files (feature `files`): its desktop entry, whose `Exec=` runs
/// `nova` from PATH until setup names the installed one, and its icon.
const FILES_FEATURE: &str = "files";
const FILES_DESKTOP: &str = include_str!("../../../features/files/nova-files.desktop");
const FILES_DESKTOP_ID: &str = "nova-files.desktop";
const FILES_EXEC: &str = "Exec=nova ";
const FILES_ICON: &str = include_str!("../../../features/files/nova-files.svg");
const FILES_ICON_PATH: &str = "icons/hicolor/scalable/apps/nova-files.svg";
const FOLDER_MIME: &str = "inode/directory";
/// Nova's file chooser backend for xdg-desktop-portal, where the portal
/// looks for backends in the user's data folder.
const FILES_PORTAL: &str = include_str!("../../../features/files/nova.portal");
const FILES_PORTAL_PATH: &str = "xdg-desktop-portal/portals/nova.portal";
/// The portal's names for the desktops setup knows (`XDG_CURRENT_DESKTOP`).
const NIRI_DESKTOP: &str = "niri";
const HYPRLAND_DESKTOP: &str = "hyprland";
const PORTAL_SERVICE: &str = "xdg-desktop-portal.service";
/// Fedora's Quickshell, until a Fedora release ships it.
const FEDORA_QUICKSHELL_COPR: &str = "errornointernet/quickshell";
/// AUR helpers, in the order setup prefers them. They install repo packages
/// too, and ask for sudo themselves.
const AUR_HELPERS: &[&str] = &["paru", "yay"];
const FLAKE_HINT: &str = "On NixOS, Nova comes from its flake: add `github:ImperaLuna/Nova` to your flake inputs \
     and set `programs.nova.enable = true;` in its home-manager or NixOS module. Turn features on with \
     `programs.nova.features.<id>.enable` (or `programs.nova.featuresFile`) and rebuild, which installs what they \
     need. `nova setup` changes nothing here.";

#[derive(clap::Args)]
pub struct SetupArgs {
    /// Print the plan and change nothing.
    #[arg(long)]
    dry_run: bool,
    /// Don't ask before running the plan, and answer yes for the package manager.
    #[arg(long, short)]
    yes: bool,
}

/// Where setup writes, for this user.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Nova's config folder (`~/.config/nova`).
    pub config_dir: PathBuf,
    /// systemd's user units (`~/.config/systemd/user`).
    pub units_dir: PathBuf,
    /// `~/.local/bin`, which novad adds to its PATH.
    pub bin_dir: PathBuf,
    /// niri's config file (`~/.config/niri/config.kdl`), when there is one.
    pub niri_config: Option<PathBuf>,
    /// Hyprland's config file (`~/.config/hypr/hyprland.conf`), when there
    /// is one.
    pub hyprland_config: Option<PathBuf>,
    /// The user's config folder (`~/.config`), which holds `mimeapps.list`.
    pub config_home: PathBuf,
    /// The user's data folder (`~/.local/share`), for desktop entries and
    /// icons.
    pub data_home: PathBuf,
    /// The running nova binary, which the unit starts.
    pub nova: PathBuf,
}

impl Paths {
    fn of_this_user() -> Result<Self, String> {
        let home = nova_paths::home().ok_or("HOME isn't set")?;
        let config_home = nova_paths::config_home().ok_or("HOME isn't set")?;
        let niri_config = nova_paths::path_var("NIRI_CONFIG").unwrap_or_else(|| config_home.join("niri/config.kdl"));
        let niri_config = niri_config.is_file().then_some(niri_config);
        let hyprland_config = config_home.join("hypr/hyprland.conf");
        let hyprland_config = hyprland_config.is_file().then_some(hyprland_config);
        Ok(Self {
            config_dir: nova_paths::config_dir(),
            units_dir: config_home.join("systemd/user"),
            bin_dir: home.join(".local/bin"),
            niri_config,
            hyprland_config,
            data_home: nova_paths::data_home().ok_or("HOME isn't set")?,
            config_home,
            nova: std::env::current_exe().map_err(|e| format!("can't find the nova binary: {e}"))?,
        })
    }
}

/// What setup looks at.
pub struct Facts {
    pub host: Host,
    pub manifests: Vec<Manifest>,
    pub selection: Selection,
    /// Where binaries are looked for.
    pub path_var: String,
    pub paths: Paths,
    /// Answer yes for the package manager.
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
    /// Runs a command; setup stops when it fails.
    Run { why: String, argv: Vec<String> },
    /// Writes a file only when it doesn't exist yet.
    Create { path: PathBuf, contents: String },
    /// Writes a file that's Nova's, replacing what's there.
    Replace { path: PathBuf, contents: String },
    /// Adds a line to the end of the user's file, unless it already has it.
    AppendLine { path: PathBuf, line: String },
    /// Points `link` at Quickshell's binary, found on PATH when the step
    /// runs (it may have just been installed).
    LinkQuickshell { link: PathBuf },
    /// Makes `app` the default for `mime` in the user's `mimeapps.list`
    /// under `config_home`.
    SetDefault { config_home: PathBuf, mime: String, app: String },
    /// Makes Nova the portal's file chooser on `desktop`, in the user's
    /// portal config under `config_home`.
    PreferPortal { config_home: PathBuf, desktop: String },
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Run { why, argv } => write!(f, "{why}:\n    {}", argv.join(" ")),
            Self::Create { path, .. } => write!(f, "create {}", path.display()),
            Self::Replace { path, .. } => write!(f, "write {}", path.display()),
            Self::AppendLine { path, line } => write!(f, "add to {}:\n    {line}", path.display()),
            Self::LinkQuickshell { link } => write!(f, "link {} to quickshell", link.display()),
            Self::SetDefault { mime, app, .. } => write!(f, "make {app} the default for {mime}"),
            Self::PreferPortal { desktop, .. } => write!(f, "use Nova's Open and Save dialogs on {desktop}"),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Plan {
    /// NixOS: nothing to do here.
    UseTheFlake,
    Steps(Vec<Step>),
}

/// The plan for `facts`. Fails on a distro setup can't install packages on.
pub fn plan(facts: &Facts) -> Result<Plan, String> {
    if facts.host.distro == Distro::NixOs {
        return Ok(Plan::UseTheFlake);
    }
    let Some(distro) = facts.host.distro.packages_key() else {
        return Err(format!(
            "nova setup installs packages on Arch, Fedora and Debian and distros built on them, not on {}. \
             Install Quickshell and the packages in features/*/feature.toml yourself, then run `nova doctor`.",
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
    steps.push(Step::LinkQuickshell { link: paths.bin_dir.join(checks::UI_PROCESS_NAME) });
    if let Some(niri_config) = &paths.niri_config {
        let rules = paths.config_dir.join(NIRI_RULES_FILE);
        steps.push(Step::Replace { path: rules.clone(), contents: NIRI_RULES.into() });
        steps.push(Step::AppendLine { path: niri_config.clone(), line: format!("include {}", quoted(&rules)) });
    }
    if let Some(hyprland_config) = &paths.hyprland_config {
        let rules = paths.config_dir.join(HYPRLAND_RULES_FILE);
        steps.push(Step::Replace { path: rules.clone(), contents: HYPRLAND_RULES.into() });
        // Hyprland takes the rest of the line as the path, unquoted.
        steps.push(Step::AppendLine { path: hyprland_config.clone(), line: format!("source = {}", rules.display()) });
    }
    if facts.manifests.iter().any(|manifest| manifest.id == FILES_FEATURE && facts.selection.is_on(manifest)) {
        steps.extend(files_steps(paths));
        steps.extend(portal_steps(paths));
    }
    Ok(Plan::Steps(steps))
}

/// Nova Files' desktop entry running this nova, its icon, and it as the
/// default for folders.
fn files_steps(paths: &Paths) -> Vec<Step> {
    let exec = format!("Exec={} ", exec_quoted(&paths.nova));
    vec![
        Step::Replace {
            path: paths.data_home.join("applications").join(FILES_DESKTOP_ID),
            contents: FILES_DESKTOP.replacen(FILES_EXEC, &exec, 1),
        },
        Step::Replace { path: paths.data_home.join(FILES_ICON_PATH), contents: FILES_ICON.into() },
        Step::SetDefault {
            config_home: paths.config_home.clone(),
            mime: FOLDER_MIME.into(),
            app: FILES_DESKTOP_ID.into(),
        },
    ]
}

/// Nova's file chooser backend, chosen on the compositors this user
/// configures, and the portal restarted so apps' next dialog is Nova's.
fn portal_steps(paths: &Paths) -> Vec<Step> {
    let desktops = [(NIRI_DESKTOP, &paths.niri_config), (HYPRLAND_DESKTOP, &paths.hyprland_config)];
    let mut steps =
        vec![Step::Replace { path: paths.data_home.join(FILES_PORTAL_PATH), contents: FILES_PORTAL.into() }];
    for (desktop, config) in desktops {
        if config.is_some() {
            steps.push(Step::PreferPortal { config_home: paths.config_home.clone(), desktop: desktop.into() });
        }
    }
    steps.push(Step::Run {
        why: "restart the desktop portal, if it runs, so it reads that".into(),
        argv: words(&["systemctl", "--user", "try-restart", PORTAL_SERVICE]),
    });
    steps
}

/// `path` as one argument of a desktop entry's `Exec=`, as the Desktop Entry
/// spec quotes one: in double quotes when it has a reserved character, with
/// `"`, `` ` ``, `$` and `\` escaped inside them, then `\` escaped again for
/// the entry's string value, and `%` doubled so it isn't a field code.
fn exec_quoted(path: &Path) -> String {
    const RESERVED: &str = " \t\n\"'\\><~|&;$*?#()`";
    let text = path.display().to_string();
    let argument = if text.chars().any(|c| RESERVED.contains(c)) {
        let mut quoted = String::from('"');
        for c in text.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        quoted.push('"');
        quoted
    } else {
        text
    };
    argument.replace('\\', "\\\\").replace('%', "%%")
}

/// Installs Quickshell while a feature with a UI is on, and the packages of
/// every feature that's on and isn't complete yet.
fn package_steps(facts: &Facts, distro: &str) -> Vec<Step> {
    let gpu = gpu_for_packages(&facts.host);
    let range = nova_quickshell::Range::supported();
    let ui_needed = facts.manifests.iter().any(|manifest| manifest.has_ui() && facts.selection.is_on(manifest));
    let quickshell = if ui_needed { range.packages(distro) } else { &[] };
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
    for package in quickshell.iter().rev() {
        if !packages.contains(package) {
            packages.insert(0, package.clone());
        }
    }
    if packages.is_empty() {
        return vec![];
    }
    let yes = facts.yes;
    let ids = needed.iter().map(|manifest| manifest.id.as_str()).collect::<Vec<_>>().join(", ");
    let mut why = match (quickshell.is_empty(), ids.is_empty()) {
        (false, true) => "install Quickshell for the UI".to_owned(),
        (false, false) => format!("install Quickshell for the UI, and packages for {ids}"),
        _ => format!("install packages for {ids}"),
    };
    let mut steps = Vec::new();
    let install = match distro {
        "arch" => match AUR_HELPERS.iter().find(|helper| facts.on_path(helper).is_some()) {
            Some(helper) => [words(&[helper, "-S", "--needed"]), flag(yes, "--noconfirm")].concat(),
            None => {
                why += " (packages only in the AUR need paru or yay installed first)";
                [words(&["sudo", "pacman", "-S", "--needed"]), flag(yes, "--noconfirm")].concat()
            }
        },
        "fedora" => {
            if !quickshell.is_empty() && facts.on_path("quickshell").is_none() {
                steps.push(Step::Run {
                    why: "add the Quickshell package repository".into(),
                    argv: words(&["sudo", "dnf", "copr", "enable", "-y", FEDORA_QUICKSHELL_COPR]),
                });
            }
            [words(&["sudo", "dnf", "install"]), flag(yes, "-y")].concat()
        }
        _ => [words(&["sudo", "apt-get", "install"]), flag(yes, "-y")].concat(),
    };
    steps.push(Step::Run { why, argv: [install, packages].concat() });
    steps
}

/// The GPU whose variant a feature installs: NVIDIA's when its own driver
/// runs (NVENC needs it; under nouveau NVIDIA's packages wouldn't work),
/// else the first other GPU.
fn gpu_for_packages(host: &Host) -> Option<Gpu> {
    if host.nvidia_driver && host.gpus.contains(&Gpu::Nvidia) {
        return Some(Gpu::Nvidia);
    }
    host.gpus.iter().copied().find(|gpu| *gpu != Gpu::Nvidia)
}

/// `path` as a double-quoted string, with `\\` and `"` escaped: how both
/// KDL and systemd's command lines spell a string.
fn quoted(path: &Path) -> String {
    let escaped = path.display().to_string().replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// `path` as one word of a unit's command line: quoted, so a path with
/// spaces stays one word, with `%` doubled so systemd doesn't read it as a
/// specifier.
fn unit_quoted(path: &Path) -> String {
    quoted(path).replace('%', "%%")
}

fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

fn flag(on: bool, flag: &str) -> Vec<String> {
    if on { vec![flag.to_owned()] } else { vec![] }
}

/// A config.toml that sets nothing, so novad's defaults apply, and shows
/// what can be set.
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
         # DP-2 = \"~/Pictures/Wallpapers/tide.jpg\"  # one screen's own\n\
         \n\
         # [files]  # Nova Files, each from glass (0) to solid (100)\n\
         # frame = 0  # the rows above the files, and the sidebar\n\
         # list = 0  # the file list\n",
        checks::DEFAULT_THEME_ID
    )
}

/// A features.toml that keeps every feature's default, listing them.
fn default_selection(manifests: &[Manifest]) -> String {
    let listed = |default: bool| {
        manifests
            .iter()
            .filter(|manifest| manifest.default == default)
            .map(|manifest| format!("#   {}: {}\n", manifest.id, manifest.name))
            .collect::<String>()
    };
    format!(
        "# Which of Nova's features are on. After turning one on, run `nova setup`\n\
         # again so it installs what that feature needs.\n\
         #\n\
         # On unless listed in `disabled`:\n{}\
         # Off unless listed in `enabled`:\n{}\
         enabled = []\n\
         disabled = []\n",
        listed(true),
        listed(false)
    )
}

/// `nova setup`.
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

/// Whether Nova's resources were found next to the binary, which is where
/// the installed service looks for them; a checkout's are relative paths.
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

/// Runs one step. `path_var` is where Quickshell is looked for.
pub fn execute(step: &Step, path_var: &str) -> Result<(), String> {
    match step {
        Step::Run { argv, .. } => {
            let (program, args) = argv.split_first().ok_or("empty command")?;
            // No timeout, unlike Nova's other waits: these run in the user's
            // terminal (sudo's password, the package manager's questions, AUR
            // builds that take long), and killing a package manager midway
            // can leave its database locked. Ctrl-C stops them.
            // The PATH binaries were found on, which may have more than ours.
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
        Step::LinkQuickshell { link } => {
            let quickshell = nova_paths::find_program("quickshell", path_var)
                .ok_or("quickshell isn't on PATH, so there's nothing to link NovaUI to")?;
            let target =
                quickshell.canonicalize().map_err(|e| format!("can't resolve {}: {e}", quickshell.display()))?;
            if std::fs::read_link(link).is_ok_and(|existing| existing == target) {
                return Ok(());
            }
            create_parent(link)?;
            match std::fs::remove_file(link) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err(format!("can't replace {}: {e}", link.display()));
                }
                _ => {}
            }
            std::os::unix::fs::symlink(&target, link).map_err(|e| format!("can't link {}: {e}", link.display()))?;
        }
        Step::SetDefault { config_home, mime, app } => {
            novad::set_default_app(config_home, mime, app)?;
        }
        Step::PreferPortal { config_home, desktop } => {
            novad::prefer_nova_file_chooser(config_home, desktop)?;
        }
    }
    Ok(())
}

/// Writes `path` by renaming a finished temp file over it, so a failed or
/// cut-off write leaves the old file whole.
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

/// After the steps: Quickshell must now be in the tested range.
fn finish(path_var: &str) -> Result<(), String> {
    let quickshell = checks::default_quickshell(path_var);
    let version = checks::read_quickshell_version(&quickshell, path_var)?;
    if let Some(problem) = Range::supported().problem(&version) {
        return Err(format!("{problem}. Install a Quickshell in that range, then run `nova doctor`."));
    }
    println!("Nova is set up. Start it with `nova`; `nova doctor` checks the rest.");
    println!("On Hyprland without uwsm, also add `exec-once = nova` to hyprland.conf so novad gets the session.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifests() -> Vec<Manifest> {
        nova_features::load_all(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features")).unwrap()
    }

    fn paths(home: &Path) -> Paths {
        Paths {
            config_dir: home.join(".config/nova"),
            units_dir: home.join(".config/systemd/user"),
            bin_dir: home.join(".local/bin"),
            niri_config: None,
            hyprland_config: None,
            config_home: home.join(".config"),
            data_home: home.join(".local/share"),
            nova: home.join(".local/bin/nova"),
        }
    }

    /// Facts for `distro` and `gpus`, with recording turned on so GPUs
    /// matter, an empty PATH (nothing installed) and no niri config.
    fn facts(distro: Distro, gpus: Vec<Gpu>, nvidia_driver: bool) -> Facts {
        Facts {
            host: Host { distro, gpus, nvidia_driver },
            manifests: manifests(),
            selection: Selection { enabled: vec!["recording".into()], disabled: vec![] },
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

    /// A temporary home with what the default features check for on its PATH.
    struct Home(nova_test_support::TempDir);

    impl Home {
        fn new(name: &str) -> Self {
            let home = nova_test_support::TempDir::new(&format!("setup-{name}"));
            std::fs::create_dir_all(home.join("usr/bin")).unwrap();
            for bin in [
                "quickshell",
                "wf-recorder",
                "grim",
                "wl-copy",
                "xdg-open",
                "awww",
                "awww-daemon",
                "upower",
                "bluetoothctl",
                "paru",
            ] {
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
        let Step::Run { why, argv } = &steps[0] else { panic!("{:?}", steps[0]) };
        // recording's wf-recorder, screenshot's grim, wl-copy and xdg-open
        // wallpaper's awww, peripherals' upower and bluetooth's bluetoothctl
        // are there; Quickshell has no binary check (the package manager
        // skips it when it's installed), nor has files' udisks2.
        assert_eq!(why, "install Quickshell for the UI, and packages for files");
        assert_eq!(argv, &words(&["paru", "-S", "--needed", "quickshell", "udisks2"]));
    }

    /// Quickshell (and Fedora's COPR for it) only while a feature that draws
    /// is on: wallpaper-engine alone has no UI.
    #[test]
    fn quickshell_is_installed_only_for_a_ui() {
        let all = manifests();
        let only = |id: &str| Selection {
            enabled: vec![id.into()],
            disabled: all.iter().filter(|m| m.id != id).map(|m| m.id.clone()).collect(),
        };
        let run_lines = |selection: Selection| {
            let facts = Facts { selection, ..facts(Distro::Fedora, vec![], false) };
            let Plan::Steps(steps) = plan(&facts).unwrap() else { panic!() };
            steps
                .iter()
                .filter(|step| matches!(step, Step::Run { .. }))
                .map(|step| step.to_string())
                .collect::<String>()
        };
        let headless = run_lines(only("wallpaper-engine"));
        assert!(!headless.contains("quickshell"), "{headless}");
        let bar = run_lines(only("bar"));
        assert!(bar.contains(FEDORA_QUICKSHELL_COPR) && bar.contains("dnf install quickshell"), "{bar}");
    }

    #[test]
    fn yes_answers_the_package_manager_too() {
        let facts = Facts { yes: true, ..facts(Distro::Debian, vec![], false) };
        let Plan::Steps(steps) = plan(&facts).unwrap() else { panic!() };
        assert_eq!(
            steps[0],
            Step::Run {
                why: "install Quickshell for the UI, and packages for bluetooth, files, peripherals, recording, screenshot"
                    .into(),
                argv: words(&[
                    "sudo",
                    "apt-get",
                    "install",
                    "-y",
                    "quickshell",
                    "bluez",
                    "udisks2",
                    "upower",
                    "wf-recorder",
                    "grim",
                    "wl-clipboard",
                    "xdg-utils"
                ]),
            }
        );
    }

    /// The file steps on a real home: twice, to show a second run changes
    /// nothing, keeps the user's files and doesn't add the include again.
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
        let link = paths.bin_dir.join("NovaUI");
        assert_eq!(std::fs::read_link(&link).unwrap(), home.0.join("usr/bin/quickshell"));
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

    /// With `files` on (its default), a second run leaves the desktop entry, the icon and
    /// the default as the first made them, and keeps the user's other
    /// defaults.
    #[test]
    fn nova_files_becomes_the_default_for_folders() {
        let home = Home::new("nova-files");
        let paths = paths(&home.0);
        let mimeapps = paths.config_home.join("mimeapps.list");
        std::fs::create_dir_all(&paths.config_home).unwrap();
        std::fs::write(&mimeapps, "[Default Applications]\ntext/plain=editor.desktop\n").unwrap();
        let facts =
            Facts { selection: Selection::default(), paths: paths.clone(), ..facts(Distro::Arch, vec![], false) };
        let Plan::Steps(steps) = plan(&facts).unwrap() else { panic!() };
        let at = steps.iter().position(|step| matches!(step, Step::SetDefault { .. })).unwrap();
        let files = &steps[at - 2..=at];
        let mut lists = Vec::new();
        for _ in 0..2 {
            for step in files {
                execute(step, &facts.path_var).unwrap();
            }
            lists.push(std::fs::read_to_string(&mimeapps).unwrap());
        }
        assert_eq!(lists[0], lists[1], "a second run changes nothing");
        let entry = std::fs::read_to_string(paths.data_home.join("applications/nova-files.desktop")).unwrap();
        assert!(entry.contains(&format!("\nExec={} files %F\n", paths.nova.display())), "{entry}");
        assert_eq!(std::fs::read_to_string(paths.data_home.join(FILES_ICON_PATH)).unwrap(), FILES_ICON);
        let list = &lists[0];
        assert!(
            list.contains("inode/directory=nova-files.desktop") && list.contains("text/plain=editor.desktop"),
            "{list}"
        );
    }

    #[test]
    fn nova_files_is_left_out_while_its_feature_is_off() {
        let selection = Selection { enabled: vec![], disabled: vec![FILES_FEATURE.into()] };
        let facts = Facts { selection, ..facts(Distro::Debian, vec![], false) };
        let Plan::Steps(steps) = plan(&facts).unwrap() else { panic!() };
        let files = |step: &Step| matches!(step, Step::SetDefault { .. } | Step::PreferPortal { .. });
        assert!(!steps.iter().any(files), "{steps:?}");
    }

    /// The portal finds Nova's backend and, on the compositors this user
    /// configures, chooses it for files.
    #[test]
    fn nova_becomes_the_portals_file_chooser_on_niri() {
        let home = Home::new("nova-portal");
        let mut paths = paths(&home.0);
        paths.niri_config = Some(paths.config_home.join("niri/config.kdl"));
        let facts =
            Facts { selection: Selection::default(), paths: paths.clone(), ..facts(Distro::Arch, vec![], false) };
        let Plan::Steps(steps) = plan(&facts).unwrap() else { panic!() };
        let chosen: Vec<_> = steps.iter().filter(|step| matches!(step, Step::PreferPortal { .. })).collect();
        assert_eq!(chosen, [&Step::PreferPortal { config_home: paths.config_home.clone(), desktop: "niri".into() }]);
        for step in steps.iter().filter(|step| matches!(step, Step::PreferPortal { .. } | Step::Replace { .. })) {
            execute(step, &facts.path_var).unwrap();
        }
        let backend = std::fs::read_to_string(paths.data_home.join(FILES_PORTAL_PATH)).unwrap();
        assert!(backend.contains("org.freedesktop.impl.portal.FileChooser"), "{backend}");
        let config = std::fs::read_to_string(paths.config_home.join("xdg-desktop-portal/niri-portals.conf")).unwrap();
        assert!(config.contains("\norg.freedesktop.impl.portal.FileChooser=nova;"), "{config}");
    }

    #[test]
    fn exec_quotes_a_path_as_the_desktop_entry_spec_says() {
        assert_eq!(exec_quoted(Path::new("/usr/bin/nova")), "/usr/bin/nova");
        assert_eq!(exec_quoted(Path::new("/home/me/my apps/nova")), r#""/home/me/my apps/nova""#);
        assert_eq!(exec_quoted(Path::new("/opt/a$b/50%/nova")), r#""/opt/a\\$b/50%%/nova""#);
    }

    #[test]
    fn the_unit_keeps_a_path_with_spaces_as_one_word() {
        assert_eq!(unit_quoted(Path::new("/home/me/my apps/50%/nova")), r#""/home/me/my apps/50%%/nova""#);
    }

    /// A helper found on setup's PATH (say `~/.local/bin`, missing from the
    /// shell's) runs from there too.
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
        let themes = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes");
        assert_eq!(checks::read_config(&config, &themes), Ok(vec![]));
        let selection = home.0.join("features.toml");
        std::fs::write(&selection, default_selection(&manifests())).unwrap();
        assert_eq!(checks::read_selection(&[selection]), Ok(Selection::default()));
    }
}
