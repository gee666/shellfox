//! The session list order every client shares, as in 0.9.15 `src/shared/status.ts`.
use crate::SessionId;
use std::cmp::Ordering;

/// What a session list sorts by. `rank` is the caller's status class, lower first.
#[derive(Clone, Copy, Debug)]
pub struct SessionOrder<'a> {
    pub pinned: bool,
    pub rank: u8,
    pub created_at: &'a str,
    pub title: &'a str,
    pub id: SessionId,
}

/// Pinned first (never by pin time), then status, newest creation, name, and id.
pub fn compare_session_order(a: &SessionOrder<'_>, b: &SessionOrder<'_>) -> Ordering {
    b.pinned
        .cmp(&a.pinned)
        .then(a.rank.cmp(&b.rank))
        .then_with(|| b.created_at.cmp(a.created_at))
        .then_with(|| compare_names(a.title, b.title))
        .then_with(|| a.id.cmp(&b.id))
}

/// Case-insensitive, with digit runs compared by value ("Shell 2" before "Shell 10"),
/// like 0.9's `Intl.Collator(undefined, { numeric: true, sensitivity: 'base' })`.
fn compare_names(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        let order = match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (x, y) = (digits(&mut a), digits(&mut b));
                let (x, y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                x.len().cmp(&y.len()).then_with(|| x.cmp(y))
            }
            (Some(x), Some(y)) => {
                a.next();
                b.next();
                x.to_lowercase().cmp(y.to_lowercase())
            }
        };
        if order != Ordering::Equal {
            return order;
        }
    }
}

fn digits(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut run = String::new();
    while let Some(digit) = chars.next_if(char::is_ascii_digit) {
        run.push(digit);
    }
    run
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key<'a>(
        label: &'a str,
        pinned: bool,
        rank: u8,
        created_at: &'a str,
        title: &'a str,
    ) -> (&'a str, SessionOrder<'a>) {
        let id = SessionId::new();
        let order = SessionOrder {
            pinned,
            rank,
            created_at,
            title,
            id,
        };
        (label, order)
    }

    #[test]
    fn pins_then_rank_then_newest_then_name_then_id() {
        let (mut first, mut second) = (SessionId::new(), SessionId::new());
        if second < first {
            std::mem::swap(&mut first, &mut second);
        }
        let mut rows = [
            key("old", false, 1, "2026-01-01", "a"),
            key("new", false, 1, "2026-02-01", "z"),
            key("beta", false, 1, "2026-01-15", "beta"),
            key("Alpha", false, 1, "2026-01-15", "Alpha"),
            key("tie-b", false, 1, "2026-01-15", "gamma"),
            key("tie-a", false, 1, "2026-01-15", "Gamma"),
            key("error", false, 0, "2025-01-01", "e"),
            key("pin-settled", true, 4, "2026-03-01", "p"),
            key("pin-error", true, 0, "2025-01-01", "q"),
        ];
        rows[4].1.id = second;
        rows[5].1.id = first;
        rows.sort_by(|a, b| compare_session_order(&a.1, &b.1));
        let ids: Vec<_> = rows.iter().map(|row| row.0).collect();
        assert_eq!(
            ids,
            [
                "pin-error",
                "pin-settled",
                "error",
                "new",
                "Alpha",
                "beta",
                "tie-a",
                "tie-b",
                "old"
            ]
        );
    }

    #[test]
    fn names_compare_digit_runs_by_value_and_ignore_case() {
        assert_eq!(compare_names("Shell 2", "Shell 10"), Ordering::Less);
        assert_eq!(compare_names("item001", "item1"), Ordering::Equal);
        assert_eq!(compare_names("Report", "report"), Ordering::Equal);
        assert_eq!(compare_names("ab", "abc"), Ordering::Less);
        assert_eq!(
            compare_names("n9999999999999999999999", "n10000000000000000000000"),
            Ordering::Less
        );
    }
}
