use icu_locale_core::{Locale, locale};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Time,
    Collate,
}

impl Category {
    fn var(self) -> &'static str {
        match self {
            Self::Time => "LC_TIME",
            Self::Collate => "LC_COLLATE",
        }
    }
}

// glibc reads LC_ALL, then the category's own variable, then LANG: https://man7.org/linux/man-pages/man7/locale.7.html
pub fn of_system(category: Category) -> Locale {
    chosen(category, |name| std::env::var(name).ok())
}

fn chosen(category: Category, var: impl Fn(&str) -> Option<String>) -> Locale {
    let set =
        ["LC_ALL", category.var(), "LANG"].into_iter().find_map(|name| var(name).filter(|value| !value.is_empty()));
    set.as_deref().and_then(from_posix).unwrap_or(locale!("en"))
}

fn from_posix(value: &str) -> Option<Locale> {
    let (value, modifier) = value.split_once('@').unwrap_or((value, ""));
    let name = value.split('.').next()?;
    if name == "C" || name == "POSIX" {
        return None;
    }
    let (language, region) = name.split_once('_').map_or((name, None), |(language, region)| (language, Some(region)));
    let script = match modifier {
        "latin" => Some("Latn"),
        "cyrillic" => Some("Cyrl"),
        _ => None,
    };
    let tag: Vec<&str> = [Some(language), script, region].into_iter().flatten().collect();
    Locale::try_from_str(&tag.join("-")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(category: Category, all: &str, own: &str, lang: &str) -> Locale {
        let (all, own, lang) = (all.to_owned(), own.to_owned(), lang.to_owned());
        chosen(category, move |name| match name {
            "LC_ALL" => Some(all.clone()),
            "LANG" => Some(lang.clone()),
            name if name == category.var() => Some(own.clone()),
            _ => None,
        })
    }

    #[test]
    fn the_locale_comes_from_lc_all_then_the_category_then_lang() {
        let time = |all, own, lang| vars(Category::Time, all, own, lang);
        assert_eq!(time("fr_FR.UTF-8", "de_DE.UTF-8", "en_US.UTF-8"), locale!("fr-FR"));
        assert_eq!(time("", "de_DE.UTF-8", "en_US.UTF-8"), locale!("de-DE"));
        assert_eq!(time("", "", "ro_RO.UTF-8@euro"), locale!("ro-RO"));
        assert_eq!(vars(Category::Collate, "", "sv_SE.UTF-8", "en_US.UTF-8"), locale!("sv-SE"));
    }

    #[test]
    fn posix_names_become_locales() {
        let lang = |value| vars(Category::Time, "", "", value);
        assert_eq!(lang("sr_RS.UTF-8@latin"), locale!("sr-Latn-RS"));
        assert_eq!(lang("uz_UZ@cyrillic"), locale!("uz-Cyrl-UZ"));
        assert_eq!(lang("C.UTF-8"), locale!("en"));
        assert_eq!(lang(""), locale!("en"));
    }
}
