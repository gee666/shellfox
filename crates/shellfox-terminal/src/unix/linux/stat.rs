use crate::unix::tree::{Identity, Record};
use anyhow::{ensure, Context, Result};

pub(super) fn parse_stat(text: &str, pid: u32) -> Result<Record> {
    let (head, tail) = text
        .rsplit_once(") ")
        .context("invalid proc stat command")?;
    let (reported, _) = head.split_once(" (").context("invalid proc stat PID")?;
    ensure!(
        reported.parse::<u32>()? == pid && pid > 0 && pid <= i32::MAX as u32,
        "proc stat PID mismatch"
    );
    let fields: Vec<_> = tail.split_whitespace().collect();
    ensure!(
        fields.len() >= 20 && fields[0].len() == 1,
        "truncated proc stat"
    );
    let ticks = fields[19].parse()?;
    ensure!(ticks > 0, "invalid process birth");
    Ok(Record {
        identity: Identity { pid, ticks },
        parent: fields[1].parse()?,
        session: fields[3].parse()?,
        state: fields[0].as_bytes()[0],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> String {
        let mut fields = ["0"; 20];
        fields[0] = "S";
        fields[1] = "7";
        fields[2] = "42";
        fields[3] = "42";
        fields[19] = "901";
        format!("42 (a ) nested (name)) {}", fields.join(" "))
    }
    #[test]
    fn parses_parentheses_and_spaces_without_moving_birth_field() {
        let text = fixture();
        let row = parse_stat(&text, 42).unwrap();
        assert_eq!(row.identity.ticks, 901);
        assert_eq!(row.parent, 7);
        assert_eq!(row.session, 42);
        assert!(parse_stat(&text, 43).is_err());
        assert!(parse_stat("42 (short) S 1", 42).is_err());
        assert!(parse_stat(&text.replace("901", "0"), 42).is_err());
    }
}
