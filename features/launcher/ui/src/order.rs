use icu_collator::options::CollatorOptions;
use icu_collator::{Collator, CollatorBorrowed};
use icu_locale_core::Locale;
use nova_app_info::App;
use nova_locale::Category;

pub struct AppOrder {
    collator: Option<CollatorBorrowed<'static>>,
}

impl AppOrder {
    pub fn of_system() -> Self {
        Self::for_locale(&nova_locale::of_system(Category::Collate))
    }

    pub fn for_locale(locale: &Locale) -> Self {
        let collator = Collator::try_new(locale.into(), CollatorOptions::default());
        if let Err(problem) = &collator {
            eprintln!("WARN nova-ui: no sort order for {locale} ({problem}), the launcher sorts apps by their letters");
        }
        Self { collator: collator.ok() }
    }

    pub fn sort(&self, apps: &mut [App]) {
        match &self.collator {
            Some(collator) => apps.sort_by(|a, b| collator.compare(&a.name, &b.name)),
            None => apps.sort_by_cached_key(|app| app.name.to_lowercase()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(locale: &str, names: &[&str]) -> Vec<String> {
        let mut apps: Vec<App> = names
            .iter()
            .map(|name| App { id: (*name).into(), name: (*name).into(), description: None, icon: None })
            .collect();
        AppOrder::for_locale(&Locale::try_from_str(locale).unwrap()).sort(&mut apps);
        apps.into_iter().map(|app| app.name).collect()
    }

    #[test]
    fn accented_and_lowercase_names_sit_with_their_letter() {
        assert_eq!(sorted("en", &["Zed", "Écrire", "firefox", "Alacritty"]), ["Alacritty", "Écrire", "firefox", "Zed"]);
    }

    #[test]
    fn the_order_follows_the_locale_s_alphabet() {
        assert_eq!(sorted("en", &["Öffnen", "Zed", "Obsidian"]), ["Obsidian", "Öffnen", "Zed"]);
        assert_eq!(sorted("sv", &["Öffnen", "Zed", "Obsidian"]), ["Obsidian", "Zed", "Öffnen"]);
    }
}
