// Part of Agent Record Adapter (see NOTICE).
//! A small JavaScript parser over the check-scripts tokens: enough of the
//! language to read a script made from a recording (statements, calls,
//! objects, arrow functions, `await`) as a tree. What it does not model is
//! kept as `Other` with its place, so the reader can name it, never guess it.

use crate::check_scripts::tokens::{Tok, Token};

#[derive(Clone, Debug)]
pub(crate) enum Expr {
    Str(String),
    /// A template literal; its source is read again for the filled parts.
    Tmpl(usize, usize),
    Num(String),
    Ident(String),
    Member(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Spread(Box<Expr>),
    New(Box<Expr>),
    Object(Vec<Prop>),
    Array(Vec<Expr>),
    Arrow(Vec<Pattern>, Body),
    Await(Box<Expr>),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Assign(String, Box<Expr>, Box<Expr>),
    Other,
}

#[derive(Clone, Debug)]
pub(crate) enum Prop {
    Pair(String, Expr),
    Spread(Expr),
    Other,
}

#[derive(Clone, Debug)]
pub(crate) enum Pattern {
    Name(String),
    /// `{ key, key: local, ...rest }`: (key, local name).
    Object(Vec<(String, Option<String>)>),
    /// `[a, , ...rest]`
    Array(Vec<Option<String>>),
    Other,
}

#[derive(Clone, Debug)]
pub(crate) enum Body {
    Block(Vec<Stmt>),
    Expr(Box<Expr>),
}

#[derive(Clone, Debug)]
pub(crate) struct Stmt {
    pub line: usize,
    /// Char offsets of the statement in the source.
    pub span: (usize, usize),
    pub kind: Kind,
}

#[derive(Clone, Debug)]
pub(crate) enum Kind {
    Decl(Pattern, Option<Expr>),
    Expr(Expr),
    Return(Option<Expr>),
    Block(Vec<Stmt>),
    /// A statement the reader does not follow: `if`, `for`, `try`, `throw`...
    /// with the keyword that starts it.
    Other(String),
    Empty,
}

const OPS: &[&str] = &[
    ">>>=", "===", "!==", "**=", "<<=", ">>=", ">>>", "&&=", "||=", "??=", "==", "!=", "<=", ">=",
    "&&", "||", "??", "?.", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "**",
    "<<", ">>",
];

const ASSIGN: &[&str] = &[
    "=", "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", ">>>=", "&=", "|=", "^=", "&&=",
    "||=", "??=",
];

fn precedence(op: &str) -> Option<u8> {
    Some(match op {
        "??" => 1,
        "||" => 2,
        "&&" => 3,
        "|" => 4,
        "^" => 5,
        "&" => 6,
        "==" | "!=" | "===" | "!==" => 7,
        "<" | ">" | "<=" | ">=" | "instanceof" | "in" => 8,
        "<<" | ">>" | ">>>" => 9,
        "+" | "-" => 10,
        "*" | "/" | "%" => 11,
        "**" => 12,
        _ => return None,
    })
}

/// The statements of a JavaScript source.
pub(crate) fn program(tokens: &[Token]) -> Vec<Stmt> {
    let mut parser = Parser { t: tokens, i: 0 };
    let mut out = Vec::new();
    while parser.i < tokens.len() {
        let before = parser.i;
        out.push(parser.statement());
        if parser.i == before {
            parser.i += 1;
        }
    }
    out
}

struct Parser<'a> {
    t: &'a [Token],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self, ahead: usize) -> Option<&Token> {
        self.t.get(self.i + ahead)
    }

    fn is(&self, ch: char) -> bool {
        self.peek(0).is_some_and(|t| t.is(ch))
    }

    fn word(&self) -> Option<&str> {
        self.peek(0).and_then(Token::ident)
    }

    fn eat(&mut self, ch: char) -> bool {
        if self.is(ch) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn line(&self) -> usize {
        self.peek(0)
            .or_else(|| self.t.last())
            .map_or(1, |t| t.line)
    }

    fn start(&self) -> usize {
        self.peek(0).map_or(0, |t| t.span.0)
    }

    fn end(&self) -> usize {
        self.i
            .checked_sub(1)
            .and_then(|k| self.t.get(k))
            .map_or(0, |t| t.span.1)
    }

    /// The operator written at the current token (`===`, `?.`, `+`), joining
    /// punctuation that touches, and how many tokens it takes.
    fn op(&self) -> Option<(String, usize)> {
        let first = self.peek(0)?;
        let Tok::Punct(_) = first.tok else {
            return None;
        };
        let mut text = String::new();
        let mut k = self.i;
        while let Some(token) = self.t.get(k) {
            let Tok::Punct(ch) = token.tok else { break };
            if k > self.i && self.t[k - 1].span.1 != token.span.0 {
                break;
            }
            text.push(ch);
            k += 1;
            if text.len() == 4 {
                break;
            }
        }
        for op in OPS {
            if text.starts_with(op) {
                return Some(((*op).to_string(), op.len()));
            }
        }
        Some((text[..1].to_string(), 1))
    }

    fn is_op(&self, wanted: &str) -> bool {
        self.op().is_some_and(|(op, _)| op == wanted)
    }

    /// The index after the bracket that closes the one at `at`.
    fn after_balanced(&self, at: usize) -> usize {
        let mut depth = 0i32;
        let mut k = at;
        while let Some(token) = self.t.get(k) {
            match token.tok {
                Tok::Punct('(' | '[' | '{') => depth += 1,
                Tok::Punct(')' | ']' | '}') => {
                    depth -= 1;
                    if depth <= 0 {
                        return k + 1;
                    }
                }
                _ => {}
            }
            k += 1;
        }
        self.t.len()
    }

    fn skip_balanced(&mut self) {
        if self.is('(') || self.is('[') || self.is('{') {
            self.i = self.after_balanced(self.i);
        }
    }

    fn statement(&mut self) -> Stmt {
        let line = self.line();
        let start = self.start();
        let kind = self.statement_kind();
        Stmt {
            line,
            span: (start, self.end().max(start)),
            kind,
        }
    }

    fn statement_kind(&mut self) -> Kind {
        if self.eat(';') {
            return Kind::Empty;
        }
        if self.is('{') {
            return Kind::Block(self.block());
        }
        match self.word() {
            Some("import") if !matches!(self.peek(1).map(|t| &t.tok), Some(Tok::Punct('(' | '.'))) => {
                self.i += 1;
                while let Some(token) = self.peek(0) {
                    let is_path = matches!(token.tok, Tok::Str(_));
                    self.i += 1;
                    if is_path {
                        break;
                    }
                }
                self.eat(';');
                Kind::Empty
            }
            Some("export") => {
                self.i += 1;
                if self.word() == Some("default") {
                    self.i += 1;
                }
                self.statement_kind()
            }
            Some("const" | "let" | "var") => {
                self.i += 1;
                self.declaration()
            }
            Some("return") => {
                let line = self.line();
                self.i += 1;
                let value = if self.is(';')
                    || self.is('}')
                    || self.peek(0).is_none_or(|t| t.line != line)
                {
                    None
                } else {
                    Some(self.expression())
                };
                self.eat(';');
                Kind::Return(value)
            }
            Some("async") if self.peek(1).and_then(Token::ident) == Some("function") => {
                self.i += 1;
                self.function_declaration()
            }
            Some("function") => self.function_declaration(),
            Some(word @ ("if" | "for" | "while" | "switch" | "with")) => {
                let word = word.to_string();
                self.i += 1;
                if self.word() == Some("await") {
                    self.i += 1;
                }
                self.skip_balanced();
                if word == "switch" {
                    self.skip_balanced();
                } else {
                    let _ = self.statement();
                    if word == "if" && self.word() == Some("else") {
                        self.i += 1;
                        let _ = self.statement();
                    }
                }
                Kind::Other(word)
            }
            Some("do") => {
                self.i += 1;
                let _ = self.statement();
                if self.word() == Some("while") {
                    self.i += 1;
                    self.skip_balanced();
                }
                self.eat(';');
                Kind::Other("do".into())
            }
            Some("try") => {
                self.i += 1;
                self.skip_balanced();
                if self.word() == Some("catch") {
                    self.i += 1;
                    if self.is('(') {
                        self.skip_balanced();
                    }
                    self.skip_balanced();
                }
                if self.word() == Some("finally") {
                    self.i += 1;
                    self.skip_balanced();
                }
                Kind::Other("try".into())
            }
            Some(word @ ("throw" | "break" | "continue" | "debugger")) => {
                let word = word.to_string();
                let line = self.line();
                self.i += 1;
                if word == "throw" {
                    let _ = self.expression();
                } else if self.peek(0).is_some_and(|t| t.line == line && t.ident().is_some()) {
                    self.i += 1;
                }
                self.eat(';');
                Kind::Other(word)
            }
            Some("class") => {
                self.i += 1;
                while self.peek(0).is_some() && !self.is('{') {
                    self.i += 1;
                }
                self.skip_balanced();
                Kind::Other("class".into())
            }
            _ => {
                let expr = self.expression();
                self.eat(';');
                Kind::Expr(expr)
            }
        }
    }

    fn block(&mut self) -> Vec<Stmt> {
        let mut out = Vec::new();
        if !self.eat('{') {
            return out;
        }
        while self.peek(0).is_some() && !self.is('}') {
            let before = self.i;
            out.push(self.statement());
            if self.i == before {
                self.i += 1;
            }
        }
        self.eat('}');
        out
    }

    fn declaration(&mut self) -> Kind {
        let mut declared = Vec::new();
        loop {
            let pattern = self.pattern();
            let init = if self.is_op("=") {
                self.i += 1;
                Some(self.assignment())
            } else {
                None
            };
            declared.push((pattern, init));
            if !self.eat(',') {
                break;
            }
        }
        self.eat(';');
        if declared.len() == 1 {
            let (pattern, init) = declared.remove(0);
            return Kind::Decl(pattern, init);
        }
        // `let app, editor;`: one declaration each, in a block of their own.
        Kind::Block(
            declared
                .into_iter()
                .map(|(pattern, init)| Stmt {
                    line: self.line(),
                    span: (0, 0),
                    kind: Kind::Decl(pattern, init),
                })
                .collect(),
        )
    }

    fn function_declaration(&mut self) -> Kind {
        self.i += 1; // function
        self.eat('*');
        let name = self.word().map(str::to_string);
        if name.is_some() {
            self.i += 1;
        }
        let params = self.params();
        let body = Body::Block(self.block());
        match name {
            Some(name) => Kind::Decl(Pattern::Name(name), Some(Expr::Arrow(params, body))),
            None => Kind::Expr(Expr::Arrow(params, body)),
        }
    }

    fn pattern(&mut self) -> Pattern {
        if let Some(name) = self.word() {
            let name = name.to_string();
            self.i += 1;
            return Pattern::Name(name);
        }
        if self.is('{') {
            self.i += 1;
            let mut entries = Vec::new();
            while self.peek(0).is_some() && !self.is('}') {
                if matches!(self.peek(0).map(|t| &t.tok), Some(Tok::Spread)) {
                    self.i += 1;
                    if let Some(name) = self.word() {
                        entries.push((name.to_string(), Some(name.to_string())));
                        self.i += 1;
                    }
                } else if self.is('[') {
                    self.skip_balanced();
                    if self.eat(':') {
                        let _ = self.pattern();
                    }
                } else {
                    let key = match self.peek(0).map(|t| t.tok.clone()) {
                        Some(Tok::Ident(key) | Tok::Str(key) | Tok::Num(key)) => key,
                        _ => {
                            self.i += 1;
                            continue;
                        }
                    };
                    self.i += 1;
                    let local = if self.eat(':') {
                        match self.pattern() {
                            Pattern::Name(local) => Some(local),
                            _ => None,
                        }
                    } else {
                        Some(key.clone())
                    };
                    entries.push((key, local));
                }
                if self.is_op("=") {
                    self.i += 1;
                    let _ = self.assignment();
                }
                if !self.eat(',') {
                    break;
                }
            }
            self.eat('}');
            return Pattern::Object(entries);
        }
        if self.is('[') {
            self.i += 1;
            let mut items = Vec::new();
            while self.peek(0).is_some() && !self.is(']') {
                if self.is(',') {
                    items.push(None);
                    self.i += 1;
                    continue;
                }
                if matches!(self.peek(0).map(|t| &t.tok), Some(Tok::Spread)) {
                    self.i += 1;
                }
                items.push(match self.pattern() {
                    Pattern::Name(name) => Some(name),
                    _ => None,
                });
                if self.is_op("=") {
                    self.i += 1;
                    let _ = self.assignment();
                }
                if !self.eat(',') {
                    break;
                }
            }
            self.eat(']');
            return Pattern::Array(items);
        }
        Pattern::Other
    }

    /// `(a, { b }, ...c)` before an arrow or a function body.
    fn params(&mut self) -> Vec<Pattern> {
        let mut out = Vec::new();
        if !self.eat('(') {
            return out;
        }
        while self.peek(0).is_some() && !self.is(')') {
            if matches!(self.peek(0).map(|t| &t.tok), Some(Tok::Spread)) {
                self.i += 1;
            }
            let before = self.i;
            out.push(self.pattern());
            if self.is_op("=") {
                self.i += 1;
                let _ = self.assignment();
            }
            if self.i == before {
                self.i += 1;
            }
            if !self.eat(',') {
                break;
            }
        }
        self.eat(')');
        out
    }

    fn expression(&mut self) -> Expr {
        let first = self.assignment();
        if !self.is(',') {
            return first;
        }
        while self.eat(',') {
            let _ = self.assignment();
        }
        Expr::Other
    }

    fn arrow_ahead(&self) -> bool {
        let mut k = self.i;
        if self.word() == Some("async")
            && self
                .t
                .get(k + 1)
                .is_some_and(|t| t.line == self.t[k].line && (t.is('(') || t.ident().is_some()))
        {
            k += 1;
        }
        match self.t.get(k) {
            Some(t) if t.ident().is_some() => {
                matches!(self.t.get(k + 1).map(|t| &t.tok), Some(Tok::Arrow))
            }
            Some(t) if t.is('(') => {
                let after = self.after_balanced(k);
                matches!(self.t.get(after).map(|t| &t.tok), Some(Tok::Arrow))
            }
            _ => false,
        }
    }

    fn arrow(&mut self) -> Expr {
        if self.word() == Some("async") && !matches!(self.peek(1).map(|t| &t.tok), Some(Tok::Arrow))
        {
            self.i += 1;
        }
        let params = if self.is('(') {
            self.params()
        } else {
            let name = self.word().unwrap_or_default().to_string();
            self.i += 1;
            vec![Pattern::Name(name)]
        };
        self.i += 1; // =>
        let body = if self.is('{') {
            Body::Block(self.block())
        } else {
            Body::Expr(Box::new(self.assignment()))
        };
        Expr::Arrow(params, body)
    }

    fn assignment(&mut self) -> Expr {
        if self.arrow_ahead() {
            return self.arrow();
        }
        let target = self.conditional();
        if let Some((op, count)) = self.op()
            && ASSIGN.contains(&op.as_str())
        {
            self.i += count;
            let value = self.assignment();
            return Expr::Assign(op, Box::new(target), Box::new(value));
        }
        target
    }

    fn conditional(&mut self) -> Expr {
        let test = self.binary(0);
        if self.is_op("?") {
            self.i += 1;
            let yes = self.assignment();
            self.eat(':');
            let no = self.assignment();
            return Expr::Cond(Box::new(test), Box::new(yes), Box::new(no));
        }
        test
    }

    fn binary(&mut self, min: u8) -> Expr {
        let mut left = self.unary();
        loop {
            let (op, count) = match self.op() {
                Some(found) => found,
                None => match self.word() {
                    Some(word @ ("instanceof" | "in")) => (word.to_string(), 1),
                    _ => break,
                },
            };
            let Some(level) = precedence(&op) else { break };
            if level <= min && !(op == "**" && level == min) {
                break;
            }
            self.i += count;
            let right = self.binary(level);
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        left
    }

    fn unary(&mut self) -> Expr {
        if let Some(word) = self.word() {
            match word {
                "await" => {
                    self.i += 1;
                    return Expr::Await(Box::new(self.unary()));
                }
                "typeof" | "void" | "delete" => {
                    let word = word.to_string();
                    self.i += 1;
                    return Expr::Unary(word, Box::new(self.unary()));
                }
                "new" => {
                    self.i += 1;
                    let callee = self.postfix(false);
                    let call = if self.is('(') {
                        let args = self.arguments();
                        Expr::Call(Box::new(callee), args)
                    } else {
                        callee
                    };
                    return self.postfix_from(Expr::New(Box::new(call)), true);
                }
                _ => {}
            }
        }
        if let Some((op, count)) = self.op()
            && matches!(op.as_str(), "!" | "-" | "+" | "~" | "++" | "--")
        {
            self.i += count;
            return Expr::Unary(op, Box::new(self.unary()));
        }
        self.postfix(true)
    }

    fn postfix(&mut self, calls: bool) -> Expr {
        let primary = self.primary();
        self.postfix_from(primary, calls)
    }

    fn postfix_from(&mut self, mut expr: Expr, calls: bool) -> Expr {
        loop {
            let line = self.i.checked_sub(1).map(|k| self.t[k].line);
            if self.is('.') {
                self.i += 1;
                let name = self.word().unwrap_or_default().to_string();
                self.i += 1;
                expr = Expr::Member(Box::new(expr), name);
            } else if self.is_op("?.") {
                self.i += 2;
                if self.is('(') && calls {
                    let args = self.arguments();
                    expr = Expr::Call(Box::new(expr), args);
                } else if self.is('[') {
                    self.i += 1;
                    let index = self.expression();
                    self.eat(']');
                    expr = Expr::Index(Box::new(expr), Box::new(index));
                } else {
                    let name = self.word().unwrap_or_default().to_string();
                    self.i += 1;
                    expr = Expr::Member(Box::new(expr), name);
                }
            } else if self.is('[') {
                self.i += 1;
                let index = self.expression();
                self.eat(']');
                expr = Expr::Index(Box::new(expr), Box::new(index));
            } else if self.is('(') && calls {
                let args = self.arguments();
                expr = Expr::Call(Box::new(expr), args);
            } else if matches!(self.peek(0).map(|t| &t.tok), Some(Tok::Tmpl(_)))
                && self.peek(0).map(|t| t.line) == line
            {
                // A tagged template.
                self.i += 1;
                expr = Expr::Other;
            } else if (self.is_op("++") || self.is_op("--"))
                && self.peek(0).map(|t| t.line) == line
            {
                self.i += 2;
                expr = Expr::Other;
            } else if self.is_op("!") && self.peek(0).map(|t| t.line) == line {
                // TypeScript's non-null `x!`, when not followed by `=`.
                if self.peek(1).is_some_and(|t| t.is('=')) {
                    break;
                }
                self.i += 1;
            } else {
                break;
            }
        }
        expr
    }

    fn arguments(&mut self) -> Vec<Expr> {
        let mut out = Vec::new();
        self.eat('(');
        while self.peek(0).is_some() && !self.is(')') {
            let before = self.i;
            if matches!(self.peek(0).map(|t| &t.tok), Some(Tok::Spread)) {
                self.i += 1;
                out.push(Expr::Spread(Box::new(self.assignment())));
            } else {
                out.push(self.assignment());
            }
            if self.i == before {
                self.i += 1;
            }
            if !self.eat(',') {
                break;
            }
        }
        self.eat(')');
        out
    }

    fn primary(&mut self) -> Expr {
        let Some(token) = self.peek(0) else {
            return Expr::Other;
        };
        let span = token.span;
        match token.tok.clone() {
            Tok::Str(text) => {
                self.i += 1;
                Expr::Str(text)
            }
            Tok::Tmpl(_) => {
                self.i += 1;
                Expr::Tmpl(span.0, span.1)
            }
            Tok::Num(text) => {
                self.i += 1;
                Expr::Num(text)
            }
            Tok::Ident(word) => match word.as_str() {
                "function" => {
                    self.i += 1;
                    self.eat('*');
                    if self.word().is_some() {
                        self.i += 1;
                    }
                    let params = self.params();
                    Expr::Arrow(params, Body::Block(self.block()))
                }
                "async" if self.peek(1).and_then(Token::ident) == Some("function") => {
                    self.i += 1;
                    self.primary()
                }
                "class" => {
                    while self.peek(0).is_some() && !self.is('{') {
                        self.i += 1;
                    }
                    self.skip_balanced();
                    Expr::Other
                }
                _ => {
                    self.i += 1;
                    Expr::Ident(word)
                }
            },
            Tok::Punct('(') => {
                self.i += 1;
                let inner = self.expression();
                self.eat(')');
                inner
            }
            Tok::Punct('[') => {
                self.i += 1;
                let mut items = Vec::new();
                while self.peek(0).is_some() && !self.is(']') {
                    let before = self.i;
                    if self.is(',') {
                        self.i += 1;
                        items.push(Expr::Other);
                        continue;
                    }
                    if matches!(self.peek(0).map(|t| &t.tok), Some(Tok::Spread)) {
                        self.i += 1;
                        items.push(Expr::Spread(Box::new(self.assignment())));
                    } else {
                        items.push(self.assignment());
                    }
                    if self.i == before {
                        self.i += 1;
                    }
                    if !self.eat(',') {
                        break;
                    }
                }
                self.eat(']');
                Expr::Array(items)
            }
            Tok::Punct('{') => self.object(),
            _ => {
                self.i += 1;
                Expr::Other
            }
        }
    }

    fn object(&mut self) -> Expr {
        self.i += 1;
        let mut props = Vec::new();
        while self.peek(0).is_some() && !self.is('}') {
            let before = self.i;
            if matches!(self.peek(0).map(|t| &t.tok), Some(Tok::Spread)) {
                self.i += 1;
                props.push(Prop::Spread(self.assignment()));
            } else if self.is('[') {
                self.skip_balanced();
                if self.eat(':') {
                    let _ = self.assignment();
                } else if self.is('(') {
                    self.skip_balanced();
                    self.skip_balanced();
                }
                props.push(Prop::Other);
            } else {
                // `get x() {}`, `async x() {}`: a method.
                if matches!(self.word(), Some("get" | "set" | "async"))
                    && self
                        .peek(1)
                        .is_some_and(|t| !t.is(':') && !t.is('(') && !t.is(',') && !t.is('}'))
                {
                    self.i += 1;
                }
                self.eat('*');
                let key = match self.peek(0).map(|t| t.tok.clone()) {
                    Some(Tok::Ident(key) | Tok::Str(key) | Tok::Num(key)) => key,
                    _ => {
                        self.i += 1;
                        continue;
                    }
                };
                self.i += 1;
                if self.eat(':') {
                    props.push(Prop::Pair(key, self.assignment()));
                } else if self.is('(') {
                    let params = self.params();
                    props.push(Prop::Pair(key, Expr::Arrow(params, Body::Block(self.block()))));
                } else {
                    props.push(Prop::Pair(key.clone(), Expr::Ident(key)));
                }
            }
            if self.i == before {
                self.i += 1;
            }
            if !self.eat(',') {
                break;
            }
        }
        self.eat('}');
        Expr::Object(props)
    }
}

/// Whether `expr` calls a function named `name` anywhere inside.
pub(crate) fn calls(expr: &Expr, name: &str) -> bool {
    let any = |items: &[Expr]| items.iter().any(|e| calls(e, name));
    match expr {
        Expr::Call(callee, args) => {
            matches!(callee.as_ref(), Expr::Ident(n) if n == name) || calls(callee, name) || any(args)
        }
        Expr::Member(inner, _) | Expr::Spread(inner) | Expr::New(inner) | Expr::Await(inner) => {
            calls(inner, name)
        }
        Expr::Unary(_, inner) => calls(inner, name),
        Expr::Index(a, b) | Expr::Binary(_, a, b) | Expr::Assign(_, a, b) => {
            calls(a, name) || calls(b, name)
        }
        Expr::Cond(a, b, c) => calls(a, name) || calls(b, name) || calls(c, name),
        Expr::Array(items) => any(items),
        Expr::Object(props) => props.iter().any(|p| match p {
            Prop::Pair(_, e) | Prop::Spread(e) => calls(e, name),
            Prop::Other => false,
        }),
        Expr::Arrow(_, body) => match body {
            Body::Expr(e) => calls(e, name),
            Body::Block(stmts) => stmts.iter().any(|s| stmt_calls(s, name)),
        },
        _ => false,
    }
}

fn stmt_calls(stmt: &Stmt, name: &str) -> bool {
    match &stmt.kind {
        Kind::Decl(_, Some(e)) | Kind::Expr(e) | Kind::Return(Some(e)) => calls(e, name),
        Kind::Block(stmts) => stmts.iter().any(|s| stmt_calls(s, name)),
        _ => false,
    }
}
