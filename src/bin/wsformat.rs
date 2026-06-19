use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use clap::{Parser as ClapParser, ValueEnum};
use tree_sitter::Parser as TreeSitterParser;
use witcherscript_language::document::parse_document_with_parser;
use witcherscript_language::files::{collect_witcherscript_files, read_text_file};
use witcherscript_language::format_config::{self, FormatConfigFile};
use witcherscript_language::formatter::{
    AnnotationPlacement, ColonSpacing, FormatOptions, format_document,
};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ColonSpacingArg {
    Spaced,
    Compact,
}

impl From<ColonSpacingArg> for ColonSpacing {
    fn from(arg: ColonSpacingArg) -> Self {
        match arg {
            ColonSpacingArg::Spaced => Self::Spaced,
            ColonSpacingArg::Compact => Self::Compact,
        }
    }
}

// Value names match the `.wsformat.toml` tokens so the CLI and the config file share one vocabulary.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum PlacementArg {
    Preserve,
    #[value(name = "ownLine")]
    OwnLine,
    #[value(name = "sameLine")]
    SameLine,
}

impl From<PlacementArg> for AnnotationPlacement {
    fn from(arg: PlacementArg) -> Self {
        match arg {
            PlacementArg::Preserve => Self::Preserve,
            PlacementArg::OwnLine => Self::OwnLine,
            PlacementArg::SameLine => Self::SameLine,
        }
    }
}

#[derive(Debug, ClapParser)]
#[command(author, version, about = "Format WitcherScript (.ws) files in place.")]
struct Cli {
    /// Files or directories to format. Directories are searched recursively for .ws files.
    #[arg(required = true)]
    paths: Vec<PathBuf>,

    /// Do not write changes; exit non-zero if any file is not already formatted.
    #[arg(long)]
    check: bool,

    /// Spaces per indent level (ignored when --use-tabs is set).
    #[arg(long)]
    tab_size: Option<u32>,

    /// Indent with tabs instead of spaces.
    #[arg(long)]
    use_tabs: bool,

    /// Column the formatter tries to keep lines within.
    #[arg(long)]
    line_limit: Option<u32>,

    /// Spacing around `:` in declarations (`x : int` vs `x: int`).
    #[arg(long)]
    colon_spacing: Option<ColonSpacingArg>,

    /// Align `:` across consecutive member declarations.
    #[arg(long)]
    align_member_colons: bool,

    /// Placement of `@`-annotations relative to the item they annotate.
    #[arg(long)]
    annotation_placement: Option<PlacementArg>,

    /// Placement of a trailing `default` relative to its statement.
    #[arg(long)]
    default_placement: Option<PlacementArg>,
}

impl Cli {
    // Precedence: built-in defaults < .wsformat.toml < explicit flags.
    fn format_options(&self, file: Option<&FormatConfigFile>) -> FormatOptions {
        let mut options = file.map_or_else(FormatOptions::default, |file| {
            file.apply_to(FormatOptions::default())
        });
        if let Some(tab_size) = self.tab_size {
            options.tab_size = tab_size;
        }
        if self.use_tabs {
            options.use_tabs = true;
        }
        if let Some(line_limit) = self.line_limit {
            options.line_limit = line_limit;
        }
        if let Some(colon_spacing) = self.colon_spacing {
            options.colon = colon_spacing.into();
        }
        if self.align_member_colons {
            options.align_member_colons = true;
        }
        if let Some(placement) = self.annotation_placement {
            options.annotation_placement = placement.into();
        }
        if let Some(placement) = self.default_placement {
            options.default_placement = placement.into();
        }
        options
    }
}

enum Outcome {
    Unchanged,
    Formatted(String),
    SyntaxErrors(usize),
}

fn format_source(
    parser: &mut TreeSitterParser,
    source: String,
    options: FormatOptions,
) -> Result<Outcome, Box<dyn Error>> {
    let document = parse_document_with_parser(parser, source)?;
    // Formatting a CST with ERROR nodes can drop source, and in-place write is irreversible.
    if !document.diagnostics.is_empty() {
        return Ok(Outcome::SyntaxErrors(document.diagnostics.len()));
    }
    let formatted = format_document(document.tree.root_node(), &document.source, options);
    Ok(if formatted == document.source {
        Outcome::Unchanged
    } else {
        Outcome::Formatted(formatted)
    })
}

enum Status {
    Clean,
    Changed,
    Skipped,
}

fn process_file(
    parser: &mut TreeSitterParser,
    path: &Path,
    options: FormatOptions,
    check: bool,
) -> Result<Status, Box<dyn Error>> {
    let source = read_text_file(path)?;
    match format_source(parser, source, options)? {
        Outcome::Unchanged => Ok(Status::Clean),
        Outcome::SyntaxErrors(count) => {
            eprintln!("skipped {} ({count} parse error(s))", path.display());
            Ok(Status::Skipped)
        }
        Outcome::Formatted(new_content) => {
            if check {
                println!("would reformat {}", path.display());
            } else {
                fs::write(path, &new_content)?;
                println!("formatted {}", path.display());
            }
            Ok(Status::Changed)
        }
    }
}

fn main() {
    match run() {
        Ok(exit_code) => std::process::exit(exit_code),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(2);
        }
    }
}

fn run() -> Result<i32, Box<dyn Error>> {
    let cli = Cli::parse();
    let files = collect_witcherscript_files(&cli.paths, &[])?;
    if files.is_empty() {
        return Err("no .ws files found".into());
    }

    let mut parser = TreeSitterParser::new();
    parser.set_language(&tree_sitter_witcherscript::language())?;

    let cwd = std::env::current_dir()?;
    let mut changed = 0usize;
    let mut failed = 0usize;

    for path in &files {
        // Each file's config comes from its own directory, not the invocation cwd.
        let dir = cwd.join(path.parent().unwrap_or(Path::new("")));
        let options = cli.format_options(format_config::load(&dir)?.as_ref());
        match process_file(&mut parser, path, options, cli.check) {
            Ok(Status::Clean) => {}
            Ok(Status::Changed) => changed += 1,
            Ok(Status::Skipped) => failed += 1,
            Err(error) => {
                eprintln!("error: {}: {error}", path.display());
                failed += 1;
            }
        }
    }

    // In write mode a reformat is success; only an unprocessable file fails. --check fails on drift.
    let exit_code = i32::from(failed > 0 || (cli.check && changed > 0));
    Ok(exit_code)
}
