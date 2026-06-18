# WitcherScript language cheat sheet

**Primitive types:** `bool`, `byte`, `float`, `int`, `name`, `string`, `void`

**Declaration keywords:** `class`, `struct`, `enum`, `state`, `function`, `event`, `var`, `autobind`, `defaults`, `hint`

**Class modifiers:** `abstract`, `statemachine`

**Function flavours:** `entry`, `exec`, `quest`, `reward`, `storyscene`, `timer`, `latent`, `import`

**Access modifiers:** `private`, `protected`, `public` (default when absent)

**Variable modifiers:** `editable`, `saved`, `const`, `final`, `optional`, `out`, `inlined`

**Special receivers:** `this` (enclosing class), `super` (base class), `parent` (state → owner class)

**Common modding annotations:**

- `@addField(ClassName)` - inject field into existing class
- `@addMethod(ClassName)` - inject method
- `@wrapMethod(ClassName)` - wrap existing method
- `@replaceMethod(ClassName)` - replace existing method

**State machines:** `statemachine class X extends Y { }` / `state S in X { entry function Run() { } }`

**No static members.** Top-level functions are the global namespace. `exec` and `quest` functions are excluded from completion globals.

**`autobind` declarations** bind game-engine objects into class fields at runtime.

**`CName` literals** use single quotes: `'SomeName'` - classified as `enumMember` in semantic tokens.
