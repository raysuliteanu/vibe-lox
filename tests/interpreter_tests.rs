use vibe_lox::interpreter::Interpreter;
use vibe_lox::interpreter::resolver::Resolver;
use vibe_lox::parser::Parser;
use vibe_lox::scanner;

fn run_fixture(source: &str) -> Vec<String> {
    let tokens = scanner::scan(source).expect("scan should succeed");
    let program = Parser::new(tokens).parse().expect("parse should succeed");
    let locals = Resolver::new()
        .resolve(&program)
        .expect("resolve should succeed");
    let mut interp = Interpreter::new();
    interp
        .interpret(&program, locals)
        .expect("interpret should succeed");
    interp.output().to_vec()
}

#[test]
fn fixture_arithmetic() {
    let source = include_str!("../fixtures/arithmetic.lox");
    let expected = include_str!("../fixtures/arithmetic.expected");
    let output = run_fixture(source);
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(output, expected_lines);
}

#[test]
fn fixture_scoping() {
    let source = include_str!("../fixtures/scoping.lox");
    let expected = include_str!("../fixtures/scoping.expected");
    let output = run_fixture(source);
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(output, expected_lines);
}

#[test]
fn fixture_classes() {
    let source = include_str!("../fixtures/classes.lox");
    let expected = include_str!("../fixtures/classes.expected");
    let output = run_fixture(source);
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(output, expected_lines);
}

#[test]
fn fixture_counter() {
    let source = include_str!("../fixtures/counter.lox");
    let expected = include_str!("../fixtures/counter.expected");
    let output = run_fixture(source);
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(output, expected_lines);
}

#[test]
fn fixture_fibonacci() {
    let source = include_str!("../fixtures/fib.lox");
    let expected = include_str!("../fixtures/fib.expected");
    let output = run_fixture(source);
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(output, expected_lines);
}

#[test]
fn fixture_hello() {
    let source = include_str!("../fixtures/hello.lox");
    let expected = include_str!("../fixtures/hello.expected");
    let output = run_fixture(source);
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(output, expected_lines);
}
