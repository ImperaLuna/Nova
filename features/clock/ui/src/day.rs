use icu_calendar::Gregorian;
use icu_datetime::fieldsets::enums::DateFieldSet;
use icu_datetime::input::Date;
use icu_datetime::pattern::{DateTimePattern, FixedCalendarDateTimeNames, WeekdayNameLength};
use icu_locale_core::{Locale, locale};
use jiff::civil;
use writeable::TryWriteable;

const LOCALE_VARS: [&str; 3] = ["LC_ALL", "LC_TIME", "LANG"];
// Qt's "ddd" is CLDR's abbreviated day name in the format context ("Sa." in German), not the stand-alone one:
// https://doc.qt.io/qt-6/qlocale.html#toString-2
const SHORT_DAY: &str = "EEE";

pub struct DayNames {
    names: Option<(FixedCalendarDateTimeNames<Gregorian, DateFieldSet>, DateTimePattern)>,
}

impl DayNames {
    pub fn of_system() -> Self {
        Self::for_locale(&time_locale(|name| std::env::var(name).ok()))
    }

    fn for_locale(locale: &Locale) -> Self {
        let names = Self::load(locale);
        if let Err(problem) = &names {
            eprintln!("WARN nova-ui: no day names for {locale} ({problem}), the clock writes them in English");
        }
        Self { names: names.ok() }
    }

    fn load(locale: &Locale) -> Result<(FixedCalendarDateTimeNames<Gregorian, DateFieldSet>, DateTimePattern), String> {
        let mut names = FixedCalendarDateTimeNames::try_new(locale.into()).map_err(|e| e.to_string())?;
        names.include_weekday_names(WeekdayNameLength::Abbreviated).map_err(|e| e.to_string())?;
        let pattern = SHORT_DAY.parse::<DateTimePattern>().map_err(|e| e.to_string())?;
        Ok((names, pattern))
    }

    pub fn day(&self, date: civil::Date) -> String {
        let name = self.names.as_ref().zip(gregorian(date)).and_then(|((names, pattern), date)| {
            names.with_pattern_unchecked(pattern).format(&date).try_write_to_string().ok().map(|name| name.into_owned())
        });
        let name = name.unwrap_or_else(|| date.strftime("%a").to_string());
        format!("{name} {}", date.day())
    }
}

fn gregorian(date: civil::Date) -> Option<Date<Gregorian>> {
    let month = u8::try_from(date.month()).ok()?;
    let day = u8::try_from(date.day()).ok()?;
    Date::try_new_gregorian(i32::from(date.year()), month, day).ok()
}

fn time_locale(var: impl Fn(&str) -> Option<String>) -> Locale {
    let set = LOCALE_VARS.iter().find_map(|name| var(name).filter(|value| !value.is_empty()));
    set.as_deref().and_then(from_posix).unwrap_or(locale!("en"))
}

fn from_posix(value: &str) -> Option<Locale> {
    let name = value.split(['.', '@']).next()?;
    if name == "C" || name == "POSIX" {
        return None;
    }
    Locale::try_from_str(&name.replace('_', "-")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn names(locale: &str) -> DayNames {
        DayNames::for_locale(&Locale::try_from_str(locale).unwrap())
    }

    #[test]
    fn it_writes_the_short_day_name_in_the_locale_s_language() {
        let saturday = date(2026, 10, 24);
        assert_eq!(names("en").day(saturday), "Sat 24");
        assert_eq!(names("de").day(saturday), "Sa. 24");
        assert_eq!(names("fr").day(saturday), "sam. 24");
        assert_eq!(names("ro").day(saturday), "sâm. 24");
    }

    #[test]
    fn the_locale_comes_from_lc_all_then_lc_time_then_lang() {
        let vars = |all: &str, time: &str, lang: &str| {
            let (all, time, lang) = (all.to_owned(), time.to_owned(), lang.to_owned());
            time_locale(move |name| match name {
                "LC_ALL" => Some(all.clone()),
                "LC_TIME" => Some(time.clone()),
                "LANG" => Some(lang.clone()),
                _ => None,
            })
        };
        assert_eq!(vars("fr_FR.UTF-8", "de_DE.UTF-8", "en_US.UTF-8"), locale!("fr-FR"));
        assert_eq!(vars("", "de_DE.UTF-8", "en_US.UTF-8"), locale!("de-DE"));
        assert_eq!(vars("", "", "ro_RO.UTF-8@latin"), locale!("ro-RO"));
        assert_eq!(vars("", "", "C.UTF-8"), locale!("en"));
        assert_eq!(vars("", "", ""), locale!("en"));
    }
}
