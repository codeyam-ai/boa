//! Tests for [`CompiledScript`]: code compiled in one context, run in another.

use boa_parser::Source;

use crate::{Context, JsValue, Script, js_string, script::CompiledScript};

fn compile(src: &str, context: &mut Context) -> (Script, Option<CompiledScript>) {
    Script::parse_reusable(Source::from_bytes(src), context).expect("script should compile")
}

fn eval(src: &str, context: &mut Context) -> JsValue {
    context
        .eval(Source::from_bytes(src))
        .expect("eval should succeed")
}

const PROGRAM: &str = r"
    var counter = 1;
    let lexical = 10;
    const constant = 100;
    class Point { constructor(x) { this.x = x; } }
    function bump() { counter += 1; lexical += 1; return counter + lexical; }
    const closure = (() => { let hidden = 5; return () => hidden++; })();
    bump() + constant + new Point(3).x + closure();
";

#[test]
fn runs_in_a_fresh_context_with_the_same_results() {
    let mut a = Context::default();
    let (script, compiled) = compile(PROGRAM, &mut a);
    let compiled = compiled.expect("program should be reusable");
    let in_a = script.evaluate(&mut a).unwrap();

    let mut b = Context::default();
    let script_b = compiled.instantiate(None, &b).expect("fresh realm matches");
    let in_b = script_b.evaluate(&mut b).unwrap();

    let mut fresh = Context::default();
    let expected = eval(PROGRAM, &mut fresh);

    assert_eq!(in_a, expected);
    assert_eq!(in_b, expected);
    // Globals of every kind are live in the new realm.
    assert_eq!(eval("bump()", &mut b), JsValue::from(15));
    assert_eq!(
        eval("counter + lexical + constant", &mut b),
        JsValue::from(115)
    );
    assert_eq!(
        eval("typeof Point + closure()", &mut b),
        js_string!("function6").into()
    );
    assert_eq!(eval("globalThis.counter", &mut b), JsValue::from(3));
    assert_eq!(eval("globalThis.lexical", &mut b), JsValue::undefined());
    // Each realm has its own state.
    assert_eq!(eval("counter", &mut a), JsValue::from(2));
}

#[test]
fn a_sequence_of_scripts_replays_in_order() {
    let first = "let a = 1; const b = 2;";
    let second = "let c = a + b; function sum() { return a + b + c; } sum()";

    let mut a = Context::default();
    let (s1, c1) = compile(first, &mut a);
    s1.evaluate(&mut a).unwrap();
    let (s2, c2) = compile(second, &mut a);
    assert_eq!(s2.evaluate(&mut a).unwrap(), JsValue::from(6));
    // A later declaration in the compiling realm does not leak into the cache.
    eval("let late = 1;", &mut a);

    let (c1, c2) = (c1.unwrap(), c2.unwrap());
    let mut b = Context::default();
    c1.instantiate(None, &b).unwrap().evaluate(&mut b).unwrap();
    let result = c2.instantiate(None, &b).unwrap().evaluate(&mut b).unwrap();
    assert_eq!(result, JsValue::from(6));
    // Later scripts compiled in the new realm see the replayed bindings.
    assert_eq!(eval("a + b + c", &mut b), JsValue::from(6));
    assert_eq!(eval("let d = 4; d + sum()", &mut b), JsValue::from(10));
    assert_eq!(eval("typeof late", &mut b), js_string!("undefined").into());
}

#[test]
fn refuses_a_realm_with_a_different_global_layout() {
    let mut a = Context::default();
    let (_, compiled) = compile("let x = 1; x", &mut a);
    let compiled = compiled.unwrap();

    // Second script out of order: its slots would not line up.
    let mut b = Context::default();
    eval("let other = 0;", &mut b);
    assert!(compiled.instantiate(None, &b).is_none());

    // The same realm already holds the script's bindings.
    assert!(compiled.instantiate(None, &a).is_none());
}

#[test]
fn a_script_without_lexical_declarations_reruns_in_the_same_realm() {
    let mut a = Context::default();
    let (script, compiled) = compile("var n = (typeof n === 'number' ? n : 0) + 1; n", &mut a);
    let compiled = compiled.unwrap();
    assert_eq!(script.evaluate(&mut a).unwrap(), JsValue::from(1));
    let again = compiled.instantiate(None, &a).unwrap();
    assert_eq!(again.evaluate(&mut a).unwrap(), JsValue::from(2));
}

#[test]
fn direct_eval_and_annex_b_hoisting_are_not_reusable() {
    let mut a = Context::default();
    let (script, compiled) = compile("let v = 7; function f() { return eval('v'); } f()", &mut a);
    assert!(compiled.is_none());
    assert_eq!(script.evaluate(&mut a).unwrap(), JsValue::from(7));

    let mut a = Context::default();
    let (script, compiled) = compile("{ function hoisted() { return 3; } } hoisted()", &mut a);
    assert!(compiled.is_none());
    assert_eq!(script.evaluate(&mut a).unwrap(), JsValue::from(3));

    // Strict-mode block functions are not hoisted, so they stay reusable.
    let mut a = Context::default();
    let (_, compiled) = compile("'use strict'; { function local() {} }", &mut a);
    assert!(compiled.is_some());
}

#[test]
fn tagged_templates_are_per_realm() {
    let src = "function tag(s) { return s; } function site() { return tag`x`; } site() === site()";
    let mut a = Context::default();
    let (script, compiled) = compile(src, &mut a);
    assert_eq!(script.evaluate(&mut a).unwrap(), JsValue::from(true));

    let mut b = Context::default();
    let script_b = compiled.unwrap().instantiate(None, &b).unwrap();
    assert_eq!(script_b.evaluate(&mut b).unwrap(), JsValue::from(true));
    // A script parsed fresh in `b` never collides with the cached site.
    assert_eq!(
        eval(
            "function other() { return tag`x`; } other() === site()",
            &mut b
        ),
        JsValue::from(false)
    );
}

#[test]
fn global_declaration_errors_still_happen_per_realm() {
    let mut a = Context::default();
    let (_, compiled) = compile("let undefinedish = 1;", &mut a);
    let compiled = compiled.unwrap();

    let mut b = Context::default();
    eval(
        "Object.defineProperty(globalThis, 'undefinedish', { value: 0, configurable: false });",
        &mut b,
    );
    let script = compiled.instantiate(None, &b).unwrap();
    assert!(script.evaluate(&mut b).is_err());
}
