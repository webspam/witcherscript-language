# Validation rules

In addition to tree-sitter parse errors, the LSP server publishes the following diagnostics.

## Summary

| #   | Code                               | Severity | Description                                                                                        |
| --- | ---------------------------------- | -------- | -------------------------------------------------------------------------------------------------- |
| 1   | `late_local_var_decl`              | error    | Local `var` declared after an executable statement                                                 |
| 2   | `duplicate_symbol`                 | error    | Two top-level declarations share a name                                                            |
| 3   | `base_script_conflict`             | error    | Workspace file redeclares a base game script                                                       |
| 4   | `duplicate_local`                  | error    | Two parameters or locals in one function share a name                                              |
| 5   | `shadows_script_global`            | warning  | Name collides with a `redscripts.ini` global                                                       |
| 6   | `shadows_class_field`              | warning  | Local `var` collides with a field on the enclosing type                                            |
| 7   | `unknown_method`                   | error    | Method not declared on the receiver type                                                           |
| 8   | `unknown_type`                     | error    | Type-position identifier doesn't resolve                                                           |
| 9   | `unknown_member`                   | error    | Field not declared on the receiver type                                                            |
| 10  | `unknown_function`                 | error    | Bare function call doesn't resolve                                                                 |
| 11  | `unknown_identifier`               | error    | Bare identifier doesn't resolve                                                                    |
| 12  | `missing_wrapped_method`           | error    | `@wrapMethod` body has no `wrappedMethod(...)` call                                                |
| 13  | `duplicate_wrapped_method`         | error    | More than one `wrappedMethod(...)` call in a `@wrapMethod` body                                    |
| 14  | `ternary_cond_expr`                | warning  | `cond ? a : b` always evaluates to 0 / false / void                                                |
| 15  | `abstract_instantiation`           | error    | `new T` on an abstract class                                                                       |
| 16  | `super_field_access`               | error    | `super.x` used outside of a method call                                                            |
| 17  | `private_member_access`            | error    | Private field or method accessed from outside its declaring class                                  |
| 18  | `type_used_as_value`               | error    | Type name (class, struct, enum, native type) used in a value position                              |
| 19  | `type_mismatch`                    | error    | A value's type is not assignable to the target slot                                                |
| 20  | `string_as_name_default`           | info     | A `name`/`CName` field default uses a string literal where a name literal is intended              |
| 21  | `native_instantiation`             | error    | `new T` on a native engine type (`CBehTreeVal*`), which cannot be instantiated                     |
| 22  | `native_default_coercion`          | info     | A native engine type (`CBehTreeVal*`) `default` uses a non-exact primitive (accepted, but coerced) |
| 23  | `struct_property_access_modifier`  | error    | An accessibility modifier (`private`/`protected`/`public`) is applied to a struct property         |
| 24  | `state_owner_not_statemachine`     | warning  | `state X in Owner` where `Owner` is a class missing the `statemachine` keyword                     |
| 25  | `state_owner_not_class`            | error    | `state X in Owner` where `Owner` resolves to something that is not a class (e.g. a struct or enum) |
| 26  | `string_linefeed`                  | error    | A string literal contains a linefeed                                                               |
| 27  | `int_overflow`                     | error    | An integer literal overflows a 32-bit int                                                          |
| 28  | `event_return_not_void`            | error    | An event declares a return type other than `void`                                                  |
| 29  | `event_bare_return`                | error    | A bare `return;` inside an event body                                                              |
| 30  | `non_constant_default`             | error    | A `default` value is a call or `new` expression                                                    |
| 31  | `annotation_targets_backing_class` | error    | A modding annotation targets a state's backing class name instead of the short state name          |
| 32  | `duplicate_inherited_field`        | error    | A field redeclares a field inherited from an ancestor                                              |
| 33  | `override_weaker_access`           | error    | A method override has weaker access than the ancestor's method                                     |
| 34  | `override_param_count`             | error    | A method override declares a different parameter count than the ancestor's method                  |
| 35  | `unused_symbol`                    | hint     | An unused local variable, parameter, or private field; rendered faded by editors                   |
| 36  | `wrapped_method_modifier`          | error    | A modifier or flavour keyword is applied to a `@wrapMethod` function                               |

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

The check runs only when the receiver type infers to a workspace `class`, `struct`, or `state`; unknown or primitive receivers yield no type and are skipped, avoiding false positives. A `super`, `parent`, `virtualParent`, or cast receiver infers to a concrete class and is checked, not skipped. Private methods reached from outside their declaring class are reported as `private_member_access` instead of `unknown_method`.

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

A bare identifier that resolves to a `class`, `struct`, `enum`, or native type declaration but appears where a value is expected, e.g. `EnumGetMin(ESomeEnum)` or `var x : int; x = MyClass;`. Also fires when a type name is called like a function, e.g. `ESomeEnum()`, except struct constructor calls (`StructName(a, b, ...)`).

Type-position uses (`extends T`, `: T` annotations, `new T in owner`, `(T) value` casts, `@addMethod(T)` annotations) are unaffected. Enum _members_ used as values are also unaffected; only the enum's own name triggers the rule.

### 19. Type mismatch

A value flowing into a typed slot whose type is not assignable to the slot's type. Covers direct assignments (`x = value`), compound assignments (`x += value`, ...) on a primitive left-hand side, local `var` initializers (`var x : T = value`), function/method call arguments matched positionally against the callee's parameters, `return` values against the enclosing function's return type, and `default x = value;` / `defaults { x = value; }` field defaults.

Assignability allows:

- an identical type;
- a derived class into a base slot (upcast, traversed up to depth 32);
- `NULL` into a class or state slot;
- an `enum` and `int` in either direction;
- the implicit primitive conversions listed below.

Everything else is reported, including `string` -> `int` (which the compiler permits only with an explicit cast) and a base class into a derived slot.

#### Implicit primitive conversions

These mirror the conversions the compiler applies without a cast:

- into `string`: from `byte`, `int`, `float`, or `name`;
- into `bool`: from `byte`, `int`, `float`, or `string`;
- into `float`: from `byte` or `int`;
- between `byte` and `int`, in either direction.

The sized engine integer spellings (`Int16`, `Int8`, `Uint16`, `Uint32`, `Uint64`) and `StringAnsi` are their own types. The compiler converts them only with an explicit cast, so they are reported here unless the source and target spellings match.

Sites where either the value's type or the target's type cannot be inferred with confidence emit nothing, as do sites inside a tree-sitter error subtree, to avoid false positives while typing. A target whose name does not resolve to a known type (including the unsubstituted generic element of `array<T>` methods) is treated as unknown and skipped. Calls with more arguments than declared parameters, or with an empty argument slot, are skipped.

#### Engine `CBehTreeVal*` wrappers

The native types `CBehTreeValBool`, `CBehTreeValInt`, `CBehTreeValFloat`, `CBehTreeValString`, and `CBehTreeValCName` receive a value only through a `default` initializer (or a native `out` parameter). A `default` accepts _any_ primitive (the engine coerces it); the exact primitive - `bool`, `int`, `float` (also `int`), `string`, and `name` respectively - is silent, and anything else is `native_default_coercion` (info, never an error). Outside a `default` (e.g. `wrapper = value;`) they accept nothing, matching the compiler.

These five are modelled as a distinct `NativeType` kind, not classes: they take no object-to-bool / to-string / `NULL` casts and cannot be `new`-instantiated (see `native_instantiation`).

### 20. String literal as a name default

A `name`/`CName` field default whose value is a double-quoted string literal, e.g. `default someVar = "Swimming";`. The compiler accepts this as a compile-time constant `name`, so it is not a type error here (unlike a `var` initializer or an assignment, where `string` -> `name` is reported as `type_mismatch`). It is surfaced at info level because a name literal (`'Swimming'`) is the intended form.

### 21. Native type instantiation

A `new T` expression where `T` is a native engine type (`CBehTreeValBool`, `CBehTreeValInt`, `CBehTreeValFloat`, `CBehTreeValString`, `CBehTreeValCName`). These are C++ value types with no script constructor, so they cannot be instantiated from WitcherScript; a value reaches them only through a `default` initializer or a native `out` parameter.

### 22. Native default coercion

A `CBehTreeVal*` `default` whose value is a primitive other than the type's exact one (e.g. `default someBool = 5;` on a `CBehTreeValBool`). The engine accepts any primitive constant here, so it is not an error; it is info-level because the value is coerced rather than an exact match. `CBehTreeValFloat` treats both `int` and `float` as exact.

### 23. Accessibility modifier on a struct property

A `private`, `protected`, or `public` specifier applied to a property declared inside a `struct`, e.g. `struct S { private var x : int; }`. The diagnostic underlines the offending keyword. Unlike most rules in this list it is purely syntactic, so the `witcherscript-check` CLI reports it as well.

### 24. State owner is not a state machine

A `state X in Owner` declaration where `Owner` resolves to a `class` (in the workspace or a base script) that lacks the `statemachine` keyword. Only a state machine can host states, so the mod compiler rejects this.

The keyword is not inherited: each owner class must carry `statemachine` itself, so a state targeting a subclass of a state machine is still flagged.

### 25. State owner is not a class

A `state X in Owner` declaration where `Owner` resolves to something that is not a class - a struct, enum, function, or another state. States can only be declared in a state machine class, so this is an error rather than a warning.

Rules 24 and 25 share one scan path: every `state` declaration's owner is resolved once and routed to whichever applies. An owner that does not resolve to any known symbol is left to the `unknown_type` rule.

### 26. String literal containing a linefeed

A double-quoted string literal that spans more than one line. The grammar tokenises it, but the compiler rejects any string containing a linefeed. Purely syntactic, so the `witcherscript-check` CLI reports it as well.

### 27. Integer literal overflow

A decimal or hex integer literal whose value does not fit a 32-bit int (the compiler's "static integer overflow"). An adjacent sign is part of the literal, so `-2147483648` is in range; a spaced `- 2147483648` is a unary minus applied to an out-of-range literal and is flagged. Purely syntactic, so the `witcherscript-check` CLI reports it as well.

### 28. Event return type is not void

An `event` declared with an explicit return type other than `void`. The conventional form omits the return type entirely, which is also accepted; the only permitted explicit suffix is `void`. The return type is otherwise ignored by the compiler (a `return` inside an event must still yield `bool` - see rule 29). Purely syntactic, so the `witcherscript-check` CLI reports it as well.

### 29. Bare return in an event

A `return;` with no value inside an event body, at any nesting depth. Events return `bool`, so the compiler rejects this with "Unable to convert from 'void' to 'Bool'". A bare `return;` in a plain function is fine. Purely syntactic, so the `witcherscript-check` CLI reports it as well.

### 30. Non-constant default value

A `default x = ...;` or `defaults { x = ...; }` value that is a function/constructor call or a `new` expression (e.g. `default v = Vector(0, 0, 0);`). The compiler only accepts compile-time constants here. Literals, signed literals, and bare identifiers (possible enum members) are allowed; only call and `new` expressions are flagged, so a parenthesised call slips through. Purely syntactic, so the `witcherscript-check` CLI reports it as well.

### 31. Annotation targets a state's backing class

A `@wrapMethod` / `@replaceMethod` / `@addMethod` / `@addField` whose argument is the engine-synthesised backing class name of a state (e.g. `@wrapMethod(CR4PlayerStateSwimming)`). The mod compiler only matches annotations against the short state name (`@wrapMethod(Swimming)`); the message suggests that spelling and links to the state declaration.

### 32. Duplicate inherited field

A class or state field whose name is already a field anywhere up the inheritance chain (including base scripts and any access level - the compiler rejects the redeclaration even for private ancestor fields). Method names may be reused; only field-over-field redeclarations fire. `@addField` declarations are exempt.

### 33. Override with weaker access

A class or state method whose name matches a class-body method up the inheritance chain, declared with weaker (more accessible) access than the ancestor's; default accessibility is `public`. Mirrors the compiler error "Function 'X' cannot have a weaker access modifier than in ancestor class 'Y'". Annotated (`@wrapMethod` etc.) functions and events are exempt.

### 34. Override parameter count mismatch

A class or state method whose name matches a class-body method up the inheritance chain but declares a different number of parameters. Mirrors the compiler error "Function 'X' takes N parameter(s) which is inconsistent with base function (M)". Optional parameters count; parameter types are not compared. Shares rule 33's scan and exemptions.

### 35. Unused symbol

An unused local variable, parameter, or `private` field. Emitted at hint severity with the LSP `Unnecessary` tag, so editors fade the declaration. An assignment counts as a use. `@addField` declarations are exempt.

### 36. Modifier on a wrapped method

An access modifier (`public`/`protected`/`private`/`final`/...) or function flavour keyword (`exec`, `timer`, ...) on a `@wrapMethod`-annotated function. The wrapper inherits the wrapped method's signature, so the compiler rejects any added modifier or flavour. Each offending keyword is flagged separately.
