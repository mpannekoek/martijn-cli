// Import Clap helpers so this module can describe the Azure command model.
use clap::{Args, Subcommand};

// Describe the argument shape for one Azure command line in tests.
#[cfg(test)]
#[derive(clap::Parser, Debug)]
#[command(name = "azure", disable_help_subcommand = true)]
struct AzureCli {
    // Store the Azure subcommand that Clap parsed from command-line arguments.
    #[command(subcommand)]
    command: AzureCommand,
}

// List the Azure commands that the non-interactive CLI understands.
#[derive(Subcommand, Debug)]
pub(crate) enum AzureCommand {
    /// Login to Azure CLI as a user or service principal.
    Login(LoginArguments),
    /// Logout from Azure CLI and clear the cached account information.
    Logout,
    /// Show the current Azure login state.
    Status,
    /// Show Azure inventory data and optionally save Markdown reports.
    #[command(subcommand, arg_required_else_help = true)]
    Inventory(InventoryCommand),
    /// Create, list, and delete JSON snapshots.
    #[command(subcommand, arg_required_else_help = true)]
    Snapshot(SnapshotCommand),
    /// List, show, and delete saved inventory reports.
    #[command(subcommand, arg_required_else_help = true)]
    Report(ReportCommand),
}

// List the commands that belong under the Azure `inventory` command group.
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub(crate) enum InventoryCommand {
    /// Work with Azure resource inventory.
    #[command(name = "resource")]
    #[command(subcommand, arg_required_else_help = true)]
    Resources(InventoryResourcesCommand),
    /// Work with Azure resource-group inventory.
    #[command(name = "group")]
    #[command(subcommand, arg_required_else_help = true)]
    Groups(InventoryGroupsCommand),
}

// List the commands that belong under `inventory resource`.
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub(crate) enum InventoryResourcesCommand {
    /// Print Azure resources as a compact list.
    List(SaveArguments),
    /// Print Azure resources grouped as a tree.
    Tree(SaveArguments),
}

// List the commands that belong under `inventory group`.
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub(crate) enum InventoryGroupsCommand {
    /// Print Azure resource groups as a compact list.
    List(SaveArguments),
}

// List the commands that belong under the Azure `snapshot` command group.
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub(crate) enum SnapshotCommand {
    /// Create a new JSON snapshot.
    #[command(subcommand, arg_required_else_help = true)]
    Create(SnapshotCreateCommand),
    /// List saved Azure snapshots.
    List,
    /// Delete one saved Azure snapshot by file name or stem.
    Delete { name: String },
}

// List the commands that belong under `snapshot create`.
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub(crate) enum SnapshotCreateCommand {
    /// Create a resource snapshot.
    #[command(name = "resource")]
    Resources,
    /// Create a resource-group snapshot.
    #[command(name = "group")]
    Groups,
    /// Create both resource and resource-group snapshots.
    All,
}

// List the commands that belong under the Azure `report` command group.
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReportCommand {
    /// List saved Azure inventory reports.
    List,
    /// Print one saved Azure inventory report.
    Show { name: String },
    /// Delete one saved Azure inventory report.
    Delete { name: String },
}

// Hold the optional `--save` value used by inventory commands.
#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub(crate) struct SaveArguments {
    // Accept an optional snapshot file name or stem for local snapshot-based inventory.
    #[arg(long)]
    pub(crate) snapshot: Option<String>,
    // Accept `--save` without a value and `--save <name>` with a custom name.
    #[arg(long, num_args = 0..=1, default_missing_value = "")]
    pub(crate) save: Option<String>,
}

// Hold the arguments that belong to the `login` subcommand.
#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoginArguments {
    // Switch to service-principal authentication instead of interactive user login.
    #[arg(long = "service-principal")]
    pub(crate) service_principal: bool,
    // Accept an optional client ID for service-principal login.
    #[arg(long = "client-id", requires = "service_principal")]
    pub(crate) client_id: Option<String>,
    // Accept an optional client secret for service-principal login.
    #[arg(long = "client-secret", requires = "service_principal")]
    pub(crate) client_secret: Option<String>,
    // Accept an optional tenant for both login modes.
    pub(crate) tenant: Option<String>,
}

// Convert tokenized Azure input into one typed command for focused parser tests.
#[cfg(test)]
pub(super) fn parse_command(tokens: &[String]) -> Result<AzureCommand, clap::Error> {
    // Import Clap's parser trait only for this test helper.
    use clap::Parser;

    // Start with a fake binary name because Clap expects argv-style input.
    let arguments = std::iter::once(String::from("azure")).chain(tokens.iter().cloned());
    // Ask Clap to parse the test command line into the Azure parser struct.
    let cli = AzureCli::try_parse_from(arguments)?;
    // Return only the subcommand because the command runner receives that shape.
    Ok(cli.command)
}

#[cfg(test)]
mod tests {
    // Import Clap's error kind enum so tests can distinguish help from real parse failures.
    use clap::error::ErrorKind;

    // Import the Azure parser and command types so the tests can validate command behavior.
    use super::{
        AzureCommand, InventoryCommand, InventoryGroupsCommand, InventoryResourcesCommand,
        LoginArguments, ReportCommand, SaveArguments, SnapshotCommand, SnapshotCreateCommand,
        parse_command,
    };

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
    fn parses_inventory_resource_list_without_save() {
        // Parse the resource list command without saving a report.
        let parsed_command = parse_command(&[
            String::from("inventory"),
            String::from("resource"),
            String::from("list"),
        ])
        .expect("command should parse");

        // Confirm that Clap routes the nested command to the resource list variant.
        assert!(matches!(
            parsed_command,
            AzureCommand::Inventory(InventoryCommand::Resources(
                InventoryResourcesCommand::List(SaveArguments {
                    snapshot: None,
                    save: None
                })
            ))
        ));
    }

    #[test]
    fn parses_inventory_resource_list_with_snapshot_name() {
        // Parse the resource list command with an explicit snapshot selector.
        let parsed_command = parse_command(&[
            String::from("inventory"),
            String::from("resource"),
            String::from("list"),
            String::from("--snapshot"),
            String::from("20260428-171547-fc400f1b"),
        ])
        .expect("command should parse");

        // Confirm that Clap stores the requested snapshot stem.
        assert!(matches!(
            parsed_command,
            AzureCommand::Inventory(InventoryCommand::Resources(
                InventoryResourcesCommand::List(SaveArguments {
                    snapshot: Some(snapshot),
                    save: None
                })
            )) if snapshot == "20260428-171547-fc400f1b"
        ));
    }

    #[test]
    fn parses_inventory_resource_tree_with_bare_save() {
        // Parse the resource tree command with `--save` but no custom name.
        let parsed_command = parse_command(&[
            String::from("inventory"),
            String::from("resource"),
            String::from("tree"),
            String::from("--save"),
        ])
        .expect("command should parse");

        // Confirm that Clap stores an empty string for the optional save value.
        assert!(matches!(
            parsed_command,
            AzureCommand::Inventory(InventoryCommand::Resources(
                InventoryResourcesCommand::Tree(SaveArguments {
                    snapshot: None,
                    save: Some(name)
                })
            )) if name.is_empty()
        ));
    }

    #[test]
    fn parses_inventory_resource_tree_with_snapshot_and_save() {
        // Parse the resource tree command with both snapshot selection and report saving.
        let parsed_command = parse_command(&[
            String::from("inventory"),
            String::from("resource"),
            String::from("tree"),
            String::from("--snapshot"),
            String::from("daily-snapshot"),
            String::from("--save"),
            String::from("daily tree"),
        ])
        .expect("command should parse");

        // Confirm that Clap keeps both independent option values.
        assert!(matches!(
            parsed_command,
            AzureCommand::Inventory(InventoryCommand::Resources(
                InventoryResourcesCommand::Tree(SaveArguments {
                    snapshot: Some(snapshot),
                    save: Some(name)
                })
            )) if snapshot == "daily-snapshot" && name == "daily tree"
        ));
    }

    #[test]
    fn parses_inventory_group_list_with_named_save() {
        // Parse the group list command with a custom report name.
        let parsed_command = parse_command(&[
            String::from("inventory"),
            String::from("group"),
            String::from("list"),
            String::from("--save"),
            String::from("daily report"),
        ])
        .expect("command should parse");

        // Confirm that Clap keeps the provided report name.
        assert!(matches!(
            parsed_command,
            AzureCommand::Inventory(InventoryCommand::Groups(
                InventoryGroupsCommand::List(SaveArguments {
                    snapshot: None,
                    save: Some(name)
                })
            )) if name == "daily report"
        ));
    }

    #[test]
    fn parses_snapshot_create_resource_as_a_real_command() {
        // Parse the snapshot command that writes a resource snapshot.
        let parsed_command = parse_command(&[
            String::from("snapshot"),
            String::from("create"),
            String::from("resource"),
        ])
        .expect("command should parse");

        // Confirm that Clap routes the nested command to the create resource variant.
        assert!(matches!(
            parsed_command,
            AzureCommand::Snapshot(SnapshotCommand::Create(SnapshotCreateCommand::Resources))
        ));
    }

    #[test]
    fn parses_snapshot_create_group_as_a_real_command() {
        // Parse the snapshot command that writes a group snapshot.
        let parsed_command = parse_command(&[
            String::from("snapshot"),
            String::from("create"),
            String::from("group"),
        ])
        .expect("command should parse");

        // Confirm that Clap routes the nested command to the create group variant.
        assert!(matches!(
            parsed_command,
            AzureCommand::Snapshot(SnapshotCommand::Create(SnapshotCreateCommand::Groups))
        ));
    }

    #[test]
    fn parses_snapshot_create_all_as_a_real_command() {
        // Parse the snapshot command that writes both snapshot types.
        let parsed_command = parse_command(&[
            String::from("snapshot"),
            String::from("create"),
            String::from("all"),
        ])
        .expect("command should parse");

        // Confirm that Clap routes the nested command to the create all variant.
        assert!(matches!(
            parsed_command,
            AzureCommand::Snapshot(SnapshotCommand::Create(SnapshotCreateCommand::All))
        ));
    }

    #[test]
    fn parses_snapshot_list_as_a_real_command() {
        // Parse the snapshot list command.
        let parsed_command = parse_command(&[String::from("snapshot"), String::from("list")])
            .expect("command should parse");

        // Confirm that Clap routes the command to the list variant.
        assert!(matches!(
            parsed_command,
            AzureCommand::Snapshot(SnapshotCommand::List)
        ));
    }

    #[test]
    fn parses_snapshot_delete_with_name() {
        // Parse the snapshot delete command with a target name.
        let parsed_command = parse_command(&[
            String::from("snapshot"),
            String::from("delete"),
            String::from("daily"),
        ])
        .expect("command should parse");

        // Confirm that Clap keeps the provided delete name.
        assert!(matches!(
            parsed_command,
            AzureCommand::Snapshot(SnapshotCommand::Delete { name }) if name == "daily"
        ));
    }

    #[test]
    fn parses_report_commands() {
        // Parse the report list command.
        let list_command = parse_command(&[String::from("report"), String::from("list")])
            .expect("report list should parse");
        // Parse the report show command.
        let show_command = parse_command(&[
            String::from("report"),
            String::from("show"),
            String::from("daily"),
        ])
        .expect("report show should parse");
        // Parse the report delete command.
        let delete_command = parse_command(&[
            String::from("report"),
            String::from("delete"),
            String::from("daily"),
        ])
        .expect("report delete should parse");

        // Confirm that report list reaches the list variant.
        assert!(matches!(
            list_command,
            AzureCommand::Report(ReportCommand::List)
        ));
        // Confirm that report show keeps the provided name.
        assert!(matches!(
            show_command,
            AzureCommand::Report(ReportCommand::Show { name }) if name == "daily"
        ));
        // Confirm that report delete keeps the provided name.
        assert!(matches!(
            delete_command,
            AzureCommand::Report(ReportCommand::Delete { name }) if name == "daily"
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
        // Confirm that the snapshot command help lists the create subcommand.
        assert!(rendered_error.contains("create"));
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
        // Confirm that the inventory command help lists the resource subcommand.
        assert!(rendered_error.contains("resource"));
        // Confirm that the inventory command help lists the group subcommand.
        assert!(rendered_error.contains("group"));
    }

    #[test]
    fn rejects_plural_resource_and_group_subcommands() {
        // Parse the old plural inventory resource command.
        let inventory_resource_error = parse_command(&[
            String::from("inventory"),
            String::from("resources"),
            String::from("list"),
        ])
        .expect_err("plural inventory resource command should fail");
        // Parse the old plural inventory group command.
        let inventory_group_error = parse_command(&[
            String::from("inventory"),
            String::from("groups"),
            String::from("list"),
        ])
        .expect_err("plural inventory group command should fail");
        // Parse the old plural snapshot resource command.
        let snapshot_resource_error = parse_command(&[
            String::from("snapshot"),
            String::from("create"),
            String::from("resources"),
        ])
        .expect_err("plural snapshot resource command should fail");
        // Parse the old plural snapshot group command.
        let snapshot_group_error = parse_command(&[
            String::from("snapshot"),
            String::from("create"),
            String::from("groups"),
        ])
        .expect_err("plural snapshot group command should fail");

        // Confirm that Clap treats the old inventory resource command as an unknown subcommand.
        assert_eq!(
            inventory_resource_error.kind(),
            ErrorKind::InvalidSubcommand
        );
        // Confirm that Clap treats the old inventory group command as an unknown subcommand.
        assert_eq!(inventory_group_error.kind(), ErrorKind::InvalidSubcommand);
        // Confirm that Clap treats the old snapshot resource command as an unknown subcommand.
        assert_eq!(snapshot_resource_error.kind(), ErrorKind::InvalidSubcommand);
        // Confirm that Clap treats the old snapshot group command as an unknown subcommand.
        assert_eq!(snapshot_group_error.kind(), ErrorKind::InvalidSubcommand);
    }

    #[test]
    fn inventory_resource_list_help_is_reported_as_display_help() {
        // Parse `inventory resource list -h`, which Clap represents as a help display request.
        let error = parse_command(&[
            String::from("inventory"),
            String::from("resource"),
            String::from("list"),
            String::from("-h"),
        ])
        .expect_err("help should be returned as a Clap display error");

        // Confirm that the parser reports intentional help output, not a real parse failure.
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    }

    #[test]
    fn inventory_resource_list_help_describes_resource_listing() {
        // Ask Clap to render the help text for the inventory resource list subcommand.
        let error = parse_command(&[
            String::from("inventory"),
            String::from("resource"),
            String::from("list"),
            String::from("-h"),
        ])
        .expect_err("help should be returned as a Clap display error");
        // Convert the rendered help into a string so the test can inspect it.
        let rendered_help = error.to_string();

        // Confirm that the help text describes listing Azure resources.
        assert!(rendered_help.contains("Print Azure resources as a compact list"));
    }
}
