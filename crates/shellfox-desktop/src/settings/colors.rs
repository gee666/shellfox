use super::api::{ColorKind, CustomColors};
pub(super) const ACCENTS: [&str; 8] = [
    "#ec4899", "#a78bfa", "#60a5fa", "#2dd4bf", "#4ade80", "#fbbf24", "#fb923c", "#fc8397",
];
pub(super) const BACKGROUNDS: [&str; 7] = [
    "#111016", "#000000", "#1e1e2e", "#0f172a", "#ffffff", "#f5f5f7", "#fdf6e3",
];
pub(super) fn normalize<'a>(
    kind: ColorKind,
    values: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let presets: &[&str] = match kind {
        ColorKind::Accent => &ACCENTS,
        ColorKind::Background => &BACKGROUNDS,
    };
    let mut result = Vec::new();
    for value in values {
        if shellfox_core::validation::color(value).is_err() {
            continue;
        }
        let value = value.to_ascii_lowercase();
        if !presets.contains(&value.as_str()) && !result.contains(&value) {
            result.push(value);
        }
    }
    result
        .into_iter()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}
pub(super) fn add(colors: &mut CustomColors, kind: ColorKind, color: &str) {
    let values = match kind {
        ColorKind::Accent => &mut colors.accent,
        ColorKind::Background => &mut colors.background,
    };
    let mut all = values.clone();
    all.push(color.to_owned());
    *values = normalize(kind, all.iter().map(String::as_str));
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_normalization_filters_lowercases_deduplicates_then_keeps_last_three() {
        assert_eq!(
            normalize(
                ColorKind::Accent,
                [
                    "bad", "#EC4899", "#ABCDEF", "#123456", "#234567", "#abcdef", "#345678"
                ]
            ),
            ["#123456", "#234567", "#345678"]
        );
        assert!(normalize(ColorKind::Background, ["#FFFFFF", "#000000", "#111016"]).is_empty());
    }
}
