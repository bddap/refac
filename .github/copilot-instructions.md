# Refac: AI-Powered Text Transformation CLI

**CRITICAL**: Always reference these instructions first and fallback to search or bash commands only when you encounter unexpected information that does not match the info here.

Refac is a Rust CLI tool that transforms text using OpenAI's API. Users select text, run the command with natural language instructions, and get AI-transformed results with sassy comments.

## Working Effectively

### Bootstrap and Build
Always run these commands in sequence to set up the development environment:

```bash
# Check that code compiles (quick validation)
cargo check
# NEVER CANCEL: Takes up to 60 seconds on first run. Set timeout to 120+ seconds.

# Build debug version
cargo build  
# NEVER CANCEL: Takes up to 30 seconds. Set timeout to 60+ seconds.

# Build optimized release version
cargo build --release
# NEVER CANCEL: Takes up to 60 seconds. Set timeout to 120+ seconds.
```

### Testing and Linting
Always run these validation steps before committing changes:

```bash
# Format code (fixes formatting automatically)
cargo fmt

# Check formatting without changes
cargo fmt --check

# Run Clippy linter
cargo clippy
# NEVER CANCEL: Takes up to 10 seconds. Set timeout to 30+ seconds.

# Run tests (currently no tests exist, but command works)
cargo test
# Takes under 5 seconds. Set timeout to 30+ seconds.
```

### Installation and Local Testing
To install and test the tool locally:

```bash
# Install from source
cargo install --path . --force
# NEVER CANCEL: Takes up to 70 seconds. Set timeout to 120+ seconds.

# Test basic functionality
refac --version
refac --help
refac tor --help

# Test without API key (should show error about missing secrets.toml)
refac tor "hello world" "capitalize"
```

## Validation

### Manual Testing Requirements
ALWAYS manually validate changes by running through these scenarios:

1. **Build Validation**: Run `cargo build --release` and ensure it succeeds
2. **CLI Help Testing**: Run `refac --help`, `refac login --help`, `refac tor --help`
3. **Error Handling**: Run `refac tor "test" "transform"` without API key to verify error message
4. **Installation**: Run `cargo install --path . --force` to verify installability

### Critical Timeout Settings
- **cargo check**: 120 seconds (first run can take 60+ seconds)
- **cargo build**: 60 seconds (typically 20-30 seconds)
- **cargo build --release**: 120 seconds (typically 50-60 seconds)  
- **cargo install**: 120 seconds (typically 50-70 seconds)
- **cargo clippy**: 30 seconds (typically under 10 seconds)
- **cargo test**: 30 seconds (currently no tests, runs instantly)

### Formatting and Style
- Always run `cargo fmt` before committing
- Always run `cargo clippy` and fix any warnings before committing
- Use standard Rust conventions and idioms

## Key Project Structure

### Repository Root
```
.
├── .git/
├── .gitignore          # Excludes /target, /tmp
├── Cargo.toml          # Main project configuration
├── Cargo.lock          # Dependency lock file
├── README.md           # User documentation and examples
├── LICENSE-APACHE.txt  # Apache 2.0 license
├── LICENSE-MIT.txt     # MIT license
└── src/                # Source code directory
    ├── main.rs         # CLI entry point and main logic
    ├── api.rs          # OpenAI API request/response types
    ├── api_client.rs   # HTTP client for OpenAI API
    ├── config_files.rs # Configuration and secrets management
    └── prompt.rs       # AI prompt templates and examples
```

### Important Files
- **src/main.rs**: Contains CLI argument parsing, main application logic, and the `refactor()` function
- **src/config_files.rs**: Handles loading/saving API keys and configuration using XDG directories
- **src/prompt.rs**: Contains system prompts and example transformations for the AI model
- **src/api.rs**: Defines OpenAI API request/response structures
- **src/api_client.rs**: HTTP client implementation for API calls
- **Cargo.toml**: Dependencies include clap, reqwest, serde, anyhow, and OpenAI-related crates

## Common Commands and Expected Output

### Repository Status
```bash
$ ls -la
total 44
drwxr-xr-x  4 user user  4096 Dec 19 10:00 .
drwxr-xr-x  3 user user  4096 Dec 19 10:00 ..
drwxr-xr-x  8 user user  4096 Dec 19 10:00 .git
-rw-r--r--  1 user user    11 Dec 19 10:00 .gitignore
-rw-r--r--  1 user user  1134 Dec 19 10:00 Cargo.lock
-rw-r--r--  1 user user   932 Dec 19 10:00 Cargo.toml
-rw-r--r--  1 user user 11357 Dec 19 10:00 LICENSE-APACHE.txt
-rw-r--r--  1 user user  1075 Dec 19 10:00 LICENSE-MIT.txt
-rw-r--r--  1 user user  8751 Dec 19 10:00 README.md
drwxr-xr-x  2 user user  4096 Dec 19 10:00 src
```

### Help Output
```bash
$ refac --help
Transform some text given a generic natural language prompt.

Usage: refac <COMMAND>

Commands:
  login  Save your openai api key for future use
  tor    Apply the instructions encoded in `transform` to the text in `selected`. Get it? 'refac tor'
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

### Build Success Indicators
- `cargo check`: Should end with "Finished `dev` profile [unoptimized + debuginfo] target(s) in X.XXs"
- `cargo build`: Should end with "Finished `dev` profile [unoptimized + debuginfo] target(s) in X.XXs"
- `cargo build --release`: Should end with "Finished `release` profile [optimized] target(s) in X.XXs"
- No tests exist, so `cargo test` shows "running 0 tests" and "test result: ok. 0 passed"

## Architecture Notes

### Core Functionality
- **CLI Interface**: Two subcommands - `login` (save API key) and `tor` (transform text)
- **Configuration**: Uses XDG directories to store OpenAI API key in `secrets.toml`
- **AI Integration**: Sends text and transformation instructions to OpenAI API
- **Output**: Returns transformed text with AI-generated comments (often sassy)

### No Complex Build Process
- Standard Rust project with no custom build scripts
- No Docker, containers, or complex deployment
- No web server, database, or external services beyond OpenAI API
- No pre-commit hooks, CI/CD, or automated testing currently set up

### Dependencies
Key external crates: `clap` (CLI), `reqwest` (HTTP), `serde` (serialization), `anyhow` (error handling), `xdg` (config dirs), `rpassword` (secure input)

## Common Development Workflows

### Making Changes to CLI Arguments
When modifying CLI arguments or commands, edit `src/main.rs`:
- Update the `Opts` struct or `SubCommand` enum
- Test with `refac --help` and `refac <command> --help`
- Always run `cargo clippy` after changes

### Modifying AI Prompts or Examples  
When changing AI behavior, edit `src/prompt.rs`:
- Update `SYSTEM_PROMPT` constant for base AI behavior
- Add new examples to the samples array
- Test by running `refac tor` with sample inputs (requires API key)

### Changing Configuration or API Key Handling
When modifying config, edit `src/config_files.rs`:
- Update `Secrets` or `Config` structs for new fields
- Test with `refac login` and verify config file creation
- Check XDG directory handling works correctly

### Error Messages and User Experience
- All user-facing error messages should be helpful and actionable
- Include suggestions like "Try logging in with 'refac login'"
- Test error scenarios without API keys or with invalid inputs

## Installation Verification Steps

After making changes, ALWAYS verify the full installation process:

```bash
# Clean install test
cargo install --path . --force

# Verify basic functionality
refac --version  # Should show current version
refac --help     # Should show updated help text
refac login --help  # Should show login help
refac tor --help    # Should show tor help

# Test error handling
refac tor "test" "transform"  # Should show secrets.toml error
```

These steps ensure your changes work for end users who install via `cargo install refac`.