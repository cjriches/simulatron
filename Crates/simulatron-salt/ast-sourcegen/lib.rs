use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Ident,
    parse::Parser,
    punctuated::Punctuated,
    token::Comma,
};

/// Automatically generate the boilerplate for a list of AstNodes.
/// We assume the definition of `SyntaxNode` and `SyntaxKind` in the
/// calling scope, as well as the `AstNode` trait.
/// For each `Name` in the input list, we will generate a struct called `Name`,
/// with an AstNode implementation that parses it from `SyntaxNode`s with
/// kind `SyntaxKind::Name`.
#[proc_macro]
pub fn derive_ast_nodes(item: TokenStream) -> TokenStream {
    // Grab a syn parser for a comma-separated list.
    let parser = Punctuated::<Ident, Comma>::parse_terminated;
    // Run the parser on the input.
    let names = parser.parse(item).expect("Invalid input.");

    // Generate the code for each name in the list.
    let defs = names.iter().map(|name| {
        quote! {
            #[derive(Debug, PartialEq, Eq, Clone)]
            pub struct #name {
                syntax: SyntaxNode,
            }

            impl AstNode for #name {
                fn cast(syntax: SyntaxNode) -> Option<Self> {
                    match syntax.kind() {
                        SyntaxKind::#name => Some(Self {syntax}),
                        _ => None,
                    }
                }
                fn syntax(&self) -> &SyntaxNode {
                    &self.syntax
                }
            }
        }
    });

    // Concatenate the code together and return it.
    let generated = quote! {
        #(#defs)*
    };
    generated.into()
}

/// Automatically generate the boilerplate for terminal token casts.
/// We assume the definition of `SyntaxElement` and `SyntaxKind` in the
/// calling scope. We also need access to `std::ops::Range`.
/// For each `TokenName` in the input list, we will generate a function called
/// `token_name_cast` that extracts the token string and its span from
/// `SyntaxElement`s with kind `SyntaxKind::Name`.
#[proc_macro]
pub fn derive_token_casts(item: TokenStream) -> TokenStream {
    // Grab a syn parser for a comma-separated list.
    let parser = Punctuated::<Ident, Comma>::parse_terminated;
    // Run the parser on the input.
    let names = parser.parse(item).expect("Invalid input.");

    // Generate the code for each name in the list.
    let defs = names.iter().map(|name| {
        let name_lower = lower_snake_case(&name.to_string());
        let func_name = format_ident!("{}_cast", name_lower);
        quote! {
            fn #func_name(element: SyntaxElement) -> Option<(String, Range<usize>)> {
                if let rowan::NodeOrToken::Token(t) = element {
                    if t.kind() == SyntaxKind::#name {
                        return Some((t.text().to_string(), t.text_range().into()));
                    }
                }
                None
            }
        }
    });

    // Concatenate the code together and return it.
    let generated = quote! {
        #(#defs)*
    };
    generated.into()
}

/// Convert PascalCase to lower_snake_case.
fn lower_snake_case(pascal_case: &str) -> String {
    let mut result = String::with_capacity(pascal_case.len());
    let mut prev = false;
    for c in pascal_case.chars() {
        if c.is_ascii_uppercase() && prev {
            result.push('_')
        }
        prev = true;
        result.push(c.to_ascii_lowercase());
    }
    result
}
