//! Desktop entries (`.desktop` files), as the freedesktop.org Desktop Entry
//! Specification 1.5 describes them: the `[Desktop Entry]` group's keys in
//! the user's language, whether the launcher lists the app at all, and its
//! `Exec` line turned into a program and its arguments. The line is never
//! handed to a shell.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nova_paths::find_program;

const GROUP: &str = "[Desktop Entry]";
/// Field codes that stand for files or URLs to open; the launcher opens
/// none, and opening a file puts it where the first one is ([`FileArg`]).
const FILE_CODES: [char; 4] = ['f', 'F', 'u', 'U'];
/// Field codes the spec deprecates, dropped like it says.
const DEPRECATED_CODES: [char; 6] = ['d', 'D', 'n', 'N', 'v', 'm'];
/// GNOME's switch for an autostart entry, which other desktops honor too.
const AUTOSTART_ENABLED: &str = "X-GNOME-Autostart-enabled";

/// An app the launcher lists.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Its desktop file id, like `org.mozilla.firefox.desktop`.
    pub id: String,
    pub name: String,
    pub generic_name: Option<String>,
    pub comment: Option<String>,
    pub icon: Option<String>,
    pub keywords: Vec<String>,
    /// The program and its arguments, field codes expanded.
    pub exec: Vec<String>,
    /// Runs in a terminal.
    pub terminal: bool,
    /// The folder to run it in.
    pub path: Option<PathBuf>,
    /// Where a file to open goes in `exec`.
    pub file_arg: FileArg,
    /// The file and link types it opens (`MimeType=`).
    pub mime_types: Vec<String>,
    /// Listed in the launcher: not `NoDisplay`. An unlisted app may still
    /// be the one that opens some type of file.
    pub listed: bool,
    /// The app id its windows get (`StartupWMClass=`), when it says.
    pub window_class: Option<String>,
    /// What kind of app it is (`Categories=`), like `AudioVideo` or `WebBrowser`.
    pub categories: Vec<String>,
}

/// Where the file an app opens goes in its `exec`: at the first file field
/// code (`%f`, `%F`, `%u` or `%U`), else at the end, as GLib and KIO do. A
/// local path stands for a URL too, which the spec allows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FileArg {
    /// The argument it goes before, or the one it's part of.
    pub index: usize,
    /// For a code inside an argument (`--file=%f`), the text either side of
    /// it: that argument is replaced by both with the file between.
    pub within: Option<(String, String)>,
}

impl Entry {
    /// Whether it opens files or links of some type: the entry the
    /// system's defaults point at, when two start the same program.
    pub fn opens_types(&self) -> bool {
        !self.mime_types.is_empty()
    }

    /// Whether `other` is it, or a copy of it under another id: the same
    /// name running the same command.
    pub fn same_app(&self, other: &Entry) -> bool {
        self.id == other.id || (self.name == other.name && self.exec == other.exec && self.terminal == other.terminal)
    }

    /// Its program and arguments with `file` to open.
    pub fn exec_with(&self, file: &str) -> Vec<String> {
        let mut argv = self.exec.clone();
        let index = self.file_arg.index.min(argv.len());
        match &self.file_arg.within {
            Some((before, after)) if index < argv.len() => argv[index] = format!("{before}{file}{after}"),
            _ => argv.insert(index, file.to_string()),
        }
        argv
    }

    /// The app ids (lower case) a window of this app may have: the one it
    /// names, its desktop file id, its program and its name. None for an
    /// app in a terminal, whose window is the terminal's.
    pub fn window_ids(&self) -> Vec<String> {
        if self.terminal {
            return vec![];
        }
        let program = self.exec.first().map(|program| program.rsplit('/').next().unwrap_or(program));
        let mut ids: Vec<String> =
            [self.window_class.as_deref(), self.id.strip_suffix(".desktop"), program, Some(&self.name)]
                .into_iter()
                .flatten()
                .map(str::to_lowercase)
                .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Whether `id` (lower case) is the first word of its name, a loose
    /// match (`zen` for "Zen Browser", `brave` for "Brave Web Browser").
    /// Never for an app in a terminal, like `window_ids`.
    pub fn first_word_is(&self, id: &str) -> bool {
        !self.terminal && self.name.split_whitespace().next().is_some_and(|word| word.to_lowercase() == id)
    }

    /// Whether `id` (lower case) is the app id it names for its windows or
    /// its desktop file id, rather than just its program or name.
    pub fn is_named(&self, id: &str) -> bool {
        let desktop_id = self.id.strip_suffix(".desktop").unwrap_or(&self.id);
        [self.window_class.as_deref(), Some(desktop_id)].into_iter().flatten().any(|named| named.to_lowercase() == id)
    }
}

/// Why a file isn't an app the launcher lists.
#[derive(Debug, PartialEq)]
pub enum Skipped {
    /// `Hidden=true`: deleted, which also hides the same id in folders
    /// further down the search path.
    Hidden,
    /// Not meant for the launcher (`NoDisplay`, not an application, not for
    /// this desktop, its program not installed).
    NotShown,
    /// Not a valid entry, and why.
    Invalid(String),
}

/// The user's language, as the keys a localized value may be under, the
/// best match first (`de_AT@euro`, `de_AT`, `de@euro`, `de`).
#[derive(Debug, Clone, Default)]
pub struct Locale(Vec<String>);

impl Locale {
    /// From `LC_ALL`, `LC_MESSAGES` or `LANG`, whichever is set first.
    pub fn from_env() -> Self {
        let value = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.is_empty());
        value.map(|value| Self::parse(&value)).unwrap_or_default()
    }

    /// From a POSIX locale name, `lang_COUNTRY.ENCODING@MODIFIER`.
    pub fn parse(name: &str) -> Self {
        let (rest, modifier) = match name.split_once('@') {
            Some((rest, modifier)) => (rest, Some(modifier)),
            None => (name, None),
        };
        let rest = rest.split('.').next().unwrap_or_default();
        let (lang, country) = match rest.split_once('_') {
            Some((lang, country)) => (lang, Some(country)),
            None => (rest, None),
        };
        if lang.is_empty() || lang == "C" || lang == "POSIX" {
            return Self::default();
        }
        let mut keys = Vec::new();
        if let (Some(country), Some(modifier)) = (country, modifier) {
            keys.push(format!("{lang}_{country}@{modifier}"));
        }
        if let Some(country) = country {
            keys.push(format!("{lang}_{country}"));
        }
        if let Some(modifier) = modifier {
            keys.push(format!("{lang}@{modifier}"));
        }
        keys.push(lang.to_string());
        Self(keys)
    }
}

/// What decides whether an entry shows here.
pub struct Context<'a> {
    pub locale: &'a Locale,
    /// `XDG_CURRENT_DESKTOP`'s names, like `niri`.
    pub desktops: &'a [String],
    /// Where `TryExec` programs are looked for.
    pub path_var: &'a str,
    /// Read for starting at login (XDG autostart), not for the launcher:
    /// `NoDisplay` doesn't matter, and `X-GNOME-Autostart-enabled=false`
    /// turns the entry off like `Hidden`.
    pub autostart: bool,
}

/// The `[Desktop Entry]` group's keys: `key` or `key[locale]` to its raw value.
fn group(text: &str) -> HashMap<&str, &str> {
    let mut keys = HashMap::new();
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            // Only the first group counts; later groups are actions and such.
            if inside {
                break;
            }
            inside = line == GROUP;
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            // The first of a repeated key wins, like GLib.
            keys.entry(key.trim_end()).or_insert(value.trim_start());
        }
    }
    keys
}

/// Parses the desktop file `id` read from `file`.
pub fn parse(id: &str, file: &Path, text: &str, context: &Context) -> Result<Entry, Skipped> {
    let keys = group(text);
    if keys.is_empty() {
        return Err(Skipped::Invalid(format!("no {GROUP} group")));
    }
    let raw = |key: &str| keys.get(key).copied();
    let flag = |key: &str| raw(key).is_some_and(|value| value.trim() == "true");
    let localized = |key: &str| {
        context
            .locale
            .0
            .iter()
            .find_map(|locale| raw(&format!("{key}[{locale}]")))
            .or_else(|| raw(key))
            .map(unescape)
            .filter(|value| !value.is_empty())
    };

    if flag("Hidden") || (context.autostart && raw(AUTOSTART_ENABLED).map(str::trim) == Some("false")) {
        return Err(Skipped::Hidden);
    }
    // Kept unlisted only to open files; one that opens none is skipped
    // before the work below.
    let opens_none = raw("MimeType").is_none_or(|types| types.trim().is_empty());
    if raw("Type").map(str::trim) != Some("Application") || (flag("NoDisplay") && !context.autostart && opens_none) {
        return Err(Skipped::NotShown);
    }
    let shown_here = |key: &str| {
        raw(key).map(|value| list(value).iter().any(|desktop| context.desktops.iter().any(|here| here == desktop)))
    };
    if shown_here("OnlyShowIn") == Some(false) || shown_here("NotShowIn") == Some(true) {
        return Err(Skipped::NotShown);
    }
    if let Some(program) = raw("TryExec").map(unescape)
        && find_program(&program, context.path_var).is_none()
    {
        return Err(Skipped::NotShown);
    }

    let name = localized("Name").ok_or_else(|| Skipped::Invalid("no Name".into()))?;
    let icon = raw("Icon").map(unescape).filter(|icon| !icon.is_empty());
    let exec = raw("Exec").ok_or_else(|| Skipped::Invalid("no Exec".into()))?;
    let (exec, file_arg) = exec_argv(&unescape(exec), &name, icon.as_deref(), file).map_err(Skipped::Invalid)?;
    if exec.is_empty() {
        return Err(Skipped::Invalid("an empty Exec".into()));
    }
    let keywords = context
        .locale
        .0
        .iter()
        .find_map(|locale| raw(&format!("Keywords[{locale}]")))
        .or_else(|| raw("Keywords"))
        .map(list)
        .unwrap_or_default();
    Ok(Entry {
        id: id.to_string(),
        name,
        generic_name: localized("GenericName"),
        comment: localized("Comment"),
        icon,
        keywords,
        exec,
        terminal: flag("Terminal"),
        path: raw("Path").map(unescape).filter(|path| !path.is_empty()).map(PathBuf::from),
        file_arg,
        mime_types: raw("MimeType").map(list).unwrap_or_default(),
        listed: context.autostart || !flag("NoDisplay"),
        window_class: raw("StartupWMClass").map(unescape).filter(|class| !class.is_empty()),
        categories: raw("Categories").map(list).unwrap_or_default(),
    })
}

/// A string value with its escapes (`\s`, `\n`, `\t`, `\r`, `\\`) undone.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            // Not an escape strings have (`\;` in lists): kept for the next step.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// A list value (`a;b;c;`), each item unescaped, empty ones dropped.
fn list(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut item = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(';') => item.push(';'),
                Some(other) => {
                    item.push('\\');
                    item.push(other);
                }
                None => item.push('\\'),
            },
            ';' => items.push(std::mem::take(&mut item)),
            _ => item.push(c),
        }
    }
    items.push(item);
    items.into_iter().map(|item| unescape(item.trim())).filter(|item| !item.is_empty()).collect()
}

/// An `Exec` line, its string escapes already undone, as a program and its
/// arguments: split at spaces outside double quotes, with `\"`, `` \` ``,
/// `\$` and `\\` inside them, and field codes expanded. `%i` is the icon,
/// `%c` the name and `%k` the file; file and URL codes are dropped, since the
/// launcher opens the app without any, and the first one's place is kept
/// for opening a file.
fn exec_argv(line: &str, name: &str, icon: Option<&str>, file: &Path) -> Result<(Vec<String>, FileArg), String> {
    // Each argument, and whether it was quoted: field codes don't apply there.
    let mut words: Vec<(String, bool)> = Vec::new();
    let mut chars = line.chars();
    let mut word: Option<(String, bool)> = None;
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => {
                words.extend(word.take());
            }
            '"' => {
                let (text, _) = word.get_or_insert_with(|| (String::new(), true));
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(escaped @ ('"' | '`' | '$' | '\\')) => text.push(escaped),
                            Some(other) => {
                                text.push('\\');
                                text.push(other);
                            }
                            None => return Err("a quote in Exec isn't closed".into()),
                        },
                        Some(other) => text.push(other),
                        None => return Err("a quote in Exec isn't closed".into()),
                    }
                }
            }
            _ => word.get_or_insert_with(|| (String::new(), false)).0.push(c),
        }
    }
    words.extend(word);

    let mut argv = Vec::new();
    let mut file_arg: Option<FileArg> = None;
    for (word, quoted) in words {
        if quoted {
            argv.push(word.replace("%%", "%"));
            continue;
        }
        match word.as_str() {
            "%i" => {
                if let Some(icon) = icon {
                    argv.extend(["--icon".to_string(), icon.to_string()]);
                }
                continue;
            }
            code if code.len() == 2
                && code.starts_with('%')
                && code[1..].chars().all(|c| FILE_CODES.contains(&c) || DEPRECATED_CODES.contains(&c)) =>
            {
                if file_arg.is_none() && code[1..].chars().all(|c| FILE_CODES.contains(&c)) {
                    file_arg = Some(FileArg { index: argv.len(), within: None });
                }
                continue;
            }
            _ => {}
        }
        let mut expanded = String::with_capacity(word.len());
        // Where in it the first file code was.
        let mut file_at = None;
        let mut chars = word.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                expanded.push(c);
                continue;
            }
            match chars.next() {
                Some('%') => expanded.push('%'),
                Some('c') => expanded.push_str(name),
                Some('k') => expanded.push_str(&file.to_string_lossy()),
                // Inside a word: dropped, like a code on its own.
                Some(code) if FILE_CODES.contains(&code) => {
                    file_at.get_or_insert(expanded.len());
                }
                Some(code) if DEPRECATED_CODES.contains(&code) || code == 'i' => {}
                Some(code) => return Err(format!("unknown field code %{code} in Exec")),
                None => return Err("a lone % at the end of Exec".into()),
            }
        }
        if let Some(at) = file_at
            && file_arg.is_none()
        {
            let within = Some((expanded[..at].to_string(), expanded[at..].to_string()));
            file_arg = Some(FileArg { index: argv.len(), within });
        }
        argv.push(expanded);
    }
    let file_arg = file_arg.unwrap_or(FileArg { index: argv.len(), within: None });
    Ok((argv, file_arg))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(locale: &Locale) -> Context<'_> {
        static DESKTOPS: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| vec!["niri".into()]);
        Context { locale, desktops: &DESKTOPS, path_var: "/bin:/usr/bin", autostart: false }
    }

    fn entry(text: &str) -> Result<Entry, Skipped> {
        parse("app.desktop", Path::new("/apps/app.desktop"), text, &context(&Locale::default()))
    }

    fn exec(line: &str) -> Result<Vec<String>, String> {
        exec_argv(&unescape(line), "My App", Some("my-icon"), Path::new("/apps/my app.desktop")).map(|(argv, _)| argv)
    }

    fn opening(line: &str, file: &str) -> Vec<String> {
        let mut found = entry(&format!("[Desktop Entry]\nType=Application\nName=X\nExec={line}\n")).unwrap();
        found.mime_types = vec!["text/plain".into()];
        found.exec_with(file)
    }

    #[test]
    fn reads_an_app_and_its_details() {
        let found = entry(
            "# a comment\n[Desktop Entry]\nType=Application\nName=Files\nGenericName=File Manager\n\
             Comment=Access and organize files\nIcon=org.gnome.Nautilus\nKeywords=folder;manager;explore;\n\
             Exec=nautilus --new-window %U\nPath=/tmp\nCategories=GNOME;GTK;Utility;Core;FileManager;\n\n[Desktop Action new-window]\nName=New Window\nExec=other\n",
        )
        .unwrap();
        assert_eq!(found.name, "Files");
        assert_eq!(found.generic_name.as_deref(), Some("File Manager"));
        assert_eq!(found.comment.as_deref(), Some("Access and organize files"));
        assert_eq!(found.icon.as_deref(), Some("org.gnome.Nautilus"));
        assert_eq!(found.keywords, ["folder", "manager", "explore"]);
        assert_eq!(found.exec, ["nautilus", "--new-window"]);
        assert_eq!(found.path, Some(PathBuf::from("/tmp")));
        assert_eq!(found.categories, ["GNOME", "GTK", "Utility", "Core", "FileManager"]);
        assert!(!found.terminal);
    }

    #[test]
    fn values_follow_the_users_language() {
        let text = "[Desktop Entry]\nType=Application\nExec=x\nName=Files\nName[de]=Dateien\n\
                    Name[de_AT]=Dateien (AT)\nKeywords=folder;\nKeywords[de]=Ordner;\n";
        let parse_as = |locale: &str| {
            let locale = Locale::parse(locale);
            parse("x.desktop", Path::new("/x.desktop"), text, &context(&locale)).unwrap()
        };
        assert_eq!(parse_as("de_AT.UTF-8@euro").name, "Dateien (AT)");
        assert_eq!(parse_as("de_CH.UTF-8").name, "Dateien");
        assert_eq!(parse_as("de_CH.UTF-8").keywords, ["Ordner"]);
        assert_eq!(parse_as("fr_FR.UTF-8").name, "Files");
        assert_eq!(parse_as("C.UTF-8").name, "Files");
    }

    #[test]
    fn apps_not_for_the_launcher_are_skipped() {
        let base = "[Desktop Entry]\nType=Application\nName=X\nExec=x\n";
        assert_eq!(entry(&format!("{base}Hidden=true\n")), Err(Skipped::Hidden));
        for extra in [
            "OnlyShowIn=GNOME;KDE;\n",
            "NotShowIn=niri;\n",
            "TryExec=/nonexistent/program\n",
            "TryExec=no-such-program-anywhere\n",
        ] {
            assert_eq!(entry(&format!("{base}{extra}")), Err(Skipped::NotShown), "{extra}");
        }
        assert!(entry(&format!("{base}OnlyShowIn=niri;\nTryExec=sh\n")).is_ok());
        // Kept unlisted when it opens files, else skipped.
        assert_eq!(entry(&format!("{base}NoDisplay=true\n")), Err(Skipped::NotShown));
        assert!(!entry(&format!("{base}NoDisplay=true\nMimeType=text/plain;\n")).unwrap().listed);
        assert!(entry(base).unwrap().listed);
        assert_eq!(entry("[Desktop Entry]\nType=Link\nName=X\nURL=https://example.com\n"), Err(Skipped::NotShown));
        assert!(matches!(entry("[Desktop Entry]\nType=Application\nName=X\n"), Err(Skipped::Invalid(_))));
        assert!(matches!(entry("[Desktop Entry]\nType=Application\nExec=x\n"), Err(Skipped::Invalid(_))));
        assert!(matches!(entry("Name=X\n"), Err(Skipped::Invalid(_))));
        // Only [Desktop Entry] counts, even when another group comes first.
        assert!(matches!(entry("[Other]\nType=Application\nName=X\nExec=x\n"), Err(Skipped::Invalid(_))));
    }

    #[test]
    fn exec_splits_like_the_spec_without_a_shell() {
        assert_eq!(exec("foot").unwrap(), ["foot"]);
        assert_eq!(exec("  foot   -e  htop ").unwrap(), ["foot", "-e", "htop"]);
        assert_eq!(exec(r#"sh -c "echo \"hi\" \$HOME; rm x" %u"#).unwrap(), ["sh", "-c", r#"echo "hi" $HOME; rm x"#]);
        // Escapes for strings come first: a file's `\\\\` is one backslash
        // inside quotes, and `\s` a space, which splits like one.
        assert_eq!(exec(r#""C:\\\\dir" a\sb"#).unwrap(), [r"C:\dir", "a", "b"]);
        assert_eq!(exec("app --name=%c %k").unwrap(), ["app", "--name=My App", "/apps/my app.desktop"]);
        assert_eq!(exec("app %i %F %d").unwrap(), ["app", "--icon", "my-icon"]);
        assert_eq!(exec("app 100%%").unwrap(), ["app", "100%"]);
        assert_eq!(exec("app \"%U\"").unwrap(), ["app", "%U"], "no field codes inside quotes");
        assert_eq!(exec("app --file=%f").unwrap(), ["app", "--file="]);
        assert_eq!(exec_argv("app %i", "X", None, Path::new("/x")).unwrap().0, ["app"]);
        assert!(exec("app \"unclosed").is_err());
        assert!(exec("app %z").is_err());
        assert!(exec("app %").is_err());
        // Shell syntax is just text: nothing runs it.
        assert_eq!(exec("app ; rm -rf ~ && $(boom) `boom` | x > y").unwrap().len(), 12);
    }

    #[test]
    fn a_file_to_open_goes_where_the_entry_says() {
        let file = "/home/me/a b.txt";
        assert_eq!(opening("viewer %f", file), ["viewer", file]);
        assert_eq!(opening("viewer --new %U --flag", file), ["viewer", "--new", file, "--flag"]);
        assert_eq!(opening("viewer %u %f", file), ["viewer", file], "only the first code's place");
        assert_eq!(opening("viewer --file=%f", file), ["viewer", "--file=/home/me/a b.txt"]);
        assert_eq!(opening("viewer", file), ["viewer", file], "no code: at the end");
        assert_eq!(opening("viewer %i", file), ["viewer", file]);
        assert_eq!(opening("viewer \"%f\"", file), ["viewer", "%f", file], "quoted codes are text");
    }

    #[test]
    fn reads_the_types_an_app_opens() {
        let found =
            entry("[Desktop Entry]\nType=Application\nName=X\nExec=x %F\nMimeType=image/png;image/jpeg;\n").unwrap();
        assert_eq!(found.mime_types, ["image/png", "image/jpeg"]);
        assert!(found.opens_types());
        assert!(!entry("[Desktop Entry]\nType=Application\nName=X\nExec=x\n").unwrap().opens_types());
    }

    #[test]
    fn lists_keep_escaped_semicolons() {
        assert_eq!(list(r"a\;b;c;;d"), ["a;b", "c", "d"]);
        assert_eq!(list("one"), ["one"]);
        assert!(list(";;").is_empty());
    }
}
