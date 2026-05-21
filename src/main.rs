use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser as ClapParser;
use tree_sitter::Parser as TreeSitterParser;
use witcherscript_language::diagnostics::format_tree;
use witcherscript_language::document::parse_document_with_parser;
use witcherscript_language::files::collect_witcherscript_files;

#[derive(Debug, ClapParser)]
#[command(author, version, about)]
struct Cli {
    /// Files or directories to parse. Directories are searched recursively for .ws files.
    #[arg(required = true)]
    paths: Vec<PathBuf>,

    /// Print a concrete syntax tree for every parsed file.
    #[arg(long)]
    dump_tree: bool,

    /// Maximum diagnostics to print per file.
    #[arg(long, default_value_t = 20)]
    max_diagnostics: usize,
}

#[derive(Debug)]
struct FileResult {
    diagnostic_count: usize,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let files = collect_witcherscript_files(&cli.paths, &[])?;

    if files.is_empty() {
        return Err("no .ws files found".into());
    }

    let mut parser = TreeSitterParser::new();
    parser.set_language(&tree_sitter_witcherscript::language())?;

    let mut results = Vec::with_capacity(files.len());

    for path in files {
        let result = parse_file(&mut parser, &path, cli.dump_tree, cli.max_diagnostics)?;
        results.push(result);
    }

    let failed_files = results
        .iter()
        .filter(|result| result.diagnostic_count > 0)
        .count();
    let diagnostic_count: usize = results.iter().map(|result| result.diagnostic_count).sum();

    if failed_files > 0 {
        eprintln!(
            "Parsed {} file(s): {} file(s) had {} parse diagnostic(s).",
            results.len(),
            failed_files,
            diagnostic_count
        );
        std::process::exit(1);
    }

    println!("Parsed {} file(s): no syntax errors found.", results.len());
    Ok(())
}

fn parse_file(
    parser: &mut TreeSitterParser,
    path: &Path,
    dump_tree: bool,
    max_diagnostics: usize,
) -> Result<FileResult, Box<dyn Error>> {
    let source = fs::read_to_string(path)?;
    let document = parse_document_with_parser(parser, source)?;
    let root = document.tree.root_node();

    if dump_tree {
        println!("== {} ==", path.display());
        print!("{}", format_tree(root));
    }

    let diagnostics = &document.diagnostics;
    if diagnostics.is_empty() {
        println!("OK {}", path.display());
    } else {
        eprintln!("ERR {}", path.display());
        for diagnostic in diagnostics.iter().take(max_diagnostics) {
            eprintln!("{}", diagnostic.display(path));
        }

        if diagnostics.len() > max_diagnostics {
            eprintln!(
                "{}: omitted {} additional parse diagnostic(s)",
                path.display(),
                diagnostics.len() - max_diagnostics
            );
        }
    }

    Ok(FileResult {
        diagnostic_count: diagnostics.len(),
    })
}
