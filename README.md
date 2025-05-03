# clamp

`clamp` is a Rust CLI tool for processing text templates (`.clamp` files), designed primarily for managing prompts for generative AI. It allows including external files directly into a template and uses a lockfile (`.clamp.lock`) to track changes in these included files.

## Workflow

1.  Create your `.clamp` template with `[[include: ...]]` directives pointing to your source files.
2.  Run `clamp my_prompt.clamp`. The processed prompt prints to stdout, and changes (initially, all files are "Added") are reported to stderr.
3.  Run `clamp update-lock my_prompt.clamp` to create the initial lockfile.
4.  Use the generated prompt with your AI.
5.  Modify the included source files based on feedback or development.
6.  Run `clamp my_prompt.clamp` again. The *updated* prompt prints to stdout, and stderr now reports exactly which included files have changed (Modified, Added, Removed) since the last `update-lock`.
7.  Repeat steps 4-6 as needed.
8.  When the sources are stable for the current prompt version, run `clamp update-lock my_prompt.clamp` again to lock the new state.

## Template Syntax

Use the `[[include: path]]` directive within your `.clamp` file. The `path` should be relative to the location of the `.clamp` file itself.

**Example `my_prompt.clamp`:**

```text
Here is the master prompt context:

[[include: src/main.rs]]

And here is the helper library:

[[include: src/utils.rs]]

Please analyze the above code.
```

## Features

*   **Template Processing:** Reads `.clamp` files and replaces `[[include: path/to/file.ext]]` directives.
*   **File Inclusion:** Includes the content of specified files, wrapping them in markdown code blocks with language hints based on file extensions.
*   **Change Tracking:** Generates a `.clamp.lock` file containing SHA256 hashes of all included files.
*   **Status Reporting:** Compares the current state of included files against the lockfile and reports Added, Modified, or Removed files.
*   **Shell Completions:** Generates completion scripts for common shells (Bash, Zsh, Fish, etc.).

## Installing

First install Rust from [rustup.rs](https://rustup.rs).

```bash
cargo install --git https://github.com/krinistof/clam
```

