//! Exit codes and the error type shared by all `sasha-cli` subcommands.

/// Documented process exit codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliExit {
    Ok,
    Failure,
    Usage,
    OpenFailed,
    Timeout,
}

impl CliExit {
    pub fn code(self) -> u8 {
        match self {
            Self::Ok => 0,
            Self::Failure => 1,
            Self::Usage => 2,
            Self::OpenFailed => 3,
            Self::Timeout => 4,
        }
    }
}

/// A failure with the exit code it maps to and a message for stderr.
#[derive(Debug)]
pub struct CliError {
    pub exit: CliExit,
    pub message: String,
}

impl CliError {
    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(CliExit::Usage, message)
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self::new(CliExit::Failure, message)
    }

    pub fn open_failed(message: impl Into<String>) -> Self {
        Self::new(CliExit::OpenFailed, message)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(CliExit::Timeout, message)
    }

    fn new(exit: CliExit, message: impl Into<String>) -> Self {
        Self {
            exit,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_match_the_documented_values() {
        assert_eq!(CliExit::Ok.code(), 0);
        assert_eq!(CliExit::Failure.code(), 1);
        assert_eq!(CliExit::Usage.code(), 2);
        assert_eq!(CliExit::OpenFailed.code(), 3);
        assert_eq!(CliExit::Timeout.code(), 4);
    }

    #[test]
    fn constructors_set_the_matching_exit() {
        assert_eq!(CliError::usage("x").exit, CliExit::Usage);
        assert_eq!(CliError::failure("x").exit, CliExit::Failure);
        assert_eq!(CliError::open_failed("x").exit, CliExit::OpenFailed);
        assert_eq!(CliError::timeout("x").exit, CliExit::Timeout);
    }
}
