//! Detects installed code editors (VS Code, Cursor, Zed, JetBrains IDEs, Neovim, Helix, …)
//! and opens folders or `file:line` targets in a detected editor without ever executing
//! arbitrary user/webview-supplied command strings.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

/// A detected code editor installed on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorInfo {
    /// Stable identifier persisted in `Settings.preferred_editor` (e.g. `"cursor"`, `"vscode"`, `"zed"`).
    pub id: String,
    /// Human-readable display name (e.g. `"Cursor"`, `"VS Code"`, `"Zed"`).
    pub name: String,
    /// Resolved executable path.
    pub command: String,
    /// True for terminal-based editors (`nvim`, `hx`) that must launch inside a terminal emulator.
    pub terminal: bool,
}

struct EditorSpec {
    id: &'static str,
    name: &'static str,
    bins: &'static [&'static str],
    #[cfg(target_os = "macos")]
    mac_paths: &'static [&'static str],
    #[cfg(windows)]
    win_rel_paths: &'static [&'static str],
    terminal: bool,
}

const SPECS: &[EditorSpec] = &[
    EditorSpec {
        id: "cursor",
        name: "Cursor",
        bins: &["cursor"],
        #[cfg(target_os = "macos")]
        mac_paths: &["/Applications/Cursor.app/Contents/Resources/app/bin/cursor"],
        #[cfg(windows)]
        win_rel_paths: &[r"Programs\cursor\resources\app\bin\cursor.cmd"],
        terminal: false,
    },
    EditorSpec {
        id: "vscode",
        name: "VS Code",
        bins: &["code"],
        #[cfg(target_os = "macos")]
        mac_paths: &["/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"],
        #[cfg(windows)]
        win_rel_paths: &[r"Programs\Microsoft VS Code\bin\code.cmd"],
        terminal: false,
    },
    EditorSpec {
        id: "vscode-insiders",
        name: "VS Code Insiders",
        bins: &["code-insiders"],
        #[cfg(target_os = "macos")]
        mac_paths: &[
            "/Applications/Visual Studio Code - Insiders.app/Contents/Resources/app/bin/code",
        ],
        #[cfg(windows)]
        win_rel_paths: &[r"Programs\Microsoft VS Code Insiders\bin\code-insiders.cmd"],
        terminal: false,
    },
    EditorSpec {
        id: "zed",
        name: "Zed",
        bins: &["zed", "zeditor"],
        #[cfg(target_os = "macos")]
        mac_paths: &["/Applications/Zed.app/Contents/MacOS/cli"],
        #[cfg(windows)]
        win_rel_paths: &[r"Programs\Zed\Zed.exe"],
        terminal: false,
    },
    EditorSpec {
        id: "windsurf",
        name: "Windsurf",
        bins: &["windsurf"],
        #[cfg(target_os = "macos")]
        mac_paths: &["/Applications/Windsurf.app/Contents/Resources/app/bin/windsurf"],
        #[cfg(windows)]
        win_rel_paths: &[r"Programs\Windsurf\bin\windsurf.cmd"],
        terminal: false,
    },
    EditorSpec {
        id: "antigravity",
        name: "Antigravity",
        bins: &["antigravity", "agy"],
        #[cfg(target_os = "macos")]
        mac_paths: &["/Applications/Antigravity.app/Contents/Resources/app/bin/antigravity"],
        #[cfg(windows)]
        win_rel_paths: &[],
        terminal: false,
    },
    EditorSpec {
        id: "intellij",
        name: "IntelliJ IDEA",
        bins: &["idea"],
        #[cfg(target_os = "macos")]
        mac_paths: &["/Applications/IntelliJ IDEA.app/Contents/MacOS/idea"],
        #[cfg(windows)]
        win_rel_paths: &[],
        terminal: false,
    },
    EditorSpec {
        id: "webstorm",
        name: "WebStorm",
        bins: &["webstorm"],
        #[cfg(target_os = "macos")]
        mac_paths: &["/Applications/WebStorm.app/Contents/MacOS/webstorm"],
        #[cfg(windows)]
        win_rel_paths: &[],
        terminal: false,
    },
    EditorSpec {
        id: "pycharm",
        name: "PyCharm",
        bins: &["pycharm"],
        #[cfg(target_os = "macos")]
        mac_paths: &["/Applications/PyCharm.app/Contents/MacOS/pycharm"],
        #[cfg(windows)]
        win_rel_paths: &[],
        terminal: false,
    },
    EditorSpec {
        id: "rustrover",
        name: "RustRover",
        bins: &["rustrover"],
        #[cfg(target_os = "macos")]
        mac_paths: &["/Applications/RustRover.app/Contents/MacOS/rustrover"],
        #[cfg(windows)]
        win_rel_paths: &[],
        terminal: false,
    },
    EditorSpec {
        id: "goland",
        name: "GoLand",
        bins: &["goland"],
        #[cfg(target_os = "macos")]
        mac_paths: &["/Applications/GoLand.app/Contents/MacOS/goland"],
        #[cfg(windows)]
        win_rel_paths: &[],
        terminal: false,
    },
    EditorSpec {
        id: "fleet",
        name: "Fleet",
        bins: &["fleet"],
        #[cfg(target_os = "macos")]
        mac_paths: &["/Applications/Fleet.app/Contents/MacOS/Fleet"],
        #[cfg(windows)]
        win_rel_paths: &[],
        terminal: false,
    },
    EditorSpec {
        id: "sublime",
        name: "Sublime Text",
        bins: &["subl"],
        #[cfg(target_os = "macos")]
        mac_paths: &["/Applications/Sublime Text.app/Contents/SharedSupport/bin/subl"],
        #[cfg(windows)]
        win_rel_paths: &[],
        terminal: false,
    },
    EditorSpec {
        id: "nvim",
        name: "Neovim",
        bins: &["nvim"],
        #[cfg(target_os = "macos")]
        mac_paths: &[],
        #[cfg(windows)]
        win_rel_paths: &[],
        terminal: true,
    },
    EditorSpec {
        id: "hx",
        name: "Helix",
        bins: &["hx", "helix"],
        #[cfg(target_os = "macos")]
        mac_paths: &[],
        #[cfg(windows)]
        win_rel_paths: &[],
        terminal: true,
    },
];

/// Returns true if `id` is a known editor id.
pub fn is_known_editor(id: &str) -> bool {
    SPECS.iter().any(|s| s.id == id)
}

fn find_spec_binary(spec: &EditorSpec) -> Option<PathBuf> {
    for bin in spec.bins {
        if let Ok(found) = which::which(bin) {
            return Some(found);
        }
    }
    #[cfg(target_os = "macos")]
    {
        for p in spec.mac_paths {
            let pb = PathBuf::from(p);
            if pb.is_file() {
                return Some(pb);
            }
        }
    }
    #[cfg(windows)]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let base = PathBuf::from(local);
            for rel in spec.win_rel_paths {
                let pb = base.join(rel);
                if pb.is_file() {
                    return Some(pb);
                }
            }
        }
    }
    None
}

/// Detects installed code editors on this machine, in preference order.
pub fn detect_editors() -> Vec<EditorInfo> {
    let mut out = Vec::new();
    for spec in SPECS {
        if let Some(cmd) = find_spec_binary(spec) {
            out.push(EditorInfo {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                command: cmd.to_string_lossy().into_owned(),
                terminal: spec.terminal,
            });
        }
    }
    out
}

/// Formats command-line arguments to open `target` (optionally at `line` / `column`) in `editor_id`.
pub fn launch_args(
    editor_id: &str,
    target: &Path,
    is_dir: bool,
    line: Option<u32>,
    column: Option<u32>,
) -> Vec<String> {
    let path_str = target.to_string_lossy().into_owned();
    let line = line.filter(|&l| l > 0);
    let col = column.filter(|&c| c > 0);

    if is_dir || line.is_none() {
        return vec![path_str];
    }
    let line = line.unwrap();
    let loc = match col {
        Some(c) => format!("{path_str}:{line}:{c}"),
        None => format!("{path_str}:{line}"),
    };

    match editor_id {
        "cursor" | "vscode" | "vscode-insiders" | "windsurf" | "antigravity" | "fleet" => {
            vec!["--goto".into(), loc]
        }
        "zed" | "sublime" | "hx" => vec![loc],
        "intellij" | "webstorm" | "pycharm" | "rustrover" | "goland" => {
            vec!["--line".into(), line.to_string(), path_str]
        }
        "nvim" => vec![format!("+{line}"), path_str],
        _ => vec![path_str],
    }
}

/// Opens `target` (a folder or file, optionally at `line`/`column`) in the chosen detected editor.
pub fn open_in_editor(
    editor_id: Option<&str>,
    preferred: Option<&str>,
    target: &Path,
    line: Option<u32>,
    column: Option<u32>,
) -> Result<EditorInfo> {
    if !target.is_absolute() {
        bail!(
            "Can't open {}: path must be absolute",
            target.to_string_lossy()
        );
    }
    if crate::files::is_network_path(&target.to_string_lossy()) {
        bail!("Network locations can't be opened in an editor");
    }
    let meta = std::fs::metadata(target)
        .map_err(|_| anyhow!("{} does not exist", target.to_string_lossy()))?;
    if !meta.is_file() && !meta.is_dir() {
        bail!("{} is not a file or folder", target.to_string_lossy());
    }

    let installed = detect_editors();
    let editor = editor_id
        .and_then(|id| installed.iter().find(|e| e.id == id))
        .or_else(|| preferred.and_then(|id| installed.iter().find(|e| e.id == id)))
        .or_else(|| installed.first())
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "No supported code editor was found on PATH (VS Code, Cursor, Zed, JetBrains, Sublime, Neovim, Helix)."
            )
        })?;

    let args = launch_args(&editor.id, target, meta.is_dir(), line, column);
    let cwd = if meta.is_dir() {
        target
    } else {
        target.parent().unwrap_or(target)
    };

    if editor.terminal {
        spawn_in_terminal(&editor.command, &args, cwd)?;
    } else {
        spawn_gui_editor(&editor.command, &args, cwd)?;
    }
    Ok(editor)
}

fn spawn_gui_editor(program: &str, args: &[String], cwd: &Path) -> Result<()> {
    let mut cmd = if cfg!(windows)
        && (program.ends_with(".cmd") || program.ends_with(".CMD") || program.ends_with(".bat"))
    {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(program).args(args);
        c
    } else {
        let mut c = Command::new(program);
        c.args(args);
        c
    };
    crate::childenv::sanitize_std_command(&mut cmd);
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn()
        .map_err(|e| anyhow!("Couldn't start {program}: {e}"))?;
    Ok(())
}

fn spawn_in_terminal(program: &str, args: &[String], cwd: &Path) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let candidates: &[(&str, &[&str])] = &[
            ("ghostty", &["-e"]),
            ("alacritty", &["-e"]),
            ("kitty", &["--"]),
            ("foot", &["--"]),
            ("wezterm", &["start", "--"]),
            ("x-terminal-emulator", &["-e"]),
            ("gnome-terminal", &["--"]),
            ("konsole", &["-e"]),
            ("xterm", &["-e"]),
        ];
        for (term, flag) in candidates {
            if let Ok(term_path) = which::which(term) {
                let mut cmd = Command::new(term_path);
                crate::childenv::sanitize_std_command(&mut cmd);
                cmd.current_dir(cwd)
                    .args(*flag)
                    .arg(program)
                    .args(args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                cmd.spawn()
                    .map_err(|e| anyhow!("Couldn't launch {term} for {program}: {e}"))?;
                return Ok(());
            }
        }
        bail!("Couldn't find a terminal emulator to launch {program}");
    }
    #[cfg(target_os = "macos")]
    {
        let mut quoted = vec![shell_quote(program)];
        for a in args {
            quoted.push(shell_quote(a));
        }
        let script = format!(
            "cd {} && {}",
            shell_quote(&cwd.to_string_lossy()),
            quoted.join(" ")
        );
        let apple = format!(
            "tell application \"Terminal\" to do script \"{}\"\ntell application \"Terminal\" to activate",
            script.replace('\\', "\\\\").replace('"', "\\\"")
        );
        let mut cmd = Command::new("osascript");
        crate::childenv::sanitize_std_command(&mut cmd);
        cmd.args(["-e", &apple])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd.spawn()
            .map_err(|e| anyhow!("Couldn't launch Terminal for {program}: {e}"))?;
        Ok(())
    }
    #[cfg(windows)]
    {
        let mut cmd = if let Ok(wt) = which::which("wt") {
            let mut c = Command::new(wt);
            c.args(["-d", &cwd.to_string_lossy(), program]).args(args);
            c
        } else {
            let mut c = Command::new("cmd");
            c.args(["/C", "start", "", program]).args(args);
            c
        };
        crate::childenv::sanitize_std_command(&mut cmd);
        cmd.current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd.spawn()
            .map_err(|e| anyhow!("Couldn't launch terminal for {program}: {e}"))?;
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (program, args, cwd);
        bail!("Terminal editors aren't supported on this platform");
    }
}

#[cfg(target_os = "macos")]
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_args_formats_line_and_column_per_editor() {
        let p = Path::new("/repo/src/main.rs");
        assert_eq!(
            launch_args("vscode", p, false, Some(42), None),
            vec!["--goto", "/repo/src/main.rs:42"]
        );
        assert_eq!(
            launch_args("cursor", p, false, Some(42), Some(7)),
            vec!["--goto", "/repo/src/main.rs:42:7"]
        );
        assert_eq!(
            launch_args("zed", p, false, Some(42), None),
            vec!["/repo/src/main.rs:42"]
        );
        assert_eq!(
            launch_args("intellij", p, false, Some(42), None),
            vec!["--line", "42", "/repo/src/main.rs"]
        );
        assert_eq!(
            launch_args("nvim", p, false, Some(42), None),
            vec!["+42", "/repo/src/main.rs"]
        );
        assert_eq!(
            launch_args("hx", p, false, Some(42), Some(3)),
            vec!["/repo/src/main.rs:42:3"]
        );
        // Directories ignore line/column.
        assert_eq!(
            launch_args("vscode", Path::new("/repo"), true, Some(42), None),
            vec!["/repo"]
        );
    }

    #[test]
    fn known_editor_ids() {
        assert!(is_known_editor("cursor"));
        assert!(is_known_editor("vscode"));
        assert!(is_known_editor("zed"));
        assert!(is_known_editor("nvim"));
        assert!(!is_known_editor("rm -rf /"));
    }
}
