use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fmt::Write,
    fs, io,
    path::{Path, PathBuf}, process::ExitCode,
};

/// Represents the data stored in the .clamp.lock file.
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct LockfileData {
    pub files: BTreeMap<PathBuf, String>, // Canonicalized Path -> SHA256 Hash (hex string)
}

/// Represents the result of processing a template.
#[derive(Debug)]
pub struct ProcessResult {
    /// The final generated content after includes are resolved.
    pub output_content: String,
    /// Map of included files (canonicalized paths) and their *current* SHA256 hashes.
    pub current_hashes: BTreeMap<PathBuf, String>,
}

/// Represents the status of a file compared to the lockfile.
#[derive(Debug, PartialEq, Eq, Clone, Copy)] // Added Clone, Copy for potential future use
pub enum ChangeStatus {
    Unchanged,
    Modified,
    Added,   // Present now, but not in lockfile.
    Removed, // Present in lockfile, but not included now.
}

/// Calculates the SHA256 hash of byte content and returns it as a hex string.
fn calculate_hash(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    let result = hasher.finalize();
    hex::encode(result)
}

/// Processes a .clamp template file.
///
/// Reads the template, resolves `[[include: path]]` directives relative to the template's
/// directory, calculates hashes of included files, and returns the final content
/// along with the map of included files and their current hashes.
///
/// Included file paths are resolved relative to the directory containing the template file.
/// Included file content is assumed to be UTF-8 and is wrapped in markdown code blocks
/// (e.g., ```rust ... ```) in the output.
///
/// Returns an error if the template or any included file cannot be read, or if an
/// included file path does not exist, or if included content is not valid UTF-8.
pub fn process_template(template_path: &Path) -> Result<ProcessResult> {
    let template_content = fs::read_to_string(template_path)
        .with_context(|| format!("Failed to read template file '{}'", template_path.display()))?;

    let base_dir = template_path
        .parent()
        .context("Template path must have a parent directory")?;

    // regex for [[include: path/to/file.ext]], allowing whitespace around the path.
    let include_regex =
        Regex::new(r"\[\[include:\s*(.*?)\s*\]\]").expect("Failed to compile include regex");

    let mut output_buffer = String::with_capacity(template_content.len());
    let mut current_pos = 0;
    let mut current_hashes = BTreeMap::new();

    for cap in include_regex.captures_iter(&template_content) {
        let full_match = cap.get(0).unwrap(); // The whole [[include: ...]]
        let path_match = cap.get(1).unwrap(); // The path inside
        let relative_path_str = path_match.as_str().trim(); // Trim whitespace just in case

        // append text before the match
        output_buffer.push_str(&template_content[current_pos..full_match.start()]);

        let include_path = base_dir.join(relative_path_str);

        if !include_path.exists() {
            bail!(
                "Include directive error: File not found at resolved path '{}' (referenced in '{}' as '{}')",
                include_path.display(),
                template_path.display(),
                relative_path_str
            );
        }
        let canonical_path = fs::canonicalize(&include_path).with_context(|| {
            format!(
                "Failed to canonicalize include path '{}'",
                include_path.display()
            )
        })?;

        let included_content_bytes = fs::read(&canonical_path).with_context(|| {
            format!(
                "Failed to read included file '{}'",
                canonical_path.display()
            )
        })?;

        let hash = calculate_hash(&included_content_bytes);

        current_hashes.insert(canonical_path.clone(), hash); // Clone path for insertion

        let content_str = String::from_utf8(included_content_bytes).with_context(|| {
            format!(
                "Included file '{}' does not contain valid UTF-8 content",
                canonical_path.display()
            )
        })?;

        let lang_hint = include_path
            .extension()
            .and_then(|os_str| os_str.to_str())
            .unwrap_or("");

        // Format and append the included content block
        // Use writeln! style formatting for clarity if multi-line
        write!(output_buffer, "```{lang_hint}\n{content_str}\n```\n")
            .expect("Writing to String buffer failed unexpectedly");

        current_pos = full_match.end();
    }

    // append remaining text after the last include
    output_buffer.push_str(&template_content[current_pos..]);

    Ok(ProcessResult {
        output_content: output_buffer,
        current_hashes,
    })
}

/// Reads and deserializes the lockfile. Returns default (empty) if not found.
pub fn read_lockfile(lockfile_path: &Path) -> Result<LockfileData> {
    if !lockfile_path.exists() {
        eprintln!(
            "Warning: Lockfile '{}' not found. Treating all includes as added.",
            lockfile_path.display()
        );
        return Ok(LockfileData {
            files: std::collections::BTreeMap::new(),
        });
    }

    match fs::read_to_string(lockfile_path) {
        Ok(content) => toml::from_str(&content).with_context(|| {
            format!(
                "Failed to parse TOML from lockfile '{}'",
                lockfile_path.display()
            )
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Ok(LockfileData::default()) // Return empty data if lockfile doesn't exist
        }
        Err(e) => {
            Err(e).with_context(|| format!("Failed to read lockfile '{}'", lockfile_path.display()))
        }
    }
}

/// Serializes lockfile data to TOML and writes it to the specified path.
pub fn write_lockfile(lockfile_path: &Path, data: &LockfileData) -> Result<()> {
    let toml_content =
        toml::to_string_pretty(data).context("Failed to serialize lockfile data to TOML")?;
    fs::write(lockfile_path, toml_content)
        .with_context(|| format!("Failed to write lockfile to '{}'", lockfile_path.display()))?;
    Ok(())
}

/// Compares current file hashes with locked hashes and identifies changes.
/// Returns a map of changed paths to their status (Modified, Added, Removed).
pub fn compare_hashes(
    current_hashes: &BTreeMap<PathBuf, String>,
    locked_hashes: &BTreeMap<PathBuf, String>,
) -> BTreeMap<PathBuf, ChangeStatus> {
    let mut changes = BTreeMap::new();

    // Check files currently included
    for (path, current_hash) in current_hashes {
        match locked_hashes.get(path) {
            Some(locked_hash) => {
                if current_hash != locked_hash {
                    changes.insert(path.clone(), ChangeStatus::Modified);
                }
                // Implicitly Unchanged if hashes match, not added to 'changes' map
            }
            None => {
                // File is included now, but wasn't in the lockfile
                changes.insert(path.clone(), ChangeStatus::Added);
            }
        }
    }

    // Check for files that were in the lockfile but are no longer included
    for path in locked_hashes.keys() {
        if !current_hashes.contains_key(path) {
            changes.insert(path.clone(), ChangeStatus::Removed);
        }
    }

    changes
}

/// Generates the path for the lock file based on the template file path.
/// E.g., `my_template.clamp` -> `my_template.clamp.lock`
pub fn get_lockfile_path(template_path: &Path) -> PathBuf {
    // Prefer .clamp.lock extension if possible, otherwise just .lock
    let extension = template_path
        .extension()
        .map(|ext| ext.to_string_lossy().to_string() + ".lock")
        .unwrap_or_else(|| "lock".to_string());

    template_path.with_extension(extension)
}

/// Writes a sample .clamp file to given path, othervise `problem.clamp`
pub fn init(new: Option<PathBuf>) -> Result<ExitCode> {
    const SAMPLE: &str = "
TL;DR how to use this?

[[include: README.md]]";

    let path = new.unwrap_or("problem.clamp".into());
    fs::write(path, SAMPLE)?;
    Ok(ExitCode::SUCCESS)
}
