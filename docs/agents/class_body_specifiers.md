# WitcherScript class body — valid specifiers and flavours

Derived from compiler testing against `scratch/class_body_specifiers.ws`.
Lines that caused the compiler to halt (parse error) are marked **INVALID**.
Lines that produced a semantic error are noted with the error message.

---

## Declaration kinds valid in a class body

| Keyword    | Takes specifiers | Notes                                            |
| ---------- | ---------------- | ------------------------------------------------ |
| `var`      | yes              |                                                  |
| `function` | yes              |                                                  |
| `autobind` | yes              |                                                  |
| `event`    | no               | name must start with `"On"` (semantic rule)      |
| `default`  | no               | single default value: `default member = expr;`   |
| `defaults` | no               | block of defaults: `defaults { member = expr; }` |
| `hint`     | no               | `hint member = "string";`                        |

---

## Specifiers valid on `var`

| Specifier   | Valid       | Notes                                                   |
| ----------- | ----------- | ------------------------------------------------------- |
| `private`   | ✓           |                                                         |
| `protected` | ✓           |                                                         |
| `public`    | ✓           |                                                         |
| `editable`  | ✓           |                                                         |
| `saved`     | ✓           |                                                         |
| `const`     | ✓           |                                                         |
| `inlined`   | ✓           | type must be a class, not a primitive                   |
| `import`    | ✓           | must be first; access modifiers **may** follow it (`import private var` ✓, `private import var` ✗) |
| `final`     | **INVALID** | grammar currently allows it; compiler halts             |
| `optional`  | **INVALID** | param-only                                              |
| `out`       | **INVALID** | param-only                                              |
| `abstract`  | **INVALID** | function-only                                           |
| `latent`    | **INVALID** | function-only                                           |

### Valid `var` pairwise specifier combinations

| Combination        | Valid       |
| ------------------ | ----------- |
| access + editable  | ✓           |
| access + saved     | ✓           |
| access + const     | ✓           |
| access + inlined   | ✓           |
| editable + saved   | ✓           |
| editable + inlined | ✓           |
| const + inlined    | ✓           |
| access + import    | **INVALID** |
| editable + const   | **INVALID** |
| editable + final   | **INVALID** |
| saved + const      | **INVALID** |
| saved + inlined    | **INVALID** |
| saved + import     | **INVALID** |
| const + final      | **INVALID** |
| const + import     | **INVALID** |
| final + anything   | **INVALID** |
| inlined + import   | **INVALID** |

---

## Specifiers valid on `function`

| Specifier   | Valid       | Notes                                                   |
| ----------- | ----------- | ------------------------------------------------------- |
| `private`   | ✓           | cannot combine with `import`                            |
| `protected` | ✓           | cannot combine with `import`                            |
| `public`    | ✓           | cannot combine with `import`                            |
| `final`     | ✓           |                                                         |
| `latent`    | ✓           |                                                         |
| `import`    | ✓           | must be first; access modifiers **may** follow it (`import public function` ✓, `public import function` ✗) |
| `abstract`  | **INVALID** | grammar currently allows it; compiler halts             |
| `editable`  | **INVALID** | var-only                                                |
| `saved`     | **INVALID** | var-only                                                |
| `const`     | **INVALID** | var-only                                                |
| `inlined`   | **INVALID** | var-only                                                |

### Valid `function` specifier combinations

| Combination             | Valid                                               |
| ----------------------- | --------------------------------------------------- |
| access + final          | ✓                                                   |
| access + latent         | ✓                                                   |
| final + latent          | ✓                                                   |
| import + latent         | ✓                                                   |
| import + final          | ✓                                                   |
| import + final + latent | ✓                                                   |
| import + access         | ✓ (`import public function`, `import private function`, etc.)    |
| access + import         | **INVALID** (order matters: `import` must be first) |
| final + import          | **INVALID** (order matters: `import` must be first) |
| latent + import         | **INVALID** (order matters: `import` must be first) |
| abstract + anything     | **INVALID**                                         |

**`import` ordering rule:** `import` must be the first specifier; all others follow it. `import public final latent function` is valid; any ordering with `import` not first is not. Confirmed by game corpus: `import public var`, `import private var`, `import protected var`, `import public function`, `import private function` all appear in the base scripts.

---

## Function flavours — class body context

| Flavour      | Valid in class | Notes                                                           |
| ------------ | -------------- | --------------------------------------------------------------- |
| `quest`      | ✓              |                                                                 |
| `reward`     | ✓              |                                                                 |
| `storyscene` | ✓              |                                                                 |
| `timer`      | ✓ (syntax)     | requires at least two parameters; a third `optional` parameter is also valid. Confirmed game usage: `timer function Foo(dt : float, optional id : int)` |
| `entry`      | **INVALID**    | state bodies only                                               |
| `cleanup`    | **INVALID**    | state bodies only                                               |
| `exec`       | **INVALID**    | top-level functions only; cannot be declared inside a class     |

Flavours combine freely with the valid specifiers above (e.g. `public final quest function` is valid).

---

## Specifiers valid on `autobind`

| Specifier         | Valid |
| ----------------- | ----- |
| `private`         | ✓     |
| `protected`       | ✓     |
| `public`          | ✓     |
| `optional`        | ✓     |
| access + optional | ✓     |

---

## Recommended grammar changes

The current `specifier` rule is a flat choice over all specifiers. The compiler evidence above suggests splitting it:

### 1. Remove `final` from var-specifier positions

`final` on `var` causes a compiler halt. The grammar should not allow it on `member_var_decl`. Consider a dedicated `var_specifier` rule that omits `final`.

### 2. Remove `abstract` from function-specifier positions

`abstract` on `function` causes a compiler halt. Either remove it from `func_decl` specifiers entirely, or document that it is a dead keyword.

### 3. Encode the `import`-first ordering constraint

`import` must be the first specifier when present; access modifiers and other specifiers may follow it. The game corpus confirms `import public var`, `import private function`, `import public final latent function` etc. are all valid — the constraint is purely positional.

One approach: replace `repeat($.specifier)` with a choice between `seq('import', repeat(non_import_specifier))` and `seq(repeat(non_import_specifier))` (where `non_import_specifier` includes access modifiers, `final`, `latent`, etc.).

### 4. Suppress context-invalid flavours per declaration site

- `entry` and `cleanup`: only valid inside `state_def`, not `class_def`
- `exec`: only valid at top-level `func_decl`, not inside `class_def` or `state_def`

The grammar can express this by using separate flavour rules for `class_def` vs `state_def` vs top-level.

### 5. `timer` parameter requirement

`timer function` requires exactly `(id : int, deltaTime : float)` — this is a semantic rule, not easily expressible in the grammar, but worth noting in diagnostics.

### 6. `event` naming convention

Event names must start with `"On"`. Semantic rule, not grammar-level.
