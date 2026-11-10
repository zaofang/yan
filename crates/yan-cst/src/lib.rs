//! The lossless syntax tree every reader builds (16.5, R995): a red-green tree (rowan) holding
//! every byte of the source, tokens and trivia alike. Readers lex and parse as before and hand
//! the builder byte ranges: one per token, one per comment, one per node their parser closed.
//! Whatever the ranges leave uncovered becomes whitespace (or skipped text), so printing the
//! tree gives the source back byte for byte. Comments and blank lines are read back from the
//! tree alone ([`Comments`]); no reader keeps trivia in another form.

mod comments;

pub use comments::{Comments, Lead};

use std::cmp::Reverse;

use rowan::{GreenNode, GreenNodeBuilder};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum Kind {
    Whitespace,
    Comment,
    DocComment,
    /// Text no token covers: an invalid comment, or what follows a fatal lexical error.
    Skipped,
    Token,
    Root,
    Item,
    Attr,
    Params,
    Body,
    Stmt,
    Arm,
    Expr,
    Pattern,
    Type,
}

impl Kind {
    const ALL: [Kind; 15] = [
        Kind::Whitespace,
        Kind::Comment,
        Kind::DocComment,
        Kind::Skipped,
        Kind::Token,
        Kind::Root,
        Kind::Item,
        Kind::Attr,
        Kind::Params,
        Kind::Body,
        Kind::Stmt,
        Kind::Arm,
        Kind::Expr,
        Kind::Pattern,
        Kind::Type,
    ];

    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            Kind::Whitespace | Kind::Comment | Kind::DocComment | Kind::Skipped
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Yan {}

impl rowan::Language for Yan {
    type Kind = Kind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Kind {
        Kind::ALL[raw.0 as usize]
    }

    fn kind_to_raw(kind: Kind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

pub type SyntaxNode = rowan::SyntaxNode<Yan>;
pub type SyntaxToken = rowan::SyntaxToken<Yan>;

/// A module's syntax tree. Like all trivia it never takes part in comparison (16.5).
#[derive(Clone, Debug)]
pub struct Cst(GreenNode);

impl PartialEq for Cst {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Default for Cst {
    fn default() -> Self {
        Builder::new("").finish()
    }
}

impl std::fmt::Display for Cst {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.syntax())
    }
}

impl Cst {
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.0.clone())
    }

    /// Doc comments, as written, with their 1-based lines.
    pub fn docs(&self) -> Vec<(u32, String)> {
        positioned(&self.syntax())
            .filter(|(_, _, t)| t.kind() == Kind::DocComment)
            .map(|(line, _, t)| (line, t.text().to_owned()))
            .collect()
    }
}

/// Every token of `root` with its 1-based line and UTF-8 byte column.
pub fn positioned(root: &SyntaxNode) -> impl Iterator<Item = (u32, u32, SyntaxToken)> {
    let (mut line, mut col) = (1u32, 1u32);
    root.descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .map(move |t| {
            let at = (line, col);
            for c in t.text().chars() {
                if c == '\n' {
                    line += 1;
                    col = 1;
                } else {
                    col += c.len_utf8() as u32;
                }
            }
            (at.0, at.1, t)
        })
}

/// Byte offsets of 1-based (line, byte column) positions.
pub struct LineIndex(Vec<usize>);

impl LineIndex {
    pub fn new(src: &str) -> Self {
        let starts = std::iter::once(0)
            .chain(src.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self(starts)
    }

    pub fn offset(&self, line: u32, col: u32) -> usize {
        self.0[line as usize - 1] + col as usize - 1
    }
}

/// Collects a reader's ranges, then builds the tree.
pub struct Builder<'a> {
    src: &'a str,
    leaves: Vec<(usize, usize, Kind)>,
    nodes: Vec<(Kind, usize, usize)>,
}

impl<'a> Builder<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src,
            leaves: Vec::new(),
            nodes: Vec::new(),
        }
    }

    pub fn token(&mut self, start: usize, end: usize) {
        self.leaves.push((start, end, Kind::Token));
    }

    pub fn comment(&mut self, start: usize, end: usize, doc: bool) {
        let kind = if doc { Kind::DocComment } else { Kind::Comment };
        self.leaves.push((start, end, kind));
    }

    /// A node over `start..end`. Nodes may be given in any order; of two with the same range,
    /// the one given later encloses the other, as a parser closes the outer one last.
    pub fn node(&mut self, kind: Kind, start: usize, end: usize) {
        self.nodes.push((kind, start, end));
    }

    pub fn finish(mut self) -> Cst {
        self.leaves.retain(|l| l.0 < l.1);
        self.leaves.sort_by_key(|l| l.0);
        let mut nodes: Vec<_> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.1 < n.2)
            .map(|(i, n)| (n.1, Reverse(n.2), Reverse(i), n.0))
            .collect();
        nodes.sort_unstable();
        let mut b = GreenNodeBuilder::new();
        b.start_node(rowan::SyntaxKind(Kind::Root as u16));
        let mut open: Vec<usize> = Vec::new();
        let mut next = nodes.into_iter().peekable();
        let mut at = 0;
        for &(start, end, kind) in &self.leaves {
            if start < at {
                debug_assert!(false, "overlapping tokens at byte {start}");
                continue;
            }
            close(&mut b, &mut open, at);
            self.gap(&mut b, at, start);
            while let Some(&(s, Reverse(e), _, k)) = next.peek().filter(|n| n.0 <= start) {
                next.next();
                // A node that starts inside an earlier token or crosses an open one is not a
                // parse of this text; the tokens stay with the enclosing node.
                if s >= at && open.last().is_none_or(|&top| e <= top) {
                    b.start_node(rowan::SyntaxKind(k as u16));
                    open.push(e);
                }
            }
            b.token(rowan::SyntaxKind(kind as u16), &self.src[start..end]);
            at = end;
        }
        close(&mut b, &mut open, at);
        self.gap(&mut b, at, self.src.len());
        close(&mut b, &mut open, usize::MAX);
        b.finish_node();
        Cst(b.finish())
    }

    fn gap(&self, b: &mut GreenNodeBuilder, from: usize, to: usize) {
        let text = &self.src[from..to];
        let mut rest = text;
        while let Some(c) = rest.chars().next() {
            let ws = c.is_whitespace();
            let n = rest
                .find(|c: char| c.is_whitespace() != ws)
                .unwrap_or(rest.len());
            let kind = if ws { Kind::Whitespace } else { Kind::Skipped };
            b.token(rowan::SyntaxKind(kind as u16), &rest[..n]);
            rest = &rest[n..];
        }
    }
}

fn close(b: &mut GreenNodeBuilder, open: &mut Vec<usize>, at: usize) {
    while open.last().is_some_and(|&e| e <= at) {
        open.pop();
        b.finish_node();
    }
}

#[cfg(test)]
mod selftest {
    use super::*;

    #[test]
    fn lossless_and_nested() {
        let src = "let x = 1 -- one\n\nf(x)  \n@@";
        let mut b = Builder::new(src);
        for (s, e) in [
            (0, 3),
            (4, 5),
            (6, 7),
            (8, 9),
            (18, 19),
            (19, 20),
            (20, 21),
            (21, 22),
        ] {
            b.token(s, e);
        }
        b.comment(10, 16, false);
        b.node(Kind::Expr, 8, 9);
        b.node(Kind::Stmt, 0, 9);
        b.node(Kind::Expr, 20, 21);
        b.node(Kind::Expr, 18, 22);
        b.node(Kind::Stmt, 18, 22);
        let cst = b.finish();
        assert_eq!(cst.to_string(), src);
        let root = cst.syntax();
        let kinds: Vec<_> = root.children().map(|n| n.kind()).collect();
        assert_eq!(kinds, [Kind::Stmt, Kind::Stmt]);
        let second = root.children().nth(1).unwrap();
        assert_eq!(second.first_child().unwrap().kind(), Kind::Expr);
        assert_eq!(
            second.first_child().unwrap().first_child().unwrap().text(),
            "x"
        );
        let last = root.last_token().unwrap();
        assert_eq!((last.kind(), last.text()), (Kind::Skipped, "@@"));
    }

    #[test]
    fn crossing_node_dropped() {
        let src = "a b c";
        let mut b = Builder::new(src);
        for (s, e) in [(0, 1), (2, 3), (4, 5)] {
            b.token(s, e);
        }
        b.node(Kind::Expr, 0, 3);
        b.node(Kind::Expr, 2, 5);
        let cst = b.finish();
        assert_eq!(cst.to_string(), src);
        assert_eq!(cst.syntax().children().count(), 1);
    }

    #[test]
    fn docs_with_lines() {
        let src = "x\n--- doc\n";
        let mut b = Builder::new(src);
        b.token(0, 1);
        b.comment(2, 9, true);
        assert_eq!(b.finish().docs(), [(2, "--- doc".to_owned())]);
    }
}
