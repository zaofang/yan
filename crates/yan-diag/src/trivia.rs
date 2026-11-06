//! Comments and blank lines the parsers drop and the canonical printers put back (spec 18.5, R376).

#[derive(Clone, Debug)]
struct Comment {
    line: u32,
    col: u32,
    text: String,
    /// Code precedes it on its line.
    trailing: bool,
    /// Column of the next code token, none at end of file.
    next_col: Option<u32>,
    used: bool,
}

/// An own-line comment and whether the author left a blank line before it.
pub type Lead = (bool, String);

#[derive(Clone, Debug, Default)]
pub struct Trivia {
    comments: Vec<Comment>,
    blank: Vec<u32>,
}

impl PartialEq for Trivia {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Trivia {
    /// `comments` are (line, column, text); `code` are the (line, column) starts of the code tokens.
    pub fn new(src: &str, comments: &[(u32, u32, String)], code: &[(u32, u32)]) -> Self {
        let mut code = code.to_vec();
        code.sort_unstable();
        let comments = comments
            .iter()
            .map(|(line, col, text)| {
                let at = code.partition_point(|p| *p < (*line, *col));
                Comment {
                    line: *line,
                    col: *col,
                    text: text.clone(),
                    trailing: at > 0 && code[at - 1].0 == *line,
                    next_col: code.get(at).map(|p| p.1),
                    used: false,
                }
            })
            .collect();
        let blank = src
            .lines()
            .enumerate()
            .filter(|(_, l)| l.trim().is_empty())
            .map(|(i, _)| i as u32 + 1)
            .collect();
        Self { comments, blank }
    }

    fn blank_between(&self, a: u32, b: u32) -> bool {
        self.blank.iter().any(|l| *l > a && *l < b)
    }

    /// Takes the unused comments `pick` accepts, in order, with the blank-line flag of each against
    /// the line before it (`prev`, none at the start of a body).
    fn take(
        &mut self,
        mut prev: Option<u32>,
        pick: impl Fn(&Comment) -> bool,
    ) -> (Vec<Lead>, Option<u32>) {
        let mut out = Vec::new();
        for i in 0..self.comments.len() {
            if self.comments[i].used || !pick(&self.comments[i]) {
                continue;
            }
            self.comments[i].used = true;
            let line = self.comments[i].line;
            let blank = prev.is_some_and(|p| self.blank_between(p, line));
            out.push((blank, self.comments[i].text.clone()));
            prev = Some(line);
        }
        (out, prev)
    }

    /// Comments before `line` and whether a blank line separates the last of them (or `prev`) from it.
    pub fn leading(&mut self, prev: Option<u32>, line: u32) -> (Vec<Lead>, bool) {
        let (out, last) = self.take(prev, |c| c.line < line);
        let blank = last.is_some_and(|p| self.blank_between(p, line));
        (out, blank)
    }

    /// The comment ending `line`.
    pub fn trailing(&mut self, line: u32) -> Option<String> {
        let c = self
            .comments
            .iter_mut()
            .find(|c| !c.used && c.trailing && c.line == line)?;
        c.used = true;
        Some(c.text.clone())
    }

    /// Own-line comments after a body's last line `end` that stay in the body: at or right of
    /// its column `col`, with the next code to the left of it.
    pub fn dangling(&mut self, end: u32, col: u32) -> Vec<Lead> {
        self.take(Some(end), |c| {
            c.line > end && !c.trailing && c.col >= col && c.next_col.is_none_or(|n| n < col)
        })
        .0
    }

    /// Every comment not yet taken, after the last item `end`.
    pub fn rest(&mut self, end: Option<u32>) -> Vec<Lead> {
        self.take(end, |_| true).0
    }
}
