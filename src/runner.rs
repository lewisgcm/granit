//! Command execution abstraction.
//!
//! All external process invocation (notably `nix`) goes through the
//! [`CommandRunner`] trait so that logic can be unit-tested with a mock runner
//! instead of shelling out for real.

use std::ffi::OsStr;
use std::process::{Command, Stdio};

/// The result of running an external command with captured output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// Whether the process exited with a success status code (0).
    pub success: bool,
    /// Exit code, if the process exited normally.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Abstraction over running external commands.
///
/// Implementations either shell out for real ([`SystemRunner`]) or return
/// canned responses for tests.
pub trait CommandRunner {
    /// Run `program` with `args`, capturing stdout/stderr.
    fn run_captured(&self, program: &str, args: &[&str]) -> std::io::Result<CommandOutput>;

    /// Run `program` with `args` in `cwd`, streaming stdout/stderr to the
    /// user's terminal. Returns the exit status success flag and code.
    fn run_streamed(
        &self,
        program: &str,
        args: &[&str],
        cwd: Option<&std::path::Path>,
    ) -> std::io::Result<CommandOutput>;

    /// Whether `program` is resolvable on the current `PATH`.
    fn which(&self, program: &str) -> bool;
}

/// A [`CommandRunner`] that executes real subprocesses.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl SystemRunner {
    pub fn new() -> Self {
        SystemRunner
    }
}

fn to_output(output: std::process::Output) -> CommandOutput {
    CommandOutput {
        success: output.status.success(),
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// Standard locations where the `nix` binary may live even if the profile is
/// not on `PATH` (common on multi-user installs and fresh shells).
const NIX_FALLBACK_DIRS: &[&str] = &[
    "/nix/var/nix/profiles/default/bin",
    "/run/current-system/sw/bin",
];

/// Resolve a program to an executable path, checking `PATH` first and then a
/// few well-known Nix profile locations for `nix`-family binaries.
pub fn resolve_program(program: &str) -> String {
    if which_on_path(program) {
        return program.to_string();
    }
    // Fall back to known Nix profile locations for nix-family tools.
    if program == "nix" || program.starts_with("nix-") {
        for dir in NIX_FALLBACK_DIRS {
            let candidate = std::path::Path::new(dir).join(program);
            if candidate.is_file() {
                return candidate.to_string_lossy().into_owned();
            }
        }
    }
    program.to_string()
}

impl CommandRunner for SystemRunner {
    fn run_captured(&self, program: &str, args: &[&str]) -> std::io::Result<CommandOutput> {
        let resolved = resolve_program(program);
        let output = Command::new(&resolved)
            .args(args.iter().map(OsStr::new))
            .output()?;
        Ok(to_output(output))
    }

    fn run_streamed(
        &self,
        program: &str,
        args: &[&str],
        cwd: Option<&std::path::Path>,
    ) -> std::io::Result<CommandOutput> {
        let resolved = resolve_program(program);
        let mut cmd = Command::new(&resolved);
        cmd.args(args.iter().map(OsStr::new))
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let status = cmd.status()?;
        Ok(CommandOutput {
            success: status.success(),
            code: status.code(),
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    fn which(&self, program: &str) -> bool {
        if which_on_path(program) {
            return true;
        }
        if program == "nix" || program.starts_with("nix-") {
            return NIX_FALLBACK_DIRS
                .iter()
                .any(|dir| std::path::Path::new(dir).join(program).is_file());
        }
        false
    }
}

/// Search the `PATH` environment variable for an executable named `program`.
pub fn which_on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return true;
        }
        // Windows executables may have an extension.
        #[cfg(windows)]
        {
            for ext in ["exe", "bat", "cmd"] {
                if dir.join(format!("{program}.{ext}")).is_file() {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
pub mod mock {
    //! A configurable mock [`CommandRunner`] for tests.
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// Records invocations and returns pre-programmed responses.
    #[derive(Default)]
    pub struct MockRunner {
        /// Map from the full command line (`"program arg1 arg2"`) to a response.
        responses: HashMap<String, CommandOutput>,
        /// Programs considered present on PATH.
        present: Vec<String>,
        /// Recorded invocations, in order.
        pub calls: RefCell<Vec<String>>,
        /// Default response when no specific match is found.
        default: Option<CommandOutput>,
    }

    impl MockRunner {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn with_present(mut self, program: &str) -> Self {
            self.present.push(program.to_string());
            self
        }

        pub fn with_response(mut self, cmdline: &str, output: CommandOutput) -> Self {
            self.responses.insert(cmdline.to_string(), output);
            self
        }

        pub fn with_default(mut self, output: CommandOutput) -> Self {
            self.default = Some(output);
            self
        }

        fn lookup(&self, program: &str, args: &[&str]) -> std::io::Result<CommandOutput> {
            let mut key = String::from(program);
            for a in args {
                key.push(' ');
                key.push_str(a);
            }
            self.calls.borrow_mut().push(key.clone());
            if let Some(resp) = self.responses.get(&key) {
                return Ok(resp.clone());
            }
            if let Some(def) = &self.default {
                return Ok(def.clone());
            }
            Ok(CommandOutput {
                success: false,
                code: Some(127),
                stdout: String::new(),
                stderr: format!("mock: no response configured for `{key}`"),
            })
        }
    }

    impl CommandRunner for MockRunner {
        fn run_captured(&self, program: &str, args: &[&str]) -> std::io::Result<CommandOutput> {
            self.lookup(program, args)
        }

        fn run_streamed(
            &self,
            program: &str,
            args: &[&str],
            _cwd: Option<&std::path::Path>,
        ) -> std::io::Result<CommandOutput> {
            self.lookup(program, args)
        }

        fn which(&self, program: &str) -> bool {
            self.present.iter().any(|p| p == program)
        }
    }

    pub fn ok(stdout: &str) -> CommandOutput {
        CommandOutput {
            success: true,
            code: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    pub fn fail(code: i32, stderr: &str) -> CommandOutput {
        CommandOutput {
            success: false,
            code: Some(code),
            stdout: String::new(),
            stderr: stderr.to_string(),
        }
    }
}
