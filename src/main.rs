use anyhow::{Context, Result, anyhow};
use clamp_lib::{
    LockfileData, compare_hashes, get_lockfile_path, process_template, read_lockfile,
    write_lockfile, init
};
use clap::Parser;
use clap_complete::{Shell, generate};
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Parser, Debug)]
#[clap(
    name = "clamp",
    author = "Kristóf Kovács <kristof@mntr.dev>",
    about = "Processes .clamp template files, manages includes, and tracks changes via a lockfile.",
    long_about = None,
    arg_required_else_help = true
)]
struct Cli {
    #[clap(subcommand)]
    command: Option<Commands>,

    /// The .clamp template file to process (default action: build and check)
    /// Only used if no subcommand is provided.
    #[clap(value_parser)]
    template_path_if_no_command: Option<PathBuf>,
}

#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// Update the lock file for a given template with the current state of its includes
    UpdateLock {
        /// The .clamp template file
        #[clap(value_parser, required = true)]
        template_path: PathBuf,
    },

    /// Generate shell completion scripts
    Completions {
        /// The shell to generate completions for
        #[clap(value_parser = clap::value_parser!(Shell))]
        shell: Shell,
    },

    /// Create a sample .clamp file
    Init {
        /// Path to where create sample .clamp
        #[clap(value_parser)]
        new: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Some(Commands::UpdateLock { template_path }) => {
            if cli.template_path_if_no_command.is_some() {
                eprintln!(
                    "Error: Cannot provide both 'update-lock' subcommand and a default template path."
                );
                return ExitCode::FAILURE;
            }
            run_update_lock(&template_path)
        }
        Some(Commands::Completions { shell }) => {
            if cli.template_path_if_no_command.is_some() {
                eprintln!(
                    "Error: Cannot provide both 'completions' subcommand and a default template path."
                );
                return ExitCode::FAILURE;
            }
            run_generate_completions(shell)
        }
        Some(Commands::Init { new }) => {
            init(new)
        }
        // Example if you add an explicit Build command:
        // Some(Commands::Build { template_path }) => { ... }
        None => match cli.template_path_if_no_command {
            Some(template_path) => run_build_check(&template_path),
            None => {
                eprintln!("Error: No command specified and no template file provided.");
                eprintln!("\nUsage: clamp <TEMPLATE_PATH>");
                eprintln!("   or: clamp <COMMAND> --help");
                return ExitCode::FAILURE;
            }
        },
    };

    match result {
        Ok(exit_code) => exit_code,
        Err(e) => {
            eprintln!("Error: {e}");
            let mut cause = e.source();
            while let Some(source) = cause {
                eprintln!("  Caused by: {source}");
                cause = source.source();
            }
            ExitCode::FAILURE
        }
    }
}

/// Implements the default action: build template, print to stdout, check against lockfile.
fn run_build_check(template_path: &Path) -> Result<ExitCode> {
    // 1. Process the template
    let process_result = process_template(template_path).map_err(|e| {
        anyhow!(e).context(format!(
            "Failed to process template '{}'",
            template_path.display()
        ))
    })?;

    // 2. Determine and read the lock file
    let lockfile_path = get_lockfile_path(template_path);
    let lockfile_data = read_lockfile(&lockfile_path)?;

    // 3. Compare current state with lock file state
    let changes = compare_hashes(&process_result.current_hashes, &lockfile_data.files);

    // 4. Print the processed template content to stdout
    if let Err(e) = io::stdout().write_all(process_result.output_content.as_bytes()) {
        eprintln!("Error writing output to stdout: {e}");
        return Err(anyhow!(e).context("Failed to write processed template to stdout"));
    }
    io::stdout().flush().context("Failed to flush stdout")?;

    // 5. Report status to stderr and determine exit code
    if changes.is_empty() {
        eprintln!(
            "Status: No changes detected relative to lockfile '{}'.",
            lockfile_path.display()
        );
        Ok(ExitCode::SUCCESS) // 0 for no changes
    } else {
        eprintln!(
            "Status: Changes detected relative to lockfile '{}':",
            lockfile_path.display()
        );
        for (path, status) in changes {
            use clamp_lib::ChangeStatus::*;
            let status_str = match status {
                Modified => "Modified",
                Added => "Added",
                Removed => "Removed",
                _ => unreachable!(),
            };
            eprintln!("  - {}: {}", status_str, path.display());
        }
        Ok(ExitCode::from(1)) // 1 for changes detected
    }
}

/// Implements the `update-lock` command.
fn run_update_lock(template_path: &Path) -> Result<ExitCode> {
    // 1. Process the template to get current includes and hashes
    let process_result = process_template(template_path).map_err(|e| {
        anyhow!(e).context(format!(
            "Failed to process template '{}' for lock update",
            template_path.display()
        ))
    })?;

    // 2. Prepare lockfile data
    let new_lockfile_data = LockfileData {
        files: process_result.current_hashes, // Use the freshly calculated hashes
    };

    // 3. Determine lockfile path and write it
    let lockfile_path = get_lockfile_path(template_path);
    write_lockfile(&lockfile_path, &new_lockfile_data).map_err(|e| {
        anyhow!(e).context(format!(
            "Failed to write lockfile '{}'",
            lockfile_path.display()
        ))
    })?;

    eprintln!(
        "Status: Lockfile '{}' updated successfully.",
        lockfile_path.display()
    );
    Ok(ExitCode::SUCCESS) // 0 for success
}

/// Implements the `completions` command.
fn run_generate_completions(shell: Shell) -> Result<ExitCode> {
    eprintln!("Generating completions for {shell:?}...");
    let mut cmd = <Cli as clap::CommandFactory>::command();
    let bin_name = cmd.get_name().to_string();

    generate(shell, &mut cmd, bin_name, &mut io::stdout());

    Ok(ExitCode::SUCCESS)
}
