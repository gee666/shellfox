use icu_decimal::{DecimalFormatter, input::Decimal};
use icu_locale::Locale;

pub(super) fn grouped_size(size: u64) -> String {
    thread_local! {
        static FORMATTER: Option<DecimalFormatter> = {
            let locale = sys_locale::get_locale().and_then(|value| value.parse::<Locale>().ok());
            DecimalFormatter::try_new(locale.map(Into::into).unwrap_or_default(), Default::default())
                .or_else(|_| DecimalFormatter::try_new(Default::default(), Default::default())).ok()
        };
    }
    FORMATTER.with(|formatter| {
        formatter.as_ref().map_or_else(
            || size.to_string(),
            |formatter| formatter.format_to_string(&Decimal::from(size)),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_the_renderer_locale_numbering_and_grouping_conventions() {
        for (locale, expected) in [
            ("en-US", "1,023,456"),
            ("de-DE", "1.023.456"),
            ("bn", "১০,২৩,৪৫৬"),
        ] {
            let locale = locale.parse::<Locale>().unwrap();
            let formatter = DecimalFormatter::try_new(locale.into(), Default::default()).unwrap();
            assert_eq!(
                formatter.format_to_string(&Decimal::from(1_023_456)),
                expected
            );
        }
    }
}
