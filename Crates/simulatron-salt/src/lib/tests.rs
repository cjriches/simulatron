use insta::{assert_debug_snapshot, assert_snapshot};
use simulatron_utils::hexprint::pretty_print_hex_block_zero;

use crate::{
    ast::{AstNode, Program},
    codegen::CodeGenerator,
    init_test_logging,
    lexer::Lexer,
    parser::Parser,
};

macro_rules! test_success {
    ($path: expr, $entrypoint: expr) => {{
        init_test_logging();
        let input = std::fs::read_to_string($path).unwrap();
        let parser = Parser::new(Lexer::new(&input));
        let cst = parser.run().unwrap();
        let ast = Program::cast(cst).unwrap();
        let codegen = CodeGenerator::new(ast, &Vec::new()).unwrap();
        let success = codegen.run($entrypoint).unwrap();
        assert_eq!(success.warnings.len(), 0);
        assert_snapshot!(pretty_print_hex_block_zero(&success.simobj));
    }};
}

macro_rules! test_success_with_warnings {
    ($path: expr, $entrypoint: expr) => {{
        init_test_logging();
        let input = std::fs::read_to_string($path).unwrap();
        let parser = Parser::new(Lexer::new(&input));
        let cst = parser.run().unwrap();
        let ast = Program::cast(cst).unwrap();
        let codegen = CodeGenerator::new(ast, &Vec::new()).unwrap();
        let success = codegen.run($entrypoint).unwrap();
        assert!(success.warnings.len() > 0);
        assert_snapshot!(pretty_print_hex_block_zero(&success.simobj));
        assert_debug_snapshot!(success.warnings);
    }};
}

macro_rules! test_failure {
    ($path: expr) => {{
        init_test_logging();
        let input = std::fs::read_to_string($path).unwrap();
        let parser = Parser::new(Lexer::new(&input));
        let cst = parser.run().unwrap();
        let ast = Program::cast(cst).unwrap();
        let failure = CodeGenerator::new(ast, &Vec::new())
            .and_then(|cg| cg.run(true))
            .unwrap_err();
        assert_debug_snapshot!(failure);
    }};
}

#[test]
fn test_addressing_modes() {
    test_success!("examples/addressing-modes.simasm", true);
    test_success!("examples/addressing-modes-2.simasm", true);
}

#[test]
fn test_arithmetic() {
    test_success!("examples/arithmetic.simasm", true);
    test_failure!("examples/arithmetic-bad.simasm");
}

#[test]
fn test_array_inferred() {
    test_success!("examples/array-inferred.simasm", true);
    test_failure!("examples/array-bad.simasm");
}

#[test]
fn test_bitwise() {
    test_success!("examples/bitwise.simasm", true);
    test_failure!("examples/bitwise-bad.simasm");
}

#[test]
fn test_blockcopy() {
    test_success!("examples/blockcopy.simasm", false);
    test_failure!("examples/blockcopy-bad.simasm");
}

#[test]
fn test_blockset() {
    test_success!("examples/blockset.simasm", true);
    test_failure!("examples/blockset-bad.simasm");
}

#[test]
fn test_branching() {
    test_success_with_warnings!("examples/branching.simasm", true);
    test_failure!("examples/branching-bad.simasm");
    test_failure!("examples/branching-bad-2.simasm");
}

#[test]
fn test_comments() {
    test_success_with_warnings!("examples/comments.simasm", false);
}

#[test]
fn test_convert() {
    test_success!("examples/convert.simasm", true);
    test_failure!("examples/convert-bad.simasm");
}

#[test]
fn test_copy() {
    test_success!("examples/copy.simasm", true);
    test_failure!("examples/copy-bad.simasm");
}

#[test]
fn test_empty() {
    test_failure!("examples/empty-file.simasm");
}

#[test]
fn test_external_refs() {
    test_success_with_warnings!("examples/external-refs.simasm", false);
}

#[test]
fn test_hello_world() {
    test_success_with_warnings!("examples/hello-world.simasm", true);
}

#[test]
fn test_minimal() {
    test_success!("examples/minimal.simasm", true);
}

#[test]
fn test_naming_violations() {
    test_success_with_warnings!("examples/naming-violations.simasm", true);
}

#[test]
fn test_negate() {
    test_success!("examples/negate.simasm", true);
}

#[test]
fn test_push_pop() {
    test_success!("examples/push-pop.simasm", true);
    test_failure!("examples/push-pop-bad.simasm");
}

#[test]
fn test_store() {
    test_success!("examples/store.simasm", true);
}

#[test]
fn test_swap() {
    test_success!("examples/swap.simasm", true);
}
