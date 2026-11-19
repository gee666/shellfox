use std::collections::BTreeMap;

pub const TTY_MODES: &[(u8, u32)] = &[
    (1, 3),
    (2, 28),
    (3, 127),
    (4, 21),
    (5, 4),
    (6, 255),
    (7, 255),
    (8, 17),
    (9, 19),
    (10, 26),
    (12, 18),
    (13, 23),
    (14, 22),
    (18, 15),
    (30, 0),
    (32, 0),
    (33, 0),
    (34, 0),
    (35, 0),
    (36, 1),
    (38, 1),
    (40, 0),
    (42, 1),
    (50, 1),
    (51, 1),
    (53, 1),
    (54, 1),
    (55, 1),
    (56, 0),
    (57, 0),
    (59, 1),
    (60, 1),
    (61, 1),
    (70, 1),
    (72, 1),
    (73, 0),
    (74, 0),
    (75, 0),
    (90, 0),
    (91, 1),
    (92, 0),
    (93, 0),
    (128, 38400),
    (129, 38400),
];
/// Explicit environment input, not process-global state. NO_COLOR and secrets never cross.
pub fn remote_environment<'a>(
    env: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> BTreeMap<String, String> {
    let mut result = BTreeMap::from([
        ("TERM".into(), "xterm-256color".into()),
        ("COLORTERM".into(), "truecolor".into()),
    ]);
    for (name, value) in env.into_iter().take(4096) {
        if result.len() >= 128 {
            break;
        }
        let locale = name == "LANG"
            || name.strip_prefix("LC_").is_some_and(|s| {
                !s.is_empty() && s.bytes().all(|c| c.is_ascii_uppercase() || c == b'_')
            });
        if locale
            && name.len() <= 128
            && !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_.@:-".contains(&c))
        {
            result.insert(name.to_owned(), value.to_owned());
        }
    }
    result
}
