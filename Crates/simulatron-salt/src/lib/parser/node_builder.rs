use rowan::{GreenNode, GreenNodeBuilder, Language};
use std::cell::RefCell;
use std::rc::Rc;

use crate::language::{SimAsmLanguage, SyntaxKind};
use crate::lexer::Token;

/// A wrapper around GreenNodeBuilder that ensures any started node gets
/// finished.
pub struct SafeNodeBuilder {
    inner: Rc<RefCell<GreenNodeBuilder<'static>>>,
}

/// Similar to MutexGuard, this struct automatically finishes the current node
/// when dropped.
#[must_use = "if unused the node will immediately be finished."]
pub struct NodeGuard {
    builder: Rc<RefCell<GreenNodeBuilder<'static>>>
}

impl Drop for NodeGuard {
    fn drop(&mut self) {
        self.builder.borrow_mut().finish_node();
    }
}

impl SafeNodeBuilder {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(GreenNodeBuilder::new())),
        }
    }

    /// Start a new node, returning a guard that finishes the node when dropped.
    pub fn start_node(&mut self, kind: SyntaxKind) -> NodeGuard {
        self.inner.borrow_mut().start_node(
            SimAsmLanguage::kind_to_raw(kind)
        );
        NodeGuard { builder: self.inner.clone() }
    }

    /// Add the given token at the current node.
    pub fn add_token(&mut self, t: Token) {
        self.inner.borrow_mut().token(
            SimAsmLanguage::kind_to_raw(t.tt.into()), t.slice)
    }

    /// Finish building. If there are no outstanding NodeGuards, this is
    /// guaranteed to succeed. If there are any outstanding NodeGuards, this
    /// is guaranteed to panic.
    pub fn finish(self) -> GreenNode {
        let ref_cell = Rc::try_unwrap(self.inner).unwrap();
        ref_cell.into_inner().finish()
    }
}
