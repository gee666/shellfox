//! Port of src/cli/terminal-queries.ts; clipboard and denied pixel queries stay local.

pub fn csi(sequence: &[u8], header: usize) -> Vec<u8> {
    let body = &sequence[header..];
    let Some((&final_byte, rest)) = body.split_last() else {
        return sequence.to_vec();
    };
    let (prefix, rest) = if rest.first().is_some_and(|byte| b"?>=".contains(byte)) {
        (rest[0], &rest[1..])
    } else {
        (0, rest)
    };
    let end = rest
        .iter()
        .position(|byte| !byte.is_ascii_digit() && !b";:".contains(byte))
        .unwrap_or(rest.len());
    let (params, intermediate) = rest.split_at(end);
    if !intermediate.iter().all(|byte| (b' '..=b'/').contains(byte)) {
        return sequence.to_vec();
    }
    let first = number(
        params
            .split(|byte| b";:".contains(byte))
            .next()
            .unwrap_or_default(),
    );
    if matches!(final_byte, b'h' | b'l') && prefix == b'?' && intermediate.is_empty() {
        let modes: Vec<_> = params
            .split(|byte| *byte == b';')
            .filter(|mode| number(mode) != 9001)
            .collect();
        if modes.len() != params.split(|byte| *byte == b';').count() {
            if modes.is_empty() {
                return vec![];
            }
            let mut output = sequence[..header].to_vec();
            output.push(b'?');
            for (index, mode) in modes.iter().enumerate() {
                if index > 0 {
                    output.push(b';');
                }
                output.extend_from_slice(mode);
            }
            output.push(final_byte);
            return output;
        }
    }
    let host_query = intermediate.is_empty()
        && match final_byte {
            b'c' => matches!(prefix, 0 | b'>'),
            b'n' => prefix == 0 && matches!(first, 5 | 6) || prefix == b'?' && first == 6,
            // Pixel geometry is denied here; never invite an external autoanswer.
            b't' => prefix == 0 && matches!(first, 4 | 8 | 14 | 16 | 18),
            _ => false,
        };
    if host_query || final_byte == b'p' && intermediate == b"$" && matches!(prefix, 0 | b'?') {
        vec![]
    } else {
        sequence.to_vec()
    }
}

pub fn string_body(sequence: &[u8], header: usize) -> Vec<u8> {
    sequence[header..]
        .iter()
        .copied()
        .filter(|byte| !matches!(byte, 0..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f))
        .collect()
}

pub fn denied_string(body: &[u8], osc: bool) -> bool {
    if osc {
        return body
            .split(|byte| *byte == b';')
            .next()
            .is_some_and(|id| number(id) == 52);
    }
    body.strip_suffix(b"$q").is_some_and(|params| {
        params
            .iter()
            .all(|byte| byte.is_ascii_digit() || *byte == b';')
    })
}

pub fn potential_query_string(body: &[u8], osc: bool) -> bool {
    if osc {
        color_string(body) || body.iter().all(u8::is_ascii_digit)
    } else {
        body.strip_suffix(b"$")
            .unwrap_or(body)
            .iter()
            .all(|byte| byte.is_ascii_digit() || *byte == b';')
    }
}

pub fn color_string(body: &[u8]) -> bool {
    body.split(|byte| *byte == b';')
        .next()
        .is_some_and(|id| matches!(number(id), 4 | 10 | 11 | 12))
}

pub fn osc(sequence: &[u8], header: usize, ending: &[u8], _historical: bool) -> Vec<u8> {
    let body = string_body(sequence, header);
    let mut values = body.split(|byte| *byte == b';');
    let id = values.next().unwrap_or_default();
    if number(id) == 52 {
        return vec![];
    }
    // Ghostty owns live color replies too; use TS replay filtering for both paths.
    if !color_string(&body) {
        return join(sequence, ending);
    }
    let values: Vec<_> = values.collect();
    if !values.contains(&b"?".as_slice()) {
        return join(sequence, ending);
    }
    let mut output = Vec::new();
    if number(id) == 4 {
        let settings: Vec<_> = values
            .chunks(2)
            .filter(|pair| pair.get(1) != Some(&b"?".as_slice()))
            .flatten()
            .collect();
        if !settings.is_empty() {
            output.extend_from_slice(&sequence[..header]);
            output.extend_from_slice(id);
            for value in settings {
                output.push(b';');
                output.extend_from_slice(value);
            }
            output.extend_from_slice(ending);
        }
    } else {
        for (index, value) in values
            .iter()
            .enumerate()
            .filter(|(_, value)| **value != b"?")
        {
            output.extend_from_slice(&sequence[..header]);
            output.extend_from_slice((number(id) + index as u32).to_string().as_bytes());
            output.push(b';');
            output.extend_from_slice(value);
            output.extend_from_slice(ending);
        }
    }
    output
}

fn join(sequence: &[u8], ending: &[u8]) -> Vec<u8> {
    let mut bytes = sequence.to_vec();
    bytes.extend_from_slice(ending);
    bytes
}
fn number(value: &[u8]) -> u32 {
    std::str::from_utf8(value)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}
