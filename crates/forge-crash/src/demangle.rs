//! Turning mangled symbol names into readable ones.
//!
//! Rust names are demangled in process. Swift has no Rust demangler, so
//! Swift names go in one batch through the Swift toolchain's demangler
//! (`swift demangle`), which every machine that builds with xtool has. If it
//! is missing or slow, the names stay mangled and the result says so.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Demangles Swift symbols in bulk. Returns `None` when it can't.
pub trait SwiftDemangler {
    fn demangle(&self, names: &[String]) -> Option<Vec<String>>;
}

/// No Swift demangling: names stay mangled.
pub struct NoSwift;

impl SwiftDemangler for NoSwift {
    fn demangle(&self, _: &[String]) -> Option<Vec<String>> {
        None
    }
}

/// The Swift toolchain's demangler: `swift-demangle`, or `swift demangle`.
/// `FORGE_SWIFT_DEMANGLE` names another program taking names on stdin.
pub struct ToolchainSwift {
    pub timeout: Duration,
}

impl Default for ToolchainSwift {
    fn default() -> Self {
        ToolchainSwift {
            timeout: Duration::from_secs(20),
        }
    }
}

impl SwiftDemangler for ToolchainSwift {
    fn demangle(&self, names: &[String]) -> Option<Vec<String>> {
        if names.is_empty() {
            return Some(Vec::new());
        }
        let mut commands: Vec<Command> = Vec::new();
        if let Ok(program) = std::env::var("FORGE_SWIFT_DEMANGLE") {
            commands.push(Command::new(program));
        }
        let mut direct = Command::new("swift-demangle");
        direct.args(["--simplified", "--compact"]);
        commands.push(direct);
        let mut via_swift = Command::new("swift");
        via_swift.args(["demangle", "--simplified", "--compact"]);
        commands.push(via_swift);
        commands
            .into_iter()
            .find_map(|c| run_with_timeout(c, names, self.timeout))
    }
}

fn run_with_timeout(mut cmd: Command, names: &[String], timeout: Duration) -> Option<Vec<String>> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let input = names.join("\n") + "\n";
    let mut stdin = child.stdin.take()?;
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let _ = writer.join();
    let text = reader.join().ok()?;
    if !status?.success() {
        return None;
    }
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    (lines.len() == names.len()).then_some(lines)
}

/// Strips the extra leading underscore Mach-O symbol tables add to C-level
/// names (`_main`, `__ZN...`, `_$s...`).
pub fn strip_macho_underscore(name: &str) -> &str {
    match name.strip_prefix('_') {
        Some(rest)
            if rest.starts_with("_ZN")
                || rest.starts_with("_R")
                || rest.starts_with('$')
                || rest.starts_with("_T") =>
        {
            rest
        }
        _ => name,
    }
}

pub fn is_swift_mangled(name: &str) -> bool {
    let n = strip_macho_underscore(name);
    n.starts_with("$s") || n.starts_with("$S") || n.starts_with("_T0") || n.starts_with("$e")
}

/// A Rust name without its hash, or `None` if `name` isn't a Rust symbol.
pub fn rust(name: &str) -> Option<String> {
    let n = strip_macho_underscore(name);
    if !(n.starts_with("_ZN") || n.starts_with("_R")) {
        return None;
    }
    rustc_demangle::try_demangle(n)
        .ok()
        .map(|d| format!("{d:#}"))
}

/// Demangles every name: Rust here, Swift in one batch through `swift`.
/// Returns the readable names and whether Swift names were demangled (true
/// when there were none).
pub fn all(names: &[String], swift: &dyn SwiftDemangler) -> (HashMap<String, String>, bool) {
    let mut out = HashMap::new();
    let mut swift_names = Vec::new();
    for name in names {
        if out.contains_key(name) {
            continue;
        }
        if let Some(r) = rust(name) {
            out.insert(name.clone(), r);
        } else if is_swift_mangled(name) {
            swift_names.push(name.clone());
        } else {
            out.insert(name.clone(), name.clone());
        }
    }
    swift_names.sort();
    swift_names.dedup();
    let mut swift_ok = true;
    if !swift_names.is_empty() {
        let input: Vec<String> = swift_names
            .iter()
            .map(|n| strip_macho_underscore(n).to_string())
            .collect();
        match swift.demangle(&input) {
            Some(readable) => {
                for (name, r) in swift_names.into_iter().zip(readable) {
                    out.insert(name, r);
                }
            }
            None => {
                swift_ok = false;
                for name in swift_names {
                    let plain = strip_macho_underscore(&name).to_string();
                    out.insert(name, plain);
                }
            }
        }
    }
    (out, swift_ok)
}

/// A name from a Mach-O symbol table without the leading underscore every
/// symbol there carries (`_main` → `main`, `__ZN…` → `_ZN…`). The phone's
/// report already drops it.
pub fn from_symbol_table(name: &str) -> &str {
    name.strip_prefix('_').unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;
    impl SwiftDemangler for Fake {
        fn demangle(&self, names: &[String]) -> Option<Vec<String>> {
            Some(names.iter().map(|n| format!("swift({n})")).collect())
        }
    }

    #[test]
    fn rust_and_swift_and_c_names() {
        let names = vec![
            "_ZN4core9panicking9panic_fmt17h0123456789abcdefE".to_string(),
            "__ZN9chat_core4room4Room4send17h1a2b3c4d5e6f7a8bE".to_string(),
            "_$s9ChatXtool8RoomViewV4bodyQrvg".to_string(),
            "trap_here".to_string(),
            "start".to_string(),
        ];
        let (map, ok) = all(&names, &Fake);
        assert!(ok);
        assert_eq!(map[&names[0]], "core::panicking::panic_fmt");
        assert_eq!(map[&names[1]], "chat_core::room::Room::send");
        assert_eq!(map[&names[2]], "swift($s9ChatXtool8RoomViewV4bodyQrvg)");
        assert_eq!(map[&names[3]], "trap_here");
        assert_eq!(map[&names[4]], "start");
    }

    #[test]
    fn swift_names_stay_mangled_without_the_toolchain() {
        let names = vec!["$s9ChatXtool8RoomViewV4bodyQrvg".to_string()];
        let (map, ok) = all(&names, &NoSwift);
        assert!(!ok);
        assert_eq!(map[&names[0]], "$s9ChatXtool8RoomViewV4bodyQrvg");
    }
}
