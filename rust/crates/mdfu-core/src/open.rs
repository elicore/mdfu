//! Launch a URL with the platform's default handler.
//!
//! Port of `internal/open/open.go`. Only web schemes (`http`, `https`,
//! `mailto`) are accepted, so a crafted document cannot invoke an arbitrary
//! program. The launcher is spawned asynchronously; its exit status is never
//! awaited by the caller.

use std::fmt;
use std::process::Command;

/// The platform launcher: the program to run plus any arguments inserted
/// before the URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Launcher {
    /// Program name passed to the OS (e.g. `xdg-open`).
    pub program: &'static str,
    /// Arguments inserted between the program and the URL.
    pub prefix_args: &'static [&'static str],
}

/// The launcher for the current target OS.
#[cfg(target_os = "macos")]
pub const LAUNCHER: Launcher = Launcher {
    program: "open",
    prefix_args: &[],
};

/// The launcher for the current target OS.
#[cfg(target_os = "windows")]
pub const LAUNCHER: Launcher = Launcher {
    program: "rundll32",
    prefix_args: &["url.dll,FileProtocolHandler"],
};

/// The launcher for the current target OS.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub const LAUNCHER: Launcher = Launcher {
    program: "xdg-open",
    prefix_args: &[],
};

/// Error returned when a URL cannot be opened.
#[derive(Debug)]
pub enum Error {
    /// The URL was empty (or whitespace only) after trimming.
    EmptyUrl,
    /// The scheme is not one of `http`, `https`, or `mailto`.
    UnsupportedScheme(String),
    /// The operating system failed to spawn the launcher process.
    Spawn(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyUrl => f.write_str("open: empty URL"),
            Self::UnsupportedScheme(scheme) => {
                write!(f, "open: unsupported scheme {scheme:?}")
            }
            Self::Spawn(err) => write!(f, "open: {err}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(err) => Some(err),
            Self::EmptyUrl | Self::UnsupportedScheme(_) => None,
        }
    }
}

/// Validates that `url` uses one of the allowed web schemes.
///
/// The input is trimmed first. On success the scheme is returned; on failure
/// an [`Error`] describes why the URL is unacceptable.
///
/// # Errors
///
/// Returns [`Error::EmptyUrl`] when the trimmed URL is empty, or
/// [`Error::UnsupportedScheme`] when it does not start with `http`, `https`,
/// or `mailto` (case-insensitively).
#[must_use = "the validation result must be inspected"]
pub fn validate_scheme(url: &str) -> Result<&str, Error> {
    let url = url.trim();
    if url.is_empty() {
        return Err(Error::EmptyUrl);
    }

    let Some((scheme, _rest)) = url.split_once(':') else {
        return Err(Error::UnsupportedScheme(String::new()));
    };

    if scheme.eq_ignore_ascii_case("http")
        || scheme.eq_ignore_ascii_case("https")
        || scheme.eq_ignore_ascii_case("mailto")
    {
        Ok(scheme)
    } else {
        Err(Error::UnsupportedScheme(scheme.to_owned()))
    }
}

/// Builds the full argv (program, prefix arguments, URL) for a launcher.
///
/// Pure: it never spawns a process, so tests can assert the platform command
/// mapping without launching a browser.
#[must_use]
pub fn launcher_argv(launcher: Launcher, url: &str) -> Vec<String> {
    let mut argv = Vec::with_capacity(launcher.prefix_args.len() + 2);
    argv.push(launcher.program.to_owned());
    argv.extend(launcher.prefix_args.iter().map(|arg| (*arg).to_owned()));
    argv.push(url.to_owned());
    argv
}

/// Opens `url` with the platform's default handler.
///
/// Only `http`, `https`, and `mailto` URLs are accepted. The launcher is
/// spawned and the function returns immediately; the child's exit status is
/// reaped on a detached thread and never blocks the caller.
///
/// # Errors
///
/// Returns the same validation errors as [`validate_scheme`], or
/// [`Error::Spawn`] when the launcher process cannot be started.
#[must_use = "the open result must be inspected"]
#[allow(clippy::module_name_repetitions)]
pub fn open_url(url: &str) -> Result<(), Error> {
    let url = url.trim();
    validate_scheme(url)?;

    let command_argv = launcher_argv(LAUNCHER, url);
    let Some((program, rest)) = command_argv.split_first() else {
        // Unreachable: `launcher_argv` always yields at least the program.
        return Err(Error::EmptyUrl);
    };

    let mut child = Command::new(program)
        .args(rest)
        .spawn()
        .map_err(Error::Spawn)?;

    // Mirror `go cmd.Wait()`: reap the child off-thread without blocking.
    let _reaper = std::thread::Builder::new()
        .name("mdfu-open-reaper".to_owned())
        .spawn(move || {
            let _ = child.wait();
        });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_whitespace() {
        assert!(matches!(validate_scheme("").unwrap_err(), Error::EmptyUrl));
        assert!(matches!(
            validate_scheme("   ").unwrap_err(),
            Error::EmptyUrl
        ));
    }

    #[test]
    fn rejects_disallowed_schemes() {
        for raw in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "ftp://example.com",
            "command:rm -rf /",
            "://bad",
        ] {
            assert!(
                matches!(
                    validate_scheme(raw).unwrap_err(),
                    Error::UnsupportedScheme(_)
                ),
                "expected {raw:?} to be rejected"
            );
        }
    }

    #[test]
    fn accepts_web_schemes_case_insensitively() {
        for raw in [
            "http://example.com",
            "https://example.com",
            "mailto:someone@example.com",
            "HTTPS://EXAMPLE.COM",
            "  mailto:someone@example.com  ",
        ] {
            assert!(
                validate_scheme(raw).is_ok(),
                "expected {raw:?} to be accepted"
            );
        }
    }

    #[test]
    fn open_url_rejects_bad_schemes_without_spawning() {
        for raw in ["", "   ", "file:///etc/passwd", "javascript:alert(1)"] {
            assert!(open_url(raw).is_err(), "open_url({raw:?}) should fail");
        }
    }

    #[test]
    fn argv_mapping_per_platform() {
        let mac = Launcher {
            program: "open",
            prefix_args: &[],
        };
        assert_eq!(
            launcher_argv(mac, "https://example.com"),
            vec!["open".to_owned(), "https://example.com".to_owned()]
        );

        let windows = Launcher {
            program: "rundll32",
            prefix_args: &["url.dll,FileProtocolHandler"],
        };
        assert_eq!(
            launcher_argv(windows, "https://example.com"),
            vec![
                "rundll32".to_owned(),
                "url.dll,FileProtocolHandler".to_owned(),
                "https://example.com".to_owned(),
            ]
        );

        let linux = Launcher {
            program: "xdg-open",
            prefix_args: &[],
        };
        assert_eq!(
            launcher_argv(linux, "https://example.com"),
            vec!["xdg-open".to_owned(), "https://example.com".to_owned()]
        );
    }

    #[test]
    fn target_launcher_matches_cfg() {
        #[cfg(target_os = "macos")]
        assert_eq!(LAUNCHER.program, "open");
        #[cfg(target_os = "windows")]
        assert_eq!(LAUNCHER.program, "rundll32");
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        assert_eq!(LAUNCHER.program, "xdg-open");
    }
}
