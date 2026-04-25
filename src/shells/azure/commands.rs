// Import the shared shell engine so parsing stays consistent with the other shells.
use crate::shells::engine;
// Import Clap helpers so this shell can describe its interactive command model.
use clap::{Args, Parser, Subcommand};

// Describe the argument shape for one Azure-shell command line.
#[derive(Parser, Debug)]
#[command(name = "azure", disable_help_subcommand = true)]
pub(super) struct AzureShellCli {
    // Store the one subcommand that the user typed in the Azure shell.
    #[command(subcommand)]
    command: AzureCommand,
}

// List the commands that the Azure shell understands.
#[derive(Subcommand, Debug)]
pub(super) enum AzureCommand {
    /// Login to Azure CLI as a user or service principal.
    Login(LoginArguments),
    /// Logout from Azure CLI and clear the cached account information.
    Logout,
    /// Show the current Azure login state.
    Status,
    /// Generate and list saved Azure inventory reports.
    #[command(subcommand, arg_required_else_help = true)]
    Inventory(InventoryCommand),
    /// Generate JSON snapshots of Azure resources.
    #[command(subcommand, arg_required_else_help = true)]
    Snapshot(SnapshotCommand),
    /// Show the Azure shell help message.
    Help,
    /// Close the current shell session.
    #[command(alias = "quit")]
    Exit,
}

// List the commands that belong under the Azure `inventory` command group.
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub(super) enum InventoryCommand {
    /// Export a new Markdown inventory report.
    Generate,
    /// List saved Azure inventory reports.
    #[command(alias = "ls")]
    List,
}

// List the commands that belong under the Azure `snapshot` command group.
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub(super) enum SnapshotCommand {
    /// Export a new JSON resource snapshot.
    Generate,
}

// Hold the arguments that belong to the `login` subcommand.
#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub(super) struct LoginArguments {
    // Switch to service-principal authentication instead of interactive user login.
    #[arg(long = "service-principal")]
    pub(super) service_principal: bool,
    // Accept an optional client ID for service-principal login.
    #[arg(long = "client-id", requires = "service_principal")]
    pub(super) client_id: Option<String>,
    // Accept an optional client secret for service-principal login.
    #[arg(long = "client-secret", requires = "service_principal")]
    pub(super) client_secret: Option<String>,
    // Accept an optional tenant for both login modes.
    pub(super) tenant: Option<String>,
}

// Convert tokenized Azure-shell input into one typed command.
pub(super) fn parse_command(tokens: &[String]) -> Result<AzureCommand, clap::Error> {
    // Reuse the shared helper so every shell performs the same Clap parsing steps.
    let cli = engine::parse_shell_command::<AzureShellCli>(super::SHELL_NAME, tokens)?;
    // Return only the subcommand because that is all the handler needs.
    Ok(cli.command)
}

#[cfg(test)]
mod tests {
    // Import Clap's error kind enum so tests can distinguish help from real parse failures.
    use clap::error::ErrorKind;

    // Import the Azure parser and command types so the tests can validate command behavior.
    use super::{AzureCommand, InventoryCommand, LoginArguments, SnapshotCommand, parse_command};

    #[test]
    fn parses_login_with_one_tenant() {
        // Parse a valid login command with exactly one tenant argument.
        let parsed_command = parse_command(&[String::from("login"), String::from("my-tenant-id")])
            .expect("command should parse");

        // Confirm that Clap keeps the tenant value and returns the login variant.
        assert!(matches!(
            parsed_command,
            AzureCommand::Login(LoginArguments {
                tenant: Some(tenant),
                service_principal: false,
                client_id: None,
                client_secret: None,
            }) if tenant == "my-tenant-id"
        ));
    }

    #[test]
    fn parses_bare_login_without_arguments() {
        // Parse a bare login command that relies on later config resolution.
        let parsed_command = parse_command(&[String::from("login")]).expect("command should parse");

        // Confirm that Clap represents the missing values as `None`.
        assert!(matches!(
            parsed_command,
            AzureCommand::Login(LoginArguments {
                tenant: None,
                service_principal: false,
                client_id: None,
                client_secret: None,
            })
        ));
    }

    #[test]
    fn parses_service_principal_login_with_flags() {
        // Parse a service-principal login command with explicit flags.
        let parsed_command = parse_command(&[
            String::from("login"),
            String::from("--service-principal"),
            String::from("--client-id"),
            String::from("client-id"),
            String::from("--client-secret"),
            String::from("secret-value"),
            String::from("tenant-id"),
        ])
        .expect("command should parse");

        // Confirm that Clap keeps all service-principal values in the typed command.
        assert!(matches!(
            parsed_command,
            AzureCommand::Login(LoginArguments {
                tenant: Some(tenant),
                service_principal: true,
                client_id: Some(client_id),
                client_secret: Some(client_secret),
            }) if tenant == "tenant-id" && client_id == "client-id" && client_secret == "secret-value"
        ));
    }

    #[test]
    fn rejects_service_principal_fields_without_service_principal_flag() {
        // Parse a login command that provides a service-principal field without the mode flag.
        let error = parse_command(&[
            String::from("login"),
            String::from("--client-id"),
            String::from("client-id"),
        ])
        .expect_err("command should fail");
        // Render the Clap error into text so we can inspect the message.
        let rendered_error = error.to_string();

        // Confirm that Clap explains the missing required `--service-principal` flag.
        assert!(rendered_error.contains("--service-principal"));
    }

    #[test]
    fn rejects_login_with_extra_arguments() {
        // Parse a login command that provides more than one value after `login`.
        let error = parse_command(&[
            String::from("login"),
            String::from("tenant-one"),
            String::from("tenant-two"),
        ])
        .expect_err("command should fail");
        // Render the Clap error into text so we can inspect the message.
        let rendered_error = error.to_string();

        // Confirm that Clap reports the unexpected extra argument.
        assert!(rendered_error.contains("unexpected argument"));
    }

    #[test]
    fn login_help_is_reported_as_display_help() {
        // Parse `login --help`, which Clap represents as a display request instead of a command.
        let error = parse_command(&[String::from("login"), String::from("--help")])
            .expect_err("help should be returned as a Clap display error");

        // Confirm that the parser reports intentional help output, not an invalid argument.
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    }

    #[test]
    fn login_help_lists_the_available_login_options() {
        // Ask Clap to render the help text for the login subcommand.
        let error = parse_command(&[String::from("login"), String::from("--help")])
            .expect_err("help should be returned as a Clap display error");
        // Convert the rendered help into a string so the test can inspect it.
        let rendered_help = error.to_string();

        // Confirm that service-principal mode is visible in the help output.
        assert!(rendered_help.contains("--service-principal"));
        // Confirm that users can discover the client ID flag from the help output.
        assert!(rendered_help.contains("--client-id"));
        // Confirm that users can discover the client secret flag from the help output.
        assert!(rendered_help.contains("--client-secret"));
        // Confirm that the optional tenant positional argument is visible in the usage line.
        assert!(rendered_help.contains("[TENANT]"));
    }

    #[test]
    fn parses_help_as_a_real_command() {
        // Parse the explicit help command that users can type inside the shell.
        let parsed_command = parse_command(&[String::from("help")]).expect("command should parse");

        // Confirm that help is represented as its own typed variant.
        assert!(matches!(parsed_command, AzureCommand::Help));
    }

    #[test]
    fn parses_inventory_generate_as_a_real_command() {
        // Parse the inventory generation command that writes a new report.
        let parsed_command = parse_command(&[String::from("inventory"), String::from("generate")])
            .expect("command should parse");

        // Confirm that Clap routes the nested command to the generate variant.
        assert!(matches!(
            parsed_command,
            AzureCommand::Inventory(InventoryCommand::Generate)
        ));
    }

    #[test]
    fn parses_inventory_list_as_a_real_command() {
        // Parse the inventory list command that shows saved reports.
        let parsed_command = parse_command(&[String::from("inventory"), String::from("list")])
            .expect("command should parse");

        // Confirm that Clap routes the nested command to the list variant.
        assert!(matches!(
            parsed_command,
            AzureCommand::Inventory(InventoryCommand::List)
        ));
    }

    #[test]
    fn parses_inventory_ls_as_an_alias_for_list() {
        // Parse the short inventory alias that should behave like `inventory list`.
        let parsed_command = parse_command(&[String::from("inventory"), String::from("ls")])
            .expect("command should parse");

        // Confirm that the alias resolves to the same typed list variant.
        assert!(matches!(
            parsed_command,
            AzureCommand::Inventory(InventoryCommand::List)
        ));
    }

    #[test]
    fn parses_snapshot_generate_as_a_real_command() {
        // Parse the snapshot generation command that writes a new JSON file.
        let parsed_command = parse_command(&[String::from("snapshot"), String::from("generate")])
            .expect("command should parse");

        // Confirm that Clap routes the nested command to the generate variant.
        assert!(matches!(
            parsed_command,
            AzureCommand::Snapshot(SnapshotCommand::Generate)
        ));
    }

    #[test]
    fn snapshot_without_a_subcommand_shows_snapshot_help() {
        // Parse the parent snapshot command without the required nested command.
        let error = parse_command(&[String::from("snapshot")])
            .expect_err("missing subcommand should show help");
        // Render the Clap error into text so we can inspect the message.
        let rendered_error = error.to_string();

        // Confirm that Clap reports intentional help output for the missing subcommand.
        assert_eq!(
            error.kind(),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
        // Confirm that the snapshot command help shows the nested command usage.
        assert!(rendered_error.contains("Usage: azure snapshot <COMMAND>"));
        // Confirm that the snapshot command help lists the generate subcommand.
        assert!(rendered_error.contains("generate"));
    }

    #[test]
    fn inventory_without_a_subcommand_shows_inventory_help() {
        // Parse the parent inventory command without the required nested command.
        let error = parse_command(&[String::from("inventory")])
            .expect_err("missing subcommand should show help");
        // Render the Clap error into text so we can inspect the message.
        let rendered_error = error.to_string();

        // Confirm that Clap reports intentional help output for the missing subcommand.
        assert_eq!(
            error.kind(),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
        // Confirm that the inventory command help shows the nested command usage.
        assert!(rendered_error.contains("Usage: azure inventory <COMMAND>"));
        // Confirm that the inventory command help lists the generate subcommand.
        assert!(rendered_error.contains("generate"));
        // Confirm that the inventory command help lists the list subcommand.
        assert!(rendered_error.contains("list"));
    }

    #[test]
    fn inventory_list_help_is_reported_as_display_help() {
        // Parse `inventory list -h`, which Clap represents as a help display request.
        let error = parse_command(&[
            String::from("inventory"),
            String::from("list"),
            String::from("-h"),
        ])
        .expect_err("help should be returned as a Clap display error");

        // Confirm that the parser reports intentional help output, not a real parse failure.
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    }

    #[test]
    fn inventory_list_help_describes_saved_report_listing() {
        // Ask Clap to render the help text for the inventory list subcommand.
        let error = parse_command(&[
            String::from("inventory"),
            String::from("list"),
            String::from("-h"),
        ])
        .expect_err("help should be returned as a Clap display error");
        // Convert the rendered help into a string so the test can inspect it.
        let rendered_help = error.to_string();

        // Confirm that the help text describes listing saved inventory reports.
        assert!(rendered_help.contains("List saved Azure inventory reports"));
    }
}
