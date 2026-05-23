# Validation rules

In addition to tree-sitter parse errors, the LSP server publishes the following
diagnostics:

- Local `var` declarations must precede executable statements within each function block.
  Blank lines, comments, and bare semicolons do not count as executable statements.
- Duplicate top-level symbol names: a class, struct, enum, state, function, or event must
  not share a name with another top-level declaration anywhere in the workspace. Each
  conflicting declaration is flagged, with related-information links to the others.
  Modding-annotation member injections (`@addMethod`/`@wrapMethod`/...) are exempt.
- Base-script conflict (`base_script_conflict`, error): a workspace file whose path and
  name match a base game script (e.g. a copied-and-edited `game/r4Player.ws`) and which
  redeclares the base script's top-level symbols. Each clashing declaration is flagged
  with a related-information link to the base declaration. This is a legacy full-script
  override; the message asks the user to either mark the directory under
  `witcherscript.legacyScriptDirectories` (a quick fix is offered) or switch to
  annotation-based modding. Inside such a file the generic duplicate-symbol error is
  suppressed in favour of this clearer one.
- Duplicate local declarations (error): two parameters, two local `var`s, or a parameter
  and a local `var` with the same name inside one function. `@wrapMethod` and
  `@replaceMethod` functions are exempt — they intentionally mirror the wrapped/replaced
  signature.
- Shadowing (warning): a parameter, local `var`, or member field whose name collides with
  a `redscripts.ini` `[globals]` entry; or a local `var` whose name collides with a field
  declared in the enclosing class/struct/state. `@wrapMethod` and `@replaceMethod`
  functions are exempt.
- Unknown method on a known receiver type: a `receiver.Method()` call where `receiver`
  resolves to a workspace `class`/`struct`/`state` but `Method` is not declared on that
  type or any of its supertypes (inheritance traversed up to depth 32). Calls on
  unknown/primitive receivers, on `super`/`parent`/`virtualParent`, on casts, or through
  indexed/parenthesised expressions are skipped to avoid false positives. Private members
  count as known.
- Unknown type (`unknown_type`): a type-position identifier that doesn't resolve to a
  workspace `class`/`struct`/`enum`/`state` or a built-in primitive. Covers `extends Foo`,
  `state S in Foo`, `: Foo` annotations (including nested generics), `new Foo in owner`,
  `(Foo) value` casts, and `@addMethod(Foo)` / `@addField(Foo)` annotation arguments.
- Unknown member (`unknown_member`): `receiver.field` on a known workspace type where
  `field` is not a member of that type or any supertype. Also fires inside `default
  field = …;`, `defaults { field = …; }`, and `hint field = "…";` blocks when the
  enclosing class/struct/state has no such field; inherited private fields count as
  visible there, since a subclass may set their default or hint. The `hint` case is
  reported at info level. Skipped when the receiver type can't be inferred (cascading)
  or is primitive; method-call cases are owned by `unknown_method`.
- Unknown function (`unknown_function`): a bare `Foo()` call where `Foo` doesn't resolve
  to a top-level function, a method on `this` (this-shorthand, including up the
  inheritance chain), or a script-environment global.
- Unknown identifier (`unknown_identifier`): a bare identifier used as a value that
  doesn't resolve to a local, parameter, field via this-shorthand, top-level symbol, or
  script-environment global. Idents inside tree-sitter error/missing subtrees and inside
  `incomplete_member_access_expr` are suppressed to avoid noise while typing. The
  `wrappedMethod` modding macro is recognised as a valid call site when it appears
  inside the body of an `@wrapMethod`-annotated function and is therefore not flagged.
- Missing wrapped-method call (`missing_wrapped_method`): an `@wrapMethod`-annotated
  function whose body does not contain a bare `wrappedMethod(...)` call. The mod
  compiler refuses to link such a function.
- Duplicate wrapped-method call (`duplicate_wrapped_method`): every bare
  `wrappedMethod(...)` call after the first inside the same `@wrapMethod` body. Only
  the first call is expanded by the compiler.
- Ternary expression (`ternary_cond_expr`): the grammar accepts `cond ? a : b`, but the
  compiler always evaluates it to `0` / `false` / `void`. Flagged so the construct is
  rewritten as an `if`/`else` before it silently returns wrong values.
- Abstract instantiation (`abstract_instantiation`): `new T` where `T` is a class declared
  with the `abstract` specifier. Abstract classes cannot be instantiated directly.
