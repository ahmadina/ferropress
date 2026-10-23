//! Command-line interface definition.

use clap::{Parser, Subcommand};

/// Repository automation for Ferropress.
#[derive(Debug, Parser)]
#[command(name = "cargo xtask", bin_name = "cargo xtask")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, PartialEq, Eq, Subcommand)]
pub(crate) enum Command {
    /// Verify that crate dependencies follow the workspace layer rules
    CheckLayers,
    /// Validate the reference manifest and hash any downloaded archives
    VerifyManifest,
    /// Manage the reference lab
    #[command(subcommand)]
    Lab(LabCommand),
}

#[derive(Debug, PartialEq, Eq, Subcommand)]
pub(crate) enum LabCommand {
    /// Build and start the lab stack and wait until it is healthy
    Up,
    /// Stop the lab stack, keeping its database
    Down,
    /// Stop the stack, delete its database and start it from scratch
    Reset,
    /// Download pinned upstream sources and archives into git-ignored lab directories
    Fetch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory as _;
    use clap::error::ErrorKind;

    fn parse(args: &[&str]) -> Result<Command, ErrorKind> {
        Cli::try_parse_from(std::iter::once("xtask").chain(args.iter().copied()))
            .map(|cli| cli.command)
            .map_err(|error| error.kind())
    }

    #[test]
    fn definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_known_commands() {
        assert_eq!(parse(&["check-layers"]), Ok(Command::CheckLayers));
        assert_eq!(parse(&["verify-manifest"]), Ok(Command::VerifyManifest));
        let lab = [
            ("up", LabCommand::Up),
            ("down", LabCommand::Down),
            ("reset", LabCommand::Reset),
            ("fetch", LabCommand::Fetch),
        ];
        for (name, command) in lab {
            assert_eq!(parse(&["lab", name]), Ok(Command::Lab(command)), "{name}");
        }
    }

    #[test]
    fn rejects_missing_unknown_and_extra_arguments() {
        assert_eq!(
            parse(&[]),
            Err(ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand)
        );
        assert_eq!(parse(&["build"]), Err(ErrorKind::InvalidSubcommand));
        assert_eq!(
            parse(&["check-layers", "--fix"]),
            Err(ErrorKind::UnknownArgument)
        );
        assert_eq!(
            parse(&["lab"]),
            Err(ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand)
        );
        assert_eq!(parse(&["lab", "start"]), Err(ErrorKind::InvalidSubcommand));
        assert_eq!(
            parse(&["lab", "up", "now"]),
            Err(ErrorKind::UnknownArgument)
        );
    }

    #[test]
    fn help_is_available() {
        assert_eq!(parse(&["--help"]), Err(ErrorKind::DisplayHelp));
        assert_eq!(parse(&["help"]), Err(ErrorKind::DisplayHelp));
    }
}
