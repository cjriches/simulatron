use proc_macro::TokenStream;
use quote::quote;
use syn::{
    Ident,
    parse::Parser,
    punctuated::Punctuated,
    token::Comma,
};

/// Automatically generate the boilerplate for a list of AstNodes.
/// We assume the definition of `SyntaxNode` and `SyntaxType` in the
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
            #[derive(Debug, Clone)]
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
