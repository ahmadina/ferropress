//! Command-line interface definition.

use clap::{Parser, Subcommand};

use test_harness::{Profile, RequestPath, Slug};

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
    /// Validate contract entries and write the completeness dashboard to docs/COMPLETENESS.md
    Dashboard,
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
    /// Stop the stack, delete its database, start it from scratch and install the site
    Reset,
    /// Install the site with fixed settings (no-op when already installed)
    Install,
    /// Download pinned upstream sources and archives into git-ignored lab directories
    Fetch,
    /// Record one GET request against the running lab into lab/captures/NAME.json
    Capture {
        /// Capture name: lowercase letters, digits and hyphens
        name: Slug,
        /// Request path with optional query string
        #[arg(default_value = "/")]
        path: RequestPath,
        /// Site installation to send the request to: single or network
        #[arg(long, default_value = "single")]
        profile: Profile,
    },
    /// Reset the lab and record the reference platform's symbol inventory into contracts/inventory
    Inventory,
    /// Record the reference lifecycle of every profile twice from fresh installs into contracts/lifecycle
    Lifecycle,
    /// Record how the reference PHP reads and writes serialized values into contracts/serialize
    Serialize,
    /// Record how the reference PHP stores values under assignments and references into contracts/value
    Value,
    /// Record how the reference PHP splits source text into tokens into contracts/lexer
    Lexer,
    /// Compare php-lexer with the reference PHP on every upstream source file (needs lab/upstream)
    LexerCorpus,
    /// Record how the reference PHP parses source text into contracts/parser
    Parser,
    /// Compare php-parser with the reference PHP on every upstream source file (needs lab/upstream)
    ParserCorpus,
    /// Record what the reference PHP reports while compiling source text into contracts/compiler
    Compiler,
    /// Compare php-compiler's diagnostics with the reference PHP on every upstream source file (needs lab/upstream)
    CompilerCorpus,
    /// Record what the reference PHP's operators and conversions return into contracts/operators
    Operators,
    /// Run contracts/scenarios/SCENARIO.toml twice from fresh installs and require identical results
    Check {
        /// Scenario id
        scenario: Slug,
    },
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
        assert_eq!(parse(&["dashboard"]), Ok(Command::Dashboard));
        let lab = [
            ("up", LabCommand::Up),
            ("down", LabCommand::Down),
            ("reset", LabCommand::Reset),
            ("install", LabCommand::Install),
            ("fetch", LabCommand::Fetch),
            ("inventory", LabCommand::Inventory),
            ("lifecycle", LabCommand::Lifecycle),
            ("serialize", LabCommand::Serialize),
            ("value", LabCommand::Value),
            ("lexer", LabCommand::Lexer),
            ("lexer-corpus", LabCommand::LexerCorpus),
            ("parser", LabCommand::Parser),
            ("parser-corpus", LabCommand::ParserCorpus),
            ("compiler", LabCommand::Compiler),
            ("compiler-corpus", LabCommand::CompilerCorpus),
            ("operators", LabCommand::Operators),
        ];
        for (name, command) in lab {
            assert_eq!(parse(&["lab", name]), Ok(Command::Lab(command)), "{name}");
        }
        assert_eq!(
            parse(&["lab", "capture", "home"]),
            Ok(Command::Lab(LabCommand::Capture {
                name: "home".parse().unwrap(),
                path: "/".parse().unwrap(),
                profile: Profile::Single,
            }))
        );
        assert_eq!(
            parse(&["lab", "capture", "post", "/?p=1"]),
            Ok(Command::Lab(LabCommand::Capture {
                name: "post".parse().unwrap(),
                path: "/?p=1".parse().unwrap(),
                profile: Profile::Single,
            }))
        );
        assert_eq!(
            parse(&[
                "lab",
                "capture",
                "second",
                "/second/",
                "--profile",
                "network"
            ]),
            Ok(Command::Lab(LabCommand::Capture {
                name: "second".parse().unwrap(),
                path: "/second/".parse().unwrap(),
                profile: Profile::Network,
            }))
        );
        assert_eq!(
            parse(&["lab", "capture", "x", "--profile", "cluster"]),
            Err(ErrorKind::ValueValidation)
        );
        assert_eq!(
            parse(&["lab", "check", "front-page"]),
            Ok(Command::Lab(LabCommand::Check {
                scenario: "front-page".parse().unwrap(),
            }))
        );
        assert_eq!(
            parse(&["lab", "check", "Front"]),
            Err(ErrorKind::ValueValidation)
        );
        assert_eq!(
            parse(&["lab", "check"]),
            Err(ErrorKind::MissingRequiredArgument)
        );
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
        assert_eq!(
            parse(&["lab", "capture", "Bad_Name"]),
            Err(ErrorKind::ValueValidation)
        );
        assert_eq!(
            parse(&["lab", "capture", "ok", "no-slash"]),
            Err(ErrorKind::ValueValidation)
        );
    }

    #[test]
    fn help_is_available() {
        assert_eq!(parse(&["--help"]), Err(ErrorKind::DisplayHelp));
        assert_eq!(parse(&["help"]), Err(ErrorKind::DisplayHelp));
    }
}
