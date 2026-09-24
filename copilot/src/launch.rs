//! Command-line options of the wrapper and how the real Copilot CLI is found and started.

use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;

use crate::circle::style::Choice;
use crate::circle::style::MagicStyle;
use crate::pty::batch_command_line;
use crate::pty::quote_arg;
use crate::pty::system_dir;

pub(crate) const HELP: &str = "\
magicopilot - GitHub Copilot CLI with the Magicodex magic circle

Usage: magicopilot [wrapper options] [copilot options]

Every option not listed here is passed to `copilot` unchanged. Inside a session,
type /magic on | off | list | random | <style> in Copilot's input box.

Wrapper options:
  --magic-style <style>   Start with this style (classic, wind, fire, water, thunder,
                          earth, holy, dark, eerie, tech, or its Chinese name), or
                          random for a different one every turn
  --magic-off             Start with the circle hidden (/magic on shows it)
  --magic-no-motion       Draw every layer at once, without motion
  --magic-copilot <path>  Run this Copilot CLI instead of the one on PATH
  --magic-help            Show this help
  --magic-version         Show the wrapper version

Environment: MAGICOPILOT_STYLE, MAGICOPILOT_OFF=1, MAGICOPILOT_NO_MOTION=1,
MAGICOPILOT_COPILOT=<path>. MAGICOPILOT_LOG=<file> writes a debug log that
contains prompts and terminal traffic.

Non-interactive runs (-p/--prompt, --acp, --help, --version, subcommands) and runs
without a console are passed straight through without the circle.
";

const SUBCOMMANDS: [&str; 15] = [
    "app",
    "login",
    "help",
    "init",
    "update",
    "version",
    "workflow",
    "sessions",
    "memories",
    "plugin",
    "mcp",
    "skill",
    "instruction",
    "lsp",
    "completion",
];

/// Copilot options that take a value, so their values are not mistaken for subcommands.
const VALUED: [&str; 34] = [
    "-i",
    "--interactive",
    "--model",
    "--reasoning-effort",
    "--context",
    "--auto-tier",
    "--agent",
    "-n",
    "--name",
    "--session-id",
    "-C",
    "--log-dir",
    "--extension-sdk-path",
    "--log-level",
    "--stream",
    "--output-format",
    "--add-dir",
    "--attachment",
    "--disable-mcp-server",
    "--add-github-mcp-toolset",
    "--add-github-mcp-tool",
    "--plugin-dir",
    "--additional-mcp-config",
    "--allow-tool",
    "--deny-tool",
    "--available-tools",
    "--excluded-tools",
    "--secret-env-vars",
    "--allow-url",
    "--deny-url",
    "--max-autopilot-continues",
    "--mode",
    "--dynamic-retrieval",
    "--max-ai-credits",
];

#[derive(Debug, PartialEq)]
pub(crate) enum Action {
    Help,
    Version,
    /// Run Copilot inside the wrapper.
    Wrap,
    /// Run Copilot directly with this process's console.
    PassThrough,
}

#[derive(Debug)]
pub(crate) struct Options {
    pub(crate) action: Action,
    pub(crate) style: Choice,
    pub(crate) enabled: bool,
    pub(crate) animations: bool,
    pub(crate) copilot: Option<PathBuf>,
    pub(crate) args: Vec<OsString>,
    pub(crate) error: Option<String>,
}

fn truthy(value: Option<OsString>) -> bool {
    value.is_some_and(|value| {
        let value = value.to_string_lossy().trim().to_ascii_lowercase();
        !matches!(value.as_str(), "" | "0" | "false" | "no" | "off")
    })
}

pub(crate) fn parse(args: Vec<OsString>, env: impl Fn(&str) -> Option<OsString>) -> Options {
    let mut options = Options {
        action: Action::Wrap,
        style: Choice::Style(MagicStyle::Classic),
        enabled: !truthy(env("MAGICOPILOT_OFF")),
        animations: !truthy(env("MAGICOPILOT_NO_MOTION")),
        copilot: env("MAGICOPILOT_COPILOT").map(PathBuf::from),
        args: Vec::new(),
        error: None,
    };
    if let Some(style) = env("MAGICOPILOT_STYLE") {
        match Choice::parse(&style.to_string_lossy()) {
            Some(style) => options.style = style,
            None => options.error = Some(format!("unknown MAGICOPILOT_STYLE: {style:?}")),
        }
    }
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy().into_owned();
        let (flag, inline) = match text.split_once('=') {
            Some((flag, value)) if flag.starts_with("--magic-") => {
                (flag.to_string(), Some(value.to_string()))
            }
            _ => (text.clone(), None),
        };
        let mut value = || {
            inline
                .clone()
                .or_else(|| args.next().map(|v| v.to_string_lossy().into_owned()))
        };
        match flag.as_str() {
            "--magic-help" => options.action = Action::Help,
            "--magic-version" => options.action = Action::Version,
            "--magic-off" => options.enabled = false,
            "--magic-no-motion" => options.animations = false,
            "--magic-style" => match value().as_deref().and_then(Choice::parse) {
                Some(style) => {
                    options.style = style;
                    options.enabled = true;
                }
                None => options.error = Some("--magic-style needs a known style".into()),
            },
            "--magic-copilot" => match value() {
                Some(path) => options.copilot = Some(PathBuf::from(path)),
                None => options.error = Some("--magic-copilot needs a path".into()),
            },
            _ if flag.starts_with("--magic-") => {
                options.error = Some(format!("unknown wrapper option {flag}"));
            }
            _ => options.args.push(arg),
        }
    }
    if options.action == Action::Wrap && !interactive(&options.args) {
        options.action = Action::PassThrough;
    }
    options
}

/// Whether these Copilot arguments start the interactive terminal UI.
fn interactive(args: &[OsString]) -> bool {
    let mut expecting_value = false;
    for arg in args {
        let arg = arg.to_string_lossy();
        if expecting_value {
            expecting_value = false;
            continue;
        }
        let flag = arg.split_once('=').map_or(arg.as_ref(), |(flag, _)| flag);
        match flag {
            "-p" | "--prompt" | "--acp" | "-h" | "--help" | "-v" | "--version" => return false,
            _ if VALUED.contains(&flag) && !arg.contains('=') => expecting_value = true,
            _ if !arg.starts_with('-') => return !SUBCOMMANDS.contains(&arg.as_ref()),
            _ => {}
        }
    }
    true
}

/// Whether the arguments already name a session, so none is created for tracking.
pub(crate) fn names_session(args: &[OsString]) -> bool {
    args.iter().any(|arg| {
        let arg = arg.to_string_lossy();
        let flag = arg.split_once('=').map_or(arg.as_ref(), |(flag, _)| flag);
        matches!(
            flag,
            "--session-id" | "-r" | "--resume" | "--continue" | "--connect"
        )
    })
}

/// The program and leading arguments that start the Copilot CLI.
#[derive(Debug, PartialEq)]
pub(crate) struct Target {
    pub(crate) program: PathBuf,
    pub(crate) prefix: Vec<OsString>,
}

impl Target {
    /// A `.cmd`/`.bat` file, which runs in `cmd.exe` with its own parsing of the command line.
    fn is_batch(&self) -> bool {
        self.program
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
    }

    /// The `CreateProcessW` command line that passes `args` unchanged.
    pub(crate) fn command_line(&self, args: &[OsString]) -> std::io::Result<String> {
        let args: Vec<&OsStr> = self
            .prefix
            .iter()
            .chain(args)
            .map(OsString::as_os_str)
            .collect();
        if self.is_batch() {
            return batch_command_line(&self.program, &args);
        }
        Ok(std::iter::once(self.program.as_os_str())
            .chain(args)
            .map(|arg| quote_arg(&arg.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(" "))
    }
}

fn platform_package() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "copilot-win32-arm64",
        _ => "copilot-win32-x64",
    }
}

/// Resolves an npm `copilot.cmd`/`copilot.ps1` shim to what it would run.
fn from_npm_shim(shim: &Path) -> Option<Target> {
    let dir = shim.parent()?;
    let package = dir.join("node_modules").join("@github").join("copilot");
    let binary = package
        .join("node_modules")
        .join("@github")
        .join(platform_package())
        .join("copilot.exe");
    if binary.is_file() {
        // The npm loader starts exactly this binary with the same arguments.
        return Some(Target {
            program: binary,
            prefix: Vec::new(),
        });
    }
    let loader = package.join("npm-loader.js");
    if !loader.is_file() {
        return None;
    }
    let node = Some(dir.join("node.exe"))
        .filter(|node| node.is_file())
        .or_else(|| search_path(OsStr::new("node.exe")))?;
    Some(Target {
        program: node,
        prefix: vec![loader.into_os_string()],
    })
}

fn search_path(name: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn target_for(path: &Path) -> Target {
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase());
    if matches!(extension.as_deref(), Some("cmd" | "bat" | "ps1"))
        && let Some(target) = from_npm_shim(path)
    {
        return target;
    }
    match extension.as_deref() {
        // Started through PowerShell as typing its name there would, under the same
        // execution policy, rather than opened with whatever program `.ps1` files open with.
        Some("ps1") => Target {
            program: system_dir().join(r"WindowsPowerShell\v1.0\powershell.exe"),
            prefix: vec!["-NoProfile".into(), "-File".into(), path.into()],
        },
        Some("js" | "mjs") => Target {
            program: search_path(OsStr::new("node.exe")).unwrap_or_else(|| PathBuf::from("node")),
            prefix: vec![path.as_os_str().to_owned()],
        },
        // Executables, and other shims (pnpm, yarn, scoop, ...): `.cmd`/`.bat` files run in
        // cmd.exe with their arguments escaped for it by `Target::command_line`.
        _ => Target {
            program: path.to_path_buf(),
            prefix: Vec::new(),
        },
    }
}

/// The path Windows opens for `path`: absolute, with trailing dots and spaces removed from the
/// file name. `copilot.cmd.` is still a batch file, so the extension must be read from this.
fn normalized(path: &Path) -> Result<PathBuf, String> {
    std::path::absolute(path).map_err(|err| format!("{}: {err}", path.display()))
}

pub(crate) fn resolve(explicit: Option<&Path>) -> Result<Target, String> {
    if let Some(path) = explicit {
        return if path.is_file() {
            Ok(target_for(&normalized(path)?))
        } else {
            Err(format!("Copilot CLI not found at {}", path.display()))
        };
    }
    let dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    for dir in dirs {
        for name in ["copilot.exe", "copilot.cmd", "copilot.ps1"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Ok(target_for(&normalized(&candidate)?));
            }
        }
    }
    Err(
        "GitHub Copilot CLI (`copilot`) was not found on PATH. Install it first, e.g. \
         `npm install -g @github/copilot` or `winget install GitHub.Copilot`, or pass \
         --magic-copilot <path>."
            .to_string(),
    )
}

/// A version-4 UUID from the process's random hash keys and the clock.
pub(crate) fn new_session_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::BuildHasher;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let state = RandomState::new();
    let high = state.hash_one((nanos, std::process::id(), 1_u8));
    let low = state.hash_one((nanos, std::process::id(), 2_u8));
    let high = (high & 0xffff_ffff_ffff_0fff) | 0x4000;
    let low = (low & 0x3fff_ffff_ffff_ffff) | 0x8000_0000_0000_0000;
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        high >> 32,
        (high >> 16) & 0xffff,
        high & 0xffff,
        low >> 48,
        low & 0xffff_ffff_ffff
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(args: &[&str]) -> Options {
        parse(args.iter().map(OsString::from).collect(), |_| None)
    }

    #[test]
    fn wrapper_options_are_taken_and_the_rest_passed_on() {
        let parsed = options(&[
            "--magic-style",
            "火",
            "--model",
            "gpt-5.4",
            "--magic-no-motion",
            "-C",
            "dir",
        ]);
        assert_eq!(parsed.action, Action::Wrap);
        assert_eq!(parsed.style, Choice::Style(MagicStyle::Fire));
        assert!(!parsed.animations);
        assert_eq!(
            parsed.args,
            ["--model", "gpt-5.4", "-C", "dir"].map(OsString::from)
        );
        let parsed = options(&["--magic-style=tech", "--magic-off"]);
        assert_eq!(
            (parsed.style, parsed.enabled),
            (Choice::Style(MagicStyle::Tech), false)
        );
        assert_eq!(options(&["--magic-style", "Random"]).style, Choice::Random);
        assert_eq!(options(&["--magic-style=随机"]).style, Choice::Random);
        assert!(options(&["--magic-style", "ice"]).error.is_some());
        assert!(options(&["--magic-bogus"]).error.is_some());
    }

    #[test]
    fn non_interactive_runs_pass_through() {
        for args in [
            &["-p", "hi"][..],
            &["--prompt=hi"],
            &["--acp"],
            &["--help"],
            &["mcp", "list"],
            &["workflow", "run", "nightly"],
            &["--model", "x", "update"],
        ] {
            assert_eq!(options(args).action, Action::PassThrough, "{args:?}");
        }
        for args in [
            &[][..],
            &["-i", "update the docs"],
            &["--model", "update"],
            &["--resume"],
        ] {
            assert_eq!(options(args).action, Action::Wrap, "{args:?}");
        }
    }

    #[test]
    fn environment_sets_defaults() {
        let parsed = parse(Vec::new(), |name| match name {
            "MAGICOPILOT_STYLE" => Some("WATER".into()),
            "MAGICOPILOT_NO_MOTION" => Some("1".into()),
            _ => None,
        });
        assert_eq!(parsed.style, Choice::Style(MagicStyle::Water));
        assert!(!parsed.animations && parsed.enabled);
    }

    #[test]
    fn sessions_named_by_arguments_are_detected() {
        assert!(names_session(&["--resume".into()]));
        assert!(names_session(&["--session-id=abc".into()]));
        assert!(!names_session(&["-i".into(), "--resume me".into()]));
    }

    #[test]
    fn session_ids_look_like_uuids() {
        let id = new_session_id();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"));
        assert_ne!(id, new_session_id());
    }

    #[test]
    fn command_lines_quote_each_argument() {
        let target = Target {
            program: PathBuf::from(r"C:\Program Files\copilot.exe"),
            prefix: Vec::new(),
        };
        assert_eq!(
            target
                .command_line(&["-i".into(), "say \"hi\"".into()])
                .unwrap(),
            r#""C:\Program Files\copilot.exe" -i "say \"hi\"""#
        );
    }

    #[test]
    fn shims_that_are_not_npm_ones_run_safely() {
        let dir = std::env::temp_dir().join(format!("magicopilot-shim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (cmd, ps1) = (dir.join("copilot.cmd"), dir.join("copilot.ps1"));
        std::fs::write(&cmd, "@echo off\r\n").unwrap();
        std::fs::write(&ps1, "").unwrap();
        let batch = target_for(&cmd);
        let script = target_for(&ps1);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(batch.program, cmd);
        let line = batch
            .command_line(&["-i".into(), "a \" & calc & \"".into()])
            .unwrap();
        assert!(
            line.to_ascii_lowercase()
                .contains(r"\system32\cmd.exe /e:on /v:off /d /c "),
            "{line}"
        );
        assert!(line.ends_with(r#" -i "a "" & calc & """""#), "{line}");
        assert!(batch.command_line(&["two\nlines".into()]).is_err());

        assert!(
            script
                .program
                .ends_with(r"WindowsPowerShell\v1.0\powershell.exe")
        );
        assert_eq!(
            script.prefix,
            ["-NoProfile".into(), "-File".into(), ps1.into_os_string()]
        );
    }

    /// Windows drops trailing dots and spaces, so these name the batch file too (CVE-2024-43402).
    #[test]
    fn trailing_dots_and_spaces_do_not_hide_a_batch_file() {
        let dir = std::env::temp_dir().join(format!("magicopilot-dots-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cmd = dir.join("copilot.cmd");
        std::fs::write(&cmd, "@echo off\r\n").unwrap();
        let targets: Vec<Target> = [" ", ".", ". .", " . "]
            .iter()
            .map(|suffix| {
                let mut path = cmd.clone().into_os_string();
                path.push(suffix);
                resolve(Some(Path::new(&path))).unwrap()
            })
            .collect();
        let _ = std::fs::remove_dir_all(&dir);
        for target in targets {
            assert_eq!(target.program, cmd);
            let line = target.command_line(&["a \" & b".into()]).unwrap();
            assert!(
                line.to_ascii_lowercase()
                    .contains(r"\system32\cmd.exe /e:on /v:off /d /c "),
                "{line}"
            );
        }
    }
}
