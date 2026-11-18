use super::OwnedProcess;
use shellfox_core::SettingsProcessRule;

const NODE_VALUES: &[&str] = &[
    "-r",
    "--require",
    "--import",
    "--loader",
    "--experimental-loader",
    "--title",
    "--conditions",
    "-C",
    "--input-type",
    "--unhandled-rejections",
    "--redirect-warnings",
    "--trace-event-categories",
    "--trace-event-file-pattern",
    "--icu-data-dir",
    "--openssl-config",
    "--heap-prof-dir",
    "--heap-prof-name",
    "--cpu-prof-dir",
    "--cpu-prof-name",
    "--test-name-pattern",
    "--test-reporter",
    "--test-reporter-destination",
    "--test-concurrency",
    "--test-shard",
    "--disable-warning",
    "--inspect-port",
    "--heapsnapshot-signal",
    "--heapsnapshot-near-heap-limit",
    "--snapshot-blob",
];
const NODE_FLAGS: &[&str] = &[
    "--no-warnings",
    "--enable-source-maps",
    "--trace-warnings",
    "--trace-uncaught",
    "--trace-deprecation",
    "--no-deprecation",
    "--throw-deprecation",
    "--use-strict",
    "--experimental-modules",
    "--inspect",
    "--inspect-brk",
    "--inspect-wait",
    "--test",
];

pub(super) fn basename(path: &str, windows: bool) -> &str {
    if windows {
        path.rsplit(['/', '\\']).next().unwrap_or(path)
    } else {
        path.rsplit('/').next().unwrap_or(path)
    }
}
fn equal(a: &str, b: &str, windows: bool) -> bool {
    if windows {
        a.to_lowercase() == b.to_lowercase()
    } else {
        a == b
    }
}
fn normalize_windows(path: &str) -> String {
    let path = path.replace('\\', "/").to_lowercase();
    let prefix = if path.starts_with("//") {
        "//"
    } else if path.starts_with('/') {
        "/"
    } else {
        ""
    };
    let mut parts = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".."
            && parts
                .last()
                .is_some_and(|last: &&str| !last.ends_with(':') && *last != "..")
        {
            parts.pop();
        } else if part != ".." || prefix.is_empty() {
            parts.push(part);
        }
    }
    format!("{prefix}{}", parts.join("/"))
}
fn executable_matches(rule: &SettingsProcessRule, executable: &str, windows: bool) -> bool {
    if !rule.executable_paths.is_empty() {
        rule.executable_paths.iter().any(|path| {
            if windows {
                normalize_windows(path) == normalize_windows(executable)
            } else {
                path == executable
            }
        })
    } else {
        rule.executable_basenames
            .iter()
            .any(|name| equal(name, basename(executable, windows), windows))
    }
}
fn suffix_matches(path: &str, suffix: &str, windows: bool) -> bool {
    let (path, suffix) = if windows {
        (normalize_windows(path), normalize_windows(suffix))
    } else {
        (path.into(), suffix.into())
    };
    let path = path.trim_start_matches('/');
    let suffix = suffix.trim_start_matches('/');
    path == suffix
        || path
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('/'))
}
enum ScriptSlot<'a> {
    Path(&'a str),
    None,
    Unsupported,
}
fn script_slot<'a>(executable: &str, argv: &'a [String], windows: bool) -> ScriptSlot<'a> {
    let executable = basename(executable, windows);
    let executable = if windows {
        executable
            .to_lowercase()
            .trim_end_matches(".exe")
            .to_owned()
    } else {
        executable.to_owned()
    };
    let mut i = 1;
    if executable == "pwsh" || executable == "powershell" {
        while let Some(arg) = argv.get(i) {
            let arg = arg.to_lowercase();
            if [
                "-command",
                "-c",
                "-encodedcommand",
                "-enc",
                "-ec",
                "-e",
                "-commandwithargs",
            ]
            .contains(&arg.as_str())
            {
                return ScriptSlot::None;
            }
            if ["-file", "-f"].contains(&arg.as_str()) {
                return argv
                    .get(i + 1)
                    .filter(|path| !path.is_empty())
                    .map_or(ScriptSlot::Unsupported, |path| ScriptSlot::Path(path));
            }
            if [
                "-executionpolicy",
                "-ep",
                "-workingdirectory",
                "-wd",
                "-inputformat",
                "-outputformat",
                "-windowstyle",
                "-configurationname",
            ]
            .contains(&arg.as_str())
            {
                i += 1;
                if i >= argv.len() {
                    return ScriptSlot::Unsupported;
                }
            } else if ![
                "-nologo",
                "-noprofile",
                "-nop",
                "-noexit",
                "-noninteractive",
            ]
            .contains(&arg.as_str())
            {
                return ScriptSlot::Unsupported;
            }
            i += 1;
        }
        return ScriptSlot::None;
    }
    if !["node", "nodejs", "bun"].contains(&executable.as_str()) {
        return ScriptSlot::Unsupported;
    }
    while let Some(arg) = argv.get(i) {
        if arg == "--" {
            return argv
                .get(i + 1)
                .map_or(ScriptSlot::None, |path| ScriptSlot::Path(path));
        }
        if ["-e", "--eval", "-p", "--print"].contains(&arg.as_str())
            || arg.starts_with("--eval=")
            || arg.starts_with("--print=")
            || arg.len() > 2 && (arg.starts_with("-e") || arg.starts_with("-p"))
        {
            return ScriptSlot::None;
        }
        if NODE_VALUES.contains(&arg.as_str()) {
            i += 2;
            if i > argv.len() {
                return ScriptSlot::Unsupported;
            }
            continue;
        }
        if NODE_FLAGS.contains(&arg.as_str()) {
            i += 1;
            continue;
        }
        if arg.split_once('=').is_some_and(|(name, _)| {
            NODE_VALUES.contains(&name)
                || ["--inspect", "--inspect-brk", "--inspect-wait"].contains(&name)
        }) {
            i += 1;
            continue;
        }
        if executable == "bun" && arg == "run" {
            return argv
                .get(i + 1)
                .filter(|path| {
                    !path.starts_with('-')
                        && (path.contains(['/', '\\'])
                            || [
                                ".js", ".cjs", ".mjs", ".jsx", ".cjsx", ".mjsx", ".ts", ".cts",
                                ".mts", ".tsx", ".ctsx", ".mtsx",
                            ]
                            .iter()
                            .any(|suffix| path.ends_with(suffix)))
                })
                .map_or(ScriptSlot::None, |path| ScriptSlot::Path(path));
        }
        if executable == "bun"
            && [
                "x", "exec", "test", "install", "build", "add", "remove", "update",
            ]
            .contains(&arg.as_str())
        {
            return ScriptSlot::None;
        }
        if arg.starts_with('-') {
            return ScriptSlot::Unsupported;
        }
        return ScriptSlot::Path(arg);
    }
    ScriptSlot::None
}
/// Counts identities, not matching rules. Diagnostic text never contains argv.
pub(super) fn match_agent(
    process: &OwnedProcess,
    rules: &[SettingsProcessRule],
    windows: bool,
) -> (bool, Option<&'static str>) {
    let Some(executable) = &process.executable else {
        return (false, None);
    };
    let mut names = vec![basename(executable, windows)];
    if let Some(name) = &process.process_name {
        names.push(name);
    }
    if let Some(first) = process.argv.as_ref().and_then(|argv| argv.first()) {
        names.push(basename(first, windows));
    }
    let named = |rule: &SettingsProcessRule| {
        rule.executable_paths.is_empty()
            && rule
                .process_names
                .as_deref()
                .unwrap_or_else(|| {
                    if rule.script_path_suffixes.is_empty() {
                        &rule.executable_basenames
                    } else {
                        &[]
                    }
                })
                .iter()
                .any(|name| names.iter().any(|actual| equal(name, actual, windows)))
    };
    if rules.iter().any(|rule| rule.enabled && named(rule)) {
        return (true, None);
    }
    let recognized_name = rules.iter().any(named);
    let mut reason = None;
    for rule in rules.iter().filter(|rule| rule.enabled) {
        if recognized_name && rule.executable_paths.is_empty()
            || !executable_matches(rule, executable, windows)
        {
            continue;
        }
        if rule.script_path_suffixes.is_empty() {
            return (true, None);
        }
        let Some(argv) = &process.argv else {
            reason = Some("A relevant descendant command line is inaccessible.");
            continue;
        };
        match script_slot(executable, argv, windows) {
            ScriptSlot::Path(path)
                if rule
                    .script_path_suffixes
                    .iter()
                    .any(|suffix| suffix_matches(path, suffix, windows)) =>
            {
                return (true, None)
            }
            ScriptSlot::Unsupported => {
                reason = Some("A script rule uses an unsupported interpreter or option form.")
            }
            _ => {}
        }
    }
    (false, reason)
}
