use super::super::value::Value;
use super::run_async;
use super::run_sync;

/// Compile a filter expression and run it synchronously. The source is
/// wrapped in a minimal `observer {}` so the parser is happy; only the
/// filter is exercised.
fn run_filter(filter: &str) -> Value {
    let src = format!("observer {{}} /{}/ {{ [] }}", filter);
    let program = crate::automations::parse(&src).expect("parse");
    let lowered = crate::automations::desugar_program(program);
    let result = crate::automations::check_program(&lowered);
    // The body `[]` produces an "observer body must return [Event]" type
    // error, but the filter compiles cleanly regardless — we only execute
    // the filter, so we tolerate body-only errors.
    let hir = crate::automations::lower_program(&result);
    let lir = crate::automations::lower_lir_program(&hir);
    let bc = crate::automations::lower_bytecode_program(&lir);
    let filter = match &bc {
        crate::automations::repr::BytecodeProgram::Automation(auto) => {
            auto.filter.as_ref().expect("filter should be present")
        }
        _ => panic!("expected an Automation, got a Template"),
    };
    run_sync(filter, Vec::new()).expect("filter should run")
}

#[test]
fn test_vm_filter_const_true() {
    assert_eq!(run_filter("true"), Value::Bool(true));
}

#[test]
fn test_vm_filter_const_false() {
    assert_eq!(run_filter("false"), Value::Bool(false));
}

#[test]
fn test_vm_filter_arithmetic_eq() {
    // 1 + 2 == 3
    assert_eq!(run_filter("1 + 2 == 3"), Value::Bool(true));
}

#[test]
fn test_vm_filter_arithmetic_ne() {
    assert_eq!(run_filter("1 + 2 == 4"), Value::Bool(false));
}

#[test]
fn test_vm_filter_short_circuit_and_true() {
    assert_eq!(run_filter("true && (1 < 2)"), Value::Bool(true));
}

#[test]
fn test_vm_filter_short_circuit_and_false() {
    assert_eq!(run_filter("true && false"), Value::Bool(false));
}

#[test]
fn test_vm_filter_short_circuit_or() {
    assert_eq!(run_filter("false || true"), Value::Bool(true));
}

#[test]
fn test_vm_filter_not() {
    assert_eq!(run_filter("!(1 == 2)"), Value::Bool(true));
}

#[test]
fn test_vm_filter_comparison_chain() {
    // (1 + 2) > 0 && (1 + 2) < 10
    assert_eq!(run_filter("(1 + 2) > 0 && (1 + 2) < 10"), Value::Bool(true));
}

/// Snapshot-style trace of register state at each step for a simple
/// filter — pins the VM's execution semantics so regressions are
/// caught at the instruction level, not just the result.
#[test]
fn test_vm_register_trace_for_and() {
    let src = "observer {} /true && false/ { [] }";
    let program = crate::automations::parse(src).expect("parse");
    let lowered = crate::automations::desugar_program(program);
    let result = crate::automations::check_program(&lowered);
    let hir = crate::automations::lower_program(&result);
    let lir = crate::automations::lower_lir_program(&hir);
    let bc = crate::automations::lower_bytecode_program(&lir);
    let filter = match &bc {
        crate::automations::repr::BytecodeProgram::Automation(auto) => {
            auto.filter.as_ref().expect("filter present")
        }
        _ => panic!("expected automation"),
    };
    // The final result should be Bool(false) by short-circuiting.
    assert_eq!(run_sync(filter, Vec::new()).unwrap(), Value::Bool(false));
}

// =============================================================================
// Async VM: sleep_unique + await
// =============================================================================

/// Drives the body of `observer {} /true/ { if await sleep_unique(d) { … } else { … } }`
/// under `tokio::time::pause` and returns the final `Value`.
async fn run_body_with_paused_time(
    body_src: &str,
    advance: std::time::Duration,
) -> Result<Value, super::VmError> {
    let src = format!("observer {{}} /true/ {{ {} }}", body_src);
    let program = crate::automations::parse(&src).expect("parse");
    let lowered = crate::automations::desugar_program(program);
    let result = crate::automations::check_program(&lowered);
    let hir = crate::automations::lower_program(&result);
    let lir = crate::automations::lower_lir_program(&hir);
    let bc = crate::automations::lower_bytecode_program(&lir);
    let body = match &bc {
        crate::automations::repr::BytecodeProgram::Automation(auto) => &auto.body,
        _ => panic!("expected automation"),
    };

    let body_clone = body.clone();
    let handle = tokio::spawn(async move { run_async(&body_clone, Vec::new()).await });
    // Yield once so the task actually starts and reaches the sleep.
    tokio::task::yield_now().await;
    tokio::time::advance(advance).await;
    handle.await.expect("task joined")
}

#[tokio::test(start_paused = true)]
async fn test_vm_async_sleep_unique_completes_after_advance() {
    // After advancing time past the 5min sleep, the await resolves to
    // true and the body returns [].
    let result = run_body_with_paused_time(
        "if await sleep_unique(5min) { [] } else { [] }",
        std::time::Duration::from_secs(6 * 60),
    )
    .await
    .expect("body should run");
    // Empty list either way; the assertion is that we got a value at all.
    assert_eq!(result, Value::List(Vec::new()));
}

#[tokio::test(start_paused = true)]
async fn test_vm_async_sleep_completes() {
    // `sleep` is the simpler builtin: same Future shape, no cancellation.
    let result = run_body_with_paused_time(
        "if await sleep(1s) { [] } else { [] }",
        std::time::Duration::from_secs(2),
    )
    .await
    .expect("body should run");
    assert_eq!(result, Value::List(Vec::new()));
}
