# HearthD Automations Language Specification

## Overview

The HearthD Automations language (`.hda` files) is a domain-specific language for writing home automation logic as pure functions. It draws inspiration from:

- **Nix**: Destructuring syntax and `inherit` keyword
- **bpftrace/dtrace**: Filter condition syntax with `/condition/` delimiters
- **Python**: List comprehensions
- **Rust**: Type system, explicit dereferencing, optional chaining

### Design Goals

1. **Pure functional model**: Automations are pure functions from `(Event, State)` to `Event` or `[Event]`
2. **Parallelizable**: Observers run in parallel on read-only state; mutators process events sequentially
3. **Type-safe**: Static type checking at load time prevents runtime errors
4. **Efficient**: Integer/enum comparisons replace string operations
5. **Readable**: Natural syntax with unit literals and destructuring

### Automation Types

- **Observer**: Watches events and emits zero or more new events
  - Signature: `(Event, State) -> [Event]`
  - Runs in parallel on read-only state
  - Results should be deterministically ordered

- **Mutator**: Transforms a single event
  - Signature: `(Event, State) -> Event`
  - Runs sequentially on event stream
  - Only one mutator processes an event at a time

## Syntax

### File Extension

Automation files use the `.hda` extension (HearthD Automation).

### Comments

```rust
// Single-line comment

/*
 * Multi-line comment
 */
```

### Keywords

Reserved words that cannot be used as identifiers:

```
observer    mutator     let         if          else
for         in          await       inherit     true
false       match       return
```

### Operators

#### Boolean Operators
- `&&` - Logical AND (short-circuit)
- `||` - Logical OR (short-circuit)
- `!` - Logical NOT

#### Comparison Operators
- `==` - Equal
- `!=` - Not equal
- `<` - Less than
- `>` - Greater than
- `<=` - Less than or equal
- `>=` - Greater than or equal

#### Arithmetic Operators
- `+` - Addition
- `-` - Subtraction
- `*` - Multiplication
- `/` - Division
- `%` - Modulo

#### Special Operators
- `*` - Explicit dereference (e.g., `*location`)
- `?` - Optional chaining (e.g., `state.person?.location`)
- `.` - Field access
- `...` - Spread operator in patterns and struct construction

#### Operator Precedence

From highest to lowest:

1. Field access (`.`), optional chaining (`?`), function call
2. Unary operators (`!`, `-`, `*`, `await`)
3. Multiplicative (`*`, `/`, `%`)
4. Additive (`+`, `-`)
5. Comparison (`<`, `>`, `<=`, `>=`)
6. Equality (`==`, `!=`)
7. Logical AND (`&&`)
8. Logical OR (`||`)

Note: Comparison operators bind tighter than logical operators, so `a == b && c == d` parses as `(a == b) && (c == d)`.

### Literals

#### Numeric Literals
```rust
42          // Integer
3.14        // Float
-5          // Negative integer
```

#### Unit Literals

Numeric literals can have unit suffixes attached directly (no whitespace):

Time units:
```rust
5s          // seconds
10min       // minutes
2h          // hours
1.5d        // days

// Long-form aliases also supported
5seconds
10minutes
2hours
1.5days
```

Angle units:
```rust
90deg       // degrees
1.57rad     // radians

// Long-form aliases
90degrees
1.57radians
```

Temperature units:
```rust
20c         // celsius
68f         // fahrenheit
293k        // kelvin

// Long-form aliases
20celsius
68fahrenheit
293kelvin
```

Unit suffixes follow the same rules as Rust numeric suffixes - they are attached directly to the number with no intervening whitespace.

#### String Literals
```rust
"hello"
"with \"escapes\""
```

#### Boolean Literals
```rust
true
false
```

### Destructuring Patterns

Patterns extract values from structs. The trailing `...` is **required** when not all fields are bound:

```rust
// Observer pattern with required ...
observer {
  event,
  state = {
    lights,
    person_tracker,
    zone,
    helpers = { guest_mode, ... },
    ...
  },
  ...
} /filter/ {
  // body
}
```

Rules:
- Must use `...` if any fields are not explicitly bound
- Nested destructuring is allowed
- Variables shadow outer scope

### Automation Definitions

#### Observer Syntax

```rust
observer {
  event,
  state = { field1, field2, ... },
  ...
} /condition/ {
  // body: must return [Event]
}
```

Example:
```rust
observer {
  event,
  state = {
    lights,
    person_tracker,
    zone,
    helpers = { guest_mode, ... },
    ...
  },
  ...
} /!guest_mode
   && event.type == Event::ZoneChange
   && event.device == person_tracker.jake
   && event.from == zone.home/ {

  wait(5 minutes, retry = cancel);

  if *person_tracker.jake != Zone::Home {
    [ Event::LightOff(l) for l in keys(lights) ]
  } else {
    []
  }
}
```

#### Mutator Syntax

```rust
mutator {
  event,
  state = { field1, field2, ... },
  ...
} /condition/ {
  // body: must return Event
}
```

Example:
```rust
mutator {
  event,
  state = { sun = { azimuth, ... }, ... },
  ...
} /event.type == Event::LightOn/ {

  let brightness = azimuth * 0.5;
  let colour = azimuth * 1.2;

  Event {
    inherit brightness colour;
    ...event
  }
}
```

### Filter Conditions

Filters use bpftrace-style `/condition/` syntax:

```rust
/event.type == Event::ZoneChange && event.device == person_tracker.jake/
```

- Boolean operators: `&&`, `||`, `!`
- Field access: `event.field`, `state.nested.field`
- Comparisons: `==`, `!=`, `<`, `>`, `<=`, `>=`
- Dereference: `*location`
- Optional chaining: `person?.location`

### Expressions

#### Let Bindings

```rust
let x = 42;
let result = compute(x);
```

#### If Expressions

If is an expression (returns a value):

```rust
if condition {
  value1
} else {
  value2
}
```

The `else` branch is required when used as an expression. Last expression in a block is the return value.

#### List Comprehensions

Python-style comprehensions with optional filter:

```rust
[ expr for item in collection ]
[ expr for item in collection if condition ]
```

Examples:
```rust
[ Event::LightOff(l) for l in keys(lights) ]
[ l for l in lights if l.brightness > 50 ]
[ l.id for room in rooms for l in room.lights if l.on ]
```

#### Struct Construction

Nix-inspired syntax with `inherit` and spread:

```rust
Event {
  inherit field1 field2;  // Shorthand for field1: field1, field2: field2
  field3: value,
  ...source               // Spread remaining fields from source
}
```

Multiple spreads are allowed but conflicts are compile-time errors:

```rust
Event {
  ...base,
  override_field: new_value,
  ...more_overrides  // Error if conflicts with override_field or base
}
```

#### Function Calls

```rust
function(arg1, arg2)
keys(lights)
wait(5 minutes, retry = cancel)
```

#### Field Access

```rust
event.type
state.sun.azimuth
```

#### Optional Chaining

Returns `None` if any part of the chain is `None`:

```rust
state.person?.location?.zone  // Returns None if person or location is None
```

#### Dereferencing

Explicit dereference with `*`:

```rust
*person_tracker.jake  // Dereferences to Location value
```

### Built-in Functions

#### Async Functions

These functions return values that can be used with the `await` keyword:

```rust
sleep(duration)         // Infallible sleep, always completes
sleep_unique(duration)  // Returns bool: true if completed, false if cancelled
```

`sleep_unique()` semantics:
- When the same automation instance triggers again, the previous instance is cancelled
- This implements atomic "most recent wins" behavior
- Cancelled instances receive `false` from `await sleep_unique()`
- Completed instances receive `true`

Example:
```rust
if await sleep_unique(5min) {
  // Timer completed - no re-trigger occurred
} else {
  // Timer was cancelled - automation re-triggered
}
```

#### Collection Functions

```rust
keys(map)           // Get map keys
values(map)         // Get map values
filter(list, fn)    // Filter list with predicate
len(collection)     // Length/count
```

#### Utility Functions

```rust
abs(n)              // Absolute value
min(a, b)           // Minimum
max(a, b)           // Maximum
clamp(n, lo, hi)    // Clamp value between lo and hi
```

## Type System

### Primitive Types

```rust
i64         // 64-bit signed integer
f64         // 64-bit float
bool        // Boolean
String      // UTF-8 string
```

### Composite Types

```rust
[T]                 // List of T
Set<T>              // Set of T
Map<K, V>           // Map from K to V
Option<T>           // Optional value (Some(T) or None)
```

### User-Defined Types

Types are defined in Rust and exposed to the automation language:

```rust
// Defined in hearthd Rust code
enum Event {
  ZoneChange { device: Device, from: Zone, to: Zone },
  LightOn { device: Light, brightness: i64, colour: i64 },
  LightOff { device: Light },
  // ...
}

enum Zone {
  Home,
  Work,
  Away,
  // ...
}

struct State {
  lights: Map<String, Light>,
  person_tracker: PersonTracker,
  zone: ZoneConfig,
  sun: SunData,
  helpers: Helpers,
  // ...
}
```

### Type Inference

Types are inferred where possible:

```rust
let x = 42;              // Inferred as i64
let y = 3.14;            // Inferred as f64
let lights = keys(map);  // Inferred from map type
```

### Type Checking

All type checking happens at load time:
- Field accesses are validated against state schema
- Function signatures are checked
- Return types must match (observer returns `[Event]`, mutator returns `Event`)
- Pattern destructuring is validated
- Spread conflicts detected at compile time

## Templates

Templates allow parameterized automations that return multiple observers/mutators:

```rust
{ target_lights: Set<Light> }:

[
  mutator {
    event,
    state = { sun = { azimuth, ... }, ... },
    ...
  } /event.type == Event::LightOn && event.device in target_lights/ {
    let brightness = azimuth * 0.5;
    let colour = azimuth * 1.2;

    Event {
      inherit brightness colour;
      ...event
    }
  }

  observer {
    event,
    state = { ... },
    ...
  } /event.type == Event::Tick/ {
    // Update lights on time tick
    [ Event::LightUpdate(l) for l in target_lights ]
  }
]
```

Template parameters require explicit types. Templates are instantiated in TOML configuration:

```toml
[automations.adaptive_lighting]
template = "adaptive_lighting.hda"
target_lights = ["living_room", "bedroom"]
```

## Error Handling

### Compile-Time Errors

The compiler validates:
- Syntax correctness
- Type consistency
- Field access validity
- Pattern completeness (required `...`)
- Spread conflicts in struct construction
- Return type matching (observer vs mutator)

Errors use the `ariadne` library for beautiful diagnostics with source context.

### Runtime Errors

Runtime errors result in:
- Silent failure (automation skipped)
- Error logged to hearthd logs
- Automation continues to be active for future events

Examples of runtime errors:
- Division by zero
- None value when Some expected (without `?` operator)
- Out-of-bounds access

## Automation Instance Lifecycle

Understanding how automation instances are created, executed, and cancelled is crucial for writing correct automations, especially when using `await sleep_unique()`.

### Instance Creation

When an automation's filter condition matches an event:
1. A new **instance** of that automation is created
2. The instance receives a snapshot of the event and state
3. The instance begins executing the automation body

### Atomic "Most Recent Wins" Semantics

For observers using `await sleep_unique()`:
- When a new instance is created, **any previous instance of the same automation is cancelled**
- This implements atomic "most recent wins" behavior
- Only the most recent instance continues executing

Example:
```rust
observer {
  event,
  state = { sensor, lights, ... },
  ...
} /event.type == Event::Motion && event.device == sensor/ {
  // Instance A starts when first motion event arrives
  if await sleep_unique(5min) {
    // If motion occurs again before 5min:
    // - Instance A is cancelled and receives false
    // - Instance B starts with a new 5min timer
    // This pattern continues until 5min pass with no motion
    [ Event::LightOff(l) for l in lights ]
  } else {
    [] // Cancelled by newer instance
  }
}
```

### Execution Model

- **Observers**: Multiple instances of different observers run in parallel
- **Same automation re-triggering**: New instance cancels previous instance
- **Mutators**: Process events sequentially, no cancellation needed

### Sleep Functions Behavior

- `sleep(duration)`: Never cancelled, always completes (blocks instance until done)
- `sleep_unique(duration)`: Can be cancelled by newer instance, returns bool:
  - `true` = Timer completed naturally
  - `false` = Cancelled by re-trigger of same automation

## Complete Examples

### Example 1: Leave Home Automation

Turn off all lights 5min after leaving home, unless guest mode is enabled:

```rust
observer {
  event,
  state = {
    lights,
    person_tracker,
    zone,
    helpers = { guest_mode, ... },
    ...
  },
  ...
} /!guest_mode
   && event.type == Event::ZoneChange
   && event.device == person_tracker.jake
   && event.from == zone.home/ {

  // Wait 5min, cancelled if automation re-triggers (Jake comes back)
  if await sleep_unique(5min) {
    // Timer completed - Jake stayed away, turn off all lights
    [ Event::LightOff(l) for l in keys(lights) ]
  } else {
    // Cancelled - Jake came back home, do nothing
    []
  }
}
```

### Example 2: Adaptive Lighting

Adjust light brightness and color based on sun position:

```rust
mutator {
  event,
  state = {
    sun = { azimuth, elevation, ... },
    ...
  },
  ...
} /event.type == Event::LightOn/ {

  // Calculate brightness from sun elevation (0-100)
  let brightness = clamp(elevation * 2, 0, 100);

  // Calculate colour temperature from azimuth (2700-6500K)
  let colour = 2700 + (azimuth / 360) * 3800;

  Event {
    inherit brightness colour;
    ...event
  }
}
```

### Example 3: Template for Room-Specific Automations

```rust
{ room_lights: Set<Light>, room_sensor: Sensor, timeout: Duration }:

[
  // Turn on lights when motion detected
  observer {
    event,
    state = { ... },
    ...
  } /event.type == Event::Motion && event.device == room_sensor/ {
    [ Event::LightOn(l) for l in room_lights ]
  }

  // Turn off lights after timeout with no motion
  observer {
    event,
    state = { ... },
    ...
  } /event.type == Event::Motion && event.device == room_sensor/ {
    // Uses atomic cancellation: each motion event cancels previous timer
    // and starts a new one, effectively implementing "restart" behavior
    if await sleep_unique(timeout) {
      // No motion for full timeout period - turn off lights
      [ Event::LightOff(l) for l in room_lights ]
    } else {
      // Motion detected again, new instance will handle turn-off
      []
    }
  }
]
```

## Grammar

### Lexical Structure

```
COMMENT       ::= '//' [^\n]* | '/*' ([^*] | '*' [^/])* '*/'
IDENTIFIER    ::= [a-zA-Z_][a-zA-Z0-9_]*
INTEGER       ::= '-'? [0-9]+
FLOAT         ::= '-'? [0-9]+ '.' [0-9]+
STRING        ::= '"' ([^"\\] | '\\' .)* '"'
UNIT_LITERAL  ::= (INTEGER | FLOAT) IDENTIFIER
                  // Suffix-style, no whitespace between number and unit
                  // e.g., 5min, 90deg, 2.5h
                  // Unit suffix validated at parse time

Keywords: observer, mutator, let, if, else, for, in, await,
          inherit, true, false, match, return

Operators: && || ! == != < > <= >= + - * / % ? . ...
```

### Grammar (EBNF)

```ebnf
program         ::= automation | template

template        ::= '{' param_list '}' ':' '[' automation_list ']'
param_list      ::= IDENTIFIER ':' type (',' IDENTIFIER ':' type)* ','?
automation_list ::= automation (',' automation)* ','?

automation      ::= ('observer' | 'mutator')
                    '{' pattern '}'
                    '/' expr '/'
                    '{' stmt_list '}'

pattern         ::= IDENTIFIER
                  | '{' field_pattern_list '}'
field_pattern_list ::= (IDENTIFIER ('=' pattern)? (',' IDENTIFIER ('=' pattern)?)*)? ',' '...'

stmt_list       ::= stmt*
stmt            ::= let_stmt | expr_stmt
let_stmt        ::= 'let' IDENTIFIER '=' expr ';'
expr_stmt       ::= expr (';')?

expr            ::= or_expr
or_expr         ::= and_expr ('||' and_expr)*
and_expr        ::= eq_expr ('&&' eq_expr)*
eq_expr         ::= rel_expr (('==' | '!=') rel_expr)*
rel_expr        ::= add_expr (('<' | '>' | '<=' | '>=') add_expr)*
add_expr        ::= mul_expr (('+' | '-') mul_expr)*
mul_expr        ::= unary_expr (('*' | '/' | '%') unary_expr)*
unary_expr      ::= ('!' | '-' | '*' | 'await') unary_expr | postfix_expr
postfix_expr    ::= primary_expr ('.' IDENTIFIER | '?.' IDENTIFIER | '(' arg_list ')')*

primary_expr    ::= IDENTIFIER
                  | INTEGER
                  | FLOAT
                  | STRING
                  | UNIT_LITERAL
                  | 'true' | 'false'
                  | '(' expr ')'
                  | if_expr
                  | list_comp
                  | struct_literal
                  | list_literal

if_expr         ::= 'if' expr '{' stmt_list '}' 'else' '{' stmt_list '}'

list_comp       ::= '[' expr 'for' IDENTIFIER 'in' expr ('if' expr)? ']'

struct_literal  ::= IDENTIFIER '{' struct_fields '}'
struct_fields   ::= (struct_field (',' struct_field)*)? ','?
struct_field    ::= 'inherit' IDENTIFIER
                  | '...' IDENTIFIER
                  | IDENTIFIER ':' expr

list_literal    ::= '[' (expr (',' expr)*)? ','? ']'

arg_list        ::= (arg (',' arg)*)? ','?
arg             ::= IDENTIFIER '=' expr | expr

type            ::= IDENTIFIER
                  | '[' type ']'
                  | 'Set' '<' type '>'
                  | 'Map' '<' type ',' type '>'
                  | 'Option' '<' type '>'
```

## Implementation Notes

### Parser Architecture

Recommended implementation in Rust:

1. **Lexer** (`src/lexer.rs`): Tokenize source into token stream
   - Use `logos` crate for efficient lexing
   - Track source positions for error reporting

2. **Parser** (`src/parser.rs`): Build AST from tokens
   - Recursive descent parser
   - Use `chumsky` or hand-written parser for better errors
   - Emit `Located<T>` nodes with source spans

3. **AST** (`src/ast.rs`): Abstract syntax tree types
   - Mirror grammar structure
   - Include source location in all nodes

4. **Type Checker** (`src/typecheck.rs`): Validate types
   - Check field accesses against state schema
   - Validate function calls
   - Ensure return types match (observer vs mutator)
   - Detect spread conflicts

5. **Diagnostics** (`src/diagnostics.rs`): Error reporting
   - Use existing `ariadne` from `hearthd_config`
   - Beautiful error messages with source context

### Integration with hearthd

1. Create new crate: `crates/hearthd_automations/`
2. Add to workspace in `/data/users/jake/repos/hearthd/Cargo.toml`
3. Define state schema from Rust types
4. Load `.hda` files from configuration directory
5. Compile to bytecode or interpret directly
6. Execute observers in parallel, mutators sequentially

### Future Considerations

- Optimization: Compile to bytecode or JIT
- Debugging: Source maps for runtime errors
- Hot reload: Watch `.hda` files for changes
- Testing: Unit test framework for automations
- Standard library: Expand built-in functions
- Pattern matching: Add `match` expressions
