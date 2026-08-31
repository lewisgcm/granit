//! Environment diagnostics (`granit doctor`).
//!
//! Verifies that the host has a working Nix installation with the flakes and
//! `nix-command` experimental features available, since Granit generates a
//! `flake.nix` and builds with `nix build`.

use crate::runner::CommandRunner;

/// Status of a single diagnostic check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStatus {
    Ok,
    Warn,
    Fail,
}

/// One diagnostic check result with a human-readable message and optional hint.
#[derive(Debug, Clone)]
pub struct Check {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
    pub hint: Option<String>,
}

/// The full doctor report.
#[derive(Debug, Clone)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    /// True if no check has status `Fail`.
    pub fn is_healthy(&self) -> bool {
        self.checks.iter().all(|c| c.status != CheckStatus::Fail)
    }
}

/// Run all diagnostic checks using the provided command runner.
pub fn diagnose<R: CommandRunner>(runner: &R) -> Report {
    let mut checks = Vec::new();

    // 1. Is `nix` on PATH?
    let nix_present = runner.which("nix");
    if !nix_present {
        checks.push(Check {
            name: "nix installed".into(),
            status: CheckStatus::Fail,
            detail: "`nix` was not found on your PATH".into(),
            hint: Some(
                "Install Nix from https://nixos.org/download (the Determinate Systems \
                 installer enables flakes by default)."
                    .into(),
            ),
        });
        // No point probing further if nix is absent.
        return Report { checks };
    }

    // 2. nix version
    match runner.run_captured("nix", &["--version"]) {
        Ok(out) if out.success => {
            checks.push(Check {
                name: "nix version".into(),
                status: CheckStatus::Ok,
                detail: out.stdout.trim().to_string(),
                hint: None,
            });
        }
        Ok(out) => checks.push(Check {
            name: "nix version".into(),
            status: CheckStatus::Warn,
            detail: format!("`nix --version` exited non-zero: {}", out.stderr.trim()),
            hint: None,
        }),
        Err(e) => checks.push(Check {
            name: "nix version".into(),
            status: CheckStatus::Warn,
            detail: format!("could not run `nix --version`: {e}"),
            hint: None,
        }),
    }

    // 3. Are the experimental features enabled?
    checks.push(check_flakes(runner));

    Report { checks }
}

/// Probe whether `nix-command` and `flakes` are enabled by asking nix to show
/// its config.
fn check_flakes<R: CommandRunner>(runner: &R) -> Check {
    // `nix config show experimental-features` prints the enabled features on
    // modern Nix. On older versions this subcommand may not exist; fall back to
    // `nix show-config`.
    let probe = runner
        .run_captured("nix", &["config", "show", "experimental-features"])
        .ok()
        .filter(|o| o.success)
        .map(|o| o.stdout)
        .or_else(|| {
            runner
                .run_captured("nix", &["show-config"])
                .ok()
                .filter(|o| o.success)
                .map(|o| o.stdout)
        });

    match probe {
        Some(text) => {
            let has_nix_command = text.contains("nix-command");
            let has_flakes = text.contains("flakes");
            if has_nix_command && has_flakes {
                Check {
                    name: "flakes enabled".into(),
                    status: CheckStatus::Ok,
                    detail: "nix-command and flakes experimental features are enabled".into(),
                    hint: None,
                }
            } else {
                let mut missing = Vec::new();
                if !has_nix_command {
                    missing.push("nix-command");
                }
                if !has_flakes {
                    missing.push("flakes");
                }
                Check {
                    name: "flakes enabled".into(),
                    status: CheckStatus::Fail,
                    detail: format!("missing experimental features: {}", missing.join(", ")),
                    hint: Some(
                        "Add `experimental-features = nix-command flakes` to \
                         ~/.config/nix/nix.conf (or /etc/nix/nix.conf), or pass \
                         `--extra-experimental-features \"nix-command flakes\"` to nix."
                            .into(),
                    ),
                }
            }
        }
        None => Check {
            name: "flakes enabled".into(),
            status: CheckStatus::Warn,
            detail: "could not determine whether flakes are enabled".into(),
            hint: Some(
                "Ensure `experimental-features = nix-command flakes` is set in your nix.conf."
                    .into(),
            ),
        },
    }
}

/// Render a report to a user-facing string.
pub fn render(report: &Report) -> String {
    let mut out = String::from("granit doctor\n=============\n");
    for check in &report.checks {
        let marker = match check.status {
            CheckStatus::Ok => "[ ok ]",
            CheckStatus::Warn => "[warn]",
            CheckStatus::Fail => "[fail]",
        };
        out.push_str(&format!("{marker} {}: {}\n", check.name, check.detail));
        if let Some(hint) = &check.hint {
            out.push_str(&format!("        -> {hint}\n"));
        }
    }
    out.push('\n');
    if report.is_healthy() {
        out.push_str("Environment looks good. You're ready to build with granit.\n");
    } else {
        out.push_str("Environment has problems that will prevent building. See hints above.\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::mock::{fail, ok, MockRunner};

    #[test]
    fn reports_fail_when_nix_missing() {
        let runner = MockRunner::new(); // nix not present
        let report = diagnose(&runner);
        assert!(!report.is_healthy());
        assert_eq!(report.checks.len(), 1);
        assert_eq!(report.checks[0].status, CheckStatus::Fail);
        assert!(report.checks[0].detail.contains("not found"));
    }

    #[test]
    fn reports_healthy_when_nix_and_flakes_present() {
        let runner = MockRunner::new()
            .with_present("nix")
            .with_response("nix --version", ok("nix (Nix) 2.21.0"))
            .with_response(
                "nix config show experimental-features",
                ok("nix-command flakes"),
            );
        let report = diagnose(&runner);
        assert!(report.is_healthy(), "expected healthy: {report:?}");
        assert!(report.checks.iter().any(|c| c.name == "nix version"
            && c.status == CheckStatus::Ok
            && c.detail.contains("2.21.0")));
        assert!(report
            .checks
            .iter()
            .any(|c| c.name == "flakes enabled" && c.status == CheckStatus::Ok));
    }

    #[test]
    fn reports_fail_when_flakes_disabled() {
        let runner = MockRunner::new()
            .with_present("nix")
            .with_response("nix --version", ok("nix (Nix) 2.21.0"))
            .with_response("nix config show experimental-features", ok(""))
            .with_response("nix show-config", ok("some-other-option = true"));
        let report = diagnose(&runner);
        assert!(!report.is_healthy());
        let flakes = report
            .checks
            .iter()
            .find(|c| c.name == "flakes enabled")
            .unwrap();
        assert_eq!(flakes.status, CheckStatus::Fail);
        assert!(flakes.detail.contains("nix-command"));
        assert!(flakes.detail.contains("flakes"));
    }

    #[test]
    fn warns_when_flakes_probe_fails() {
        let runner = MockRunner::new()
            .with_present("nix")
            .with_response("nix --version", ok("nix (Nix) 2.21.0"))
            .with_response("nix config show experimental-features", fail(1, "boom"))
            .with_response("nix show-config", fail(1, "boom"));
        let report = diagnose(&runner);
        let flakes = report
            .checks
            .iter()
            .find(|c| c.name == "flakes enabled")
            .unwrap();
        assert_eq!(flakes.status, CheckStatus::Warn);
    }
}
