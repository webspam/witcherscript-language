### Writing parser / mod resolve code

- Use Test-Driven Design to drive your implementation
- Parsing MUST use Concrete Syntax Tree (CST), NOT basic / brittle text parsing
  - Immediately report any situation where the CST requirement is causing friction, delays, or code to become too complex
- Look for and re-use existing helpers

### Dumping syntax trees for example code

- Use `dump_tree` to dump syntax trees via stdin, usage:
  - From stdin: `echo 'function Hello() { var x : int; x = 1; }' | cargo run --bin dump_tree 2>&1`
  - From a file: `cargo run --bin dump_tree -- path/to/script.ws`
- Immediately report all instances where the tree-sitter grammar is incorrect OR inefficient

### The grammar is an external repo

- `tree-sitter-witcherscript`, pinned by `tag` in `Cargo.toml`. New syntax support: tag it there, bump the pin, `cargo update`.
- Node kinds / rules: the `grammar-locator` subagent or <https://github.com/webspam/tree-sitter-witcherscript>.
