//! Validate the names and link graph before ditto writes anything.
use anyhow::{Context, Result, ensure};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
};
const UNSAFE: &str = "Unsafe or unsupported update archive";
const ROOT: &str = "shellfox.app";

#[cfg(test)]
mod tests;

pub(super) fn validate(path: &Path) -> Result<()> {
    let mut zip = BufReader::new(File::open(path)?);
    let len = zip.get_ref().metadata()?.len();
    let tail_len = len.min(65_557) as usize;
    let tail = bytes(&mut zip, len - tail_len as u64, tail_len)?;
    let end = tail
        .windows(22)
        .enumerate()
        .rev()
        .find_map(|(i, header)| {
            (sig(header, 0) == 0x06054b50 && i + 22 + short(header, 20) == tail_len)
                .then_some((i, header))
        })
        .context(UNSAFE)?;
    let count = short(end.1, 10);
    let start = u64::from(sig(end.1, 16));
    let stop = len - tail_len as u64 + end.0 as u64;
    ensure!(
        short(end.1, 4) == 0
            && short(end.1, 6) == 0
            && count > 0
            && count < 65535
            && count == short(end.1, 8)
            && start + u64::from(sig(end.1, 12)) == stop,
        "{UNSAFE}"
    );
    let mut cursor = start;
    let mut total = 0u64;
    let mut names = HashSet::new();
    let mut links = HashMap::new();
    let mut ranges = Vec::new();
    for _ in 0..count {
        ensure!(cursor + 46 <= stop, "{UNSAFE}");
        let central = bytes(&mut zip, cursor, 46)?;
        let (flags, method) = (short(&central, 8), short(&central, 10));
        let (packed, unpacked) = (sig(&central, 20), sig(&central, 24));
        let name_len = short(&central, 28);
        let extra_len = short(&central, 30);
        let local = u64::from(sig(&central, 42));
        let kind = (sig(&central, 38) >> 16) & 0o170000;
        let next = cursor + 46 + (name_len + extra_len + short(&central, 32)) as u64;
        ensure!(
            sig(&central, 0) == 0x02014b50
                && next <= stop
                && short(&central, 34) == 0
                && flags & 1 == 0
                && matches!(method, 0 | 8)
                && local + 30 <= start
                && packed != u32::MAX
                && unpacked != u32::MAX
                && matches!(kind, 0 | 0o100000 | 0o040000 | 0o120000),
            "{UNSAFE}"
        );
        let name_bytes = bytes(&mut zip, cursor + 46, name_len)?;
        validate_extra(&bytes(&mut zip, cursor + 46 + name_len as u64, extra_len)?)?;
        let name = std::str::from_utf8(&name_bytes)?;
        let normalized = name.strip_suffix('/').unwrap_or(name);
        ensure!(
            safe_text(name)
                && matches!(
                    normalized.split('/').next(),
                    Some("Shellfox.app" | "__MACOSX")
                )
                && normalized
                    .split('/')
                    .all(|part| !matches!(part, "" | "." | "..")),
            "{UNSAFE}"
        );
        let folded = normalized.to_lowercase();
        ensure!(names.insert(folded.clone()), "{UNSAFE}");
        let header = bytes(&mut zip, local, 30)?;
        let local_extra = short(&header, 28);
        let data = local + 30 + (name_len + local_extra) as u64;
        let data_end = data + u64::from(packed);
        ensure!(
            sig(&header, 0) == 0x04034b50
                && short(&header, 6) == flags
                && short(&header, 8) == method
                && short(&header, 26) == name_len
                && data_end <= start,
            "{UNSAFE}"
        );
        ensure!(
            bytes(&mut zip, local + 30, name_len)? == name_bytes,
            "{UNSAFE}"
        );
        validate_extra(&bytes(&mut zip, local + 30 + name_len as u64, local_extra)?)?;
        let record_end = if flags & 8 == 0 {
            ensure!(
                sig(&header, 18) == packed && sig(&header, 22) == unpacked,
                "{UNSAFE}"
            );
            data_end
        } else {
            // Validate the descriptor too, so stream readers and central readers agree.
            let first = bytes(&mut zip, data_end, 4)?;
            let descriptor = data_end + if sig(&first, 0) == 0x08074b50 { 4 } else { 0 };
            ensure!(descriptor + 12 <= start, "{UNSAFE}");
            let desc = bytes(&mut zip, descriptor, 12)?;
            ensure!(
                sig(&desc, 0) == sig(&central, 16)
                    && sig(&desc, 4) == packed
                    && sig(&desc, 8) == unpacked,
                "{UNSAFE}"
            );
            descriptor + 12
        };
        ranges.push((local, record_end));
        total += u64::from(unpacked);
        ensure!(total <= 3 * 1024 * 1024 * 1024, "{UNSAFE}");
        if kind == 0o120000 {
            ensure!(
                unpacked <= 4096
                    && packed <= 8192
                    && !name.ends_with('/')
                    && name.starts_with("Shellfox.app/"),
                "{UNSAFE}"
            );
            let raw = bytes(&mut zip, data, packed as usize)?;
            let mut target = Vec::new();
            if method == 0 {
                target = raw;
            } else {
                flate2::read::DeflateDecoder::new(raw.as_slice())
                    .take(4097)
                    .read_to_end(&mut target)?;
            }
            ensure!(target.len() == unpacked as usize, "{UNSAFE}");
            let target = std::str::from_utf8(&target)?;
            ensure!(safe_text(target) && !target.starts_with('/'), "{UNSAFE}");
            links.insert(folded, target.to_lowercase());
        }
        cursor = next;
    }
    ranges.sort_unstable();
    ensure!(
        cursor == stop
            && ranges.windows(2).all(|r| r[0].1 <= r[1].0)
            && names.contains("shellfox.app/contents/info.plist")
            && names.contains("shellfox.app/contents/macos/shellfox"),
        "{UNSAFE}"
    );
    validate_links(&names, &links)
}
fn validate_links(names: &HashSet<String>, links: &HashMap<String, String>) -> Result<()> {
    for name in names {
        let mut next = name.as_str();
        while let Some((parent, _)) = next.rsplit_once('/') {
            ensure!(!links.contains_key(parent), "{UNSAFE}");
            next = parent;
        }
    }
    for (name, target) in links {
        let (parent, _) = name.rsplit_once('/').context(UNSAFE)?;
        let mut queue: VecDeque<_> = parent.split('/').chain(target.split('/')).collect();
        let mut resolved = Vec::new();
        let mut hops = 0;
        while let Some(part) = queue.pop_front() {
            match part {
                "" | "." => continue,
                ".." => {
                    ensure!(resolved.len() > 1, "{UNSAFE}");
                    resolved.pop();
                }
                _ => {
                    resolved.push(part);
                    ensure!(resolved.first() == Some(&ROOT), "{UNSAFE}");
                    if let Some(link) = links.get(&resolved.join("/")) {
                        hops += 1;
                        ensure!(hops <= 64, "{UNSAFE}");
                        resolved.pop();
                        link.rsplit('/').for_each(|part| queue.push_front(part));
                    }
                }
            }
        }
        ensure!(resolved.len() > 1, "{UNSAFE}");
    }
    Ok(())
}
fn validate_extra(mut extra: &[u8]) -> Result<()> {
    while !extra.is_empty() {
        ensure!(extra.len() >= 4, "{UNSAFE}");
        let kind = short(extra, 0);
        let size = short(extra, 2);
        // Alternate Unicode names, ZIP64 and encrypted entries aren't supported.
        ensure!(
            !matches!(kind, 1 | 0x7075 | 0x9901) && size + 4 <= extra.len(),
            "{UNSAFE}"
        );
        extra = &extra[size + 4..];
    }
    Ok(())
}
fn safe_text(value: &str) -> bool {
    // The release bundle uses ASCII names. Reject case/normalization aliases on APFS.
    !value.is_empty() && value.is_ascii() && !value.chars().any(|ch| ch.is_control() || ch == '\\')
}
fn bytes(file: &mut (impl Read + Seek), offset: u64, size: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![0; size];
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}
fn short(bytes: &[u8], i: usize) -> usize {
    usize::from(u16::from_le_bytes([bytes[i], bytes[i + 1]]))
}
fn sig(bytes: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]])
}
