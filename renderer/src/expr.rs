//! The ScreenSafe expression language.
//!
//! An expression is a pure, total function of the names in scope: it cannot loop, recurse, define functions,
//! read or write anything outside its slots, or fail at run time. Every arithmetic result that is not finite
//! becomes 0. Expressions are type-checked when a pack loads and compiled to a flat list of stack operations
//! whose length is the expression's cost; nothing is ever compiled to native code.

use std::collections::HashMap;
use std::fmt;

pub const MAX_SOURCE: usize = 2048;
pub const MAX_NODES: usize = 512;
pub const MAX_DEPTH: usize = 48;
const MAX_VARIADIC: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    Num,
    Bool,
    Color,
}

impl Ty {
    fn lanes(self) -> usize {
        if self == Ty::Color {
            4
        } else {
            1
        }
    }

    fn name(self) -> &'static str {
        match self {
            Ty::Num => "number",
            Ty::Bool => "true/false",
            Ty::Color => "color",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub message: String,
    /// Byte offset into the source, when the error has a position.
    pub at: Option<usize>,
}

impl Error {
    fn at(at: usize, message: impl Into<String>) -> Self {
        Error { message: message.into(), at: Some(at) }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.at {
            Some(at) => write!(f, "{} (at character {})", self.message, at + 1),
            None => write!(f, "{}", self.message),
        }
    }
}

/// Names an expression may use, each bound to a slot (a color takes four consecutive slots).
#[derive(Clone, Default)]
pub struct Scope {
    names: HashMap<String, (u16, Ty)>,
    /// Names that exist elsewhere but not here, with why.
    reserved: HashMap<String, &'static str>,
}

impl Scope {
    pub fn bind(&mut self, name: &str, slot: u16, ty: Ty) {
        self.names.insert(name.to_string(), (slot, ty));
    }

    /// Keeps `name` free for the places that provide it, explaining where that is.
    pub fn reserve(&mut self, name: &str, why: &'static str) {
        self.reserved.insert(name.to_string(), why);
    }

    pub fn get(&self, name: &str) -> Option<(u16, Ty)> {
        self.names.get(name).copied()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.names.contains_key(name) || self.reserved.contains_key(name) || is_reserved(name)
    }

    fn known(&self) -> impl Iterator<Item = &str> {
        self.names.keys().map(|s| s.as_str()).chain(CONSTANTS.iter().map(|(n, _)| *n)).chain(FUNCTIONS.iter().copied())
    }
}

const CONSTANTS: &[(&str, f64)] = &[("pi", std::f64::consts::PI), ("tau", std::f64::consts::TAU)];

const FUNCTIONS: &[&str] = &[
    "sin",
    "cos",
    "tan",
    "asin",
    "acos",
    "atan",
    "atan2",
    "sqrt",
    "exp",
    "log",
    "pow",
    "abs",
    "sign",
    "floor",
    "ceil",
    "round",
    "fract",
    "trunc",
    "int",
    "sqr",
    "mod",
    "min",
    "max",
    "clamp",
    "mix",
    "smoothstep",
    "step",
    "sigmoid",
    "remap",
    "ease_in",
    "ease_out",
    "ease_in_out",
    "tri",
    "saw",
    "hash",
    "rand",
    "noise",
    "noise2",
    "interp",
    "pick",
    "rgb",
    "rgba",
    "hsv",
    "gray",
    "alpha",
];

/// Names a pack may not bind, so a value can never hide a function or constant.
pub fn is_reserved(name: &str) -> bool {
    matches!(name, "true" | "false") || CONSTANTS.iter().any(|(n, _)| *n == name) || FUNCTIONS.contains(&name)
}

pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('a'..='z' | '_')) && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_')) && name.len() <= 40
}

// ---------------------------------------------------------------------------------------------------------------
// Lexing

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Color([f64; 4]),
    Ident(String),
    Sym(&'static str),
    End,
}

const SYMBOLS: &[&str] = &["<=", ">=", "==", "!=", "&&", "||", "+", "-", "*", "/", "%", "^", "<", ">", "!", "?", ":", "(", ")", ","];

fn lex(src: &str) -> Result<Vec<(Tok, usize)>, Error> {
    if src.len() > MAX_SOURCE {
        return Err(Error { message: format!("expression is longer than {MAX_SOURCE} characters"), at: None });
    }
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        if c.is_ascii_digit() || (c == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                let mut j = i + 1;
                if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
                    j += 1;
                }
                if j < bytes.len() && bytes[j].is_ascii_digit() {
                    i = j;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text = &src[start..i];
            let value: f64 = text.parse().map_err(|_| Error::at(start, format!("'{text}' is not a number")))?;
            if !value.is_finite() {
                return Err(Error::at(start, format!("'{text}' is too large")));
            }
            out.push((Tok::Num(value), start));
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            out.push((Tok::Ident(src[start..i].to_string()), start));
        } else if c == b'#' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                i += 1;
            }
            let color =
                parse_hex(&src[start..i]).ok_or_else(|| Error::at(start, format!("'{}' is not a color; write #rgb, #rrggbb or #rrggbbaa", &src[start..i])))?;
            out.push((Tok::Color(color), start));
        } else if let Some(sym) = SYMBOLS.iter().find(|s| src[i..].starts_with(**s)) {
            i += sym.len();
            out.push((Tok::Sym(sym), start));
        } else {
            let ch = src[i..].chars().next().unwrap_or('?');
            return Err(Error::at(start, format!("unexpected character '{ch}'")));
        }
    }
    out.push((Tok::End, src.len()));
    Ok(out)
}

/// Parses #rgb, #rgba, #rrggbb or #rrggbbaa into RGBA components between 0 and 1.
pub fn parse_hex(text: &str) -> Option<[f64; 4]> {
    let hex = text.strip_prefix('#')?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok().map(|v| v as f64 / 15.0);
    let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok().map(|v| v as f64 / 255.0);
    match hex.len() {
        3 => Some([digit(0)?, digit(1)?, digit(2)?, 1.0]),
        4 => Some([digit(0)?, digit(1)?, digit(2)?, digit(3)?]),
        6 => Some([pair(0)?, pair(2)?, pair(4)?, 1.0]),
        8 => Some([pair(0)?, pair(2)?, pair(4)?, pair(6)?]),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Parsing

#[derive(Debug)]
enum Node {
    Num(f64),
    Color([f64; 4]),
    Bool(bool),
    Name(String),
    Unary(&'static str, Box<Node>),
    Binary(&'static str, Box<Node>, Box<Node>),
    Ternary(Box<Node>, Box<Node>, Box<Node>),
    Call(String, Vec<Node>),
}

struct Parser {
    toks: Vec<(Tok, usize)>,
    pos: usize,
    nodes: usize,
    depth: usize,
}

type Spanned = (Node, usize);

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].0
    }

    fn here(&self) -> usize {
        self.toks[self.pos].1
    }

    fn eat(&mut self, sym: &str) -> bool {
        if matches!(self.peek(), Tok::Sym(s) if *s == sym) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, sym: &str) -> Result<(), Error> {
        if self.eat(sym) {
            Ok(())
        } else {
            Err(Error::at(self.here(), format!("expected '{sym}'")))
        }
    }

    fn node(&mut self, node: Node, at: usize) -> Result<Spanned, Error> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(Error::at(at, format!("expression has more than {MAX_NODES} parts")));
        }
        Ok((node, at))
    }

    fn enter(&mut self) -> Result<(), Error> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Error::at(self.here(), format!("expression nests more than {MAX_DEPTH} levels deep")));
        }
        Ok(())
    }

    fn expr(&mut self) -> Result<Spanned, Error> {
        self.enter()?;
        let cond = self.or()?;
        let result = if matches!(self.peek(), Tok::Sym("?")) {
            let at = self.here();
            self.pos += 1;
            let a = self.expr()?;
            self.expect(":")?;
            let b = self.expr()?;
            self.node(Node::Ternary(Box::new(cond.0), Box::new(a.0), Box::new(b.0)), at)?
        } else {
            cond
        };
        self.depth -= 1;
        Ok(result)
    }

    fn binary_level(&mut self, ops: &[&'static str], next: fn(&mut Self) -> Result<Spanned, Error>, chain: bool) -> Result<Spanned, Error> {
        let mut left = next(self)?;
        while let Tok::Sym(sym) = *self.peek() {
            let Some(op) = ops.iter().find(|o| **o == sym) else { break };
            let at = self.here();
            self.pos += 1;
            let right = next(self)?;
            left = self.node(Node::Binary(op, Box::new(left.0), Box::new(right.0)), at)?;
            if !chain {
                if let Tok::Sym(s) = *self.peek() {
                    if ops.contains(&s) {
                        return Err(Error::at(self.here(), "comparisons can't be chained; combine them with && or ||"));
                    }
                }
                break;
            }
        }
        Ok(left)
    }

    fn or(&mut self) -> Result<Spanned, Error> {
        self.binary_level(&["||"], Self::and, true)
    }

    fn and(&mut self) -> Result<Spanned, Error> {
        self.binary_level(&["&&"], Self::cmp, true)
    }

    fn cmp(&mut self) -> Result<Spanned, Error> {
        self.binary_level(&["<=", ">=", "==", "!=", "<", ">"], Self::add, false)
    }

    fn add(&mut self) -> Result<Spanned, Error> {
        self.binary_level(&["+", "-"], Self::mul, true)
    }

    fn mul(&mut self) -> Result<Spanned, Error> {
        self.binary_level(&["*", "/", "%"], Self::unary, true)
    }

    fn unary(&mut self) -> Result<Spanned, Error> {
        if let Tok::Sym(sym @ ("-" | "!")) = *self.peek() {
            let at = self.here();
            self.pos += 1;
            self.enter()?;
            let inner = self.unary()?;
            self.depth -= 1;
            return self.node(Node::Unary(sym, Box::new(inner.0)), at);
        }
        self.power()
    }

    fn power(&mut self) -> Result<Spanned, Error> {
        let base = self.primary()?;
        if matches!(self.peek(), Tok::Sym("^")) {
            let at = self.here();
            self.pos += 1;
            self.enter()?;
            let exponent = self.unary()?;
            self.depth -= 1;
            return self.node(Node::Binary("^", Box::new(base.0), Box::new(exponent.0)), at);
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<Spanned, Error> {
        let at = self.here();
        match self.peek().clone() {
            Tok::Num(v) => {
                self.pos += 1;
                self.node(Node::Num(v), at)
            }
            Tok::Color(c) => {
                self.pos += 1;
                self.node(Node::Color(c), at)
            }
            Tok::Ident(name) => {
                self.pos += 1;
                if self.eat("(") {
                    let mut args = Vec::new();
                    if !self.eat(")") {
                        loop {
                            args.push(self.expr()?.0);
                            if args.len() > MAX_VARIADIC * 2 + 1 {
                                return Err(Error::at(at, format!("too many arguments to {name}()")));
                            }
                            if self.eat(")") {
                                break;
                            }
                            if !self.eat(",") {
                                return Err(Error::at(self.here(), format!("expected ',' or ')' in the call to {name}()")));
                            }
                        }
                    }
                    self.node(Node::Call(name, args), at)
                } else if name == "true" || name == "false" {
                    self.node(Node::Bool(name == "true"), at)
                } else {
                    self.node(Node::Name(name), at)
                }
            }
            Tok::Sym("(") => {
                self.pos += 1;
                let inner = self.expr()?;
                self.expect(")")?;
                Ok(inner)
            }
            Tok::End => Err(Error::at(at, "expression ended early")),
            Tok::Sym(s) => Err(Error::at(at, format!("unexpected '{s}'"))),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Compiling

#[derive(Clone, Copy, Debug, PartialEq)]
enum F1 {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Sqrt,
    Exp,
    Log,
    Abs,
    Sign,
    Floor,
    Ceil,
    Round,
    Fract,
    Trunc,
    Sqr,
    EaseIn,
    EaseOut,
    EaseInOut,
    Tri,
    Saw,
    Hash,
    Rand,
    Noise,
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum F2 {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Atan2,
    Step,
    Sigmoid,
    Noise2,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Const(f64),
    Load(u16),
    /// Load a color from four consecutive slots.
    Load4(u16),
    F1(F1),
    F2(F2),
    Clamp,
    Mix,
    MixColor,
    Smoothstep,
    Remap,
    Min(u8),
    Max(u8),
    /// cond, a, b -> a or b, each `lanes` wide.
    Select(u8),
    /// index, then `count` options each `lanes` wide.
    Pick(u8, u8),
    /// x, then `pairs` (x, y) pairs.
    Interp(u8),
    Rgba,
    Hsv,
    Alpha,
}

#[derive(Clone, Debug)]
pub struct Program {
    ops: Vec<Op>,
    pub ty: Ty,
    max_stack: usize,
}

impl Program {
    /// The number of operations one evaluation performs.
    pub fn cost(&self) -> usize {
        self.ops.len()
    }
}

struct Compiler<'a> {
    scope: &'a Scope,
    ops: Vec<Op>,
    depth: usize,
    max_depth: usize,
}

fn suggest<'a>(name: &str, known: impl Iterator<Item = &'a str>) -> String {
    let best = known.map(|k| (edit_distance(name, k), k)).filter(|(d, _)| *d <= 2).min();
    match best {
        Some((_, k)) => format!(" (did you mean '{k}'?)"),
        None => String::new(),
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push((prev[j] + usize::from(ca != *cb)).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

impl Compiler<'_> {
    fn push(&mut self, op: Op, pushed: usize, popped: usize) {
        self.ops.push(op);
        self.depth = self.depth + pushed - popped;
        self.max_depth = self.max_depth.max(self.depth);
    }

    fn want(&mut self, node: &Node, at: usize, ty: Ty, what: &str) -> Result<(), Error> {
        let got = self.compile(node, at)?;
        if got != ty {
            return Err(Error::at(at, format!("{what} must be a {}, not a {}", ty.name(), got.name())));
        }
        Ok(())
    }

    fn compile(&mut self, node: &Node, at: usize) -> Result<Ty, Error> {
        Ok(match node {
            Node::Num(v) => {
                self.push(Op::Const(*v), 1, 0);
                Ty::Num
            }
            Node::Bool(b) => {
                self.push(Op::Const(if *b { 1.0 } else { 0.0 }), 1, 0);
                Ty::Bool
            }
            Node::Color(c) => {
                for v in c {
                    self.push(Op::Const(*v), 1, 0);
                }
                Ty::Color
            }
            Node::Name(name) => {
                if let Some((slot, ty)) = self.scope.get(name) {
                    if ty == Ty::Color {
                        self.push(Op::Load4(slot), 4, 0);
                    } else {
                        self.push(Op::Load(slot), 1, 0);
                    }
                    ty
                } else if let Some((_, v)) = CONSTANTS.iter().find(|(n, _)| n == name) {
                    self.push(Op::Const(*v), 1, 0);
                    Ty::Num
                } else if FUNCTIONS.contains(&name.as_str()) {
                    return Err(Error::at(at, format!("{name} is a function; call it as {name}(...)")));
                } else if let Some(why) = self.scope.reserved.get(name) {
                    return Err(Error::at(at, format!("'{name}' {why}")));
                } else {
                    return Err(Error::at(at, format!("unknown name '{name}'{}", suggest(name, self.scope.known()))));
                }
            }
            Node::Unary(op, inner) => {
                if *op == "-" {
                    self.want(inner, at, Ty::Num, "the value after '-'")?;
                    self.push(Op::F1(F1::Neg), 1, 1);
                    Ty::Num
                } else {
                    self.want(inner, at, Ty::Bool, "the value after '!'")?;
                    self.push(Op::F1(F1::Not), 1, 1);
                    Ty::Bool
                }
            }
            Node::Binary(op, a, b) => {
                let (operand, result, f) = match *op {
                    "+" => (Ty::Num, Ty::Num, F2::Add),
                    "-" => (Ty::Num, Ty::Num, F2::Sub),
                    "*" => (Ty::Num, Ty::Num, F2::Mul),
                    "/" => (Ty::Num, Ty::Num, F2::Div),
                    "%" => (Ty::Num, Ty::Num, F2::Mod),
                    "^" => (Ty::Num, Ty::Num, F2::Pow),
                    "<" => (Ty::Num, Ty::Bool, F2::Lt),
                    "<=" => (Ty::Num, Ty::Bool, F2::Le),
                    ">" => (Ty::Num, Ty::Bool, F2::Gt),
                    ">=" => (Ty::Num, Ty::Bool, F2::Ge),
                    "&&" => (Ty::Bool, Ty::Bool, F2::And),
                    "||" => (Ty::Bool, Ty::Bool, F2::Or),
                    "==" | "!=" => {
                        let ta = self.compile(a, at)?;
                        let tb = self.compile(b, at)?;
                        if ta != tb || ta == Ty::Color {
                            return Err(Error::at(at, format!("'{op}' compares two numbers or two true/false values")));
                        }
                        self.push(Op::F2(if *op == "==" { F2::Eq } else { F2::Ne }), 1, 2);
                        return Ok(Ty::Bool);
                    }
                    _ => unreachable!("parser only produces known operators"),
                };
                let side = format!("both sides of '{op}'");
                self.want(a, at, operand, &side)?;
                self.want(b, at, operand, &side)?;
                self.push(Op::F2(f), 1, 2);
                result
            }
            Node::Ternary(c, a, b) => {
                self.want(c, at, Ty::Bool, "the condition before '?'")?;
                let ta = self.compile(a, at)?;
                let tb = self.compile(b, at)?;
                if ta != tb {
                    return Err(Error::at(at, format!("both results of '?:' must be the same kind, not a {} and a {}", ta.name(), tb.name())));
                }
                let lanes = ta.lanes();
                self.push(Op::Select(lanes as u8), lanes, 1 + 2 * lanes);
                ta
            }
            Node::Call(name, args) => self.call(name, args, at)?,
        })
    }

    fn args(&mut self, name: &str, args: &[Node], at: usize, tys: &[Ty]) -> Result<(), Error> {
        if args.len() != tys.len() {
            let n = tys.len();
            return Err(Error::at(at, format!("{name}() takes {n} argument{}", if n == 1 { "" } else { "s" })));
        }
        for (i, (arg, ty)) in args.iter().zip(tys).enumerate() {
            self.want(arg, at, *ty, &format!("argument {} of {name}()", i + 1))?;
        }
        Ok(())
    }

    fn call(&mut self, name: &str, args: &[Node], at: usize) -> Result<Ty, Error> {
        use Ty::*;
        let f1 = match name {
            "sin" => Some(F1::Sin),
            "cos" => Some(F1::Cos),
            "tan" => Some(F1::Tan),
            "asin" => Some(F1::Asin),
            "acos" => Some(F1::Acos),
            "atan" => Some(F1::Atan),
            "sqrt" => Some(F1::Sqrt),
            "exp" => Some(F1::Exp),
            "log" => Some(F1::Log),
            "abs" => Some(F1::Abs),
            "sign" => Some(F1::Sign),
            "floor" => Some(F1::Floor),
            "ceil" => Some(F1::Ceil),
            "round" => Some(F1::Round),
            "fract" => Some(F1::Fract),
            "trunc" | "int" => Some(F1::Trunc),
            "sqr" => Some(F1::Sqr),
            "ease_in" => Some(F1::EaseIn),
            "ease_out" => Some(F1::EaseOut),
            "ease_in_out" => Some(F1::EaseInOut),
            "tri" => Some(F1::Tri),
            "saw" => Some(F1::Saw),
            "hash" => Some(F1::Hash),
            "rand" => Some(F1::Rand),
            "noise" => Some(F1::Noise),
            _ => None,
        };
        if let Some(f) = f1 {
            self.args(name, args, at, &[Num])?;
            self.push(Op::F1(f), 1, 1);
            return Ok(Num);
        }
        let f2 = match name {
            "atan2" => Some(F2::Atan2),
            "pow" => Some(F2::Pow),
            "mod" => Some(F2::Mod),
            "step" => Some(F2::Step),
            "sigmoid" => Some(F2::Sigmoid),
            "noise2" => Some(F2::Noise2),
            _ => None,
        };
        if let Some(f) = f2 {
            self.args(name, args, at, &[Num, Num])?;
            self.push(Op::F2(f), 1, 2);
            return Ok(Num);
        }
        Ok(match name {
            "clamp" => {
                self.args(name, args, at, &[Num, Num, Num])?;
                self.push(Op::Clamp, 1, 3);
                Num
            }
            "smoothstep" => {
                self.args(name, args, at, &[Num, Num, Num])?;
                self.push(Op::Smoothstep, 1, 3);
                Num
            }
            "remap" => {
                self.args(name, args, at, &[Num, Num, Num, Num, Num])?;
                self.push(Op::Remap, 1, 5);
                Num
            }
            "mix" => {
                if args.len() != 3 {
                    return Err(Error::at(at, "mix() takes 3 arguments"));
                }
                let ty = self.compile(&args[0], at)?;
                if ty == Bool {
                    return Err(Error::at(at, "mix() blends two numbers or two colors"));
                }
                self.want(&args[1], at, ty, "argument 2 of mix()")?;
                self.want(&args[2], at, Num, "argument 3 of mix()")?;
                if ty == Color {
                    self.push(Op::MixColor, 4, 9);
                } else {
                    self.push(Op::Mix, 1, 3);
                }
                ty
            }
            "min" | "max" => {
                if args.len() < 2 || args.len() > MAX_VARIADIC {
                    return Err(Error::at(at, format!("{name}() takes 2 to {MAX_VARIADIC} numbers")));
                }
                for (i, arg) in args.iter().enumerate() {
                    self.want(arg, at, Num, &format!("argument {} of {name}()", i + 1))?;
                }
                let n = args.len() as u8;
                self.push(if name == "min" { Op::Min(n) } else { Op::Max(n) }, 1, args.len());
                Num
            }
            "pick" => {
                if args.len() < 3 || args.len() > MAX_VARIADIC + 1 {
                    return Err(Error::at(at, format!("pick() takes an index and 2 to {MAX_VARIADIC} choices")));
                }
                self.want(&args[0], at, Num, "the index given to pick()")?;
                let ty = self.compile(&args[1], at)?;
                for (i, arg) in args.iter().enumerate().skip(2) {
                    self.want(arg, at, ty, &format!("choice {i} of pick()"))?;
                }
                let count = args.len() - 1;
                self.push(Op::Pick(count as u8, ty.lanes() as u8), ty.lanes(), 1 + count * ty.lanes());
                ty
            }
            "interp" => {
                if args.len() < 5 || args.len().is_multiple_of(2) || args.len() > 2 * MAX_VARIADIC + 1 {
                    return Err(Error::at(at, format!("interp() takes a position and 2 to {MAX_VARIADIC} (position, value) pairs")));
                }
                for (i, arg) in args.iter().enumerate() {
                    self.want(arg, at, Num, &format!("argument {} of interp()", i + 1))?;
                }
                let pairs = (args.len() - 1) / 2;
                self.push(Op::Interp(pairs as u8), 1, args.len());
                Num
            }
            "rgb" | "gray" | "rgba" | "hsv" => {
                match name {
                    "rgb" => {
                        self.args(name, args, at, &[Num, Num, Num])?;
                        self.push(Op::Const(1.0), 1, 0);
                        self.push(Op::Rgba, 4, 4);
                    }
                    "gray" => {
                        self.args(name, args, at, &[Num])?;
                        // Repeat the value: load it once per channel by recompiling the same argument.
                        self.compile(&args[0], at)?;
                        self.compile(&args[0], at)?;
                        self.push(Op::Const(1.0), 1, 0);
                        self.push(Op::Rgba, 4, 4);
                    }
                    "rgba" => {
                        self.args(name, args, at, &[Num, Num, Num, Num])?;
                        self.push(Op::Rgba, 4, 4);
                    }
                    _ => {
                        self.args(name, args, at, &[Num, Num, Num])?;
                        self.push(Op::Hsv, 4, 3);
                    }
                }
                Color
            }
            "alpha" => {
                self.args(name, args, at, &[Color, Num])?;
                self.push(Op::Alpha, 4, 5);
                Color
            }
            _ => {
                let known = FUNCTIONS.iter().copied();
                return Err(Error::at(at, format!("unknown function '{name}'{}", suggest(name, known))));
            }
        })
    }
}

/// Compiles `src` in `scope`, requiring a result of type `want`.
pub fn compile(src: &str, scope: &Scope, want: Ty) -> Result<Program, Error> {
    let program = compile_any(src, scope)?;
    if program.ty != want {
        return Err(Error { message: format!("expected a {}, but this is a {}", want.name(), program.ty.name()), at: None });
    }
    Ok(program)
}

pub fn compile_any(src: &str, scope: &Scope) -> Result<Program, Error> {
    let toks = lex(src)?;
    let mut parser = Parser { toks, pos: 0, nodes: 0, depth: 0 };
    let (node, at) = parser.expr()?;
    if !matches!(parser.peek(), Tok::End) {
        return Err(Error::at(parser.here(), "unexpected text after the expression"));
    }
    let mut compiler = Compiler { scope, ops: Vec::new(), depth: 0, max_depth: 0 };
    let ty = compiler.compile(&node, at)?;
    debug_assert_eq!(compiler.depth, ty.lanes());
    Ok(Program { ops: compiler.ops, ty, max_stack: compiler.max_depth })
}

// ---------------------------------------------------------------------------------------------------------------
// Evaluating

#[inline]
fn fin(x: f64) -> f64 {
    if x.is_finite() {
        x
    } else {
        0.0
    }
}

#[inline]
fn truth(x: f64) -> bool {
    x != 0.0
}

#[inline]
fn flag(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
}

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A value in [0, 1) that depends only on `x` and `seed`.
pub fn hash(x: f64, seed: u64) -> f64 {
    let x = if x == 0.0 { 0.0 } else { x }; // -0 and 0 hash alike
    (splitmix(x.to_bits() ^ splitmix(seed)) >> 11) as f64 / (1u64 << 53) as f64
}

fn smooth(f: f64) -> f64 {
    f * f * (3.0 - 2.0 * f)
}

fn noise1(x: f64) -> f64 {
    let i = x.floor();
    let f = x - i;
    let a = hash(i, 0x6e6f);
    let b = hash(i + 1.0, 0x6e6f);
    a + (b - a) * smooth(f)
}

fn noise2(x: f64, y: f64) -> f64 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (smooth(x - ix), smooth(y - iy));
    let corner = |dx: f64, dy: f64| hash(ix + dx + (iy + dy) * 7919.0, 0x6e32);
    let top = corner(0.0, 0.0) + (corner(1.0, 0.0) - corner(0.0, 0.0)) * fx;
    let bottom = corner(0.0, 1.0) + (corner(1.0, 1.0) - corner(0.0, 1.0)) * fx;
    top + (bottom - top) * fy
}

fn hsv(h: f64, s: f64, v: f64) -> [f64; 3] {
    let h = (h - h.floor()) * 6.0;
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r + m, g + m, b + m]
}

fn apply1(f: F1, x: f64, seed: u64) -> f64 {
    fin(match f {
        F1::Sin => x.sin(),
        F1::Cos => x.cos(),
        F1::Tan => x.tan(),
        F1::Asin => x.clamp(-1.0, 1.0).asin(),
        F1::Acos => x.clamp(-1.0, 1.0).acos(),
        F1::Atan => x.atan(),
        F1::Sqrt => {
            if x > 0.0 {
                x.sqrt()
            } else {
                0.0
            }
        }
        F1::Exp => x.exp(),
        F1::Log => {
            if x > 0.0 {
                x.ln()
            } else {
                0.0
            }
        }
        F1::Abs => x.abs(),
        F1::Sign => {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                0.0
            }
        }
        F1::Floor => x.floor(),
        F1::Ceil => x.ceil(),
        F1::Round => x.round(),
        F1::Fract => x - x.floor(),
        F1::Trunc => x.trunc(),
        F1::Sqr => x * x,
        F1::EaseIn => {
            let u = x.clamp(0.0, 1.0);
            u * u * u
        }
        F1::EaseOut => {
            let u = 1.0 - x.clamp(0.0, 1.0);
            1.0 - u * u * u
        }
        F1::EaseInOut => {
            let u = x.clamp(0.0, 1.0);
            if u < 0.5 {
                4.0 * u * u * u
            } else {
                1.0 - (-2.0 * u + 2.0).powi(3) / 2.0
            }
        }
        F1::Tri => 1.0 - (2.0 * (x - x.floor()) - 1.0).abs(),
        F1::Saw => x - x.floor(),
        F1::Hash => hash(x, 0),
        F1::Rand => hash(x, seed),
        F1::Noise => noise1(x),
        F1::Neg => -x,
        F1::Not => flag(!truth(x)),
    })
}

fn apply2(f: F2, a: f64, b: f64) -> f64 {
    fin(match f {
        F2::Add => a + b,
        F2::Sub => a - b,
        F2::Mul => a * b,
        F2::Div => {
            if b == 0.0 {
                0.0
            } else {
                a / b
            }
        }
        F2::Mod => {
            if b == 0.0 {
                0.0
            } else {
                a - b * (a / b).floor()
            }
        }
        F2::Pow => a.powf(b),
        F2::Atan2 => a.atan2(b),
        F2::Step => flag(b >= a),
        F2::Sigmoid => 1.0 / (1.0 + (-(a * b)).min(700.0).exp()),
        F2::Noise2 => noise2(a, b),
        F2::Lt => flag(a < b),
        F2::Le => flag(a <= b),
        F2::Gt => flag(a > b),
        F2::Ge => flag(a >= b),
        F2::Eq => flag(a == b),
        F2::Ne => flag(a != b),
        F2::And => flag(truth(a) && truth(b)),
        F2::Or => flag(truth(a) || truth(b)),
    })
}

/// Reusable evaluation memory, so a frame allocates nothing.
#[derive(Default)]
pub struct Machine {
    stack: Vec<f64>,
    pub seed: u64,
}

impl Machine {
    pub fn new(seed: u64) -> Self {
        Machine { stack: Vec::with_capacity(64), seed }
    }

    fn run(&mut self, program: &Program, slots: &[f64]) {
        let stack = &mut self.stack;
        stack.clear();
        stack.reserve(program.max_stack);
        for op in &program.ops {
            match *op {
                Op::Const(v) => stack.push(v),
                Op::Load(slot) => stack.push(slots[slot as usize]),
                Op::Load4(slot) => stack.extend_from_slice(&slots[slot as usize..slot as usize + 4]),
                Op::F1(f) => {
                    let x = stack.pop().unwrap_or(0.0);
                    stack.push(apply1(f, x, self.seed));
                }
                Op::F2(f) => {
                    let b = stack.pop().unwrap_or(0.0);
                    let a = stack.pop().unwrap_or(0.0);
                    stack.push(apply2(f, a, b));
                }
                Op::Clamp => {
                    let hi = stack.pop().unwrap_or(0.0);
                    let lo = stack.pop().unwrap_or(0.0);
                    let x = stack.pop().unwrap_or(0.0);
                    stack.push(x.max(lo.min(hi)).min(lo.max(hi)));
                }
                Op::Mix => {
                    let t = stack.pop().unwrap_or(0.0);
                    let b = stack.pop().unwrap_or(0.0);
                    let a = stack.pop().unwrap_or(0.0);
                    stack.push(fin(a + (b - a) * t));
                }
                Op::MixColor => {
                    let t = stack.pop().unwrap_or(0.0).clamp(0.0, 1.0);
                    let n = stack.len();
                    if n >= 8 {
                        for k in 0..4 {
                            let a = stack[n - 8 + k];
                            let b = stack[n - 4 + k];
                            stack[n - 8 + k] = a + (b - a) * t;
                        }
                        stack.truncate(n - 4);
                    }
                }
                Op::Smoothstep => {
                    let x = stack.pop().unwrap_or(0.0);
                    let e1 = stack.pop().unwrap_or(0.0);
                    let e0 = stack.pop().unwrap_or(0.0);
                    let u = if e1 == e0 { flag(x >= e0) } else { ((x - e0) / (e1 - e0)).clamp(0.0, 1.0) };
                    stack.push(fin(smooth(u)));
                }
                Op::Remap => {
                    let d = stack.pop().unwrap_or(0.0);
                    let c = stack.pop().unwrap_or(0.0);
                    let b = stack.pop().unwrap_or(0.0);
                    let a = stack.pop().unwrap_or(0.0);
                    let x = stack.pop().unwrap_or(0.0);
                    let u = if b == a { 0.0 } else { (x - a) / (b - a) };
                    stack.push(fin(c + (d - c) * u));
                }
                Op::Min(n) | Op::Max(n) => {
                    let start = stack.len() - n as usize;
                    let is_min = matches!(op, Op::Min(_));
                    let mut best = stack[start];
                    for &v in &stack[start + 1..] {
                        best = if is_min { best.min(v) } else { best.max(v) };
                    }
                    stack.truncate(start);
                    stack.push(best);
                }
                Op::Select(lanes) => {
                    let lanes = lanes as usize;
                    let n = stack.len();
                    let cond = stack[n - 1 - 2 * lanes];
                    let from = if truth(cond) { n - 2 * lanes } else { n - lanes };
                    let base = n - 1 - 2 * lanes;
                    for k in 0..lanes {
                        stack[base + k] = stack[from + k];
                    }
                    stack.truncate(base + lanes);
                }
                Op::Pick(count, lanes) => {
                    let (count, lanes) = (count as usize, lanes as usize);
                    let n = stack.len();
                    let base = n - count * lanes - 1;
                    let index = stack[base].floor();
                    let chosen = (index - (count as f64) * (index / count as f64).floor()) as usize % count;
                    let from = base + 1 + chosen * lanes;
                    for k in 0..lanes {
                        stack[base + k] = stack[from + k];
                    }
                    stack.truncate(base + lanes);
                }
                Op::Interp(pairs) => {
                    let pairs = pairs as usize;
                    let n = stack.len();
                    let base = n - 2 * pairs - 1;
                    let x = stack[base];
                    let key = |k: usize| (stack[base + 1 + 2 * k], stack[base + 2 + 2 * k]);
                    let mut value = key(pairs - 1).1;
                    if x <= key(0).0 {
                        value = key(0).1;
                    } else {
                        for k in 0..pairs - 1 {
                            let (x0, y0) = key(k);
                            let (x1, y1) = key(k + 1);
                            if x < x1 {
                                value = if x1 > x0 { y0 + (y1 - y0) * ((x - x0) / (x1 - x0)).clamp(0.0, 1.0) } else { y1 };
                                break;
                            }
                        }
                    }
                    stack.truncate(base);
                    stack.push(fin(value));
                }
                Op::Rgba => {
                    let n = stack.len();
                    for v in &mut stack[n - 4..] {
                        *v = v.clamp(0.0, 1.0);
                    }
                }
                Op::Hsv => {
                    let v = stack.pop().unwrap_or(0.0);
                    let s = stack.pop().unwrap_or(0.0);
                    let h = stack.pop().unwrap_or(0.0);
                    let [r, g, b] = hsv(h, s, v);
                    stack.extend_from_slice(&[r, g, b, 1.0]);
                }
                Op::Alpha => {
                    let a = stack.pop().unwrap_or(0.0).clamp(0.0, 1.0);
                    let n = stack.len();
                    stack[n - 1] *= a;
                }
            }
        }
    }

    pub fn num(&mut self, program: &Program, slots: &[f64]) -> f64 {
        self.run(program, slots);
        fin(self.stack.last().copied().unwrap_or(0.0))
    }

    pub fn truth(&mut self, program: &Program, slots: &[f64]) -> bool {
        self.num(program, slots) != 0.0
    }

    pub fn color(&mut self, program: &Program, slots: &[f64]) -> [f64; 4] {
        self.run(program, slots);
        let n = self.stack.len();
        if n < 4 {
            return [0.0, 0.0, 0.0, 1.0];
        }
        let mut c = [0.0; 4];
        for (k, v) in c.iter_mut().enumerate() {
            *v = fin(self.stack[n - 4 + k]).clamp(0.0, 1.0);
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> Scope {
        let mut s = Scope::default();
        s.bind("t", 0, Ty::Num);
        s.bind("u", 1, Ty::Num);
        s.bind("accent", 2, Ty::Color);
        s.bind("ok", 6, Ty::Bool);
        s.reserve("s", "is only available in px and py");
        s
    }

    fn slots() -> Vec<f64> {
        vec![2.0, 0.5, 0.2, 0.4, 0.6, 1.0, 1.0]
    }

    fn num(src: &str) -> f64 {
        let p = compile(src, &scope(), Ty::Num).unwrap_or_else(|e| panic!("{src}: {e}"));
        Machine::new(7).num(&p, &slots())
    }

    fn boolean(src: &str) -> bool {
        let p = compile(src, &scope(), Ty::Bool).unwrap_or_else(|e| panic!("{src}: {e}"));
        Machine::new(7).truth(&p, &slots())
    }

    fn color(src: &str) -> [f64; 4] {
        let p = compile(src, &scope(), Ty::Color).unwrap_or_else(|e| panic!("{src}: {e}"));
        Machine::new(7).color(&p, &slots())
    }

    fn err(src: &str) -> String {
        compile_any(src, &scope()).err().unwrap_or_else(|| panic!("{src} compiled")).to_string()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(num("1 + 2 * 3"), 7.0);
        assert_eq!(num("(1 + 2) * 3"), 9.0);
        assert_eq!(num("2 ^ 3 ^ 2"), 512.0);
        assert_eq!(num("-2 ^ 2"), -4.0);
        assert_eq!(num("2 ^ -1"), 0.5);
        assert_eq!(num("10 - 4 - 3"), 3.0);
        assert_eq!(num("t * u"), 1.0);
        assert_eq!(num("1.5e2 + .5"), 150.5);
        assert_eq!(num("7 % 3"), 1.0);
        assert_eq!(num("-7 % 3"), 2.0);
    }

    #[test]
    fn totality() {
        assert_eq!(num("1 / 0"), 0.0);
        assert_eq!(num("1 % 0"), 0.0);
        assert_eq!(num("sqrt(-4)"), 0.0);
        assert_eq!(num("log(0)"), 0.0);
        assert_eq!(num("log(-1)"), 0.0);
        assert_eq!(num("(-8) ^ 0.5"), 0.0);
        assert_eq!(num("exp(100000)"), 0.0);
        assert_eq!(num("pow(10, 400)"), 0.0);
        assert_eq!(num("tan(pi / 2) * 0"), 0.0);
        assert_eq!(num("1e308 * 10"), 0.0);
        assert_eq!(num("asin(5)"), std::f64::consts::FRAC_PI_2);
    }

    #[test]
    fn functions() {
        assert!(close(num("mix(0.012, 0.17, u)"), 0.091));
        assert_eq!(num("clamp(5, 0, 1)"), 1.0);
        assert_eq!(num("clamp(5, 1, 0)"), 1.0);
        assert_eq!(num("min(3, 1, 2)"), 1.0);
        assert_eq!(num("max(3, 1, 2)"), 3.0);
        assert_eq!(num("smoothstep(0, 1, 0.5)"), 0.5);
        assert_eq!(num("step(0.5, u)"), 1.0);
        assert_eq!(num("remap(5, 0, 10, 100, 200)"), 150.0);
        assert_eq!(num("pick(1, 10, 20, 30)"), 20.0);
        assert_eq!(num("pick(4, 10, 20, 30)"), 20.0);
        assert_eq!(num("pick(-1, 10, 20, 30)"), 30.0);
        assert_eq!(num("interp(0.5, 0, 0, 1, 10)"), 5.0);
        assert_eq!(num("interp(-1, 0, 3, 1, 10)"), 3.0);
        assert_eq!(num("interp(2, 0, 3, 1, 10)"), 10.0);
        assert_eq!(num("interp(1.5, 0, 0, 1, 10, 2, 0)"), 5.0);
        assert_eq!(num("sqr(3)"), 9.0);
        assert_eq!(num("int(-2.7)"), -2.0);
        assert_eq!(num("fract(-0.25)"), 0.75);
        assert!(close(num("sigmoid(0, 4)"), 0.5));
        assert_eq!(num("ease_in_out(0.5)"), 0.5);
        assert_eq!(num("tri(0)"), 0.0);
        assert_eq!(num("tri(0.5)"), 1.0);
        assert_eq!(num("saw(1.25)"), 0.25);
        let h = num("hash(3)");
        assert!((0.0..1.0).contains(&h));
        assert_eq!(h, num("hash(3)"));
        assert_ne!(num("hash(3)"), num("hash(4)"));
        let n = num("noise(2.5)");
        assert!((0.0..=1.0).contains(&n));
        assert_eq!(num("noise(2)"), hash(2.0, 0x6e6f));
    }

    #[test]
    fn rand_depends_on_seed() {
        let p = compile("rand(5)", &scope(), Ty::Num).unwrap();
        let a = Machine::new(1).num(&p, &slots());
        let b = Machine::new(2).num(&p, &slots());
        assert_ne!(a, b);
        assert_eq!(a, Machine::new(1).num(&p, &slots()));
    }

    #[test]
    fn booleans_and_ternary() {
        assert!(boolean("t > 1 && u < 1"));
        assert!(!boolean("!(t > 1)"));
        assert!(boolean("t == 2 || ok"));
        assert!(boolean("ok != false"));
        assert_eq!(num("t > 1 ? 10 : 20"), 10.0);
        assert_eq!(num("t > 5 ? 10 : t > 1 ? 30 : 20"), 30.0);
    }

    #[test]
    fn colors() {
        assert_eq!(color("#ff0000"), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(color("#f00a"), [1.0, 0.0, 0.0, 10.0 / 15.0]);
        assert_eq!(color("accent"), [0.2, 0.4, 0.6, 1.0]);
        assert_eq!(color("mix(#000000, #ffffff, 0.5)"), [0.5, 0.5, 0.5, 1.0]);
        assert_eq!(color("mix(#000000, #ffffff, 7)"), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(color("alpha(accent, 0.5)"), [0.2, 0.4, 0.6, 0.5]);
        assert_eq!(color("rgb(2, -1, 0.5)"), [1.0, 0.0, 0.5, 1.0]);
        assert_eq!(color("gray(u)"), [0.5, 0.5, 0.5, 1.0]);
        assert_eq!(color("pick(2, #000, #fff, accent)"), [0.2, 0.4, 0.6, 1.0]);
        assert_eq!(color("t > 1 ? accent : #000"), [0.2, 0.4, 0.6, 1.0]);
        let red = color("hsv(0, 1, 1)");
        assert_eq!(red, [1.0, 0.0, 0.0, 1.0]);
        let blue = color("hsv(2 / 3, 1, 1)");
        assert!(close(blue[2], 1.0) && blue[0] < 1e-9);
    }

    #[test]
    fn type_errors() {
        assert!(err("1 + accent").contains("must be a number"));
        assert!(err("t ? 1 : 2").contains("condition"));
        assert!(err("ok ? 1 : accent").contains("same kind"));
        assert!(err("accent == accent").contains("compares"));
        assert!(err("mix(ok, ok, 1)").contains("blends"));
        assert!(err("-ok").contains("after '-'"));
        assert!(compile("t", &scope(), Ty::Color).is_err());
    }

    #[test]
    fn syntax_errors() {
        assert!(err("1 +").contains("ended early"));
        assert!(err("(1 + 2").contains("expected ')'"));
        assert!(err("1 2").contains("after the expression"));
        assert!(err("1 < 2 < 3").contains("chained"));
        assert!(err("x = 1").contains("unexpected character '='") || err("x = 1").contains("unknown name"));
        assert!(err("$").contains("unexpected character"));
        assert!(err("tt").contains("did you mean 't'"));
        assert!(err("coss(1)").contains("did you mean 'cos'"));
        assert!(err("sin").contains("is a function"));
        assert!(err("s + 1").contains("only available in px and py"));
        assert!(err("sin(1, 2)").contains("takes 1 argument"));
        assert!(err("#12345").contains("not a color"));
        assert!(err("1e999").contains("too large"));
        assert!(err("é").contains("unexpected character"));
    }

    #[test]
    fn limits() {
        let deep = "(".repeat(100) + "1" + &")".repeat(100);
        assert!(err(&deep).contains("nests"));
        let negs = "-".repeat(100) + "1";
        assert!(err(&negs).contains("nests"));
        let long = "1+".repeat(1100) + "1";
        assert!(err(&long).contains("longer"));
        let many = "1+".repeat(600) + "1";
        assert!(err(&many).contains("parts"));
        let args = format!("min({})", vec!["1"; 17].join(","));
        assert!(err(&args).contains("2 to 16"));
        let pow = "2^".repeat(60) + "2";
        assert!(err(&pow).contains("nests"));
    }

    #[test]
    fn no_panics_on_garbage() {
        // Deterministic pseudo-random inputs drawn from the language's own alphabet.
        let alphabet: Vec<char> = "0123456789.e+-*/%^<>=!&|?:(),# tuacokpisnmxrgbfhl_".chars().collect();
        let mut state = 1u64;
        for _ in 0..20000 {
            let len = (splitmix(state) % 40) as usize;
            let mut src = String::new();
            for _ in 0..len {
                state = splitmix(state);
                src.push(alphabet[(state % alphabet.len() as u64) as usize]);
            }
            state = splitmix(state);
            if let Ok(p) = compile_any(&src, &scope()) {
                let mut m = Machine::new(state);
                match p.ty {
                    Ty::Color => {
                        let c = m.color(&p, &slots());
                        assert!(c.iter().all(|v| (0.0..=1.0).contains(v)), "{src}");
                    }
                    _ => assert!(m.num(&p, &slots()).is_finite(), "{src}"),
                }
            }
        }
    }

    #[test]
    fn reserved_names() {
        assert!(is_reserved("sin") && is_reserved("pi") && is_reserved("true") && is_reserved("rgb"));
        assert!(!is_reserved("spin"));
        assert!(valid_name("spin_2") && !valid_name("2spin") && !valid_name("Spin") && !valid_name(""));
    }

    #[test]
    fn cost_counts_operations() {
        let p = compile("sin(t) * 2 + 1", &scope(), Ty::Num).unwrap();
        assert_eq!(p.cost(), 6);
    }
}
