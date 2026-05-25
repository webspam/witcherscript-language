# Validation rules

In addition to tree-sitter parse errors, the LSP server publishes the following diagnostics.

## Summary

| # | Code | Severity | Description |
| --- | --- | --- | --- |
| 1 | `late_local_var_decl` | error | Local `var` declared after an executable statement |
| 2 | `duplicate_symbol` | error | Two top-level declarations share a name |
| 3 | `base_script_conflict` | error | Workspace file redeclares a base game script |
| 4 | `duplicate_local` | error | Two parameters or locals in one function share a name |
| 5 | `shadows_script_global` | warning | Name collides with a `redscripts.ini` global |
| 6 | `shadows_class_field` | warning | Local `var` collides with a field on the enclosing type |
| 7 | `unknown_method` | error | Method not declared on the receiver type |
| 8 | `unknown_type` | error | Type-position identifier doesn't resolve |
| 9 | `unknown_member` | error | Field not declared on the receiver type |
| 10 | `unknown_function` | error | Bare function call doesn't resolve |
| 11 | `unknown_identifier` | error | Bare identifier doesn't resolve |
| 12 | `missing_wrapped_method` | error | `@wrapMethod` body has no `wrappedMethod(...)` call |
| 13 | `duplicate_wrapped_method` | error | More than one `wrappedMethod(...)` call in a `@wrapMethod` body |
| 14 | `ternary_cond_expr` | warning | `cond ? a : b` always evaluates to 0 / false / void |
| 15 | `abstract_instantiation` | error | `new T` on an abstract class |
| 16 | `super_field_access` | error | `super.x` used outside of a method call |
| 17 | `private_member_access` | error | Private field or method accessed from outside its declaring class |
| 18 | `type_used_as_value` | error | Type name (class, struct, state, enum) used in a value position |

## Details

### 1. Local var declared after a statement

Local `var` declarations must precede executable statements within each function block. Blank lines, comments, and bare semicolons do not count as executable statements.

### 2. Duplicate top-level symbol

A class, struct, enum, state, function, or event must not share a name with another top-level declaration anywhere in the workspace. Each conflicting declaration is flagged, with related-information links to the others.

Modding-annotation member injections (`@addMethod`, `@wrapMethod`, ...) are exempt.

### 3. Base-script conflict

Fires on a workspace file whose path and name match a base game script (e.g. a copied-and-edited `game/r4Player.ws`) and which redeclares the base script's top-level symbols. Each clashing declaration is flagged with a related-information link to the base declaration.

This is a legacy full-script override. The message asks the user to either mark the directory under `witcherscript.legacyScriptDirectories` (a quick fix is offered) or switch to annotation-based modding.

Inside such a file the generic `duplicate_symbol` error is suppressed in favour of this clearer one.

### 4. Duplicate local declaration

Two parameters, two local `var`s, or a parameter and a local `var` with the same name inside one function.

`@wrapMethod` and `@replaceMethod` functions are exempt; they intentionally mirror the wrapped or replaced signature.

### 5. Shadows a script global

A parameter, local `var`, or member field whose name collides with a `redscripts.ini` `[globals]` entry.

`@wrapMethod` and `@replaceMethod` functions are exempt.

### 6. Shadows a class field

A local `var` whose name collides with a field declared in the enclosing class, struct, or state.

`@wrapMethod` and `@replaceMethod` functions are exempt.

### 7. Unknown method

A `receiver.Method()` call where `receiver` resolves to a workspace `class`, `struct`, or `state`, but `Method` is not declared on that type or any of its supertypes (inheritance traversed up to depth 32).

Calls on unknown or primitive receivers, on `super` / `parent` / `virtualParent`, on casts, or through indexed or parenthesised expressions are skipped to avoid false positives. Private methods reached from outside their declaring class are reported as `private_member_access` instead of `unknown_method`.

### 8. Unknown type

A type-position identifier that doesn't resolve to a workspace `class`, `struct`, `enum`, or `state`, or a built-in primitive.

Covers `extends Foo`, `state S in Foo`, `: Foo` annotations (including nested generics), `new Foo in owner`, `(Foo) value` casts, and `@addMethod(Foo)` / `@addField(Foo)` annotation arguments.

### 9. Unknown member

`receiver.field` on a known workspace type where `field` is not a member of that type or any supertype.

Also fires inside `default field = ...;`, `defaults { field = ...; }`, and `hint field = "...";` blocks when the enclosing class, struct, or state has no such field. Inherited private fields count as visible there, since a subclass may set their default or hint. The `hint` case is reported at info level.

Skipped when the receiver type can't be inferred (cascading) or is primitive. Method-call cases are owned by `unknown_method`. Private fields reached from outside their declaring class are reported as `private_member_access` instead.

### 10. Unknown function

A bare `Foo()` call where `Foo` doesn't resolve to a top-level function, a method on `this` (this-shorthand, including up the inheritance chain), or a script-environment global.

### 11. Unknown identifier

A bare identifier used as a value that doesn't resolve to a local, parameter, field via this-shorthand, top-level symbol, or script-environment global.

Identifiers inside tree-sitter error or missing subtrees and inside `incomplete_member_access_expr` are suppressed to avoid noise while typing.

The `wrappedMethod` modding macro is recognised as a valid call site when it appears inside the body of a `@wrapMethod`-annotated function, and is therefore not flagged.

### 12. Missing wrapped-method call

A `@wrapMethod`-annotated function whose body does not contain a bare `wrappedMethod(...)` call. The mod compiler refuses to link such a function.

### 13. Duplicate wrapped-method call

Every bare `wrappedMethod(...)` call after the first inside the same `@wrapMethod` body. Only the first call is expanded by the compiler.

### 14. Ternary expression

The grammar accepts `cond ? a : b`, but the compiler always evaluates it to `0` / `false` / `void`. Flagged so the construct is rewritten as an `if` / `else` before it silently returns wrong values.

### 15. Abstract class instantiation

`new T` where `T` is a class declared with the `abstract` specifier. Abstract classes cannot be instantiated directly.

### 16. `super.` outside a method call

`super.x` used as a field access (read, assignment target, or anywhere other than the callee of a `(...)` call).

The compiler only resolves the `super.` qualifier for method dispatch. Inherited `protected` and `public` fields (and `private` ones from a `protected`/`public`-typed view of the base) are reachable on `this` without the qualifier; `super.` itself is reserved for explicitly dispatching to a base-class method.

### 17. Private member access

`receiver.member` or `receiver.method()` where `member` / `method` is declared `private` on a type, accessed from code outside that declaring type. Access from inside the declaring class, struct, or state is allowed.

`default` and `hint` blocks intentionally allow private inherited fields and are not affected.

### 18. Type used as value

A bare identifier that resolves to a `class`, `struct`, `state`, or `enum` declaration but appears where a value is expected, e.g. `EnumGetMin(ESomeEnum)` or `var x : int; x = MyClass;`. Also fires when a type name is called like a function, e.g. `ESomeEnum()`.

Type-position uses (`extends T`, `: T` annotations, `new T in owner`, `(T) value` casts, `@addMethod(T)` annotations) are unaffected. Enum *variants* used as values are also unaffected; only the enum's own name triggers the rule.
