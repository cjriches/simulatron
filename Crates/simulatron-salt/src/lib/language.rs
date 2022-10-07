use crate::lexer::TokenType;

/// All terminals and non-terminals in the grammar, i.e. all possible types for
/// a node in the AST.
#[repr(u16)]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Hash)]
pub enum SyntaxKind {
    // Non-terminals
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

    // Terminals
    KwConst,
    KwStatic,
    KwPub,
    KwMut,
    KwByte,
    KwHalf,
    KwWord,
    KwSizeof,
    OpenSquare,
    CloseSquare,
    OpenParen,
    CloseParen,
    Comma,
    Colon,
    DoubleDot,
    IntLiteral,
    FloatLiteral,
    CharLiteral,
    StringLiteral,
    Identifier,
    Comment,
    Newline,
    Whitespace,
    Unknown,

    // Marker for conversion: DO NOT MOVE.
    __LAST,
}

/// TokenType can be (almost) losslessly converted to SyntaxKind.
impl From<TokenType> for SyntaxKind {
    fn from(tt: TokenType) -> Self {
        match tt {
            TokenType::Const => SyntaxKind::KwConst,
            TokenType::Static => SyntaxKind::KwStatic,
            TokenType::Pub => SyntaxKind::KwPub,
            TokenType::Mut => SyntaxKind::KwMut,
            TokenType::Byte => SyntaxKind::KwByte,
            TokenType::Half => SyntaxKind::KwHalf,
            TokenType::Word => SyntaxKind::KwWord,
            TokenType::Sizeof => SyntaxKind::KwSizeof,
            TokenType::OpenSquare => SyntaxKind::OpenSquare,
            TokenType::CloseSquare => SyntaxKind::CloseSquare,
            TokenType::OpenParen => SyntaxKind::OpenParen,
            TokenType::CloseParen => SyntaxKind::CloseParen,
            TokenType::Comma => SyntaxKind::Comma,
            TokenType::Colon => SyntaxKind::Colon,
            TokenType::DoubleDot => SyntaxKind::DoubleDot,
            TokenType::IntLiteral => SyntaxKind::IntLiteral,
            TokenType::FloatLiteral => SyntaxKind::FloatLiteral,
            TokenType::CharLiteral => SyntaxKind::CharLiteral,
            TokenType::StringLiteral => SyntaxKind::StringLiteral,
            TokenType::Identifier => SyntaxKind::Identifier,
            TokenType::Comment => SyntaxKind::Comment,
            TokenType::Newline => SyntaxKind::Newline,
            TokenType::Whitespace => SyntaxKind::Whitespace,
            TokenType::Unknown => SyntaxKind::Unknown,
        }
    }
}

/// Empty type to define the language on.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Hash)]
pub enum SimAsmLanguage {}

impl rowan::Language for SimAsmLanguage {
    type Kind = SyntaxKind;

    // Since SyntaxKind is `repr(u16)`, the transmutations are safe.

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        let limit: u16 = SyntaxKind::__LAST as u16;
        assert!(raw.0 < limit);
        let converted: SyntaxKind = unsafe { std::mem::transmute(raw.0) };
        converted
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        let converted: u16 = unsafe { std::mem::transmute(kind) };
        rowan::SyntaxKind(converted)
    }
}

/// Rowan types specialised to our language.
pub type SyntaxNode = rowan::SyntaxNode<SimAsmLanguage>;
pub type SyntaxElement = rowan::SyntaxElement<SimAsmLanguage>;
