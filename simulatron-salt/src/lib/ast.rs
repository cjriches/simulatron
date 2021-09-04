use ast_gen::derive_ast_nodes;

use crate::language::{SyntaxKind, SyntaxNode};

/// A thin strongly-typed layer over the weakly-typed SyntaxNode.
pub trait AstNode {
    fn cast(syntax: SyntaxNode) -> Option<Self> where Self: Sized;
    fn syntax(&self) -> &SyntaxNode;
}

// Proc macro invocation to derive boilerplate AstNode implementations for
// each AST node type.
derive_ast_nodes! {
    Program,
    Line,
    ConstDecl,
    DataDecl,
    DataType,
    Label,
    Instruction,
    Operand,
    ArrayLiteral,
    Literal,
}

/// Programs contain Const Declarations, Data Declarations, Labels,
/// and Instructions.
impl Program {
    pub fn const_decls(&self) -> Vec<ConstDecl> {
        self.syntax.children()
            .filter_map(Line::cast)
            .filter_map(|line| line.as_const())
            .collect()
    }

    pub fn data_decls(&self) -> Vec<DataDecl> {
        self.syntax.children()
            .filter_map(Line::cast)
            .filter_map(|line| line.as_data())
            .collect()
    }

    pub fn labels(&self) -> Vec<Label> {
        self.syntax.children()
            .filter_map(Line::cast)
            .filter_map(|line| line.as_label())
            .collect()
    }

    pub fn instructions(&self) -> Vec<Instruction> {
        self.syntax.children()
            .filter_map(Line::cast)
            .filter_map(|line| line.as_instruction())
            .collect()
    }
}

/// Lines can be Const Declarations, Data Declarations, Labels, or Instructions.
impl Line {
    pub fn as_const(&self) -> Option<ConstDecl> {
        self.syntax.children().find_map(ConstDecl::cast)
    }

    pub fn as_data(&self) -> Option<DataDecl> {
        self.syntax.children().find_map(DataDecl::cast)
    }

    pub fn as_label(&self) -> Option<Label> {
        self.syntax.children().find_map(Label::cast)
    }

    pub fn as_instruction(&self) -> Option<Instruction> {
        self.syntax.children().find_map(Instruction::cast)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_test_logging;
    use crate::{lexer::Lexer, parser::Parser};

    use insta::assert_debug_snapshot;

    fn setup(path: &str) -> SyntaxNode {
        init_test_logging();
        let input = std::fs::read_to_string(path).unwrap();
        let parser = Parser::new(Lexer::new(&input));
        parser.run().unwrap()
    }

    #[test]
    fn test_program_components() {
        let cst = setup("examples/hello-world.simasm");
        let ast = Program::cast(cst).unwrap();
        assert_eq!(ast.const_decls().len(), 4);
        assert_eq!(ast.data_decls().len(), 1);
        assert_eq!(ast.labels().len(), 1);
        assert_eq!(ast.instructions().len(), 6);
    }
}
