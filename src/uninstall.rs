use std::env;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use crate::config;
use crate::integration::{
    self, ClaudeUninstallResult, CodexUninstallResult, OpenCodeUninstallResult, PiUninstallResult,
};

const HELP_TEXT: &str = "\
Usage: herdr uninstall

Removes herdr from this machine:
  - the herdr binary itself
  - the config directory (~/.config/herdr or $XDG_CONFIG_HOME/herdr)
  - any installed agent integrations (pi, claude, codex, opencode)

herdr must not be running. Close all herdr sessions before invoking.
";

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Run,
    Help,
}

#[derive(Debug)]
struct UsageError(String);

#[derive(Debug, Clone)]
pub(crate) struct UninstallPaths {
    pub binary: PathBuf,
    pub config_dir: PathBuf,
    /// `Some` only when `HERDR_CONFIG_PATH` points outside `config_dir`.
    pub extra_config_file: Option<PathBuf>,
    pub socket: PathBuf,
}

#[derive(Debug)]
pub(crate) enum StepStatus {
    Removed,
    NotPresent,
    Failed(io::Error),
}

#[derive(Debug)]
pub(crate) struct UninstallReport {
    pub binary_path: PathBuf,
    pub binary_status: StepStatus,
    pub config_dir: PathBuf,
    pub config_dir_status: StepStatus,
    pub extra_config_file: Option<PathBuf>,
    pub extra_config_file_status: Option<StepStatus>,
    pub socket_path: PathBuf,
    pub socket_status: StepStatus,
    pub pi: Result<PiUninstallResult, io::Error>,
    pub claude: Result<ClaudeUninstallResult, io::Error>,
    pub codex: Result<CodexUninstallResult, io::Error>,
    pub opencode: Result<OpenCodeUninstallResult, io::Error>,
}

impl UninstallReport {
    pub(crate) fn has_errors(&self) -> bool {
        let step_failed = |status: &StepStatus| matches!(status, StepStatus::Failed(_));
        step_failed(&self.binary_status)
            || step_failed(&self.config_dir_status)
            || step_failed(&self.socket_status)
            || self
                .extra_config_file_status
                .as_ref()
                .map(step_failed)
                .unwrap_or(false)
            || self.pi.is_err()
            || self.claude.is_err()
            || self.codex.is_err()
            || self.opencode.is_err()
    }
}

#[derive(Debug)]
pub(crate) enum UninstallAbort {
    Running {
        socket: PathBuf,
    },
    LivenessProbeFailed {
        socket: PathBuf,
        error: io::Error,
    },
}

impl std::fmt::Display for UninstallAbort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running { socket } => write!(
                f,
                "herdr is currently running (socket: {})",
                socket.display()
            ),
            Self::LivenessProbeFailed { socket, error } => write!(
                f,
                "failed to probe herdr socket {}: {error}",
                socket.display()
            ),
        }
    }
}

pub(crate) fn execute(paths: &UninstallPaths) -> Result<UninstallReport, UninstallAbort> {
    use std::fs;

    match detect_running_herdr(&paths.socket) {
        Ok(true) => {
            return Err(UninstallAbort::Running {
                socket: paths.socket.clone(),
            });
        }
        Ok(false) => {}
        Err(error) => {
            return Err(UninstallAbort::LivenessProbeFailed {
                socket: paths.socket.clone(),
                error,
            });
        }
    }

    let pi = integration::uninstall_pi();
    let claude = integration::uninstall_claude();
    let codex = integration::uninstall_codex();
    let opencode = integration::uninstall_opencode();

    let config_dir_status = match fs::remove_dir_all(&paths.config_dir) {
        Ok(()) => StepStatus::Removed,
        Err(err) if err.kind() == io::ErrorKind::NotFound => StepStatus::NotPresent,
        Err(err) => StepStatus::Failed(err),
    };

    let extra_config_file_status = paths.extra_config_file.as_ref().map(|path| {
        match integration::remove_file_if_exists(path) {
            Ok(true) => StepStatus::Removed,
            Ok(false) => StepStatus::NotPresent,
            Err(err) => StepStatus::Failed(err),
        }
    });

    let socket_status = if paths.socket.starts_with(&paths.config_dir)
        && matches!(
            config_dir_status,
            StepStatus::Removed | StepStatus::NotPresent
        )
    {
        StepStatus::NotPresent
    } else {
        match integration::remove_file_if_exists(&paths.socket) {
            Ok(true) => StepStatus::Removed,
            Ok(false) => StepStatus::NotPresent,
            Err(err) => StepStatus::Failed(err),
        }
    };

    let binary_status = match integration::remove_file_if_exists(&paths.binary) {
        Ok(true) => StepStatus::Removed,
        Ok(false) => StepStatus::NotPresent,
        Err(err) => StepStatus::Failed(err),
    };

    Ok(UninstallReport {
        binary_path: paths.binary.clone(),
        binary_status,
        config_dir: paths.config_dir.clone(),
        config_dir_status,
        extra_config_file: paths.extra_config_file.clone(),
        extra_config_file_status,
        socket_path: paths.socket.clone(),
        socket_status,
        pi,
        claude,
        codex,
        opencode,
    })
}

fn compute_paths() -> io::Result<UninstallPaths> {
    let binary = env::current_exe()?;
    let config_dir = config::config_dir();
    let config_file = config::config_path();
    let socket = crate::api::socket_path();

    let extra_config_file = if config_file.starts_with(&config_dir) {
        None
    } else {
        Some(config_file)
    };

    Ok(UninstallPaths {
        binary,
        config_dir,
        extra_config_file,
        socket,
    })
}

fn print_report(report: &UninstallReport) {
    println!("Uninstalling herdr integrations:");
    print_pi(&report.pi);
    print_claude(&report.claude);
    print_codex(&report.codex);
    print_opencode(&report.opencode);
    println!();

    println!(
        "Removing config directory: {} ({})",
        report.config_dir.display(),
        format_step_status(&report.config_dir_status)
    );
    if let (Some(path), Some(status)) = (
        report.extra_config_file.as_ref(),
        report.extra_config_file_status.as_ref(),
    ) {
        println!(
            "Removing extra config file (HERDR_CONFIG_PATH): {} ({})",
            path.display(),
            format_step_status(status)
        );
    }
    if !matches!(report.socket_status, StepStatus::NotPresent) {
        println!(
            "Removing socket: {} ({})",
            report.socket_path.display(),
            format_step_status(&report.socket_status)
        );
    }
    println!(
        "Removing binary: {} ({})",
        report.binary_path.display(),
        format_step_status(&report.binary_status)
    );

    println!();
    if report.has_errors() {
        println!("herdr has been uninstalled with errors.");
        println!("Inspect the messages above and clean up manually if needed.");
    } else {
        println!("herdr has been uninstalled.");
    }
}

fn format_step_status(status: &StepStatus) -> String {
    match status {
        StepStatus::Removed => "removed".to_string(),
        StepStatus::NotPresent => "not present".to_string(),
        StepStatus::Failed(err) => format!("FAILED: {err}"),
    }
}

fn print_pi(result: &Result<PiUninstallResult, io::Error>) {
    match result {
        Ok(r) if r.removed_extension => println!("  pi:       removed {}", r.extension_path.display()),
        Ok(r) => println!("  pi:       not installed (skipped) [{}]", r.extension_path.display()),
        Err(err) => println!("  pi:       FAILED: {err}"),
    }
}

fn print_claude(result: &Result<ClaudeUninstallResult, io::Error>) {
    match result {
        Ok(r) if r.removed_hook_file || r.updated_settings => {
            if r.removed_hook_file {
                println!("  claude:   removed {}", r.hook_path.display());
            }
            if r.updated_settings {
                println!("            updated {}", r.settings_path.display());
            }
        }
        Ok(_) => println!("  claude:   not installed (skipped)"),
        Err(err) => println!("  claude:   FAILED: {err}"),
    }
}

fn print_codex(result: &Result<CodexUninstallResult, io::Error>) {
    match result {
        Ok(r) if r.removed_hook_file || r.updated_hooks => {
            if r.removed_hook_file {
                println!("  codex:    removed {}", r.hook_path.display());
            }
            if r.updated_hooks {
                println!("            updated {}", r.hooks_path.display());
            }
            println!(
                "            note: {} still contains 'codex_hooks = true' (left intact by design)",
                r.config_path.display()
            );
        }
        Ok(_) => println!("  codex:    not installed (skipped)"),
        Err(err) => println!("  codex:    FAILED: {err}"),
    }
}

fn print_opencode(result: &Result<OpenCodeUninstallResult, io::Error>) {
    match result {
        Ok(r) if r.removed_plugin => println!("  opencode: removed {}", r.plugin_path.display()),
        Ok(_) => println!("  opencode: not installed (skipped)"),
        Err(err) => println!("  opencode: FAILED: {err}"),
    }
}

pub(crate) fn run(args: &[String]) -> io::Result<()> {
    match parse_args(args) {
        Err(UsageError(msg)) => {
            eprintln!("error: {msg}");
            eprintln!("usage: herdr uninstall [--help]");
            std::process::exit(2);
        }
        Ok(Action::Help) => {
            println!("{HELP_TEXT}");
            return Ok(());
        }
        Ok(Action::Run) => {}
    }

    ensure_home_set()?;

    let paths = compute_paths()?;

    println!("Checking for running herdr...");
    match execute(&paths) {
        Err(err) => {
            eprintln!("error: {err}");
            match err {
                UninstallAbort::Running { .. } => {
                    eprintln!("Close all herdr sessions and run 'herdr uninstall' again.");
                }
                UninstallAbort::LivenessProbeFailed { .. } => {
                    eprintln!("Resolve the probe failure above and run 'herdr uninstall' again.");
                }
            }
            std::process::exit(1);
        }
        Ok(report) => {
            println!("  none detected.");
            println!();
            print_report(&report);
            if report.has_errors() {
                std::process::exit(1);
            }
            Ok(())
        }
    }
}

fn detect_running_herdr(socket: &Path) -> io::Result<bool> {
    match UnixStream::connect(socket) {
        Ok(_) => Ok(true),
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::NotFound
                    | io::ErrorKind::TimedOut
            ) =>
        {
            Ok(false)
        }
        Err(err) => Err(err),
    }
}

fn ensure_home_set() -> io::Result<()> {
    if env::var_os("HOME").is_none() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "HOME is not set; refusing to uninstall (cannot determine what to remove)",
        ));
    }
    Ok(())
}

fn parse_args(args: &[String]) -> Result<Action, UsageError> {
    match args {
        [] => Ok(Action::Run),
        [flag] if flag == "--help" || flag == "-h" => Ok(Action::Help),
        _ => Err(UsageError(format!(
            "unexpected arguments: {}",
            args.join(" ")
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    struct HomeGuard(Option<std::ffi::OsString>);
    impl HomeGuard {
        fn capture() -> Self {
            Self(env::var_os("HOME"))
        }
    }
    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }
        }
    }

    #[test]
    fn parse_args_empty_is_run() {
        let parsed = parse_args(&args(&[])).expect("empty args should parse");
        assert_eq!(parsed, Action::Run);
    }

    #[test]
    fn parse_args_long_help_flag_is_help() {
        let parsed = parse_args(&args(&["--help"])).expect("--help should parse");
        assert_eq!(parsed, Action::Help);
    }

    #[test]
    fn parse_args_short_help_flag_is_help() {
        let parsed = parse_args(&args(&["-h"])).expect("-h should parse");
        assert_eq!(parsed, Action::Help);
    }

    #[test]
    fn parse_args_unknown_flag_is_usage_error() {
        assert!(parse_args(&args(&["--foo"])).is_err());
    }

    #[test]
    fn parse_args_extra_positional_is_usage_error() {
        assert!(parse_args(&args(&["extra"])).is_err());
    }

    #[test]
    fn parse_args_help_with_extra_is_usage_error() {
        assert!(parse_args(&args(&["--help", "extra"])).is_err());
    }

    #[test]
    fn ensure_home_set_fails_when_home_unset() {
        let _lock = env_lock();
        let _guard = HomeGuard::capture();
        env::remove_var("HOME");

        let result = ensure_home_set();
        assert!(result.is_err(), "expected error when HOME is unset");
    }

    #[test]
    fn ensure_home_set_succeeds_when_home_present() {
        let _lock = env_lock();
        let _guard = HomeGuard::capture();
        env::set_var("HOME", "/some/home");

        ensure_home_set().expect("expected ok when HOME is set");
    }

    fn unique_tmp_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "herdr-uninstall-{}-{}-{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn detect_running_herdr_returns_true_when_socket_listening() {
        use std::os::unix::net::UnixListener;

        let socket_path = unique_tmp_path("running");
        let _listener = UnixListener::bind(&socket_path).expect("bind unix socket");

        let detected = detect_running_herdr(&socket_path).expect("probe should succeed");

        let _ = std::fs::remove_file(&socket_path);
        assert!(
            detected,
            "expected detect_running_herdr to find a listening socket"
        );
    }

    #[test]
    fn detect_running_herdr_returns_false_when_no_socket() {
        let socket_path = unique_tmp_path("missing");
        let _ = std::fs::remove_file(&socket_path);

        let detected = detect_running_herdr(&socket_path).expect("probe should succeed");
        assert!(!detected);
    }

    fn seed_full_install(base: &Path) -> UninstallPaths {
        use std::fs;

        let home = base.join("home");
        fs::create_dir_all(&home).unwrap();

        let config_dir = home.join(".config/herdr");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config.toml"), b"# stub").unwrap();
        fs::write(config_dir.join("session.json"), b"{}").unwrap();
        fs::write(config_dir.join("herdr.log"), b"log").unwrap();

        let pi_dir = home.join(".pi/agent/extensions");
        fs::create_dir_all(&pi_dir).unwrap();
        fs::write(pi_dir.join("herdr-agent-state.ts"), b"// stub").unwrap();

        let claude_hooks = home.join(".claude/hooks");
        fs::create_dir_all(&claude_hooks).unwrap();
        fs::write(claude_hooks.join("herdr-agent-state.sh"), b"#!/bin/sh").unwrap();
        let claude_settings = home.join(".claude/settings.json");
        fs::write(
            &claude_settings,
            br#"{"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"~/.claude/hooks/herdr-agent-state.sh"}]}]}}"#,
        )
        .unwrap();

        let codex_dir = home.join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        fs::write(codex_dir.join("herdr-agent-state.sh"), b"#!/bin/sh").unwrap();
        fs::write(
            codex_dir.join("hooks.json"),
            br#"{"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"~/.codex/herdr-agent-state.sh"}]}]}}"#,
        )
        .unwrap();
        fs::write(
            codex_dir.join("config.toml"),
            b"[features]\ncodex_hooks = true\n",
        )
        .unwrap();

        let opencode_plugins = home.join(".config/opencode/plugins");
        fs::create_dir_all(&opencode_plugins).unwrap();
        fs::write(opencode_plugins.join("herdr-agent-state.js"), b"// stub").unwrap();

        let bin_dir = home.join(".local/bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let binary = bin_dir.join("herdr");
        fs::write(&binary, b"#!/bin/sh\n").unwrap();

        UninstallPaths {
            binary,
            config_dir,
            extra_config_file: None,
            socket: base.join("nonexistent.sock"),
        }
    }

    fn assert_removed(status: &StepStatus, label: &str) {
        match status {
            StepStatus::Removed => (),
            other => panic!("expected {label} = Removed, got {other:?}"),
        }
    }

    fn assert_not_present(status: &StepStatus, label: &str) {
        match status {
            StepStatus::NotPresent => (),
            other => panic!("expected {label} = NotPresent, got {other:?}"),
        }
    }

    #[test]
    fn execute_full_removal_when_nothing_is_running() {
        let _lock = env_lock();
        let _guard = HomeGuard::capture();
        let base = unique_tmp_path("happy");
        std::fs::create_dir_all(&base).unwrap();
        env::set_var("HOME", base.join("home"));

        let paths = seed_full_install(&base);

        let report = execute(&paths).expect("execute should succeed when herdr is not running");

        assert!(!paths.binary.exists(), "binary should be removed");
        assert!(!paths.config_dir.exists(), "config dir should be removed");
        assert!(
            !base
                .join("home/.pi/agent/extensions/herdr-agent-state.ts")
                .exists(),
            "pi extension should be removed"
        );
        assert!(
            !base
                .join("home/.claude/hooks/herdr-agent-state.sh")
                .exists(),
            "claude hook should be removed"
        );
        assert!(
            !base.join("home/.codex/herdr-agent-state.sh").exists(),
            "codex hook should be removed"
        );
        assert!(
            !base
                .join("home/.config/opencode/plugins/herdr-agent-state.js")
                .exists(),
            "opencode plugin should be removed"
        );
        assert!(
            base.join("home/.codex/config.toml").exists(),
            "codex config.toml must remain (left intact by design)"
        );

        assert_removed(&report.binary_status, "binary");
        assert_removed(&report.config_dir_status, "config_dir");
        assert!(report.pi.is_ok(), "pi result should be Ok");
        assert!(report.claude.is_ok(), "claude result should be Ok");
        assert!(report.codex.is_ok(), "codex result should be Ok");
        assert!(report.opencode.is_ok(), "opencode result should be Ok");
        assert!(!report.has_errors(), "report should not have errors");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn execute_removes_extra_config_file_when_set() {
        let _lock = env_lock();
        let _guard = HomeGuard::capture();
        let base = unique_tmp_path("override");
        std::fs::create_dir_all(&base).unwrap();
        env::set_var("HOME", base.join("home"));

        let mut paths = seed_full_install(&base);
        let extra = base.join("herdr-custom.toml");
        std::fs::write(&extra, b"# custom config").unwrap();
        paths.extra_config_file = Some(extra.clone());

        let report = execute(&paths).expect("execute should succeed");

        assert!(!extra.exists(), "extra config file should be removed");
        match report.extra_config_file_status {
            Some(StepStatus::Removed) => (),
            other => panic!("expected extra_config_file_status = Some(Removed), got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn execute_extra_config_file_status_is_none_when_not_set() {
        let _lock = env_lock();
        let _guard = HomeGuard::capture();
        let base = unique_tmp_path("no-override");
        std::fs::create_dir_all(&base).unwrap();
        env::set_var("HOME", base.join("home"));

        let paths = seed_full_install(&base);
        assert!(paths.extra_config_file.is_none());

        let report = execute(&paths).expect("execute should succeed");
        assert!(report.extra_config_file_status.is_none());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn execute_retries_in_config_socket_when_config_dir_removal_fails() {
        let _lock = env_lock();
        let _guard = HomeGuard::capture();
        let base = unique_tmp_path("retry-socket");
        std::fs::create_dir_all(&base).unwrap();
        let home = base.join("home");
        std::fs::create_dir_all(&home).unwrap();
        env::set_var("HOME", &home);

        std::fs::create_dir_all(home.join(".pi/agent/extensions")).unwrap();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::create_dir_all(home.join(".config/opencode")).unwrap();

        let fake_config = home.join(".config/herdr");
        std::fs::create_dir_all(fake_config.parent().unwrap()).unwrap();
        std::fs::write(&fake_config, b"not a dir").unwrap();

        let paths = UninstallPaths {
            binary: home.join(".local/bin/herdr"),
            config_dir: fake_config.clone(),
            extra_config_file: None,
            socket: fake_config.clone(),
        };

        let report = execute(&paths).expect("execute should return a report even on failures");

        assert!(
            matches!(report.config_dir_status, StepStatus::Failed(_)),
            "expected config_dir_status = Failed, got {:?}",
            report.config_dir_status
        );
        assert!(
            !matches!(report.socket_status, StepStatus::NotPresent),
            "socket_status must not shortcut to NotPresent when config_dir removal failed; got {:?}",
            report.socket_status
        );

        let _ = std::fs::remove_file(&fake_config);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn execute_integration_failure_does_not_abort_remaining_steps() {
        let _lock = env_lock();
        let _guard = HomeGuard::capture();
        let base = unique_tmp_path("partial");
        std::fs::create_dir_all(&base).unwrap();
        env::set_var("HOME", base.join("home"));

        let paths = seed_full_install(&base);

        std::fs::write(
            base.join("home/.claude/settings.json"),
            b"{this is not valid json",
        )
        .unwrap();

        let report = execute(&paths).expect("execute should still succeed structurally");

        assert!(
            report.claude.is_err(),
            "claude uninstall should have failed on corrupt JSON, got {:?}",
            report.claude
        );

        assert!(report.pi.is_ok(), "pi should still have run: {:?}", report.pi);
        assert!(
            report.codex.is_ok(),
            "codex should still have run: {:?}",
            report.codex
        );
        assert!(
            report.opencode.is_ok(),
            "opencode should still have run: {:?}",
            report.opencode
        );
        assert_removed(&report.config_dir_status, "config_dir");
        assert_removed(&report.binary_status, "binary");

        assert!(
            !base
                .join("home/.pi/agent/extensions/herdr-agent-state.ts")
                .exists(),
            "pi extension should still have been removed"
        );
        assert!(
            !base.join("home/.codex/herdr-agent-state.sh").exists(),
            "codex hook should still have been removed"
        );
        assert!(
            !base
                .join("home/.config/opencode/plugins/herdr-agent-state.js")
                .exists(),
            "opencode plugin should still have been removed"
        );
        assert!(!paths.binary.exists(), "binary should still have been removed");

        assert!(report.has_errors(), "report should report errors");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn execute_is_idempotent_on_empty_state() {
        let _lock = env_lock();
        let _guard = HomeGuard::capture();
        let base = unique_tmp_path("empty");
        let home = base.join("home");
        std::fs::create_dir_all(&home).unwrap();
        env::set_var("HOME", &home);

        std::fs::create_dir_all(home.join(".pi/agent/extensions")).unwrap();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::create_dir_all(home.join(".config/opencode")).unwrap();

        let paths = UninstallPaths {
            binary: home.join(".local/bin/herdr"),
            config_dir: home.join(".config/herdr"),
            extra_config_file: None,
            socket: base.join("nonexistent.sock"),
        };

        let report = execute(&paths).expect("idempotent execute should succeed");

        assert_not_present(&report.binary_status, "binary");
        assert_not_present(&report.config_dir_status, "config_dir");
        assert_not_present(&report.socket_status, "socket");
        assert!(report.pi.is_ok(), "pi result should be Ok on empty state: {:?}", report.pi);
        assert!(report.claude.is_ok(), "claude result should be Ok on empty state: {:?}", report.claude);
        assert!(report.codex.is_ok(), "codex result should be Ok on empty state: {:?}", report.codex);
        assert!(report.opencode.is_ok(), "opencode result should be Ok on empty state: {:?}", report.opencode);
        assert!(!report.has_errors(), "empty-state report should have no errors");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn execute_aborts_when_herdr_is_running_and_touches_nothing() {
        use std::os::unix::net::UnixListener;

        let _lock = env_lock();
        let _guard = HomeGuard::capture();
        let base = unique_tmp_path("abort");
        std::fs::create_dir_all(&base).unwrap();
        env::set_var("HOME", base.join("home"));

        let mut paths = seed_full_install(&base);
        paths.socket = base.join("alive.sock");
        let _listener = UnixListener::bind(&paths.socket).expect("bind alive socket");

        let err = execute(&paths).expect_err("expected execute to abort while herdr is running");
        assert!(
            matches!(&err, UninstallAbort::Running { socket } if socket == &paths.socket),
            "expected Running variant, got {err:?}"
        );

        // Critical: nothing was deleted
        assert!(paths.binary.exists(), "binary must NOT be removed");
        assert!(paths.config_dir.exists(), "config dir must NOT be removed");
        assert!(
            base.join("home/.pi/agent/extensions/herdr-agent-state.ts")
                .exists(),
            "pi integration must NOT be touched"
        );
        assert!(
            base.join("home/.claude/hooks/herdr-agent-state.sh")
                .exists(),
            "claude hook must NOT be touched"
        );
        assert!(
            base.join("home/.codex/herdr-agent-state.sh").exists(),
            "codex hook must NOT be touched"
        );
        assert!(
            base.join("home/.config/opencode/plugins/herdr-agent-state.js")
                .exists(),
            "opencode plugin must NOT be touched"
        );

        let _ = std::fs::remove_file(&paths.socket);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn detect_running_herdr_returns_false_when_socket_file_is_stale() {
        let socket_path = unique_tmp_path("stale");
        std::fs::write(&socket_path, b"stale").unwrap();

        let detected = detect_running_herdr(&socket_path).expect("probe should succeed");

        let _ = std::fs::remove_file(&socket_path);
        assert!(
            !detected,
            "stale non-socket file should not be reported as running"
        );
    }
}
