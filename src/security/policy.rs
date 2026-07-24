// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use parking_lot::{Mutex, RwLock};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AutonomyLevel {

    ReadOnly,

    #[default]
    Supervised,

    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandRiskLevel {
    Low,
    Medium,
    High,
}

impl CommandRiskLevel {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            CommandRiskLevel::Low => "low",
            CommandRiskLevel::Medium => "medium",
            CommandRiskLevel::High => "high",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolOperation {
    Read,
    Act,
}

#[derive(Debug)]
pub struct ActionTracker {

    actions: Mutex<Vec<Instant>>,
}

impl ActionTracker {
    pub fn new() -> Self {
        Self {
            actions: Mutex::new(Vec::new()),
        }
    }

    pub fn record(&self) -> usize {
        let mut actions = self.actions.lock();
        let cutoff = Instant::now()
            .checked_sub(std::time::Duration::from_secs(3600))
            .unwrap_or_else(Instant::now);
        actions.retain(|t| *t > cutoff);
        actions.push(Instant::now());
        actions.len()
    }

    pub fn count(&self) -> usize {
        let mut actions = self.actions.lock();
        let cutoff = Instant::now()
            .checked_sub(std::time::Duration::from_secs(3600))
            .unwrap_or_else(Instant::now);
        actions.retain(|t| *t > cutoff);
        actions.len()
    }
}

impl Clone for ActionTracker {
    fn clone(&self) -> Self {
        let actions = self.actions.lock();
        Self {
            actions: Mutex::new(actions.clone()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SecurityPolicy {
    pub autonomy: AutonomyLevel,
    workspace_root: Arc<RwLock<PathBuf>>,
    pub workspace_only: bool,
    pub allowed_commands: Vec<String>,
    pub forbidden_paths: Vec<String>,
    pub allowed_roots: Vec<PathBuf>,
    pub max_actions_per_hour: u32,
    pub max_cost_per_day_cents: u32,
    pub require_approval_for_medium_risk: bool,
    pub block_high_risk_commands: bool,
    pub shell_env_passthrough: Vec<String>,
    enable_command_policy: Arc<AtomicBool>,
    pub tracker: ActionTracker,
}

impl SecurityPolicy {
    #[inline]
    pub fn is_command_policy_enabled(&self) -> bool {
        self.enable_command_policy.load(Ordering::Acquire)
    }

    #[inline]
    pub fn set_command_policy_enabled(&self, value: bool) {
        self.enable_command_policy.store(value, Ordering::Release);
    }
}

#[cfg(not(target_os = "windows"))]
fn default_allowed_commands() -> Vec<String> {
    #[allow(unused_mut)]
    let mut cmds = vec![
        "git".into(),
        "npm".into(),
        "cargo".into(),
        "ls".into(),
        "cat".into(),
        "grep".into(),
        "find".into(),
        "echo".into(),
        "pwd".into(),
        "wc".into(),
        "head".into(),
        "tail".into(),
        "date".into(),
        "df".into(),
        "du".into(),
        "uname".into(),
        "uptime".into(),
        "hostname".into(),
        "python".into(),
        "python3".into(),
        "pip".into(),
        "node".into(),
    ];

    #[cfg(target_os = "linux")]
    cmds.push("free".into());
    cmds
}

#[cfg(target_os = "windows")]
fn default_allowed_commands() -> Vec<String> {
    vec![

        "git".into(),
        "npm".into(),
        "cargo".into(),
        "echo".into(),

        "dir".into(),
        "type".into(),
        "findstr".into(),
        "where".into(),
        "more".into(),
        "date".into(),

        "ls".into(),
        "cat".into(),
        "grep".into(),
        "find".into(),
        "pwd".into(),
        "wc".into(),
        "head".into(),
        "tail".into(),
        "df".into(),
        "du".into(),
        "uname".into(),
        "uptime".into(),
        "hostname".into(),
        "python".into(),
        "python3".into(),
        "pip".into(),
        "node".into(),
    ]
}

#[cfg(not(target_os = "windows"))]
fn default_forbidden_paths() -> Vec<String> {
    vec![
        "/etc".into(),
        "/root".into(),
        "/home".into(),
        "/usr".into(),
        "/bin".into(),
        "/sbin".into(),
        "/lib".into(),
        "/opt".into(),
        "/boot".into(),
        "/dev".into(),
        "/proc".into(),
        "/sys".into(),
        "/var".into(),
        "/tmp".into(),
        "~/.ssh".into(),
        "~/.gnupg".into(),
        "~/.aws".into(),
        "~/.config".into(),
    ]
}

#[cfg(target_os = "windows")]
fn default_forbidden_paths() -> Vec<String> {
    vec![
        "C:\\Windows".into(),
        "C:\\Windows\\System32".into(),
        "C:\\Program Files".into(),
        "C:\\Program Files (x86)".into(),
        "C:\\ProgramData".into(),
        "~/.ssh".into(),
        "~/.gnupg".into(),
        "~/.aws".into(),
        "~/.config".into(),
    ]
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            autonomy: AutonomyLevel::Supervised,
            workspace_root: Arc::new(RwLock::new(PathBuf::from("."))),
            workspace_only: true,
            allowed_commands: default_allowed_commands(),
            forbidden_paths: default_forbidden_paths(),
            allowed_roots: Vec::new(),
            max_actions_per_hour: 0,
            max_cost_per_day_cents: 500,
            require_approval_for_medium_risk: true,
            block_high_risk_commands: true,
            shell_env_passthrough: vec![],
            enable_command_policy: Arc::new(AtomicBool::new(true)),
            tracker: ActionTracker::new(),
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
    }
}

#[must_use]
pub fn is_system_path(path: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        let lower = path.to_string_lossy().to_ascii_lowercase().replace('/', "\\");
        for var in ["windir", "SystemRoot"] {
            if let Ok(root) = std::env::var(var) {
                let root = root.to_ascii_lowercase().replace('/', "\\");
                if !root.is_empty() && lower.starts_with(&root) {
                    return true;
                }
            }
        }
        lower.starts_with("c:\\windows")
            || lower.contains("\\system32")
            || lower.contains("\\syswow64")
            || lower.starts_with("c:\\program files")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let s = path.to_string_lossy();
        s == "/"
            || s.starts_with("/bin")
            || s.starts_with("/sbin")
            || s.starts_with("/usr")
            || s.starts_with("/etc")
            || s.starts_with("/boot")
            || s.starts_with("/proc")
            || s.starts_with("/sys")
            || s.starts_with("/System/")
            || s.starts_with("/Library/")
    }
}

fn lexically_normalise(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut out: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                match out.last() {
                    Some(Component::Normal(_)) => {
                        out.pop();
                    }

                    Some(Component::RootDir) | Some(Component::Prefix(_)) => {}
                    _ => out.push(component),
                }
            }
            other => out.push(other),
        }
    }
    if out.is_empty() {
        PathBuf::from(".")
    } else {
        out.iter().collect()
    }
}

fn expand_user_path(path: &str) -> PathBuf {
    if path == "~" {
        if let Some(home) = home_dir() {
            return home;
        }
    }

    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(stripped);
        }
    }

    PathBuf::from(path)
}

fn rootless_path(path: &Path) -> Option<PathBuf> {
    let mut relative = PathBuf::new();

    for component in path.components() {
        match component {
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::CurDir => {}
            std::path::Component::ParentDir => return None,
            std::path::Component::Normal(part) => relative.push(part),
        }
    }

    if relative.as_os_str().is_empty() {
        None
    } else {
        Some(relative)
    }
}

fn skip_env_assignments(s: &str) -> &str {
    let mut rest = s;
    loop {
        let Some(word) = rest.split_whitespace().next() else {
            return rest;
        };

        if word.contains('=')
            && word
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        {

            rest = rest[word.len()..].trim_start();
        } else {
            return rest;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuoteState {
    None,
    Single,
    Double,
}

fn split_unquoted_segments(command: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut quote = QuoteState::None;
    let mut escaped = false;
    let mut chars = command.chars().peekable();

    let push_segment = |segments: &mut Vec<String>, current: &mut String| {
        let trimmed = current.trim();
        if !trimmed.is_empty() {
            segments.push(trimmed.to_string());
        }
        current.clear();
    };

    while let Some(ch) = chars.next() {
        match quote {
            QuoteState::Single => {
                if ch == '\'' {
                    quote = QuoteState::None;
                }
                current.push(ch);
            }
            QuoteState::Double => {
                if escaped {
                    escaped = false;
                    current.push(ch);
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                    current.push(ch);
                    continue;
                }
                if ch == '"' {
                    quote = QuoteState::None;
                }
                current.push(ch);
            }
            QuoteState::None => {
                if escaped {
                    escaped = false;
                    current.push(ch);
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                    current.push(ch);
                    continue;
                }

                match ch {
                    '\'' => {
                        quote = QuoteState::Single;
                        current.push(ch);
                    }
                    '"' => {
                        quote = QuoteState::Double;
                        current.push(ch);
                    }
                    ';' | '\n' => push_segment(&mut segments, &mut current),
                    '|' => {
                        chars.next_if_eq(&'|');
                        push_segment(&mut segments, &mut current);
                    }
                    '&' => {
                        if chars.next_if_eq(&'&').is_some() {

                            push_segment(&mut segments, &mut current);
                        } else {
                            current.push(ch);
                        }
                    }
                    _ => current.push(ch),
                }
            }
        }
    }

    let trimmed = current.trim();
    if !trimmed.is_empty() {
        segments.push(trimmed.to_string());
    }

    segments
}

fn contains_unquoted_single_ampersand(command: &str) -> bool {
    let mut quote = QuoteState::None;
    let mut escaped = false;
    let mut chars = command.chars().peekable();

    while let Some(ch) = chars.next() {
        match quote {
            QuoteState::Single => {
                if ch == '\'' {
                    quote = QuoteState::None;
                }
            }
            QuoteState::Double => {
                if escaped {
                    escaped = false;
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                    continue;
                }
                if ch == '"' {
                    quote = QuoteState::None;
                }
            }
            QuoteState::None => {
                if escaped {
                    escaped = false;
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                    continue;
                }
                match ch {
                    '\'' => quote = QuoteState::Single,
                    '"' => quote = QuoteState::Double,
                    '&' => {
                        if chars.next_if_eq(&'&').is_none() {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    false
}

fn contains_unquoted_shell_variable_expansion(command: &str) -> bool {
    let mut quote = QuoteState::None;
    let mut escaped = false;
    let chars: Vec<char> = command.chars().collect();

    for i in 0..chars.len() {
        let ch = chars[i];

        match quote {
            QuoteState::Single => {
                if ch == '\'' {
                    quote = QuoteState::None;
                }
                continue;
            }
            QuoteState::Double => {
                if escaped {
                    escaped = false;
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                    continue;
                }
                if ch == '"' {
                    quote = QuoteState::None;
                    continue;
                }
            }
            QuoteState::None => {
                if escaped {
                    escaped = false;
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                    continue;
                }
                if ch == '\'' {
                    quote = QuoteState::Single;
                    continue;
                }
                if ch == '"' {
                    quote = QuoteState::Double;
                    continue;
                }
            }
        }

        if ch != '$' {
            continue;
        }

        let Some(next) = chars.get(i + 1).copied() else {
            continue;
        };
        if next.is_ascii_alphanumeric()
            || matches!(
                next,
                '_' | '{' | '(' | '#' | '?' | '!' | '$' | '*' | '@' | '-'
            )
        {
            return true;
        }
    }

    false
}

fn strip_wrapping_quotes(token: &str) -> &str {
    token.trim_matches(|c| c == '"' || c == '\'')
}

fn looks_like_path(candidate: &str) -> bool {
    candidate.starts_with('/')
        || candidate.starts_with("./")
        || candidate.starts_with("../")
        || candidate.starts_with('~')
        || candidate == "."
        || candidate == ".."
        || candidate.contains('/')

        || (cfg!(target_os = "windows")
            && (candidate
                .get(1..3)
                .is_some_and(|s| s == ":\\" || s == ":/")
                || candidate.starts_with("\\\\")))
}

fn attached_short_option_value(token: &str) -> Option<&str> {

    let body = token.strip_prefix('-')?;
    if body.starts_with('-') || body.len() < 2 {
        return None;
    }
    let value = body[1..].trim_start_matches('=').trim();
    if value.is_empty() { None } else { Some(value) }
}

fn is_safe_shell_device_path(candidate: &str) -> bool {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return false;
    }
    matches!(
        trimmed,
        "/dev/null" | "/dev/stdout" | "/dev/stderr" | "NUL" | "nul"
    )
}

fn redirection_target(token: &str) -> Option<&str> {
    let marker_idx = token.find(['<', '>'])?;
    let mut rest = &token[marker_idx + 1..];
    rest = rest.trim_start_matches(['<', '>']);
    rest = rest.trim_start_matches('&');
    rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn command_basename(raw: &str) -> &str {
    let after_fwd = raw.rsplit('/').next().unwrap_or(raw);
    after_fwd.rsplit('\\').next().unwrap_or(after_fwd)
}

fn is_interpreter_base(base: &str) -> bool {
    matches!(base, "python" | "python3" | "python2" | "py" | "node" | "nodejs" | "deno" | "bun" | "ruby" | "perl" | "php")
        || base.starts_with("python3.")
}

fn strip_windows_exe_suffix(name: &str) -> &str {
    if cfg!(target_os = "windows") {
        name.strip_suffix(".exe")
            .or_else(|| name.strip_suffix(".cmd"))
            .or_else(|| name.strip_suffix(".bat"))
            .unwrap_or(name)
    } else {
        name
    }
}

fn is_allowlist_entry_match(allowed: &str, executable: &str, executable_base: &str) -> bool {
    let allowed = strip_wrapping_quotes(allowed).trim();
    if allowed.is_empty() {
        return false;
    }

    if allowed == "*" {
        return true;
    }

    if looks_like_path(allowed) {
        let allowed_path = expand_user_path(allowed);
        let executable_path = expand_user_path(executable);
        return executable_path == allowed_path;
    }

    if allowed == executable_base {
        return true;
    }

    #[cfg(target_os = "windows")]
    {
        let base_lower = executable_base.to_ascii_lowercase();
        let allowed_lower = allowed.to_ascii_lowercase();
        for ext in &[".exe", ".cmd", ".bat"] {
            if base_lower == format!("{allowed_lower}{ext}") {
                return true;
            }
            if allowed_lower == format!("{base_lower}{ext}") {
                return true;
            }
        }
    }

    false
}

impl SecurityPolicy {

    #[must_use]
    pub fn workspace_dir(&self) -> PathBuf {
        self.workspace_root.read().clone()
    }

    #[must_use]
    pub fn safe_artifact_anchor(&self) -> PathBuf {
        let ws = self.workspace_dir();
        if ws.is_absolute() && !is_system_path(&ws) {
            return ws;
        }
        if let Some(home) = home_dir() {
            return home.join(".senweavercoding").join("runtime");
        }
        let mut tmp = std::env::temp_dir();
        tmp.push("SenAgentOS");
        tmp.push("runtime");
        tmp
    }

    #[must_use]
    pub fn workspace_root_handle(&self) -> Arc<RwLock<PathBuf>> {
        Arc::clone(&self.workspace_root)
    }

    pub fn retarget_session_workspace_root(&self, raw: impl AsRef<Path>) {
        let raw_pb = raw.as_ref().to_path_buf();
        if raw_pb.as_os_str().is_empty() {
            return;
        }
        let canon = std::fs::canonicalize(&raw_pb).unwrap_or(raw_pb);
        *self.workspace_root.write() = canon;
    }

    pub fn command_risk_level(&self, command: &str) -> CommandRiskLevel {
        let mut saw_medium = false;

        for segment in split_unquoted_segments(command) {
            let cmd_part = skip_env_assignments(&segment);
            let mut words = cmd_part.split_whitespace();
            let Some(base_raw) = words.next() else {
                continue;
            };

            let base_owned = command_basename(base_raw).to_ascii_lowercase();
            let base = strip_windows_exe_suffix(&base_owned);

            let args: Vec<String> = words.map(|w| w.to_ascii_lowercase()).collect();
            let joined_segment = cmd_part.to_ascii_lowercase();

            if matches!(
                base,
                "rm" | "mkfs"
                    | "dd"
                    | "shutdown"
                    | "reboot"
                    | "halt"
                    | "poweroff"
                    | "sudo"
                    | "su"
                    | "chown"
                    | "chmod"
                    | "useradd"
                    | "userdel"
                    | "usermod"
                    | "passwd"
                    | "mount"
                    | "umount"
                    | "iptables"
                    | "ufw"
                    | "firewall-cmd"
                    | "curl"
                    | "wget"
                    | "nc"
                    | "ncat"
                    | "netcat"
                    | "scp"
                    | "ssh"
                    | "ftp"
                    | "telnet"

                    | "del"
                    | "rmdir"
                    | "format"
                    | "reg"
                    | "net"
                    | "runas"
                    | "icacls"
                    | "takeown"
                    | "powershell"
                    | "pwsh"
                    | "wmic"
                    | "sc"
                    | "netsh"
            ) {
                return CommandRiskLevel::High;
            }

            if joined_segment.contains("rm -rf /")
                || joined_segment.contains("rm -fr /")
                || joined_segment.contains(":(){:|:&};:")

                || joined_segment.contains("del /s /q")
                || joined_segment.contains("rmdir /s /q")
                || joined_segment.contains("format c:")
            {
                return CommandRiskLevel::High;
            }

            if is_interpreter_base(base)
                && args.iter().any(|arg| {
                    matches!(
                        arg.as_str(),
                        "-c" | "-e" | "--eval" | "-eval" | "--command" | "-command"
                            | "-encodedcommand" | "-enc" | "-"
                    )
                })
            {
                return CommandRiskLevel::High;
            }

            let medium = match base {
                "git" => args.first().is_some_and(|verb| {
                    matches!(
                        verb.as_str(),
                        "commit"
                            | "push"
                            | "reset"
                            | "clean"
                            | "rebase"
                            | "merge"
                            | "cherry-pick"
                            | "revert"
                            | "branch"
                            | "checkout"
                            | "switch"
                            | "tag"
                    )
                }),
                "npm" | "pnpm" | "yarn" => args.first().is_some_and(|verb| {
                    matches!(
                        verb.as_str(),
                        "install" | "add" | "remove" | "uninstall" | "update" | "publish"
                    )
                }),
                "cargo" => args.first().is_some_and(|verb| {
                    matches!(
                        verb.as_str(),
                        "add" | "remove" | "install" | "clean" | "publish"
                    )
                }),
                "touch" | "mkdir" | "mv" | "cp" | "ln"

                | "copy" | "xcopy" | "robocopy" | "move" | "ren" | "rename" | "mklink" => true,
                _ => false,
            };

            saw_medium |= medium;
        }

        if saw_medium {
            CommandRiskLevel::Medium
        } else {
            CommandRiskLevel::Low
        }
    }

    pub fn is_catastrophic_command(command: &str) -> bool {
        Self::is_catastrophic_command_depth(command, 0)
    }

    fn is_catastrophic_command_depth(command: &str, depth: u8) -> bool {
        const MAX_UNWRAP_DEPTH: u8 = 4;
        if depth > MAX_UNWRAP_DEPTH {
            return false;
        }
        let compact: String = command
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        if compact.contains(":(){:|:&};:") {
            return true;
        }

        for segment in split_unquoted_segments(command) {
            let cmd_part = skip_env_assignments(segment.trim());
            let tokens: Vec<String> = cmd_part
                .split_whitespace()
                .map(|w| strip_wrapping_quotes(w).to_ascii_lowercase())
                .collect();
            if Self::tokens_are_catastrophic(&tokens, depth) {
                return true;
            }
        }

        false
    }

    fn tokens_are_catastrophic(tokens: &[String], depth: u8) -> bool {
        const WRAPPERS: &[&str] = &[
            "sudo", "doas", "env", "nohup", "nice", "ionice", "stdbuf", "time", "timeout",
            "xargs", "command", "exec", "setsid", "unbuffer",
        ];
        const SHELLS: &[&str] = &[
            "sh", "bash", "zsh", "dash", "ksh", "fish", "pwsh", "powershell", "cmd",
        ];

        let mut idx = 0usize;
        loop {
            let Some(base_raw) = tokens.get(idx) else {
                return false;
            };
            let base_owned = command_basename(base_raw).to_ascii_lowercase();
            let base = strip_windows_exe_suffix(&base_owned).to_string();

            if SHELLS.contains(&base.as_str()) {
                let has_command_flag = tokens[idx + 1..]
                    .iter()
                    .any(|t| t == "-c" || t == "/c" || t == "-command");
                if has_command_flag {
                    let payload_start = tokens[idx + 1..]
                        .iter()
                        .position(|t| t == "-c" || t == "/c" || t == "-command")
                        .map(|p| idx + 1 + p + 1)
                        .unwrap_or(tokens.len());
                    let payload = tokens[payload_start.min(tokens.len())..].join(" ");
                    let payload = payload.trim_matches(|c| c == '"' || c == '\'');
                    return !payload.is_empty()
                        && Self::is_catastrophic_command_depth(payload, depth + 1);
                }
                return false;
            }

            if WRAPPERS.contains(&base.as_str()) {
                const VALUE_TAKING_FLAGS: &[&str] = &[
                    "-u", "-g", "-h", "-p", "-r", "-t", "-a", "-c", "-n", "--user", "--group",
                    "--host", "--prompt", "--role", "--type", "--chdir", "--login-class",
                    "-i", "-o", "-e", "-d", "--delimiter", "--max-args", "--max-procs",
                ];
                let takes_value = matches!(
                    base.as_str(),
                    "sudo" | "doas" | "xargs" | "nice" | "ionice" | "stdbuf"
                );
                idx += 1;
                while let Some(tok) = tokens.get(idx) {
                    let is_flag = tok.starts_with('-');
                    let is_env_assign = base == "env" && tok.contains('=');
                    let is_timeout_duration = base == "timeout"
                        && tok
                            .trim_end_matches(['s', 'm', 'h', 'd'])
                            .parse::<f64>()
                            .is_ok();
                    if is_flag {
                        let consumes_next = takes_value
                            && !tok.contains('=')
                            && VALUE_TAKING_FLAGS.contains(&tok.as_str());
                        idx += 1;
                        if consumes_next && tokens.get(idx).is_some() {
                            idx += 1;
                        }
                    } else if is_env_assign || is_timeout_duration {
                        idx += 1;
                    } else {
                        break;
                    }
                }
                continue;
            }

            let rest = &tokens[(idx + 1).min(tokens.len())..];
            return Self::base_command_is_catastrophic(&base, rest);
        }
    }

    fn base_command_is_catastrophic(base: &str, rest: &[String]) -> bool {
        if base == "mkfs" || base.starts_with("mkfs.") {
            return true;
        }

        if base == "dd"
            && rest.iter().any(|a| {
                a.starts_with("of=/dev/")
                    || a.starts_with("of=\\\\.\\physicaldrive")
                    || a.starts_with("of=\\\\.\\")
            })
        {
            return true;
        }

        if base == "format"
            && rest
                .iter()
                .any(|a| a.len() == 2 && a.ends_with(':') && a.as_bytes()[0].is_ascii_alphabetic())
        {
            return true;
        }

        if base == "rm" {
            let recursive_force = rest.iter().any(|a| {
                a.starts_with('-')
                    && !a.starts_with("--")
                    && a.contains('r')
                    && a.contains('f')
            }) || (rest
                .iter()
                .any(|a| a == "-r" || a == "-rf" || a == "--recursive")
                && rest.iter().any(|a| a == "-f" || a == "--force"));
            if recursive_force {
                if rest.iter().any(|a| a == "--no-preserve-root") {
                    return true;
                }
                for t in rest.iter().filter(|a| !a.starts_with('-')) {
                    if matches!(t.as_str(), "/" | "/*" | "~" | "~/" | "$home" | "$home/") {
                        return true;
                    }
                }
            }
        }

        false
    }

    pub fn validate_command_execution(
        &self,
        command: &str,
        approved: bool,
    ) -> Result<CommandRiskLevel, String> {
        if Self::is_catastrophic_command(command) {
            tracing::error!(
                "Security: refused irreversible/destructive command regardless of policy state"
            );
            return Err(
                "Command blocked: irreversible/destructive operation is never permitted".into(),
            );
        }
        if !self.is_command_policy_enabled() {
            if self.autonomy == AutonomyLevel::ReadOnly {
                return Err("Command blocked: autonomy level is ReadOnly".into());
            }
            return Ok(CommandRiskLevel::Low);
        }
        if !self.is_command_allowed(command) {
            return Err(format!("Command not allowed by security policy: {command}"));
        }

        let risk = self.command_risk_level(command);

        let has_wildcard = self.allowed_commands.iter().any(|c| c.trim() == "*");
        if has_wildcard && !self.block_high_risk_commands {
            tracing::warn!(
                "Security: allowed_commands=['*'] with block_high_risk_commands=false \
                 disables all command restrictions. This should only be used in \
                 trusted development environments."
            );
            return Ok(risk);
        }

        if risk == CommandRiskLevel::High {
            if self.block_high_risk_commands && !self.is_command_explicitly_allowed(command) {
                return Err("Command blocked: high-risk command is disallowed by policy".into());
            }
            if self.autonomy == AutonomyLevel::Supervised && !approved {
                return Err(
                    "Command requires explicit approval (approved=true): high-risk operation"
                        .into(),
                );
            }
        }

        if risk == CommandRiskLevel::Medium
            && self.autonomy == AutonomyLevel::Supervised
            && self.require_approval_for_medium_risk
            && !approved
        {
            return Err(
                "Command requires explicit approval (approved=true): medium-risk operation".into(),
            );
        }

        Ok(risk)
    }

    fn is_command_explicitly_allowed(&self, command: &str) -> bool {
        let segments = split_unquoted_segments(command);
        for segment in &segments {
            let cmd_part = skip_env_assignments(segment);
            let mut words = cmd_part.split_whitespace();
            let executable = strip_wrapping_quotes(words.next().unwrap_or("")).trim();
            let base_cmd_owned = command_basename(executable).to_ascii_lowercase();
            let base_cmd = strip_windows_exe_suffix(&base_cmd_owned);

            if base_cmd.is_empty() {
                continue;
            }

            let explicitly_listed = self.allowed_commands.iter().any(|allowed| {
                let allowed = strip_wrapping_quotes(allowed).trim();

                if allowed.is_empty() || allowed == "*" {
                    return false;
                }
                is_allowlist_entry_match(allowed, executable, base_cmd)
            });

            if !explicitly_listed {
                return false;
            }
        }

        segments.iter().any(|s| {
            let s = skip_env_assignments(s.trim());
            s.split_whitespace().next().is_some_and(|w| !w.is_empty())
        })
    }

    pub fn is_command_allowed(&self, command: &str) -> bool {
        if self.autonomy == AutonomyLevel::ReadOnly {
            return false;
        }

        if !self.is_command_policy_enabled() {
            return true;
        }

        if command.contains('`')
            || contains_unquoted_shell_variable_expansion(command)
            || command.contains("<(")
            || command.contains(">(")
        {
            return false;
        }

        if command
            .split_whitespace()
            .any(|w| w == "tee" || w.ends_with("/tee"))
        {
            return false;
        }

        if contains_unquoted_single_ampersand(command) {
            return false;
        }

        let segments = split_unquoted_segments(command);
        for segment in &segments {

            let cmd_part = skip_env_assignments(segment);

            let mut words = cmd_part.split_whitespace();
            let executable = strip_wrapping_quotes(words.next().unwrap_or("")).trim();
            let base_cmd_owned = command_basename(executable).to_ascii_lowercase();
            let base_cmd = strip_windows_exe_suffix(&base_cmd_owned);

            if base_cmd.is_empty() {
                continue;
            }

            if !self
                .allowed_commands
                .iter()
                .any(|allowed| is_allowlist_entry_match(allowed, executable, base_cmd))
            {
                return false;
            }

            let args: Vec<String> = words.map(|w| w.to_ascii_lowercase()).collect();
            if !self.is_args_safe(base_cmd, &args) {
                return false;
            }
        }

        let has_cmd = segments.iter().any(|s| {
            let s = skip_env_assignments(s.trim());
            s.split_whitespace().next().is_some_and(|w| !w.is_empty())
        });

        has_cmd
    }

    fn is_args_safe(&self, base: &str, args: &[String]) -> bool {
        let base = base.to_ascii_lowercase();
        match base.as_str() {
            "find" => {

                !args.iter().any(|arg| arg == "-exec" || arg == "-ok")
            }
            "git" => {

                !args.iter().any(|arg| {
                    arg == "config"
                        || arg.starts_with("config.")
                        || arg == "alias"
                        || arg.starts_with("alias.")
                        || arg == "-c"
                })
            }
            _ => true,
        }
    }

    pub fn forbidden_path_argument(&self, command: &str) -> Option<String> {
        if !self.is_command_policy_enabled() {
            return None;
        }
        let forbidden_candidate = |raw: &str| {
            let candidate = strip_wrapping_quotes(raw).trim();
            if candidate.is_empty() || candidate.contains("://") {
                return None;
            }
            if looks_like_path(candidate) && !self.is_path_allowed(candidate) {
                Some(candidate.to_string())
            } else {
                None
            }
        };
        let redirect_target_candidate = |raw: &str| -> Option<String> {
            let candidate = strip_wrapping_quotes(raw).trim();
            if candidate.is_empty() {
                return None;
            }
            if is_safe_shell_device_path(candidate) {
                return None;
            }
            forbidden_candidate(candidate)
        };

        for segment in split_unquoted_segments(command) {
            let cmd_part = skip_env_assignments(&segment);
            let mut words = cmd_part.split_whitespace();
            let Some(executable) = words.next() else {
                continue;
            };

            if let Some(target) = redirection_target(strip_wrapping_quotes(executable)) {
                if let Some(blocked) = redirect_target_candidate(target) {
                    return Some(blocked);
                }
            }

            for token in words {
                let candidate = strip_wrapping_quotes(token).trim();
                if candidate.is_empty() || candidate.contains("://") {
                    continue;
                }

                if let Some(target) = redirection_target(candidate) {
                    if let Some(blocked) = redirect_target_candidate(target) {
                        return Some(blocked);
                    }
                    continue;
                }

                if candidate.starts_with('-') {
                    if let Some((_, value)) = candidate.split_once('=') {
                        if let Some(blocked) = forbidden_candidate(value) {
                            return Some(blocked);
                        }
                    }
                    if let Some(value) = attached_short_option_value(candidate) {
                        if let Some(blocked) = forbidden_candidate(value) {
                            return Some(blocked);
                        }
                    }
                    continue;
                }

                if let Some(blocked) = forbidden_candidate(candidate) {
                    return Some(blocked);
                }
            }
        }

        None
    }

    pub fn is_path_allowed(&self, path: &str) -> bool {
        if path.contains('\0') {
            return false;
        }

        let lower = path.to_lowercase();
        if lower.contains("..%2f") || lower.contains("%2f..") {
            return false;
        }

        if !self.is_command_policy_enabled() {
            return true;
        }

        if path.starts_with('~') && path != "~" && !path.starts_with("~/") {
            return false;
        }

        let expanded_path = expand_user_path(path);

        let workspace = self.workspace_dir();
        let absolute = if expanded_path.is_absolute() {
            expanded_path.clone()
        } else {
            workspace.join(&expanded_path)
        };
        let normalised = lexically_normalise(&absolute);

        for forbidden in &self.forbidden_paths {
            let forbidden_path = expand_user_path(forbidden);
            if crate::util::path_is_within(&normalised, &forbidden_path) {
                return false;
            }
        }

        let in_workspace = crate::util::path_is_within(&normalised, &workspace);
        let in_allowed_root = self
            .allowed_roots
            .iter()
            .any(|root| crate::util::path_is_within(&normalised, root));

        if in_workspace || in_allowed_root {
            return true;
        }

        if self.workspace_only {
            return false;
        }

        true
    }

    pub fn is_resolved_path_allowed(&self, resolved: &Path) -> bool {
        if !self.is_command_policy_enabled() {
            return true;
        }

        for forbidden in &self.forbidden_paths {
            let forbidden_path = expand_user_path(forbidden);
            if crate::util::path_is_within(resolved, &forbidden_path) {
                return false;
            }
        }

        let ws = self.workspace_dir();
        let workspace_root = ws.canonicalize().unwrap_or(ws);
        if crate::util::path_is_within(resolved, &workspace_root) {
            return true;
        }

        for root in &self.allowed_roots {
            let canonical = root.canonicalize().unwrap_or_else(|_| root.clone());
            if crate::util::path_is_within(resolved, &canonical) {
                return true;
            }
        }

        if !self.workspace_only {
            return true;
        }

        false
    }

    fn runtime_config_dir(&self) -> Option<PathBuf> {
        let ws = self.workspace_dir();
        let parent = ws.parent()?.to_path_buf();
        Some(parent.canonicalize().unwrap_or(parent))
    }

    pub fn is_runtime_config_path(&self, resolved: &Path) -> bool {
        if !self.is_command_policy_enabled() {
            return false;
        }
        let Some(config_dir) = self.runtime_config_dir() else {
            return false;
        };
        if !resolved.starts_with(&config_dir) {
            return false;
        }
        if resolved.parent() != Some(config_dir.as_path()) {
            return false;
        }

        let Some(file_name) = resolved.file_name().and_then(|value| value.to_str()) else {
            return false;
        };

        file_name == "config.toml"
            || file_name == "config.toml.bak"
            || file_name == "active_workspace.toml"
            || file_name.starts_with(".config.toml.tmp-")
            || file_name.starts_with(".active_workspace.toml.tmp-")
    }

    pub fn runtime_config_violation_message(&self, resolved: &Path) -> String {
        format!(
            "Refusing to modify SenWeaverCoding runtime config/state file: {}. Use dedicated config tools or edit it manually outside the agent loop.",
            resolved.display()
        )
    }

    pub fn resolved_path_violation_message(&self, resolved: &Path) -> String {
        let guidance = if self.allowed_roots.is_empty() {
            "Add the directory to [autonomy].allowed_roots (for example: allowed_roots = [\"/absolute/path\"]), or move the file into the workspace."
        } else {
            "Add a matching parent directory to [autonomy].allowed_roots, or move the file into the workspace."
        };

        format!(
            "Resolved path escapes workspace allowlist: {}. {}",
            resolved.display(),
            guidance
        )
    }

    pub fn can_act(&self) -> bool {

        if crate::util::get_runtime_var("SEN_READ_ONLY").as_deref() == Some("1") {
            return false;
        }

        if crate::util::get_runtime_var("SEN_DRY_RUN").as_deref() == Some("1") {
            return true;
        }
        self.autonomy != AutonomyLevel::ReadOnly
    }

    pub fn is_dry_run(&self) -> bool {
        crate::util::get_runtime_var("SEN_DRY_RUN").as_deref() == Some("1")
    }

    pub fn enforce_tool_operation(
        &self,
        operation: ToolOperation,
        operation_name: &str,
    ) -> Result<(), String> {
        match operation {
            ToolOperation::Read => Ok(()),
            ToolOperation::Act => {
                if !self.can_act() {
                    return Err(format!(
                        "Security policy: read-only mode, cannot perform '{operation_name}'"
                    ));
                }

                if !self.record_action() {
                    return Err("Rate limit exceeded: action budget exhausted".to_string());
                }

                Ok(())
            }
        }
    }

    pub fn record_action(&self) -> bool {
        if !self.is_command_policy_enabled() {
            return true;
        }
        if self.max_actions_per_hour == 0 {
            return true;
        }
        let count = self.tracker.record();
        count <= self.max_actions_per_hour as usize
    }

    pub fn is_rate_limited(&self) -> bool {
        if !self.is_command_policy_enabled() {
            return false;
        }
        if self.max_actions_per_hour == 0 {
            return false;
        }
        self.tracker.count() >= self.max_actions_per_hour as usize
    }

    pub fn should_filter_shell_env(&self) -> bool {
        self.is_command_policy_enabled()
    }

    pub fn resolve_tool_path(&self, path: &str) -> PathBuf {
        let expanded = expand_user_path(path);
        let base = self.workspace_dir();
        if expanded.is_absolute() {
            expanded
        } else if let Some(workspace_hint) = rootless_path(&base) {
            if let Ok(stripped) = expanded.strip_prefix(&workspace_hint) {
                if stripped.as_os_str().is_empty() {
                    base
                } else {
                    base.join(stripped)
                }
            } else {
                base.join(expanded)
            }
        } else {
            base.join(expanded)
        }
    }

    pub fn is_under_allowed_root(&self, path: &str) -> bool {
        let expanded = expand_user_path(path);
        if !expanded.is_absolute() {
            return false;
        }
        self.allowed_roots.iter().any(|root| {
            let canonical = root.canonicalize().unwrap_or_else(|_| root.clone());
            crate::util::path_is_within(&expanded, &canonical)
                || crate::util::path_is_within(&expanded, root)
        })
    }

    pub fn from_config(
        autonomy_config: &crate::config::AutonomyConfig,
        workspace_dir: &Path,
    ) -> Self {
        let initial = workspace_dir.to_path_buf();
        let canon_root = std::fs::canonicalize(workspace_dir).unwrap_or(initial.clone());
        Self {
            autonomy: autonomy_config.level,
            workspace_root: Arc::new(RwLock::new(canon_root)),
            workspace_only: autonomy_config.workspace_only,
            allowed_commands: autonomy_config.allowed_commands.clone(),
            forbidden_paths: autonomy_config.forbidden_paths.clone(),
            allowed_roots: autonomy_config
                .allowed_roots
                .iter()
                .map(|root| {
                    let expanded = expand_user_path(root);
                    if expanded.is_absolute() {
                        expanded
                    } else {
                        workspace_dir.join(expanded)
                    }
                })
                .collect(),
            max_actions_per_hour: autonomy_config.max_actions_per_hour,
            max_cost_per_day_cents: autonomy_config.max_cost_per_day_cents,
            require_approval_for_medium_risk: autonomy_config.require_approval_for_medium_risk,
            block_high_risk_commands: autonomy_config.block_high_risk_commands,
            shell_env_passthrough: autonomy_config.shell_env_passthrough.clone(),
            enable_command_policy: Arc::new(AtomicBool::new(autonomy_config.enable_command_policy)),
            tracker: ActionTracker::new(),
        }
    }

    pub fn prompt_summary(&self) -> String {
        use std::fmt::Write;

        let mut out = String::new();

        let _ = writeln!(out, "**Autonomy level**: {:?}", self.autonomy);

        if !self.is_command_policy_enabled() {
            let _ = writeln!(
                out,
                "**Command policy**: disabled. Per-tool execution approval is the only gate; \
                 there is no command allowlist, risk classification, output-redirection ban, \
                 forbidden-path list, or workspace boundary on shell commands or file paths."
            );
            return out;
        }

        if self.workspace_only {
            let _ = writeln!(
                out,
                "**Workspace boundary**: file operations are restricted to `{}`.",
                self.workspace_dir().display()
            );
        }

        if !self.allowed_roots.is_empty() {
            let roots: Vec<String> = self
                .allowed_roots
                .iter()
                .map(|p| format!("`{}`", p.display()))
                .collect();
            let _ = writeln!(out, "**Additional allowed paths**: {}", roots.join(", "));
        }

        if !self.allowed_commands.is_empty() {
            let cmds: Vec<String> = self
                .allowed_commands
                .iter()
                .map(|c| format!("`{c}`"))
                .collect();
            let _ = writeln!(
                out,
                "**Allowed shell commands**: {}. \
                 You may execute these commands freely.",
                cmds.join(", ")
            );
        }

        if !self.forbidden_paths.is_empty() {
            let paths: Vec<String> = self
                .forbidden_paths
                .iter()
                .map(|p| format!("`{p}`"))
                .collect();
            let _ = writeln!(
                out,
                "**Forbidden paths**: {}. \
                 Avoid accessing these paths.",
                paths.join(", ")
            );
        }

        if self.block_high_risk_commands {
            let _ = writeln!(
                out,
                "Exercise caution with destructive commands (rm, kill, reboot, etc.)."
            );
        }
        if self.require_approval_for_medium_risk {
            let _ = writeln!(
                out,
                "**Medium-risk commands** require user approval before execution."
            );
        }

        if self.max_actions_per_hour > 0 {
            let _ = writeln!(
                out,
                "**Rate limit**: max {} actions per hour.",
                self.max_actions_per_hour
            );
        }

        out
    }
}
