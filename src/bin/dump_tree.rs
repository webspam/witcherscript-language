use std::io::{self, Read};
use std::path::Path;

use tree_sitter::Parser;
use witcherscript_language::diagnostics::format_tree;
use witcherscript_language::files::read_text_file;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let source = match args.next().as_deref() {
        Some("--string" | "-s") => args.next().ok_or("--string requires a value")?,
        Some(path) => read_text_file(Path::new(path))?,
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };

    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_witcherscript::language())?;

    let tree = parser
        .parse(&source, None)
        .ok_or("parser returned no tree")?;

    print!("{}", format_tree(tree.root_node()));
    Ok(())
}
