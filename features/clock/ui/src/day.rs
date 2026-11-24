use gpui::Global;
use icu_calendar::Gregorian;
use icu_datetime::fieldsets::enums::DateFieldSet;
use icu_datetime::input::Date;
use icu_datetime::pattern::{DateTimePattern, FixedCalendarDateTimeNames, WeekdayNameLength};
use icu_locale_core::Locale;
use jiff::civil;
use nova_locale::Category;
use writeable::TryWriteable;

// Qt's "ddd d" is CLDR's abbreviated day name in the format context ("Sa." in German), not the stand-alone one,
// and the day in the locale's digits: https://doc.qt.io/qt-6/qlocale.html#toString-2
const DAY: &str = "EEE d";
const FALLBACK_DAY: &str = "%a %-d";

pub struct DayNames {
    names: Option<(FixedCalendarDateTimeNames<Gregorian, DateFieldSet>, DateTimePattern)>,
}

impl Global for DayNames {}

impl DayNames {
    pub fn of_system() -> Self {
        Self::for_locale(&nova_locale::of_system(Category::Time))
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
        names.include_decimal_formatter().map_err(|e| e.to_string())?;
        let pattern = DAY.parse::<DateTimePattern>().map_err(|e| e.to_string())?;
        Ok((names, pattern))
    }

    pub fn day(&self, date: civil::Date) -> String {
        let day = self.names.as_ref().zip(gregorian(date)).and_then(|((names, pattern), date)| {
            names.with_pattern_unchecked(pattern).format(&date).try_write_to_string().ok().map(|day| day.into_owned())
        });
        day.unwrap_or_else(|| date.strftime(FALLBACK_DAY).to_string())
    }
}

fn gregorian(date: civil::Date) -> Option<Date<Gregorian>> {
    let month = u8::try_from(date.month()).ok()?;
    let day = u8::try_from(date.day()).ok()?;
    Date::try_new_gregorian(i32::from(date.year()), month, day).ok()
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
        assert_eq!(names("sr-Latn-RS").day(saturday), "sub 24");
        assert_eq!(names("ar-EG").day(saturday), "السبت ٢٤");
    }
}
