//! Tests for the synchronous bytecode VM.
//!
//! Each test compiles real DSL source through the whole pipeline
//! (parse → desugar → check → HIR → LIR → bytecode → relocate) and executes
//! the result, so what runs here is exactly what the runner will run.
//!
//! [`compile`] asserts the source type-checks cleanly. The VM's safety
//! argument is that its input has already passed the checker, so a test
//! executing unchecked bytecode would prove nothing. Filters are wrapped
//! in `observer { event, ... } /FILTER/ { [event] }` and bodies in
//! `observer { event, ... } /true/ { BODY }` — the smallest observers that
//! check without errors.
//!
//! Outcomes are rendered to a string so a value and a [`VmError`] pin the
//! same way. Adding a case is one `insta::assert_snapshot!` with an empty
//! `@""`, then `cargo insta accept --all`.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::Pending;
use super::Program;
use super::Quantity;
use super::Suspension;
use super::Value;
use super::VmError;
use crate::automations::repr::BinOpTag;
use crate::automations::repr::Bytecode;
use crate::automations::repr::BytecodeAutomation;
use crate::automations::repr::BytecodeProgram;
use crate::automations::repr::Opcode;
use crate::automations::repr::function::FunctionIdentity;
use crate::automations::schema::DeploymentSchema;

// ============================================================================
// Harness
// ============================================================================

/// Compile DSL source to bytecode, asserting it type-checks cleanly.
fn compile(src: &str) -> BytecodeAutomation {
    let program = crate::automations::parse(src).expect("source should parse");
    let lowered = crate::automations::desugar_program(program);
    let checked = crate::automations::check_program(&lowered);
    assert!(
        checked.errors.is_empty(),
        "source should type-check cleanly, got {:?}",
        checked.errors,
    );
    let hir = crate::automations::lower_program(&checked);
    let lir = crate::automations::lower_lir_program(&hir);
    let relocatable = crate::automations::lower_bytecode_program(&lir);
    // These automations name no entities, so relocating against an empty
    // deployment resolves everything there is to resolve.
    let bytecode = crate::automations::relocate_program(&relocatable, &DeploymentSchema::default())
        .expect("source names no entities, so relocation has nothing to resolve");
    match bytecode {
        BytecodeProgram::Automation(auto) => auto,
        BytecodeProgram::Template { .. } => panic!("expected an Automation, got a Template"),
    }
}

/// The `event` argument fed to every filter and body.
///
/// Struct-like `Event` variants lower to a single-argument `Variant`
/// wrapping a field struct — the shape the VM's field access unwraps so
/// `event.node_id` resolves. Holding the convention in one place documents
/// what the runner will have to marshal engine events into.
fn sample_event() -> Value {
    event_with_node_id(7)
}

/// An `Event::OnOffChanged` carrying the given `node_id`.
fn event_with_node_id(node_id: i64) -> Value {
    Value::Variant {
        enum_name: "Event".to_string(),
        variant: "OnOffChanged".to_string(),
        args: vec![Value::Struct(BTreeMap::from([
            ("node_id".to_string(), Value::Int(node_id)),
            ("endpoint_id".to_string(), Value::Int(1)),
            (
                "attributes".to_string(),
                Value::Struct(BTreeMap::from([("on_off".to_string(), Value::Bool(true))])),
            ),
        ]))],
    }
}

/// Run a filter expression against [`sample_event`] and render the outcome.
fn run_filter(filter: &str) -> String {
    run_filter_with(filter, sample_event())
}

/// Run a filter expression against a specific `event` value.
fn run_filter_with(filter: &str, event: Value) -> String {
    let auto = compile(&format!(
        "observer {{ event, ... }} /{}/ {{ [event] }}",
        filter
    ));
    let bc = auto.filter.expect("an observer with a filter compiles one");
    build_and_run(bc, vec![event])
}

/// Run an automation body and render the outcome.
fn run_body(body: &str) -> String {
    let auto = compile(&format!("observer {{ event, ... }} /true/ {{ {} }}", body));
    build_and_run(auto.body, vec![sample_event()])
}

/// Run an automation body on the async driver and render the outcome.
///
/// Callers are `#[tokio::test(start_paused = true)]`, so the runtime
/// auto-advances its clock whenever nothing is runnable: a `sleep` resolves
/// as soon as the body is the only thing left, and a test costs no wall
/// time however long the automation waits.
async fn run_body_async(body: &str) -> String {
    run_body_with(body, &Timer).await
}

/// [`run_body_async`] against a given suspension policy.
async fn run_body_with(body: &str, suspension: &dyn Suspension) -> String {
    let auto = compile(&format!("observer {{ event, ... }} /true/ {{ {} }}", body));
    match Program::new(auto.body) {
        Ok(program) => render(
            Arc::new(program)
                .instance()
                .run_async(vec![sample_event()], suspension)
                .await,
        ),
        Err(err) => format!("error: {}", err),
    }
}

/// A [`Suspension`] with nothing to supersede anything: every wait is served
/// in full, so `sleep_unique` always finishes.
struct Timer;

#[async_trait::async_trait]
impl Suspension for Timer {
    async fn sleep_unique(&self, duration: std::time::Duration) -> bool {
        tokio::time::sleep(duration).await;
        true
    }
}

/// A [`Suspension`] where a newer instance always exists, so every
/// `sleep_unique` fails at once and no wait of one is ever served.
///
/// `sleep` is left defaulted, which is the point: nothing may cut one short,
/// so a driver that supersedes everything it can still has no say over it.
struct Superseded;

#[async_trait::async_trait]
impl Suspension for Superseded {
    async fn sleep_unique(&self, _duration: std::time::Duration) -> bool {
        false
    }
}

/// Describe the register interface a compiled filter exposes: the
/// parameters the runner must supply, and the register file size.
fn describe_filter(filter: &str) -> String {
    let auto = compile(&format!(
        "observer {{ event, ... }} /{}/ {{ [event] }}",
        filter
    ));
    let bc = auto.filter.expect("an observer with a filter compiles one");
    format!("params: [{}]\nnum_regs: {}", param_list(&bc), bc.num_regs)
}

/// Render a bytecode function's parameters as `name=rN` pairs.
fn param_list(bc: &Bytecode) -> String {
    bc.params
        .iter()
        .map(|p| format!("{}=r{}", p.name, p.reg))
        .collect::<Vec<String>>()
        .join(", ")
}

/// Build a `Bytecode` from a raw opcode stream, for cases the compiler
/// cannot produce (an unknown opcode, a bare `Await`).
fn raw_bytecode(num_regs: u32, code: Vec<u8>) -> Bytecode {
    Bytecode {
        params: Vec::new(),
        num_regs,
        consts: Vec::new(),
        code,
    }
}

/// Build a VM and run it, rendering a failure from either step the same
/// way. Construction can fail on its own — a unit literal that does not fit
/// its canonical unit is caught there, not at the instruction that loads it.
fn build_and_run(bc: Bytecode, params: Vec<Value>) -> String {
    match Program::new(bc) {
        Ok(program) => render(Arc::new(program).instance().run_sync(params)),
        Err(err) => format!("error: {}", err),
    }
}

/// [`build_and_run`] over a hand-assembled opcode stream.
fn build_and_run_raw(num_regs: u32, code: Vec<u8>) -> String {
    build_and_run(raw_bytecode(num_regs, code), Vec::new())
}

/// Render a VM outcome so values and errors snapshot uniformly.
fn render(outcome: Result<Value, VmError>) -> String {
    match outcome {
        Ok(value) => value.to_string(),
        Err(err) => format!("error: {}", err),
    }
}

// ============================================================================
// Literals and constants
// ============================================================================

#[test]
fn test_vm_const_bool_true() {
    insta::assert_snapshot!(run_filter("true"), @"true");
}

#[test]
fn test_vm_const_bool_false() {
    insta::assert_snapshot!(run_filter("false"), @"false");
}

#[test]
fn test_vm_const_string_eq() {
    insta::assert_snapshot!(run_filter(r#""a" == "a""#), @"true");
}

#[test]
fn test_vm_const_string_ne() {
    insta::assert_snapshot!(run_filter(r#""a" == "b""#), @"false");
}

/// `LoadConstUnit` decodes the literal into a [`Quantity`] carrying its
/// dimension, so two identical durations compare equal.
#[test]
fn test_vm_const_unit_literal_duration() {
    insta::assert_snapshot!(run_filter("5min == 5min"), @"true");
}

#[test]
fn test_vm_const_unit_literal_differing() {
    insta::assert_snapshot!(run_filter("5min == 6min"), @"false");
}

/// Equality holds across units of the same dimension: both sides normalise
/// to the canonical unit before comparing, so the spelling does not matter.
#[test]
fn test_vm_const_unit_literal_cross_unit_equal() {
    insta::assert_snapshot!(run_filter("1h == 60min"), @"true");
    insta::assert_snapshot!(run_filter("1d == 24h"), @"true");
    insta::assert_snapshot!(run_filter("180deg == 3.141592653589793rad"), @"true");
}

/// The same normalisation keeps differing magnitudes apart. Without it
/// `1h == 1min` compared the bare magnitudes `1` and `1` and held.
#[test]
fn test_vm_const_unit_literal_cross_unit_differing() {
    insta::assert_snapshot!(run_filter("1h == 1min"), @"false");
    insta::assert_snapshot!(run_filter("1h != 1min"), @"true");
    insta::assert_snapshot!(run_filter("90deg == 90rad"), @"false");
}

/// Temperature conversions are affine, so `20c` and `20f` are different
/// quantities while `20c` and `68f` are the same one.
#[test]
fn test_vm_const_unit_literal_temperature_offset() {
    insta::assert_snapshot!(run_filter("20c == 20f"), @"false");
    insta::assert_snapshot!(run_filter("20c == 293.15k"), @"true");
    insta::assert_snapshot!(run_filter("20c == 68f"), @"true");
    insta::assert_snapshot!(run_filter("32f == 273.15k"), @"true");
    insta::assert_snapshot!(run_filter("0c == 273.15k"), @"true");
}

/// Fahrenheit is one of the two conversions that cannot be exact — it
/// divides by nine — so it rounds to the nearest hundredth of a degree.
/// Two spellings of one temperature round to the same integer, which is
/// what makes them equal without any tolerance in the comparison.
#[test]
fn test_vm_const_unit_literal_conversion_is_rounded() {
    insta::assert_snapshot!(run_filter("206f == 96.66666666666667c"), @"true");
    insta::assert_snapshot!(run_filter("206f == 96.67c"), @"true");
}

/// Durations are exact past the point an `f64` stops holding integers,
/// which is about 104 days in nanoseconds. Scaling the literal's decimal
/// text straight to an integer is what keeps this true.
#[test]
fn test_vm_const_unit_literal_large_duration_is_exact() {
    insta::assert_snapshot!(run_filter("200d == 4800h"), @"true");
    insta::assert_snapshot!(run_filter("1000d == 86400000s"), @"true");
    insta::assert_snapshot!(run_filter("1000d == 86400001s"), @"false");
}

/// A quantity carries its dimension, so it never equals a bare number or a
/// quantity of another dimension — the checker types `==` as `Bool` for any
/// operand pair, so the VM is the only thing standing between `1h` and
/// `3600`.
#[test]
fn test_vm_const_unit_literal_dimension_distinguishes() {
    insta::assert_snapshot!(run_filter("1h == 3600"), @"false");
    insta::assert_snapshot!(run_filter("1rad == 1.0"), @"false");
    insta::assert_snapshot!(run_filter("1s == 1rad"), @"false");
    insta::assert_snapshot!(run_filter("0k == 0deg"), @"false");
}

/// A duration literal large enough to leave `i64` nanoseconds is the
/// automation's own doing, not a broken compiler: `1000000000d`
/// type-checks, so the VM must report it rather than panic. It surfaces
/// when the VM is built, so an automation that can never run is rejected
/// once at deploy rather than on the event that first reaches the literal.
#[test]
fn test_vm_unit_literal_overflow_is_reported() {
    insta::assert_snapshot!(
        run_filter("1000000000d == 1s"),
        @"error: integer overflow: unit literal `1000000000d`"
    );
    // The largest duration that still fits, either side of the boundary.
    insta::assert_snapshot!(run_filter("106751d == 106751d"), @"true");
}

/// `Display` renders the canonical integer back in the unit an author
/// writes, which is exact because the two differ by a power of ten.
#[test]
fn test_quantity_display_renders_in_authored_units() {
    use crate::automations::repr::ast::UnitType;

    let rendered = [
        (UnitType::Hours, "1.5"),
        (UnitType::Seconds, "0.25"),
        (UnitType::Days, "1000"),
        (UnitType::Degrees, "90.5"),
        (UnitType::Celsius, "20"),
        (UnitType::Kelvin, "0"),
        (UnitType::Fahrenheit, "206"),
    ]
    .map(|(unit, text)| {
        format!(
            "{}{} => {}",
            text,
            unit,
            Quantity::from_unit_literal(unit, text).expect("representable")
        )
    })
    .join("\n");

    insta::assert_snapshot!(rendered, @"
    1.5h => 5400s
    0.25s => 0.25s
    1000d => 86400000s
    90.5deg => 90.5deg
    20c => 20c
    0k => -273.15c
    206f => 96.67c
    ");
}

/// The canonical magnitude each unit converts to, pinned directly rather
/// than through a filter: an observer body must return `[Event]`, so a
/// quantity cannot be the result of a compiled automation. This is the
/// conversion table the equality tests above rest on.
#[test]
fn test_quantity_canonical_conversions() {
    use crate::automations::repr::ast::UnitType;

    let rendered = [
        UnitType::Seconds,
        UnitType::Minutes,
        UnitType::Hours,
        UnitType::Days,
        UnitType::Degrees,
        UnitType::Radians,
        UnitType::Celsius,
        UnitType::Fahrenheit,
        UnitType::Kelvin,
    ]
    .map(|unit| {
        format!(
            "1{} => {:?}",
            unit,
            Quantity::from_unit_literal(unit, "1").expect("1 of any unit is representable")
        )
    })
    .join("\n");

    insta::assert_snapshot!(rendered, @"
    1s => Duration(1000000000)
    1min => Duration(60000000000)
    1h => Duration(3600000000000)
    1d => Duration(86400000000000)
    1deg => Angle(10)
    1rad => Angle(573)
    1c => Temperature(100)
    1f => Temperature(-1722)
    1k => Temperature(-27215)
    ");
}

// ============================================================================
// Integer arithmetic
// ============================================================================

#[test]
fn test_vm_int_add() {
    insta::assert_snapshot!(run_filter("1 + 2 == 3"), @"true");
}

#[test]
fn test_vm_int_sub() {
    insta::assert_snapshot!(run_filter("5 - 3 == 2"), @"true");
}

#[test]
fn test_vm_int_mul() {
    insta::assert_snapshot!(run_filter("3 * 4 == 12"), @"true");
}

#[test]
fn test_vm_int_div_truncates() {
    insta::assert_snapshot!(run_filter("7 / 2 == 3"), @"true");
}

#[test]
fn test_vm_int_mod() {
    insta::assert_snapshot!(run_filter("7 % 3 == 1"), @"true");
}

#[test]
fn test_vm_int_precedence() {
    insta::assert_snapshot!(run_filter("1 + 2 * 3 == 7"), @"true");
}

#[test]
fn test_vm_int_arithmetic_false() {
    insta::assert_snapshot!(run_filter("1 + 2 == 4"), @"false");
}

// ============================================================================
// Checked integer arithmetic
//
// Filter source is user-authored, so these must surface as `VmError`
// rather than panicking the thread the runner evaluates filters on.
// ============================================================================

#[test]
fn test_vm_div_by_zero_is_an_error() {
    insta::assert_snapshot!(run_filter("1 / 0 == 0"), @"error: divide by zero");
}

#[test]
fn test_vm_mod_by_zero_is_an_error() {
    insta::assert_snapshot!(run_filter("1 % 0 == 0"), @"error: divide by zero");
}

/// `i64::MIN % -1` is 0 and representable, so it is an answer rather than
/// an overflow. Only the matching division genuinely leaves the `i64`
/// range, which is why `checked_rem` would be wrong here.
#[test]
fn test_vm_mod_min_by_negative_one() {
    insta::assert_snapshot!(run_filter("(0 - 9223372036854775807 - 1) % (0 - 1) == 0"), @"true");
    insta::assert_snapshot!(
        run_filter("(0 - 9223372036854775807 - 1) / (0 - 1) == 0"),
        @"error: integer overflow: -9223372036854775808 div -1"
    );
}

#[test]
fn test_vm_add_overflow_is_an_error() {
    insta::assert_snapshot!(run_filter("9223372036854775807 + 1 == 0"), @"error: integer overflow: 9223372036854775807 add 1");
}

#[test]
fn test_vm_sub_overflow_is_an_error() {
    insta::assert_snapshot!(run_filter("(0 - 9223372036854775807) - 2 == 0"), @"error: integer overflow: -9223372036854775807 sub 2");
}

#[test]
fn test_vm_mul_overflow_is_an_error() {
    insta::assert_snapshot!(run_filter("9223372036854775807 * 2 == 0"), @"error: integer overflow: 9223372036854775807 mul 2");
}

#[test]
fn test_vm_neg_overflow_is_an_error() {
    insta::assert_snapshot!(run_filter("-(0 - 9223372036854775807 - 1) == 0"), @"error: integer overflow: neg -9223372036854775808");
}

#[test]
fn test_vm_abs_overflow_is_an_error() {
    insta::assert_snapshot!(run_filter("abs(0 - 9223372036854775807 - 1) == 0"), @"error: integer overflow: abs -9223372036854775808");
}

/// Float division by zero follows IEEE 754 rather than erroring.
#[test]
fn test_vm_float_div_by_zero_is_infinity() {
    insta::assert_snapshot!(run_filter("1.0 / 0.0 == 0.0"), @"false");
}

// ============================================================================
// Float arithmetic
// ============================================================================

#[test]
fn test_vm_float_add() {
    insta::assert_snapshot!(run_filter("1.5 + 2.5 == 4.0"), @"true");
}

#[test]
fn test_vm_float_sub() {
    insta::assert_snapshot!(run_filter("2.5 - 1.0 == 1.5"), @"true");
}

#[test]
fn test_vm_float_mul() {
    insta::assert_snapshot!(run_filter("1.5 * 2.0 == 3.0"), @"true");
}

#[test]
fn test_vm_float_div() {
    insta::assert_snapshot!(run_filter("3.0 / 2.0 == 1.5"), @"true");
}

// ============================================================================
// Comparisons
// ============================================================================

#[test]
fn test_vm_int_lt() {
    insta::assert_snapshot!(run_filter("1 < 2"), @"true");
}

#[test]
fn test_vm_int_le_at_boundary() {
    insta::assert_snapshot!(run_filter("2 <= 2"), @"true");
}

#[test]
fn test_vm_int_gt() {
    insta::assert_snapshot!(run_filter("1 > 2"), @"false");
}

#[test]
fn test_vm_int_ge_at_boundary() {
    insta::assert_snapshot!(run_filter("2 >= 2"), @"true");
}

#[test]
fn test_vm_int_ne() {
    insta::assert_snapshot!(run_filter("1 != 2"), @"true");
}

#[test]
fn test_vm_float_lt() {
    insta::assert_snapshot!(run_filter("1.5 < 2.5"), @"true");
}

#[test]
fn test_vm_float_ge() {
    insta::assert_snapshot!(run_filter("2.5 >= 2.5"), @"true");
}

/// Equality is structural over `Value`, so it covers any two values of the
/// same shape without a per-type arm.
#[test]
fn test_vm_eq_is_structural_over_lists() {
    insta::assert_snapshot!(run_filter("[1, 2] == [1, 2]"), @"true");
}

#[test]
fn test_vm_eq_is_structural_over_nested_lists() {
    insta::assert_snapshot!(run_filter("[[1], [2]] == [[1], [3]]"), @"false");
}

// ============================================================================
// Logical operators
//
// `&&` and `||` desugar to branches, so these exercise `JumpIf`, `Jump`
// and the `Copy` at the merge point rather than any binop.
// ============================================================================

#[test]
fn test_vm_and_both_true() {
    insta::assert_snapshot!(run_filter("true && (1 < 2)"), @"true");
}

/// A division by zero on the right-hand side would error if it ran, so
/// this pins that the short circuit really skips it.
#[test]
fn test_vm_and_short_circuits_on_false_lhs() {
    insta::assert_snapshot!(run_filter("false && (1 / 0 == 0)"), @"false");
}

#[test]
fn test_vm_and_false_rhs() {
    insta::assert_snapshot!(run_filter("true && false"), @"false");
}

#[test]
fn test_vm_or_short_circuits_on_true_lhs() {
    insta::assert_snapshot!(run_filter("true || (1 / 0 == 0)"), @"true");
}

#[test]
fn test_vm_or_falls_through_to_rhs() {
    insta::assert_snapshot!(run_filter("false || true"), @"true");
}

#[test]
fn test_vm_or_both_false() {
    insta::assert_snapshot!(run_filter("false || false"), @"false");
}

#[test]
fn test_vm_and_chain() {
    insta::assert_snapshot!(run_filter("(1 + 2) > 0 && (1 + 2) < 10"), @"true");
}

#[test]
fn test_vm_and_or_mixed() {
    insta::assert_snapshot!(run_filter("false && true || true"), @"true");
}

// ============================================================================
// Unary operators
// ============================================================================

#[test]
fn test_vm_not() {
    insta::assert_snapshot!(run_filter("!(1 == 2)"), @"true");
}

#[test]
fn test_vm_not_double() {
    insta::assert_snapshot!(run_filter("!!true"), @"true");
}

#[test]
fn test_vm_neg_int() {
    insta::assert_snapshot!(run_filter("-(3) == 0 - 3"), @"true");
}

#[test]
fn test_vm_neg_float() {
    insta::assert_snapshot!(run_filter("-(1.5) == 0.0 - 1.5"), @"true");
}

// ============================================================================
// Field access
// ============================================================================

#[test]
fn test_vm_field_on_event() {
    insta::assert_snapshot!(run_filter("event.node_id == 7"), @"true");
}

#[test]
fn test_vm_field_on_event_mismatch() {
    insta::assert_snapshot!(run_filter("event.endpoint_id == 99"), @"false");
}

#[test]
fn test_vm_field_nested() {
    insta::assert_snapshot!(run_filter("event.attributes.on_off"), @"true");
}

/// `OptionalField` decodes identically to `Field` and behaves the same;
/// `Value` has no `Option` representation for it to return.
#[test]
fn test_vm_optional_field_behaves_like_field() {
    insta::assert_snapshot!(run_filter("event?.node_id == 7"), @"true");
}

#[test]
fn test_vm_field_missing_is_an_error() {
    let event = Value::Variant {
        enum_name: "Event".to_string(),
        variant: "OnOffChanged".to_string(),
        args: vec![Value::Struct(BTreeMap::new())],
    };
    insta::assert_snapshot!(run_filter_with("event.node_id == 7", event), @"error: VM invariant violated: unknown field `node_id`");
}

#[test]
fn test_vm_field_on_non_struct_is_an_error() {
    insta::assert_snapshot!(run_filter_with("event.node_id == 7", Value::Int(1)), @"error: VM invariant violated: field access `.node_id` on Int(1)");
}

// ============================================================================
// Control flow
// ============================================================================

#[test]
fn test_vm_if_else_takes_then_branch() {
    insta::assert_snapshot!(run_body("if true { [event] } else { [] }"), @"[Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: 7})]");
}

#[test]
fn test_vm_if_else_takes_else_branch() {
    insta::assert_snapshot!(run_body("if false { [] } else { [event] }"), @"[Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: 7})]");
}

#[test]
fn test_vm_if_without_else_falls_through() {
    insta::assert_snapshot!(run_body("if false { 1 }; [event]"), @"[Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: 7})]");
}

#[test]
fn test_vm_let_bindings_in_body() {
    insta::assert_snapshot!(run_body("let x = 1; let y = x + 1; [event]"), @"[Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: 7})]");
}

// ============================================================================
// Lists and comprehensions
//
// Comprehensions are the only construct that reaches `IterInit`,
// `IterNext`, `EmptyList` and `ListPush`.
// ============================================================================

#[test]
fn test_vm_empty_list() {
    insta::assert_snapshot!(run_filter("[1] != []"), @"true");
}

#[test]
fn test_vm_list_literal() {
    insta::assert_snapshot!(run_filter("[1, 2, 3] == [1, 2, 3]"), @"true");
}

#[test]
fn test_vm_comprehension_identity() {
    insta::assert_snapshot!(run_filter("[x for x in [1, 2, 3]] == [1, 2, 3]"), @"true");
}

#[test]
fn test_vm_comprehension_maps_each_element() {
    insta::assert_snapshot!(run_filter("[x * 2 for x in [1, 2, 3]] == [2, 4, 6]"), @"true");
}

#[test]
fn test_vm_comprehension_with_filter() {
    insta::assert_snapshot!(run_filter("[x for x in [1, 2, 3] if x > 1] == [2, 3]"), @"true");
}

#[test]
fn test_vm_comprehension_over_empty_list() {
    insta::assert_snapshot!(run_filter("[x for x in []] == []"), @"true");
}

#[test]
fn test_vm_comprehension_filters_everything_out() {
    insta::assert_snapshot!(run_filter("[x for x in [1, 2, 3] if x > 99] == []"), @"true");
}

#[test]
fn test_vm_nested_comprehension() {
    insta::assert_snapshot!(run_filter("[[y for y in [1, 2]] for x in [1, 2]] == [[1, 2], [1, 2]]"), @"true");
}

// ============================================================================
// Struct and variant construction
// ============================================================================

#[test]
fn test_vm_struct_literal() {
    insta::assert_snapshot!(run_body("OnOffCluster { on_off: true }; [event]"), @"[Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: 7})]");
}

#[test]
fn test_vm_struct_spread() {
    insta::assert_snapshot!(run_body("let a = event.attributes; OnOffCluster { ...a }; [event]"), @"[Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: 7})]");
}

#[test]
fn test_vm_variant_construction() {
    insta::assert_snapshot!(run_body("[Event::OnOffChanged(1, 2, event.attributes)]"), @"[Event::OnOffChanged(1, 2, {on_off: true})]");
}

#[test]
fn test_vm_variant_construction_from_event_fields() {
    insta::assert_snapshot!(run_body("[Event::OnOffChanged(event.node_id, event.endpoint_id, event.attributes)]"), @"[Event::OnOffChanged(7, 1, {on_off: true})]");
}

// ============================================================================
// Builtins
// ============================================================================

#[test]
fn test_vm_builtin_len_list() {
    insta::assert_snapshot!(run_filter("len([1, 2, 3]) == 3"), @"true");
}

#[test]
fn test_vm_builtin_len_empty_list() {
    insta::assert_snapshot!(run_filter("len([]) == 0"), @"true");
}

#[test]
fn test_vm_builtin_len_string() {
    insta::assert_snapshot!(run_filter(r#"len("abc") == 3"#), @"true");
}

/// `len` on a string counts characters, not UTF-8 bytes, so a name with an
/// accent in it does not read as longer than it looks.
#[test]
fn test_vm_builtin_len_string_is_not_bytes() {
    insta::assert_snapshot!(run_filter(r#"len("é") == 1"#), @"true");
    insta::assert_snapshot!(run_filter(r#"len("café") == 4"#), @"true");
}

#[test]
fn test_vm_builtin_abs_negative() {
    insta::assert_snapshot!(run_filter("abs(0 - 5) == 5"), @"true");
}

#[test]
fn test_vm_builtin_abs_float() {
    insta::assert_snapshot!(run_filter("abs(0.0 - 1.5) == 1.5"), @"true");
}

// ============================================================================
// Parameter interface
//
// The runner supplies filter arguments positionally against the registers
// the bytecode declares; these pin that contract.
// ============================================================================

#[test]
fn test_vm_filter_declares_pattern_bound_params() {
    insta::assert_snapshot!(describe_filter("true"), @"
    params: [event=r0]
    num_regs: 2
    ");
}

/// Only top-level pattern fields become parameters. A nested binding like
/// `nodes` is extracted from `state` by the function's own prelude, so the
/// runner supplies `state` whole rather than each leaf.
#[test]
fn test_vm_filter_params_grow_with_the_pattern() {
    let auto = compile("observer { event, state = { nodes, ... }, ... } /true/ { [event] }");
    let bc = auto.filter.expect("filter");
    insta::assert_snapshot!(param_list(&bc), @"event=r0, state=r1");
}

#[test]
fn test_vm_param_value_reaches_the_filter() {
    let event = Value::Variant {
        enum_name: "Event".to_string(),
        variant: "OnOffChanged".to_string(),
        args: vec![Value::Struct(BTreeMap::from([(
            "node_id".to_string(),
            Value::Int(99),
        )]))],
    };
    insta::assert_snapshot!(run_filter_with("event.node_id == 99", event), @"true");
}

#[test]
#[should_panic(expected = "param count mismatch")]
fn test_vm_param_count_mismatch_panics() {
    let auto = compile("observer { event, ... } /true/ { [event] }");
    let bc = auto.filter.expect("filter");
    let _ = build_and_run(bc, Vec::new());
}

// ============================================================================
// Error paths
// ============================================================================

/// `Await` is rejected outright: filters must not suspend, and this
/// flavor of the VM has no executor to suspend onto. The compiler cannot
/// emit a bare `Await` into a filter, so the stream is built by hand.
#[test]
fn test_vm_await_is_rejected_in_sync() {
    let mut code = vec![Opcode::Await as u8];
    code.extend_from_slice(&0u32.to_le_bytes());
    code.extend_from_slice(&1u32.to_le_bytes());
    insta::assert_snapshot!(build_and_run_raw(2, code), @"error: VM invariant violated: await in a function the checker promised cannot suspend");
}

#[test]
fn test_vm_unknown_opcode_is_an_error() {
    insta::assert_snapshot!(build_and_run_raw(1, vec![0xff]), @"error: VM invariant violated: undecodable opcode 0xff");
}

#[test]
fn test_vm_jump_if_on_non_bool_is_an_error() {
    let mut code = vec![Opcode::JumpIf as u8];
    // Condition register 0 is left at its initial `Unit`.
    for _ in 0..3 {
        code.extend_from_slice(&0u32.to_le_bytes());
    }
    insta::assert_snapshot!(build_and_run_raw(1, code), @"error: VM invariant violated: jump_if on Unit");
}

#[test]
fn test_vm_iter_next_on_non_iter_is_an_error() {
    let mut code = vec![Opcode::IterNext as u8];
    for _ in 0..4 {
        code.extend_from_slice(&0u32.to_le_bytes());
    }
    insta::assert_snapshot!(build_and_run_raw(1, code), @"error: VM invariant violated: iter_next on Unit");
}

/// The checker requires a collection on the right of `in`, so this arm is
/// only reachable if a runtime value contradicts its static type.
#[test]
fn test_vm_in_on_non_collection_is_an_error() {
    let mut code = vec![Opcode::BinOp as u8];
    code.extend_from_slice(&0u32.to_le_bytes()); // dst
    code.push(BinOpTag::In as u8);
    code.extend_from_slice(&0u32.to_le_bytes()); // needle, left Unit
    code.extend_from_slice(&0u32.to_le_bytes()); // haystack, left Unit
    insta::assert_snapshot!(build_and_run_raw(1, code), @"error: VM invariant violated: in on Unit");
}

#[test]
fn test_vm_list_push_on_non_list_is_an_error() {
    let mut code = vec![Opcode::ListPush as u8];
    for _ in 0..2 {
        code.extend_from_slice(&0u32.to_le_bytes());
    }
    insta::assert_snapshot!(build_and_run_raw(1, code), @"error: VM invariant violated: list_push on Unit");
}
// ============================================================================
// Membership
// ============================================================================

/// The operator the design doc's canonical filter uses
/// (`event.device in target_lights`).
#[test]
fn test_vm_in_list_present() {
    insta::assert_snapshot!(run_filter("2 in [1, 2, 3]"), @"true");
}

#[test]
fn test_vm_in_list_absent() {
    insta::assert_snapshot!(run_filter("9 in [1, 2, 3]"), @"false");
}

#[test]
fn test_vm_in_empty_list() {
    insta::assert_snapshot!(run_filter("1 in []"), @"false");
}

#[test]
fn test_vm_in_list_of_strings() {
    insta::assert_snapshot!(run_filter(r#""b" in ["a", "b"]"#), @"true");
}

/// Membership uses the same promoting equality as `==`.
#[test]
fn test_vm_in_promotes_numerics() {
    insta::assert_snapshot!(run_filter("1 in [1.0, 2.0]"), @"true");
}

// ============================================================================
// Numeric promotion
//
// The checker specifies that a `Float` on either side contaminates the
// result, so mixed operands are promoted rather than rejected. Integer pairs
// keep their exact checked arithmetic.
// ============================================================================

#[test]
fn test_vm_mixed_comparison_int_lhs() {
    insta::assert_snapshot!(run_filter("1 < 2.0"), @"true");
}

#[test]
fn test_vm_mixed_comparison_float_lhs() {
    insta::assert_snapshot!(run_filter("2.5 > 2"), @"true");
}

#[test]
fn test_vm_mixed_addition_yields_float() {
    insta::assert_snapshot!(run_filter("(1 + 0.5) == 1.5"), @"true");
}

#[test]
fn test_vm_mixed_division_does_not_truncate() {
    insta::assert_snapshot!(run_filter("(1 / 2.0) == 0.5"), @"true");
}

/// Integer division still truncates when both sides are `Int`, so promotion
/// has not leaked into the same-type path.
#[test]
fn test_vm_int_division_still_truncates() {
    insta::assert_snapshot!(run_filter("(1 / 2) == 0"), @"true");
}

/// Overflow checking is likewise unaffected: an `Int` pair never promotes.
#[test]
fn test_vm_int_pair_still_overflow_checked() {
    insta::assert_snapshot!(run_filter("9223372036854775807 + 1 == 0"), @"error: integer overflow: 9223372036854775807 add 1");
}

#[test]
fn test_vm_mixed_equality() {
    insta::assert_snapshot!(run_filter("1 == 1.0"), @"true");
}

#[test]
fn test_vm_mixed_inequality() {
    insta::assert_snapshot!(run_filter("1 != 1.5"), @"true");
}

/// Equality stays structural for anything non-numeric.
#[test]
fn test_vm_equality_of_different_kinds() {
    insta::assert_snapshot!(run_filter(r#"[1] == "1""#), @"false");
}

/// The promotion reaches inside containers, so it does not matter how
/// deeply a number is buried: if `1 == 1.0` holds then so does
/// `[1] == [1.0]`.
#[test]
fn test_vm_mixed_equality_nested() {
    insta::assert_snapshot!(run_filter("[1] == [1.0]"), @"true");
    insta::assert_snapshot!(run_filter("[[1], [2]] == [[1.0], [2.0]]"), @"true");
    insta::assert_snapshot!(run_filter("1 in [1.0]"), @"true");
    insta::assert_snapshot!(run_filter("[1] in [[1.0]]"), @"true");
    insta::assert_snapshot!(run_filter("[1] == [1.5]"), @"false");
    insta::assert_snapshot!(run_filter("[1] == [1.0, 2.0]"), @"false");
}

/// The promotion reaches through variants and structs as well as lists,
/// and the enum and variant names still have to agree.
#[test]
fn test_vm_equality_on_a_struct_is_refused() {
    // The shape `TemperatureMeasurementCluster` and
    // `RelativeHumidityMeasurementCluster` share: one `measured_value`.
    // Structurally these are the same value, which is exactly why the VM
    // must not answer.
    let reading = || {
        Value::Struct(BTreeMap::from([(
            "measured_value".to_string(),
            Value::Int(20),
        )]))
    };
    insta::assert_snapshot!(
        render(super::ops::values_equal(&reading(), &reading()).map(Value::Bool)),
        @"error: VM invariant violated: equality on a value carrying no identity: {measured_value: 20} and {measured_value: 20}"
    );
}

/// The same refusal covers variants, and reaches a struct nested inside
/// one — the shape the runner actually binds `event` as.
#[test]
fn test_vm_equality_on_a_variant_is_refused() {
    insta::assert_snapshot!(
        render(super::ops::values_equal(&sample_event(), &sample_event()).map(Value::Bool)),
        @"error: VM invariant violated: equality on a value carrying no identity: Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: 7}) and Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: 7})"
    );
    insta::assert_snapshot!(
        render(
            super::ops::values_equal(
                &Value::List(vec![sample_event()]),
                &Value::List(vec![sample_event()]),
            )
            .map(Value::Bool)
        ),
        @"error: VM invariant violated: equality on a value carrying no identity: Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: 7}) and Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: 7})"
    );
}

#[test]
fn test_vm_float_mod() {
    insta::assert_snapshot!(run_filter("(5.5 % 2.0) == 1.5"), @"true");
}

#[test]
fn test_vm_mixed_mod() {
    insta::assert_snapshot!(run_filter("(5 % 2.0) == 1.0"), @"true");
}

// ============================================================================
// Numeric builtins
//
// `min`/`max`/`clamp` follow the same promotion rule as the operators.
// ============================================================================

#[test]
fn test_vm_builtin_min_ints() {
    insta::assert_snapshot!(run_filter("min(1, 2) == 1"), @"true");
}

#[test]
fn test_vm_builtin_max_ints() {
    insta::assert_snapshot!(run_filter("max(1, 2) == 2"), @"true");
}

#[test]
fn test_vm_builtin_min_promotes() {
    insta::assert_snapshot!(run_filter("min(1, 0.5) == 0.5"), @"true");
}

#[test]
fn test_vm_builtin_max_floats() {
    insta::assert_snapshot!(run_filter("max(1.5, 2.5) == 2.5"), @"true");
}

#[test]
fn test_vm_builtin_clamp_within_range() {
    insta::assert_snapshot!(run_filter("clamp(5, 0, 9) == 5"), @"true");
}

#[test]
fn test_vm_builtin_clamp_below_range() {
    insta::assert_snapshot!(run_filter("clamp(0 - 5, 0, 9) == 0"), @"true");
}

#[test]
fn test_vm_builtin_clamp_above_range() {
    insta::assert_snapshot!(run_filter("clamp(50, 0, 9) == 9"), @"true");
}

#[test]
fn test_vm_builtin_clamp_promotes() {
    insta::assert_snapshot!(run_filter("clamp(5, 0.0, 1.5) == 1.5"), @"true");
}

/// `clamp` is total rather than panicking like `f64::clamp`: inverted
/// bounds are used in whichever order puts the smaller first.
#[test]
fn test_vm_builtin_clamp_inverted_bounds_int() {
    insta::assert_snapshot!(run_filter("clamp(5, 9, 0) == 5"), @"true");
}

#[test]
fn test_vm_builtin_clamp_inverted_bounds_clamps() {
    insta::assert_snapshot!(run_filter("clamp(50, 9, 0) == 9"), @"true");
}

#[test]
fn test_vm_builtin_clamp_inverted_bounds_float() {
    insta::assert_snapshot!(run_filter("clamp(50.0, 9.0, 0.0) == 9.0"), @"true");
}

/// A NaN bound constrains nothing, so the value passes the NaN side and is
/// still bounded by the other.
#[test]
fn test_vm_builtin_clamp_nan_upper_bound() {
    insta::assert_snapshot!(run_filter("clamp(50.0, 0.0, 0.0 / 0.0) == 50.0"), @"true");
}

#[test]
fn test_vm_builtin_clamp_nan_lower_bound_still_caps() {
    insta::assert_snapshot!(run_filter("clamp(50.0, 0.0 / 0.0, 9.0) == 9.0"), @"true");
}

/// A NaN value compares false against both bounds, so it passes through.
/// Asserted via NaN's self-inequality, since the result being non-zero alone
/// would not distinguish pass-through from clamping to a bound.
#[test]
fn test_vm_builtin_clamp_nan_value_passes_through() {
    insta::assert_snapshot!(run_filter("clamp(0.0 / 0.0, 0.0, 9.0) != clamp(0.0 / 0.0, 0.0, 9.0)"), @"true");
}

// ============================================================================
// Futures
//
// `sleep` builds a handle carrying the wait it stands for. A filter may
// construct one and discard it; only the async driver may look inside.
// ============================================================================

/// A filter can construct a future and count it without observing it, so
/// the call succeeds where anything that inspected the handle would not.
#[test]
fn test_vm_filter_may_construct_a_future() {
    insta::assert_snapshot!(run_filter("len([sleep(5min)]) == 1"), @"true");
}

/// The synchronous driver refuses the `Await` opcode rather than running
/// past it. This is the promise `run_sync` rests on: the checker keeps
/// `await` out of a filter, so reaching one means something above the VM
/// is broken.
#[test]
fn test_vm_sync_driver_refuses_await() {
    insta::assert_snapshot!(
        run_body("await sleep(5min); [event]"),
        @"error: VM invariant violated: await in a function the checker promised cannot suspend"
    );
}

/// Two futures standing for the same wait are still distinct suspensions,
/// so equality is reported rather than answered. The checker rejects
/// `Future` equality, which makes this unreachable from source and a
/// standing assertion that it stays that way.
#[test]
fn test_vm_future_equality_is_rejected() {
    insta::assert_snapshot!(
        render(
            super::ops::values_equal(
                &Value::Future(Pending::Sleep(1)),
                &Value::Future(Pending::Sleep(1)),
            )
            .map(Value::Bool)
        ),
        @"error: VM invariant violated: equality on an unawaited future"
    );
}

/// A future renders as the wait it stands for, so a diagnostic naming one
/// says which builtin built it and how long it runs.
#[test]
fn test_vm_future_renders_its_wait() {
    insta::assert_snapshot!(Value::Future(Pending::SleepUnique(300_000_000_000)), @"<sleep_unique(300s)>");
}

// ============================================================================
// The async driver
//
// Everything above runs on `run_sync`. These run the same compiled bodies on
// `run_async`, which is the only driver that may pass an `Await`.
// ============================================================================

/// A body suspends at its `await` and runs on afterwards to its return
/// value. `sleep` is typed `Future<()>`, so the `await` is a statement
/// rather than something to branch on.
#[tokio::test(start_paused = true)]
async fn test_vm_async_await_resumes_the_body() {
    insta::assert_snapshot!(
        run_body_async("await sleep(5min); [event]").await,
        @"[Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: 7})]"
    );
}

/// `sleep_unique` suspends exactly as `sleep` does, and resumes `true`: a
/// wait that reaches its end is by definition the one that was not
/// superseded. Whether it was is the [`Suspension`]'s to say — [`Timer`]
/// has nothing to supersede it with, so every wait it serves resolves
/// `true`.
#[tokio::test(start_paused = true)]
async fn test_vm_async_sleep_unique_resumes_true() {
    insta::assert_snapshot!(
        run_body_async("if await sleep_unique(5min) { [] } else { [event] }").await,
        @"[]"
    );
}

/// The await really waits: the clock advances by the full duration, so a
/// body cannot resume early.
#[tokio::test(start_paused = true)]
async fn test_vm_async_await_waits_out_its_duration() {
    let start = tokio::time::Instant::now();
    let _ = run_body_async("await sleep(1h); [event]").await;
    insta::assert_snapshot!(start.elapsed().as_secs(), @"3600");
}

/// Two suspensions in one body, so the driver loops back to `poll` and the
/// second `await` picks up where the first left off. One `await` cannot pin
/// that: it is the resumed program counter that the operand reading after a
/// suspension has to leave correct, and the waits sum only if both ran.
#[tokio::test(start_paused = true)]
async fn test_vm_async_successive_awaits_each_resume() {
    let start = tokio::time::Instant::now();
    let rendered = run_body_async("await sleep(1min); await sleep(2min); [event]").await;
    insta::assert_snapshot!(rendered, @"[Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: 7})]");
    insta::assert_snapshot!(start.elapsed().as_secs(), @"180");
}

/// A body with no `await` runs on the async driver unchanged — the two
/// drivers share the dispatch loop and differ only at the suspension.
#[tokio::test(start_paused = true)]
async fn test_vm_async_body_without_await() {
    insta::assert_snapshot!(run_body_async("[event]").await, @"[Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: 7})]");
}

/// A body whose `sleep_unique` is superseded takes its `else` branch and
/// runs on. This is the path the `Future<Bool>` typing exists for, and the
/// reason the machine asks a [`Suspension`] rather than deciding itself: a
/// register machine has no notion of a second instance.
#[tokio::test(start_paused = true)]
async fn test_vm_async_a_superseded_sleep_unique_resolves_false() {
    insta::assert_snapshot!(
        run_body_with("if await sleep_unique(5min) { [] } else { [event] }", &Superseded).await,
        @"[Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: 7})]"
    );
}

/// `sleep` has no failing form to reach. Even a `Suspension` that supersedes
/// everything it can resolves one normally, because `Future<()>` leaves it
/// nothing to report.
#[tokio::test(start_paused = true)]
async fn test_vm_async_sleep_has_no_superseded_form() {
    insta::assert_snapshot!(
        run_body_with("await sleep(5min); [event]", &Superseded).await,
        @"[Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: 7})]"
    );
}

/// Two instances of one body execute independently over a shared program.
/// The parameters each was bound with stay its own, which is what lets a
/// re-triggered automation run beside the firing it followed.
#[tokio::test(start_paused = true)]
async fn test_vm_async_instances_do_not_share_registers() {
    let auto = compile("observer { event, ... } /true/ { await sleep(5min); [event] }");
    let program = Arc::new(Program::new(auto.body).expect("the body builds"));

    let first = program
        .instance()
        .run_async(vec![event_with_node_id(1)], &Timer);
    let second = program
        .instance()
        .run_async(vec![event_with_node_id(2)], &Timer);
    let (first, second) = tokio::join!(first, second);

    insta::assert_snapshot!(render(first), @"[Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: 1})]");
    insta::assert_snapshot!(render(second), @"[Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: 2})]");
}

/// Awaiting a register that holds something other than a future is
/// reported rather than run. The checker types `await`'s operand as a
/// `Future`, so this is only ever reachable by hand.
#[tokio::test(start_paused = true)]
async fn test_vm_async_await_on_non_future_is_an_error() {
    let mut code = vec![Opcode::Await as u8];
    code.extend_from_slice(&0u32.to_le_bytes());
    // Source register 1 is left at its initial `Unit`.
    code.extend_from_slice(&1u32.to_le_bytes());
    let program = Arc::new(Program::new(raw_bytecode(2, code)).expect("no constants to decode"));
    insta::assert_snapshot!(
        render(program.instance().run_async(Vec::new(), &Timer).await),
        @"error: VM invariant violated: await on ()"
    );
}

// ============================================================================
// Known gaps
//
// These sources type-check but the VM has no implementation for them. The
// snapshots pin today's behaviour so filling a gap shows up as a visible
// one-line diff rather than a silent change.
// ============================================================================

/// `keys` and `values` need a `Map` representation that no `Value` has yet,
/// so they are reported rather than answered. Driven through `call`
/// directly: nothing the harness can compile produces a `Map` to pass.
#[test]
fn test_vm_gap_keys_needs_a_map() {
    insta::assert_snapshot!(
        render(super::ops::call(FunctionIdentity::Keys, vec![Value::List(vec![])])),
        @"error: keys is not implemented"
    );
}

// ============================================================================
// Reuse across runs
//
// A `Vm` is built once per compiled function and run per set of inputs, so
// these pin that a second run neither reallocates nor observes the first.
// ============================================================================

#[test]
fn test_vm_reruns_with_different_params() {
    let auto = compile("observer { event, ... } /event.node_id == 7/ { [event] }");
    let mut vm = Arc::new(Program::new(auto.filter.expect("filter")).expect("builds")).instance();

    let matching = event_with_node_id(7);
    let other = event_with_node_id(8);

    insta::assert_snapshot!(
        format!(
            "{} then {} then {}",
            render(vm.run_sync(vec![matching.clone()])),
            render(vm.run_sync(vec![other])),
            render(vm.run_sync(vec![matching])),
        ),
        @"true then false then true"
    );
}

/// `bind` resets every register before each run. Without that, a register
/// this run never writes would still hold the previous run's value.
#[test]
fn test_vm_rerun_does_not_observe_previous_registers() {
    // The comprehension accumulates into a register the compiler reuses
    // across runs, so a stale list would show up as doubled output.
    let auto = compile("observer { event, ... } /[x for x in [1, 2]] == [1, 2]/ { [event] }");
    let mut vm = Arc::new(Program::new(auto.filter.expect("filter")).expect("builds")).instance();

    let first = render(vm.run_sync(vec![sample_event()]));
    let second = render(vm.run_sync(vec![sample_event()]));
    assert_eq!(
        first, second,
        "a rerun must not see the previous run's registers"
    );
    insta::assert_snapshot!(second, @"true");
}

#[test]
fn test_vm_body_reruns_independently() {
    let auto = compile(
        "observer { event, ... } /true/ { [Event::OnOffChanged(event.node_id, 1, event.attributes)] }",
    );
    let mut vm = Arc::new(Program::new(auto.body).expect("builds")).instance();
    insta::assert_snapshot!(render(vm.run_sync(vec![event_with_node_id(1)])), @"[Event::OnOffChanged(1, 1, {on_off: true})]");
    insta::assert_snapshot!(render(vm.run_sync(vec![event_with_node_id(2)])), @"[Event::OnOffChanged(2, 1, {on_off: true})]");
}
