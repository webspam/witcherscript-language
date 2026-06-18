# WitcherScript language rules

Terse reference. Each bullet is one rule. Spellings are exact.

## Types

- Primitives: `bool`, `byte`, `int`, `float`, `name`, `string`, `void`
- `int` is signed 32-bit
- `name` literals are `CName`; `name` and `string` are distinct types
- Sized engine ints are distinct types: `Int8`, `Int16`, `Uint16`, `Uint32`, `Uint64`
- `StringAnsi` is a distinct type from `string`
- `array<T>` is the only generic type; write `array<ElementType>`
- Generics nest by grammar but only one level is used (`array<T>`)
- No static members, methods, or fields
- Native value types `CBehTreeValBool`, `CBehTreeValInt`, `CBehTreeValFloat`, `CBehTreeValString`, `CBehTreeValCName` are not classes
- Native value types cannot be `new`-instantiated; they receive a value only via a `default` or a native `out` param

## Literals

- `int`: digits or hex `0x...`, optional leading `+`/`-` (sign is part of the token)
- `float`: `1.0`, `1.`, `.5`; optional trailing `f` (meaningless)
- `string`: double quotes; single line only; only `\"` is escaped
- `name`: single quotes `'Name'`; only `\'` is escaped
- `bool`: `true`, `false`
- `NULL`

## Casts

Implicit:

- Identical type
- Derived class into a base slot (upcast)
- `NULL` into a class or state slot
- `enum` and `int`, either direction
- Into `string`: from `bool`, `byte`, `int`, `float`, `name`
- Into `bool`: from `byte`, `int`, `float`, `string`
- Into `float`: from `byte`, `int`
- `byte` and `int`, either direction

Explicit syntax: `(Type)value`

## Names and scope

- Every top-level name is unique
- Valid global types: `class`, `struct`, `enum`, `state`, or primitive
- Bare calls: `this` method, global function, struct constructor `Vector(0,0,0)`
  - Functions can shadow struct constructors
- Bare symbols: local, param, field, enum / script global
- params and local vars share a namespace
- shadowing outside of namespace is allowed

## Declarations

Top-level kinds: `class`, `struct`, `enum`, `state`, `function`, `event`

### class

- `[abstract] [statemachine] class Name [extends Base] { ... }`
- Single inheritance: at most one `extends`
- `abstract` class cannot be `new`-instantiated
- Body members: `var`, `function`, `event`, `autobind`, `default`, `defaults`, `hint`

### state machines

- `statemachine class Name { ... }` can host states
- `state Name in Owner [extends Base] { ... }`; body is identical to a class body
- `Owner` must be a `class` carrying `statemachine`
- `statemachine` is not inherited; each owner class declares it itself
- A state exposes `parent` and `virtual_parent`, both the owner class

### struct

- `struct Name { ... }`
- Members: `var`, `default`, `defaults`, `hint` only
- No access modifiers on props
- Construct with `Name(a, b, ...)`
- `default`(s) only set when constructor called
- struct is accessible without calling constructor, but all props set to type default (e.g. ints/floats 0, string "")

### enum

- `enum Name { A, B = 5, C }`; trailing comma allowed
- Member value is optional: `= int` or `= hex`
- Members are global; reference by bare name (`A`), not `Name.A`

### function

- `[annotation] [specifiers] [flavour] function Name(params) [: ReturnType] { ... }`
- Body is a block `{ ... }` or `;` (import/abstract)
- Omitted return type means `void`
- `import function` is a native engine declaration; body is `;`

### event

- `event Name(params) [: void] { ... }`
- Name must start with `On`
- Takes no specifiers and no flavour
- Return type, if written, must be `void` (meaningless, engine treats it as `bool` anyway)
- Inside an event a `return` must return a `bool`; bare `return;` is invalid, but zero `return`s is valid

### member var

- `[annotation] [specifiers] var name [, name2 ...]: Type ;`
- Multiple names in one declaration share the type

### autobind

- `[access] [optional] autobind name: Type = ( single | "binding" ) ;`

### default / defaults / hint

- `default member = expr ;` sets one field default
- `defaults { member = expr ; ... }` sets several
- `hint member = "text" ;` sets the field tooltip
- The targeted member must be a field of the type (inherited private fields included)
- A default value must be a compile-time constant: literal, signed literal, or bare identifier (enum member)
- A default value cannot be a call or `new` expression
- For a `name`/`CName` default, use a name literal `'x'`; a string `"x"` is accepted but coerced

## Specifiers

- Access modifiers: `private`, `protected`, `public`; default is `public`
- `import` must be the first specifier; only access may follow it

Fields (`var`):

- Allowed: access, `editable`, `saved`, `const`, `inlined`, `import`
- `import var` adds access only, not `editable`/`saved`/`const`/`inlined`
- Mutually exclusive: `editable`/`const`, `saved`/`const`, `saved`/`inlined`
- `inlined` requires a class type, not a primitive
- `final` is rejected on a `var` (the grammar accepts it; the compiler halts)

`function`:

- Allowed: access, `final`, `latent`, `import`; these combine freely (import first)
- `abstract` is rejected on a `function` (the grammar accepts it; the compiler halts)

`autobind`: access and `optional`

Parameter: `out`, `optional`

## Function flavours

- Class body: `quest`, `reward`, `storyscene`, `timer`
- State body: `entry`, `cleanup`
- Top level only: `exec`
- A flavour combines with the valid specifiers (e.g. `public final quest function`)
- `timer function` requires two params `(dt: float, id: int)`, both may be `optional`

## Parameters

- `name: Type`
- `a, b, c: int`
- Passed by value
- `out` keyword: by-reference
- `optional` keyword
- No default values

## Statements

- Local `var` declaration must precede every executable statement in block
- `if (cond) stmt [else stmt]`
- `for (init; cond; iter) stmt`; each clause is optional; clauses may be comma-separated expressions
- `while (cond) stmt`
- `do stmt while (cond)` (trailing `;` optional)
- `switch (expr) { case v: ... default: ... }`
- `break;`, `continue;`
- `return [expr];`
- `delete expr;` frees an object instance
- `{ ... }` is a block; `;` alone is a no-op

## Operators (section unconfirmed)

Precedence, tightest first:

1. `.` member access, `()` call, `[]` index
2. `new`, unary `-` `!` `~` `+`, cast `(Type)expr`
3. `*` `/` `%`
4. `+` `-`
5. `<` `<=` `>` `>=`
6. `==` `!=`
7. `&`
8. `^`
9. `|`
10. `&&`
11. `||`
12. ternary `? :` (right-associative)
13. assignment `=` `+=` `-=` `*=` `/=` `&=` `|=`

- `new Class in <lifetimeObject>`
- Receivers: `this` (enclosing type), `super` (base), `parent`/`virtual_parent` (state owner)
- `super` is valid only for method (`super.M()`)
- Ternary `cond ? a: b` always yields `0`/`false`/`void`; rewrite as `if`/`else`

## Inheritance and overrides

- Field cannot redeclare a field inherited from any ancestor (any access level)
- Method override must declare the same parameter count as the ancestor (optional params counted; types not compared)
- Overrides access must be no more permissive than ancestor

## Modding annotations

- `@addField(Class)` injects field into existing class
- `@addMethod(Class)` injects method
- `@wrapMethod(Class)` wraps existing method
- `@replaceMethod(Class)` replaces existing method
- `@wrapMethod` body must call `wrappedMethod(...)` exactly once
- `@wrapMethod` function takes no added specifier / flavour (inherits wrapped signature)
- Annotation targeting a state uses short state name (`Swimming`), not engine backing-class name
