use vibe_lox::parser::Parser;
use vibe_lox::scanner;

fn parse_errors(source: &str) -> Vec<String> {
    let tokens = scanner::scan(source).expect("scan should succeed");
    Parser::new(tokens)
        .parse()
        .unwrap_err()
        .into_iter()
        .map(|e| e.to_string())
        .collect()
}

#[test]
fn missing_semicolon_in_nested_function_reports_single_error() {
    let source = include_str!("../fixtures/error_missing_semicolon.lox");
    let errors = parse_errors(source);
    assert_eq!(
        errors.len(),
        1,
        "expected 1 error but got {}: {errors:?}",
        errors.len()
    );
    assert!(
        errors[0].contains("';'"),
        "error should mention missing semicolon: {}",
        errors[0]
    );
}

#[test]
fn class_method_error_reports_single_error() {
    let source = include_str!("../fixtures/error_class_method.lox");
    let errors = parse_errors(source);
    assert_eq!(
        errors.len(),
        1,
        "expected 1 error but got {}: {errors:?}",
        errors.len()
    );
    assert!(
        errors[0].contains("';'"),
        "error should mention missing semicolon: {}",
        errors[0]
    );
}

#[test]
fn valid_code_after_error_in_class_still_parses() {
    // The second method is valid; the parser should recover from the first
    // method's error and not report additional errors.
    let source = r#"
        class Foo {
            broken() {
                var x = 1
                print x;
            }
            working() {
                return 42;
            }
        }
    "#;
    let errors = parse_errors(source);
    assert_eq!(
        errors.len(),
        1,
        "only the broken method should produce an error, got: {errors:?}"
    );
}

#[test]
fn multiple_independent_errors_all_reported() {
    // Two separate statements each missing a semicolon, separated by enough
    // context that synchronization recovers before the second error.
    let source = "var x = 1\nprint x;\nvar y = 2\nprint y;\n";
    let errors = parse_errors(source);
    assert_eq!(
        errors.len(),
        2,
        "each missing semicolon should be reported independently: {errors:?}"
    );
}
