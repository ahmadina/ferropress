//! Expressions: operator precedence, operands, variables and the postfix chain.

use super::{
    Parser, is_ampersand, is_cast, is_identifier, is_magic_constant, is_name, starts_expression,
};
use crate::ast::{
    Arg, Args, ArrayItem, ArraySyntax, AssignOp, BinaryOp, CastKind, ClassKind, ClassLike, Expr,
    ExprKind, IncDecOp, IncludeKind, MagicConstant, MatchArm, Member, Modifiers, Name, NameKind,
    StringPart, UnaryOp, VarName,
};
use crate::float;
use crate::literal::{self, Number};
use crate::stream::PResult;
use php_lexer::TokenKind as K;

/// Binding strengths, weakest first, as the reference grammar declares them.
pub(super) mod prec {
    pub(crate) const THROW: u8 = 1;
    pub(crate) const ARROW_FUNCTION: u8 = 2;
    pub(crate) const INCLUDE: u8 = 3;
    pub(crate) const LOGICAL_OR: u8 = 4;
    pub(crate) const LOGICAL_XOR: u8 = 5;
    pub(crate) const LOGICAL_AND: u8 = 6;
    pub(crate) const PRINT: u8 = 7;
    pub(crate) const YIELD: u8 = 8;
    pub(crate) const YIELD_FROM: u8 = 10;
    pub(crate) const ASSIGN: u8 = 11;
    pub(crate) const TERNARY: u8 = 12;
    pub(crate) const COALESCE: u8 = 13;
    pub(crate) const BOOLEAN_OR: u8 = 14;
    pub(crate) const BOOLEAN_AND: u8 = 15;
    pub(crate) const BIT_OR: u8 = 16;
    pub(crate) const BIT_XOR: u8 = 17;
    pub(crate) const BIT_AND: u8 = 18;
    pub(crate) const EQUALITY: u8 = 19;
    pub(crate) const COMPARISON: u8 = 20;
    pub(crate) const PIPE: u8 = 21;
    pub(crate) const CONCAT: u8 = 22;
    pub(crate) const SHIFT: u8 = 23;
    pub(crate) const ADDITIVE: u8 = 24;
    pub(crate) const MULTIPLICATIVE: u8 = 25;
    pub(crate) const NOT: u8 = 26;
    pub(crate) const INSTANCEOF: u8 = 27;
    pub(crate) const UNARY: u8 = 28;
    pub(crate) const POW: u8 = 29;
    pub(crate) const CLONE: u8 = 30;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Assoc {
    Left,
    Right,
    Non,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Infix {
    Binary(BinaryOp),
    Ternary,
    Instanceof,
}

/// The infix operator a token stands for, with its strength and associativity.
const fn infix(kind: K) -> Option<(Infix, u8, Assoc)> {
    use Assoc::{Left, Non, Right};
    use BinaryOp as B;
    let (op, strength, assoc) = match kind {
        K::LogicalOr => (B::Or, prec::LOGICAL_OR, Left),
        K::LogicalXor => (B::Xor, prec::LOGICAL_XOR, Left),
        K::LogicalAnd => (B::And, prec::LOGICAL_AND, Left),
        K::Char(b'?') => return Some((Infix::Ternary, prec::TERNARY, Left)),
        K::Coalesce => (B::Coalesce, prec::COALESCE, Right),
        K::BooleanOr => (B::Or, prec::BOOLEAN_OR, Left),
        K::BooleanAnd => (B::And, prec::BOOLEAN_AND, Left),
        K::Char(b'|') => (B::BitOr, prec::BIT_OR, Left),
        K::Char(b'^') => (B::BitXor, prec::BIT_XOR, Left),
        K::AmpersandFollowedByVarOrVararg | K::AmpersandNotFollowedByVarOrVararg => {
            (B::BitAnd, prec::BIT_AND, Left)
        }
        K::Equal => (B::Equal, prec::EQUALITY, Non),
        K::NotEqual => (B::NotEqual, prec::EQUALITY, Non),
        K::Identical => (B::Identical, prec::EQUALITY, Non),
        K::NotIdentical => (B::NotIdentical, prec::EQUALITY, Non),
        K::Spaceship => (B::Spaceship, prec::EQUALITY, Non),
        K::Char(b'<') => (B::Less, prec::COMPARISON, Non),
        K::LessOrEqual => (B::LessOrEqual, prec::COMPARISON, Non),
        K::Char(b'>') => (B::Greater, prec::COMPARISON, Non),
        K::GreaterOrEqual => (B::GreaterOrEqual, prec::COMPARISON, Non),
        K::Pipe => (B::Pipe, prec::PIPE, Left),
        K::Char(b'.') => (B::Concat, prec::CONCAT, Left),
        K::ShiftLeft => (B::ShiftLeft, prec::SHIFT, Left),
        K::ShiftRight => (B::ShiftRight, prec::SHIFT, Left),
        K::Char(b'+') => (B::Add, prec::ADDITIVE, Left),
        K::Char(b'-') => (B::Sub, prec::ADDITIVE, Left),
        K::Char(b'*') => (B::Mul, prec::MULTIPLICATIVE, Left),
        K::Char(b'/') => (B::Div, prec::MULTIPLICATIVE, Left),
        K::Char(b'%') => (B::Mod, prec::MULTIPLICATIVE, Left),
        K::Instanceof => return Some((Infix::Instanceof, prec::INSTANCEOF, Left)),
        K::Pow => (B::Pow, prec::POW, Right),
        _ => return None,
    };
    Some((Infix::Binary(op), strength, assoc))
}

const fn compound_assign(kind: K) -> Option<AssignOp> {
    Some(match kind {
        K::PlusAssign => AssignOp::Add,
        K::MinusAssign => AssignOp::Sub,
        K::MulAssign => AssignOp::Mul,
        K::DivAssign => AssignOp::Div,
        K::ModAssign => AssignOp::Mod,
        K::PowAssign => AssignOp::Pow,
        K::ConcatAssign => AssignOp::Concat,
        K::ShiftLeftAssign => AssignOp::ShiftLeft,
        K::ShiftRightAssign => AssignOp::ShiftRight,
        K::OrAssign => AssignOp::BitOr,
        K::AndAssign => AssignOp::BitAnd,
        K::XorAssign => AssignOp::BitXor,
        K::CoalesceAssign => AssignOp::Coalesce,
        _ => return None,
    })
}

const fn cast_kind(kind: K) -> CastKind {
    match kind {
        K::IntCast => CastKind::Int,
        K::FloatCast => CastKind::Float,
        K::StringCast => CastKind::String,
        K::ArrayCast => CastKind::Array,
        K::ObjectCast => CastKind::Object,
        K::BoolCast => CastKind::Bool,
        _ => CastKind::Unset,
    }
}

const fn magic_constant(kind: K) -> MagicConstant {
    match kind {
        K::LineMagic => MagicConstant::Line,
        K::FileMagic => MagicConstant::File,
        K::DirMagic => MagicConstant::Dir,
        K::ClassMagic => MagicConstant::Class,
        K::TraitMagic => MagicConstant::Trait,
        K::MethodMagic => MagicConstant::Method,
        K::FunctionMagic => MagicConstant::Function,
        K::PropertyMagic => MagicConstant::Property,
        _ => MagicConstant::Namespace,
    }
}

/// The tokens accepted after a dereferenceable value in a position that needs a variable.
pub(super) const DEREFERENCE: [K; 3] =
    [K::Char(b'['), K::ObjectOperator, K::NullsafeObjectOperator];

/// What a parsed operand is, which decides what may follow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shape {
    /// A variable that can be called: `$a`, `$a[0]`, calls and method calls.
    CallableVariable,
    /// A variable that cannot be called directly: property and static property fetches.
    Variable,
    /// `(expr)`.
    Parenthesized,
    /// A string or `array(…)` literal.
    Scalar,
    /// A `[…]` literal, which may also be a destructuring target.
    ShortArray,
    /// `list(…)`, which must be assigned to.
    List,
    /// `A::B` or `A::class`.
    ClassConstant,
    /// `new A(…)` or an anonymous class.
    New,
    /// `new A` without arguments.
    NewBare,
    /// A constant or magic constant.
    Constant,
    /// Anything that cannot be dereferenced.
    Plain,
}

impl Shape {
    /// Whether `[`, `->` and `?->` may follow.
    const fn can_index(self) -> bool {
        !matches!(self, Self::List | Self::NewBare | Self::Plain)
    }

    /// Whether `::` may follow.
    const fn can_access_static(self) -> bool {
        !matches!(
            self,
            Self::List | Self::NewBare | Self::Plain | Self::Constant
        )
    }

    /// Whether `(` may follow.
    const fn can_call(self) -> bool {
        matches!(
            self,
            Self::CallableVariable
                | Self::Parenthesized
                | Self::Scalar
                | Self::ShortArray
                | Self::New
        )
    }

    pub(super) const fn is_variable(self) -> bool {
        matches!(self, Self::CallableVariable | Self::Variable)
    }
}

/// Tokens a `variable` can start with.
pub(super) const fn starts_variable(kind: K) -> bool {
    is_name(kind)
        || is_magic_constant(kind)
        || matches!(
            kind,
            K::Variable
                | K::Char(b'$' | b'(' | b'[' | b'"')
                | K::Static
                | K::Readonly
                | K::Array
                | K::ConstantString
                | K::New
        )
}

/// Tokens a call argument can start with.
pub(super) const fn starts_argument(kind: K) -> bool {
    starts_expression(kind) || is_identifier(kind) || matches!(kind, K::Ellipsis)
}

/// Tokens a `new` or `instanceof` class reference can start with.
const fn starts_class_reference(kind: K) -> bool {
    is_name(kind) || matches!(kind, K::Static | K::Variable | K::Char(b'$' | b'('))
}

const fn expr(kind: ExprKind, line: u32) -> Expr {
    Expr { kind, line }
}

/// A variable name computed from `expr`; a string literal is a name known while parsing.
pub(super) fn var_name(value: Expr) -> VarName {
    match value.kind {
        ExprKind::String(bytes) => VarName::Name(bytes),
        _ => VarName::Expr(Box::new(value)),
    }
}

/// A member name computed from `expr`; a string literal is a name known while parsing.
fn member(value: Expr) -> Member {
    match value.kind {
        ExprKind::String(bytes) => Member::Name(bytes),
        _ => Member::Expr(Box::new(value)),
    }
}

/// The text a literal contributes to a folded concatenation.
fn literal_text(value: &Expr) -> Option<Vec<u8>> {
    match &value.kind {
        ExprKind::String(bytes) => Some(bytes.clone()),
        ExprKind::Int(number) => Some(number.to_string().into_bytes()),
        ExprKind::Float(number) => Some(float::format(*number, float::PRECISION).into_bytes()),
        _ => None,
    }
}

/// `left op right`; two literals joined by `.` fold into one string.
fn binary(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    let line = left.line;
    if op == BinaryOp::Concat
        && let (Some(mut text), Some(tail)) = (literal_text(&left), literal_text(&right))
    {
        text.extend_from_slice(&tail);
        return expr(ExprKind::String(text), line);
    }
    expr(
        ExprKind::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
        line,
    )
}

/// A fully qualified function name such as the `exit` an `exit(…)` expression calls.
fn qualified_callee(bytes: &[u8], line: u32) -> Expr {
    expr(
        ExprKind::Name(Name {
            kind: NameKind::FullyQualified,
            bytes: bytes.to_vec(),
        }),
        line,
    )
}

/// Where an interpolated string is, which decides the tokens its error states accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Interpolation {
    Double,
    Heredoc,
    Backtick,
}

impl Parser<'_> {
    /// Any expression.
    pub(super) fn expr(&mut self) -> PResult<Expr> {
        self.expr_bp(0)
    }

    /// An expression whose operators bind more tightly than `min` (or as tightly, for a
    /// right-associative operator).
    pub(super) fn expr_bp(&mut self, min: u8) -> PResult<Expr> {
        let left = self.unary()?;
        self.binary_loop(left, min)
    }

    /// Continues an expression whose first operand is `left`.
    pub(super) fn binary_loop(&mut self, mut left: Expr, min: u8) -> PResult<Expr> {
        loop {
            let kind = self.peek()?;
            let Some((op, strength, assoc)) = infix(kind) else {
                return Ok(left);
            };
            if strength < min || (strength == min && assoc != Assoc::Right) {
                return Ok(left);
            }
            self.bump()?;
            left = match op {
                Infix::Ternary => self.ternary(left)?,
                Infix::Instanceof => {
                    let class = self.class_reference(&[])?;
                    let line = left.line;
                    expr(
                        ExprKind::Instanceof {
                            expr: Box::new(left),
                            class: Box::new(class),
                        },
                        line,
                    )
                }
                Infix::Binary(op) => {
                    let right = self.expr_bp(strength)?;
                    let node = binary(op, left, right);
                    if assoc == Assoc::Non {
                        let next = self.peek()?;
                        if matches!(infix(next), Some((_, level, Assoc::Non)) if level == strength)
                        {
                            return self.unexpected(&[]);
                        }
                    }
                    node
                }
            };
        }
    }

    fn ternary(&mut self, condition: Expr) -> PResult<Expr> {
        let line = condition.line;
        let then = if self.eat(K::Char(b':'))?.is_some() {
            None
        } else {
            let then = self.expr()?;
            self.expect_in(K::Char(b':'), &[])?;
            Some(Box::new(then))
        };
        let otherwise = self.expr_bp(prec::TERNARY)?;
        Ok(expr(
            ExprKind::Ternary {
                condition: Box::new(condition),
                then,
                otherwise: Box::new(otherwise),
                parenthesized: false,
            },
            line,
        ))
    }

    /// A prefix operator applied to an operand of strength `strength`.
    fn prefix(&mut self, strength: u8, build: fn(Box<Expr>) -> ExprKind) -> PResult<Expr> {
        let line = self.bump()?.line;
        let operand = self.expr_bp(strength)?;
        Ok(expr(build(Box::new(operand)), line))
    }

    fn unary(&mut self) -> PResult<Expr> {
        let kind = self.peek()?;
        match kind {
            K::Char(b'!') => self.prefix(prec::NOT, |operand| ExprKind::Unary {
                op: UnaryOp::Not,
                operand,
            }),
            K::Char(b'~') => self.prefix(prec::UNARY, |operand| ExprKind::Unary {
                op: UnaryOp::BitNot,
                operand,
            }),
            K::Char(b'@') => self.prefix(prec::UNARY, |operand| ExprKind::Unary {
                op: UnaryOp::Silence,
                operand,
            }),
            K::Char(b'-') => self.prefix(prec::UNARY, |operand| ExprKind::Unary {
                op: UnaryOp::Minus,
                operand,
            }),
            K::Char(b'+') => self.prefix(prec::UNARY, |operand| ExprKind::Unary {
                op: UnaryOp::Plus,
                operand,
            }),
            kind if is_cast(kind) => {
                let line = self.bump()?.line;
                let operand = self.expr_bp(prec::UNARY)?;
                Ok(expr(
                    ExprKind::Cast {
                        kind: cast_kind(kind),
                        operand: Box::new(operand),
                    },
                    line,
                ))
            }
            K::Increment | K::Decrement => {
                let line = self.bump()?.line;
                let operand = self.variable()?;
                let op = if kind == K::Increment {
                    IncDecOp::PreInc
                } else {
                    IncDecOp::PreDec
                };
                Ok(expr(
                    ExprKind::IncDec {
                        op,
                        operand: Box::new(operand),
                    },
                    line,
                ))
            }
            K::Print => self.prefix(prec::PRINT, ExprKind::Print),
            K::YieldFrom => self.prefix(prec::YIELD_FROM, ExprKind::YieldFrom),
            K::Throw => self.prefix(prec::THROW, ExprKind::Throw),
            K::Include => self.include(IncludeKind::Include),
            K::IncludeOnce => self.include(IncludeKind::IncludeOnce),
            K::Require => self.include(IncludeKind::Require),
            K::RequireOnce => self.include(IncludeKind::RequireOnce),
            K::Yield => self.yield_expr(),
            K::Clone => self.clone_expr(),
            K::Function | K::Fn => self.closure(Vec::new(), false),
            K::Static if matches!(self.peek_second()?, K::Function | K::Fn) => {
                self.bump()?;
                self.closure(Vec::new(), true)
            }
            K::Attribute => {
                let attributes = self.attributes()?;
                match self.peek()? {
                    K::Function | K::Fn => self.closure(attributes, false),
                    K::Static => {
                        self.bump()?;
                        if matches!(self.peek()?, K::Function | K::Fn) {
                            self.closure(attributes, true)
                        } else {
                            self.unexpected(&[K::Function, K::Fn])
                        }
                    }
                    _ => self.unexpected(&[K::Function, K::Fn, K::Static, K::Attribute]),
                }
            }
            _ => self.operand(),
        }
    }

    fn include(&mut self, kind: IncludeKind) -> PResult<Expr> {
        let line = self.bump()?.line;
        let operand = self.expr_bp(prec::INCLUDE)?;
        Ok(expr(
            ExprKind::Include {
                kind,
                expr: Box::new(operand),
            },
            line,
        ))
    }

    fn yield_expr(&mut self) -> PResult<Expr> {
        let line = self.bump()?.line;
        if !starts_expression(self.peek()?) {
            return Ok(expr(
                ExprKind::Yield {
                    key: None,
                    value: None,
                },
                line,
            ));
        }
        let first = self.expr_bp(prec::YIELD)?;
        let (key, value) = if self.eat(K::DoubleArrow)?.is_some() {
            (Some(first), self.expr_bp(prec::YIELD)?)
        } else {
            (None, first)
        };
        Ok(expr(
            ExprKind::Yield {
                key: key.map(Box::new),
                value: Some(Box::new(value)),
            },
            line,
        ))
    }

    /// `clone expr`, or `clone(…)` with an argument list; both call the `clone` function.
    fn clone_expr(&mut self) -> PResult<Expr> {
        let line = self.bump()?.line;
        let args = if self.at(K::Char(b'('))? {
            self.clone_parenthesized()?
        } else {
            Args::List(vec![Arg::Positional(self.expr_bp(prec::CLONE)?)])
        };
        Ok(expr(
            ExprKind::Call {
                callee: Box::new(qualified_callee(b"clone", line)),
                args,
            },
            line,
        ))
    }

    /// What follows `clone (`: an argument list, or a parenthesized operand.
    fn clone_parenthesized(&mut self) -> PResult<Args> {
        self.bump()?;
        if self.eat(K::Char(b')'))?.is_some() {
            return Ok(Args::List(Vec::new()));
        }
        if self.at(K::Ellipsis)? && self.peek_second()? == K::Char(b')') {
            self.bump()?;
            self.bump()?;
            return Ok(Args::CallableConvert);
        }
        let first = if self.at(K::Ellipsis)?
            || (is_identifier(self.peek()?) && self.peek_second()? == K::Char(b':'))
        {
            self.argument()?
        } else {
            let value = self.expr()?;
            if self.at(K::Char(b')'))? {
                self.bump()?;
                let value = parenthesized(value);
                let (value, shape) = self.postfix_chain(value, Shape::Parenthesized)?;
                let value = self.assignment_suffix(value, shape)?;
                let value = self.binary_loop(value, prec::CLONE)?;
                return Ok(Args::List(vec![Arg::Positional(value)]));
            }
            if self.at(K::Char(b','))? {
                let next = self.peek_second()?;
                if next != K::Char(b')') && !starts_argument(next) {
                    self.bump()?;
                    return self.unexpected(&[]);
                }
            }
            Arg::Positional(value)
        };
        self.argument_tail(vec![first])
    }

    /// An operand: a primary value, its postfix chain, and an assignment or `++`/`--` after it.
    fn operand(&mut self) -> PResult<Expr> {
        let (value, shape) = self.postfix_operand()?;
        self.assignment_suffix(value, shape)
    }

    fn assignment_suffix(&mut self, target: Expr, shape: Shape) -> PResult<Expr> {
        let kind = self.peek()?;
        let line = target.line;
        let assignable = shape.is_variable();
        if kind == K::Char(b'=') && (assignable || matches!(shape, Shape::ShortArray | Shape::List))
        {
            self.bump()?;
            if assignable && is_ampersand(self.peek()?) {
                self.bump()?;
                let value = self.variable()?;
                return Ok(expr(
                    ExprKind::AssignRef {
                        target: Box::new(target),
                        value: Box::new(value),
                    },
                    line,
                ));
            }
            let value = self.expr_bp(prec::ASSIGN)?;
            return Ok(expr(
                ExprKind::Assign {
                    target: Box::new(target),
                    value: Box::new(value),
                },
                line,
            ));
        }
        if shape == Shape::List {
            return self.unexpected(&[K::Char(b'=')]);
        }
        if !assignable {
            return Ok(target);
        }
        if let Some(op) = compound_assign(kind) {
            self.bump()?;
            let value = self.expr_bp(prec::ASSIGN)?;
            return Ok(expr(
                ExprKind::CompoundAssign {
                    op,
                    target: Box::new(target),
                    value: Box::new(value),
                },
                line,
            ));
        }
        let op = match kind {
            K::Increment => IncDecOp::PostInc,
            K::Decrement => IncDecOp::PostDec,
            _ => return Ok(target),
        };
        self.bump()?;
        Ok(expr(
            ExprKind::IncDec {
                op,
                operand: Box::new(target),
            },
            line,
        ))
    }

    /// A `variable`: where only a variable is accepted (`&$a`, `++$a`, `unset`, `foreach`).
    pub(super) fn variable(&mut self) -> PResult<Expr> {
        if !starts_variable(self.peek()?) {
            return self.unexpected(&[]);
        }
        let (value, shape) = self.postfix_operand()?;
        match shape {
            shape if shape.is_variable() => Ok(value),
            Shape::NewBare => self.unexpected(&[K::Char(b'(')]),
            _ => self.unexpected(&DEREFERENCE),
        }
    }

    /// A `variable` that must be followed by one of `followers`; anything else is reported in
    /// the state that still expects the variable to continue.
    pub(super) fn variable_followed_by(&mut self, followers: &[K]) -> PResult<Expr> {
        let value = self.variable()?;
        if followers.contains(&self.peek()?) {
            Ok(value)
        } else {
            self.unexpected(&DEREFERENCE)
        }
    }

    fn postfix_operand(&mut self) -> PResult<(Expr, Shape)> {
        let (value, shape) = self.primary()?;
        self.postfix_chain(value, shape)
    }

    fn postfix_chain(&mut self, mut value: Expr, mut shape: Shape) -> PResult<(Expr, Shape)> {
        loop {
            let kind = self.peek()?;
            let line = value.line;
            (value, shape) = match kind {
                K::Char(b'[') if shape.can_index() => {
                    self.bump()?;
                    let index = self.optional_index()?;
                    (
                        expr(
                            ExprKind::Dim {
                                base: Box::new(value),
                                index,
                            },
                            line,
                        ),
                        Shape::CallableVariable,
                    )
                }
                K::ObjectOperator | K::NullsafeObjectOperator if shape.can_index() => {
                    self.bump()?;
                    let nullsafe = kind == K::NullsafeObjectOperator;
                    let name = self.property_name()?;
                    if self.at(K::Char(b'('))? {
                        let args = self.arguments()?;
                        (
                            expr(
                                ExprKind::MethodCall {
                                    object: Box::new(value),
                                    name,
                                    args,
                                    nullsafe,
                                },
                                line,
                            ),
                            Shape::CallableVariable,
                        )
                    } else {
                        (
                            expr(
                                ExprKind::Property {
                                    object: Box::new(value),
                                    name,
                                    nullsafe,
                                },
                                line,
                            ),
                            Shape::Variable,
                        )
                    }
                }
                K::DoubleColon if shape.can_access_static() => {
                    self.bump()?;
                    self.static_member(value)?
                }
                K::Char(b'(') if shape.can_call() => {
                    let args = self.arguments()?;
                    (
                        expr(
                            ExprKind::Call {
                                callee: Box::new(value),
                                args,
                            },
                            line,
                        ),
                        Shape::CallableVariable,
                    )
                }
                _ => return Ok((value, shape)),
            };
        }
    }

    /// The optional expression and `]` after `[`.
    fn optional_index(&mut self) -> PResult<Option<Box<Expr>>> {
        let index = if starts_expression(self.peek()?) {
            Some(Box::new(self.expr()?))
        } else {
            None
        };
        self.expect(K::Char(b']'))?;
        Ok(index)
    }

    /// What follows `->`.
    fn property_name(&mut self) -> PResult<Member> {
        match self.peek()? {
            K::Identifier => {
                let tok = self.bump()?;
                Ok(Member::Name(self.text(&tok).to_vec()))
            }
            K::Char(b'{') => {
                self.bump()?;
                let name = self.expr()?;
                self.expect_in(K::Char(b'}'), &[])?;
                Ok(member(name))
            }
            K::Variable | K::Char(b'$') => {
                let line = self.stream.peek()?.line;
                let name = self.simple_variable()?;
                Ok(Member::Expr(Box::new(expr(ExprKind::Variable(name), line))))
            }
            _ => self.unexpected(&[K::Identifier, K::Variable, K::Char(b'{'), K::Char(b'$')]),
        }
    }

    /// `$name`, `$$…` or `${expr}`.
    pub(super) fn simple_variable(&mut self) -> PResult<VarName> {
        match self.peek()? {
            K::Variable => {
                let tok = self.bump()?;
                Ok(VarName::Name(self.text(&tok)[1..].to_vec()))
            }
            K::Char(b'$') => {
                self.bump()?;
                match self.peek()? {
                    K::Char(b'{') => {
                        self.bump()?;
                        let name = self.expr()?;
                        self.expect_in(K::Char(b'}'), &[])?;
                        Ok(var_name(name))
                    }
                    K::Variable | K::Char(b'$') => {
                        let line = self.stream.peek()?.line;
                        let inner = self.simple_variable()?;
                        Ok(VarName::Expr(Box::new(expr(
                            ExprKind::Variable(inner),
                            line,
                        ))))
                    }
                    _ => self.unexpected(&[K::Variable, K::Char(b'{'), K::Char(b'$')]),
                }
            }
            _ => self.unexpected(&[K::Variable, K::Char(b'$')]),
        }
    }

    /// What follows `Class::`.
    fn static_member(&mut self, class: Expr) -> PResult<(Expr, Shape)> {
        let line = class.line;
        let class = Box::new(class);
        let kind = self.peek()?;
        if matches!(kind, K::Variable | K::Char(b'$')) {
            let name_line = self.stream.peek()?.line;
            let name = self.simple_variable()?;
            if self.at(K::Char(b'('))? {
                let args = self.arguments()?;
                let name = Member::Expr(Box::new(expr(ExprKind::Variable(name), name_line)));
                return Ok((
                    expr(ExprKind::StaticCall { class, name, args }, line),
                    Shape::CallableVariable,
                ));
            }
            return Ok((
                expr(ExprKind::StaticProperty { class, name }, line),
                Shape::Variable,
            ));
        }
        let name = if is_identifier(kind) {
            let tok = self.bump()?;
            let bytes = self.identifier_bytes(&tok)?;
            if !self.at(K::Char(b'('))? && bytes.eq_ignore_ascii_case(b"class") {
                return Ok((expr(ExprKind::ClassName(class), line), Shape::ClassConstant));
            }
            Member::Name(bytes)
        } else if kind == K::Char(b'{') {
            self.bump()?;
            let name = self.expr()?;
            self.expect_in(K::Char(b'}'), &[])?;
            member(name)
        } else {
            return self.unexpected(&[]);
        };
        if self.at(K::Char(b'('))? {
            let args = self.arguments()?;
            return Ok((
                expr(ExprKind::StaticCall { class, name, args }, line),
                Shape::CallableVariable,
            ));
        }
        Ok((
            expr(ExprKind::ClassConstant { class, name }, line),
            Shape::ClassConstant,
        ))
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one arm per token that starts a value mirrors the grammar's alternatives"
    )]
    fn primary(&mut self) -> PResult<(Expr, Shape)> {
        let kind = self.peek()?;
        let line = self.stream.peek()?.line;
        let plain = |kind| Ok((expr(kind, line), Shape::Plain));
        match kind {
            K::Variable | K::Char(b'$') => {
                let name = self.simple_variable()?;
                Ok((
                    expr(ExprKind::Variable(name), line),
                    Shape::CallableVariable,
                ))
            }
            kind if is_name(kind) => {
                let tok = self.bump()?;
                let name = literal::name(kind, self.text(&tok));
                self.name_operand(name, line)
            }
            K::Static => {
                self.bump()?;
                self.expect_in(K::DoubleColon, &[K::DoubleColon])?;
                self.static_member(expr(ExprKind::Name(static_name()), line))
            }
            K::Readonly => {
                let tok = self.bump()?;
                if !self.at(K::Char(b'('))? {
                    return self.unexpected(&[K::Char(b'(')]);
                }
                let args = self.arguments()?;
                let callee = Box::new(qualified_callee(self.text(&tok), line));
                Ok((
                    expr(ExprKind::Call { callee, args }, line),
                    Shape::CallableVariable,
                ))
            }
            K::Char(b'(') => {
                self.bump()?;
                let inner = self.expr()?;
                self.expect_in(K::Char(b')'), &[])?;
                Ok((parenthesized(inner), Shape::Parenthesized))
            }
            K::Char(b'[') => {
                self.bump()?;
                let items = self.array_items(b']')?;
                Ok((
                    expr(
                        ExprKind::Array {
                            items,
                            syntax: ArraySyntax::Short,
                        },
                        line,
                    ),
                    Shape::ShortArray,
                ))
            }
            K::Array | K::List => {
                self.bump()?;
                self.expect(K::Char(b'('))?;
                let items = self.array_items(b')')?;
                let (syntax, shape) = if kind == K::Array {
                    (ArraySyntax::Long, Shape::Scalar)
                } else {
                    (ArraySyntax::List, Shape::List)
                };
                Ok((expr(ExprKind::Array { items, syntax }, line), shape))
            }
            K::ConstantString => {
                let tok = self.bump()?;
                Ok((expr(ExprKind::String(tok.value), line), Shape::Scalar))
            }
            K::Char(b'"') => {
                self.bump()?;
                let parts = self.interpolated(K::Char(b'"'), Interpolation::Double)?;
                Ok((expr(ExprKind::Interpolated(parts), line), Shape::Scalar))
            }
            K::StartHeredoc => {
                self.bump()?;
                let mut parts = self.interpolated(K::EndHeredoc, Interpolation::Heredoc)?;
                let kind = match parts.as_mut_slice() {
                    [] => ExprKind::String(Vec::new()),
                    [StringPart::Literal(text)] => ExprKind::String(std::mem::take(text)),
                    _ => ExprKind::Interpolated(parts),
                };
                plain(kind)
            }
            K::Char(b'`') => {
                self.bump()?;
                let parts = self.interpolated(K::Char(b'`'), Interpolation::Backtick)?;
                plain(ExprKind::ShellExec(parts))
            }
            K::Integer | K::Float => {
                let tok = self.bump()?;
                plain(match literal::number(kind, self.text(&tok)) {
                    Number::Int(value) => ExprKind::Int(value),
                    Number::Float(value) => ExprKind::Float(value),
                })
            }
            kind if is_magic_constant(kind) => {
                self.bump()?;
                Ok((
                    expr(ExprKind::Magic(magic_constant(kind)), line),
                    Shape::Constant,
                ))
            }
            K::New => self.new_expr(),
            K::Isset => {
                self.bump()?;
                self.isset(line).map(|value| (value, Shape::Plain))
            }
            K::Empty => {
                self.bump()?;
                self.expect(K::Char(b'('))?;
                let inner = self.expr()?;
                self.expect_in(K::Char(b')'), &[])?;
                plain(ExprKind::Empty(Box::new(inner)))
            }
            K::Eval => {
                self.bump()?;
                self.expect(K::Char(b'('))?;
                let inner = self.expr()?;
                self.expect_in(K::Char(b')'), &[])?;
                plain(ExprKind::Include {
                    kind: IncludeKind::Eval,
                    expr: Box::new(inner),
                })
            }
            K::Exit => {
                self.bump()?;
                let args = if self.at(K::Char(b'('))? {
                    self.arguments()?
                } else {
                    Args::List(Vec::new())
                };
                plain(ExprKind::Call {
                    callee: Box::new(qualified_callee(b"exit", line)),
                    args,
                })
            }
            K::Match => self.match_expr().map(|value| (value, Shape::Plain)),
            _ => self.unexpected(&[]),
        }
    }

    /// A name followed by a call, a static member, or nothing (a constant).
    fn name_operand(&mut self, name: Name, line: u32) -> PResult<(Expr, Shape)> {
        match self.peek()? {
            K::Char(b'(') => {
                let args = self.arguments()?;
                let callee = Box::new(expr(ExprKind::Name(name), line));
                Ok((
                    expr(ExprKind::Call { callee, args }, line),
                    Shape::CallableVariable,
                ))
            }
            K::DoubleColon => {
                self.bump()?;
                self.static_member(expr(ExprKind::Name(name), line))
            }
            _ => Ok((expr(ExprKind::Constant(name), line), Shape::Constant)),
        }
    }

    fn isset(&mut self, line: u32) -> PResult<Expr> {
        self.expect(K::Char(b'('))?;
        let first = self.expr()?;
        let mut result = expr(ExprKind::Isset(Box::new(first)), line);
        while self.eat(K::Char(b','))?.is_some() {
            if !starts_expression(self.peek()?) {
                break;
            }
            let next = self.expr()?;
            let next_line = next.line;
            result = binary(
                BinaryOp::And,
                result,
                expr(ExprKind::Isset(Box::new(next)), next_line),
            );
        }
        self.expect(K::Char(b')'))?;
        Ok(result)
    }

    fn match_expr(&mut self) -> PResult<Expr> {
        let line = self.bump()?.line;
        self.expect(K::Char(b'('))?;
        let subject = self.expr()?;
        self.expect_in(K::Char(b')'), &[])?;
        self.expect(K::Char(b'{'))?;
        let mut arms = Vec::new();
        loop {
            let kind = self.peek()?;
            let conditions = if kind == K::Default {
                self.bump()?;
                self.eat(K::Char(b','))?;
                None
            } else if starts_expression(kind) {
                let mut conditions = vec![self.expr()?];
                while self.eat(K::Char(b','))?.is_some() {
                    if !starts_expression(self.peek()?) {
                        break;
                    }
                    conditions.push(self.expr()?);
                }
                Some(conditions)
            } else {
                break;
            };
            self.expect(K::DoubleArrow)?;
            let body = self.expr()?;
            arms.push(MatchArm { conditions, body });
            if self.eat(K::Char(b','))?.is_none() {
                break;
            }
        }
        self.expect(K::Char(b'}'))?;
        Ok(expr(
            ExprKind::Match {
                subject: Box::new(subject),
                arms,
            },
            line,
        ))
    }

    /// The items of an array literal up to and including `close`.
    pub(super) fn array_items(&mut self, close: u8) -> PResult<Vec<Option<ArrayItem>>> {
        let close = K::Char(close);
        let mut items = Vec::new();
        loop {
            let kind = self.peek()?;
            let item = if kind == K::Char(b',') || kind == close || !starts_array_item(kind) {
                None
            } else {
                Some(self.array_item(close)?)
            };
            items.push(item);
            if self.eat(K::Char(b','))?.is_none() {
                break;
            }
        }
        self.expect(close)?;
        if items.last() == Some(&None) {
            items.pop();
        }
        Ok(items)
    }

    fn array_item(&mut self, close: K) -> PResult<ArrayItem> {
        let kind = self.peek()?;
        if kind == K::Ellipsis {
            self.bump()?;
            return Ok(ArrayItem::Spread(self.expr()?));
        }
        if is_ampersand(kind) || kind == K::List {
            let (value, by_ref) = self.array_value(close)?;
            return Ok(ArrayItem::Value {
                key: None,
                value,
                by_ref,
            });
        }
        let first = self.expr()?;
        if self.eat(K::DoubleArrow)?.is_none() {
            return Ok(ArrayItem::Value {
                key: None,
                value: first,
                by_ref: false,
            });
        }
        let next = self.peek()?;
        let (value, by_ref) = if is_ampersand(next) || next == K::List {
            self.array_value(close)?
        } else {
            (self.expr()?, false)
        };
        Ok(ArrayItem::Value {
            key: Some(first),
            value,
            by_ref,
        })
    }

    /// An array value written `&variable` or `list(…)`.
    fn array_value(&mut self, close: K) -> PResult<(Expr, bool)> {
        if self.at(K::List)? {
            let line = self.bump()?.line;
            self.expect(K::Char(b'('))?;
            let items = self.array_items(b')')?;
            return Ok((
                expr(
                    ExprKind::Array {
                        items,
                        syntax: ArraySyntax::List,
                    },
                    line,
                ),
                false,
            ));
        }
        self.bump()?;
        let value = self.variable_followed_by(&[K::Char(b','), close])?;
        Ok((value, true))
    }

    /// The parts of an interpolated string up to and including `close`.
    fn interpolated(&mut self, close: K, place: Interpolation) -> PResult<Vec<StringPart>> {
        let mut parts = Vec::new();
        loop {
            let kind = self.peek()?;
            match kind {
                K::StringFragment => parts.push(StringPart::Literal(self.bump()?.value)),
                K::Variable | K::DollarOpenCurlyBraces | K::CurlyOpen => {
                    parts.push(StringPart::Expr(self.interpolated_variable()?));
                }
                kind if kind == close => {
                    self.bump()?;
                    return Ok(parts);
                }
                _ => {
                    let only_text = matches!(parts.as_slice(), [StringPart::Literal(_)]);
                    let expected: &[K] = match (place, parts.is_empty(), only_text) {
                        (Interpolation::Backtick, ..) => &[K::Char(b'`')],
                        (Interpolation::Double, true, _) => &[
                            K::Variable,
                            K::StringFragment,
                            K::DollarOpenCurlyBraces,
                            K::CurlyOpen,
                        ],
                        (Interpolation::Double, _, true) => {
                            &[K::Variable, K::DollarOpenCurlyBraces, K::CurlyOpen]
                        }
                        (Interpolation::Heredoc, _, true) => &[
                            K::EndHeredoc,
                            K::Variable,
                            K::DollarOpenCurlyBraces,
                            K::CurlyOpen,
                        ],
                        _ => &[],
                    };
                    return self.unexpected(expected);
                }
            }
        }
    }

    /// `$a`, `$a[…]`, `$a->b`, `${…}` or `{$…}` inside a string.
    fn interpolated_variable(&mut self) -> PResult<Expr> {
        let tok = self.bump()?;
        let line = tok.line;
        match tok.kind {
            K::Variable => {
                let variable = expr(
                    ExprKind::Variable(VarName::Name(self.text(&tok)[1..].to_vec())),
                    line,
                );
                match self.peek()? {
                    K::Char(b'[') => {
                        self.bump()?;
                        let index = self.interpolated_offset()?;
                        self.expect(K::Char(b']'))?;
                        Ok(expr(
                            ExprKind::Dim {
                                base: Box::new(variable),
                                index: Some(Box::new(index)),
                            },
                            line,
                        ))
                    }
                    kind @ (K::ObjectOperator | K::NullsafeObjectOperator) => {
                        self.bump()?;
                        let name = self.expect(K::Identifier)?;
                        Ok(expr(
                            ExprKind::Property {
                                object: Box::new(variable),
                                name: Member::Name(self.text(&name).to_vec()),
                                nullsafe: kind == K::NullsafeObjectOperator,
                            },
                            line,
                        ))
                    }
                    _ => Ok(variable),
                }
            }
            K::DollarOpenCurlyBraces => {
                if !self.at(K::VariableName)? {
                    let name = self.expr()?;
                    self.expect_in(K::Char(b'}'), &[])?;
                    return Ok(expr(ExprKind::Variable(var_name(name)), line));
                }
                let name = self.bump()?;
                let variable = expr(
                    ExprKind::Variable(VarName::Name(self.text(&name).to_vec())),
                    line,
                );
                match self.peek()? {
                    K::Char(b'[') => {
                        self.bump()?;
                        let index = self.expr()?;
                        self.expect_in(K::Char(b']'), &[])?;
                        self.expect(K::Char(b'}'))?;
                        Ok(expr(
                            ExprKind::Dim {
                                base: Box::new(variable),
                                index: Some(Box::new(index)),
                            },
                            line,
                        ))
                    }
                    K::Char(b'}') => {
                        self.bump()?;
                        Ok(variable)
                    }
                    _ => self.unexpected(&[K::Char(b'['), K::Char(b'}')]),
                }
            }
            _ => {
                let value = self.variable_followed_by(&[K::Char(b'}')])?;
                self.bump()?;
                Ok(value)
            }
        }
    }

    /// The offset in `"$a[offset]"`.
    fn interpolated_offset(&mut self) -> PResult<Expr> {
        let kind = self.peek()?;
        let line = self.stream.peek()?.line;
        let offset = |value: Result<i64, Vec<u8>>| match value {
            Ok(number) => ExprKind::Int(number),
            Err(text) => ExprKind::String(text),
        };
        let value = match kind {
            K::Identifier => {
                let tok = self.bump()?;
                ExprKind::String(self.text(&tok).to_vec())
            }
            K::OffsetNumber => {
                let tok = self.bump()?;
                offset(literal::offset_number(self.text(&tok)))
            }
            K::Char(b'-') => {
                self.bump()?;
                let tok = self.expect(K::OffsetNumber)?;
                match literal::offset_number(self.text(&tok)) {
                    Ok(0) => ExprKind::String(b"-0".to_vec()),
                    Ok(number) => ExprKind::Int(-number),
                    Err(text) => {
                        let mut negated = b"-".to_vec();
                        negated.extend_from_slice(&text);
                        ExprKind::String(negated)
                    }
                }
            }
            K::Variable => {
                let tok = self.bump()?;
                ExprKind::Variable(VarName::Name(self.text(&tok)[1..].to_vec()))
            }
            _ => {
                return self.unexpected(&[
                    K::Char(b'-'),
                    K::Identifier,
                    K::Variable,
                    K::OffsetNumber,
                ]);
            }
        };
        Ok(expr(value, line))
    }

    /// `new …`.
    fn new_expr(&mut self) -> PResult<(Expr, Shape)> {
        let line = self.bump()?.line;
        let kind = self.peek()?;
        if matches!(
            kind,
            K::Class | K::Abstract | K::Final | K::Readonly | K::Attribute
        ) {
            return self.anonymous_class(line);
        }
        let class = self.class_reference(&[K::Class])?;
        if self.at(K::Char(b'('))? {
            let args = self.arguments()?;
            let class = Box::new(class);
            return Ok((expr(ExprKind::New { class, args }, line), Shape::New));
        }
        Ok((
            expr(
                ExprKind::New {
                    class: Box::new(class),
                    args: Args::List(Vec::new()),
                },
                line,
            ),
            Shape::NewBare,
        ))
    }

    fn anonymous_class(&mut self, line: u32) -> PResult<(Expr, Shape)> {
        let attributes = self.attributes()?;
        let mut modifiers = Modifiers::NONE;
        loop {
            let modifier = match self.peek()? {
                K::Abstract => "abstract",
                K::Final => "final",
                K::Readonly => "readonly",
                _ => break,
            };
            self.bump()?;
            if modifier != "readonly" {
                return self.compile_error(format!(
                    "Cannot use the {modifier} modifier on an anonymous class"
                ));
            }
            if modifiers.contains(Modifiers::READONLY) {
                return self.compile_error("Multiple readonly modifiers are not allowed");
            }
            modifiers = modifiers.union(Modifiers::READONLY);
        }
        let class_line = self.expect(K::Class)?.line;
        let args = if self.at(K::Char(b'('))? {
            self.arguments()?
        } else {
            Args::List(Vec::new())
        };
        let extends = if self.eat(K::Extends)?.is_some() {
            Some(self.class_name()?)
        } else {
            None
        };
        let implements = if self.eat(K::Implements)?.is_some() {
            self.class_name_list()?
        } else {
            Vec::new()
        };
        self.expect(K::Char(b'{'))?;
        let members = self.class_body()?;
        let class = ClassLike {
            kind: ClassKind::Class,
            name: None,
            modifiers,
            extends,
            implements,
            backing_type: None,
            members,
            attributes,
            line: class_line,
        };
        Ok((
            expr(
                ExprKind::NewAnonymous {
                    class: Box::new(class),
                    args,
                },
                line,
            ),
            Shape::New,
        ))
    }

    /// The class after `new` or `instanceof`; `missing` lists the tokens reported when there is
    /// none.
    fn class_reference(&mut self, missing: &[K]) -> PResult<Expr> {
        let kind = self.peek()?;
        if !starts_class_reference(kind) {
            return self.unexpected(missing);
        }
        let line = self.stream.peek()?.line;
        if kind == K::Char(b'(') {
            self.bump()?;
            let class = self.expr()?;
            self.expect_in(K::Char(b')'), &[])?;
            return Ok(class);
        }
        let mut class = if matches!(kind, K::Variable | K::Char(b'$')) {
            expr(ExprKind::Variable(self.simple_variable()?), line)
        } else {
            let name = self.class_name()?;
            let class = expr(ExprKind::Name(name), line);
            if !self.at(K::DoubleColon)? {
                return Ok(class);
            }
            class
        };
        loop {
            class = match self.peek()? {
                K::Char(b'[') => {
                    self.bump()?;
                    let index = self.optional_index()?;
                    expr(
                        ExprKind::Dim {
                            base: Box::new(class),
                            index,
                        },
                        line,
                    )
                }
                kind @ (K::ObjectOperator | K::NullsafeObjectOperator) => {
                    self.bump()?;
                    let name = self.property_name()?;
                    expr(
                        ExprKind::Property {
                            object: Box::new(class),
                            name,
                            nullsafe: kind == K::NullsafeObjectOperator,
                        },
                        line,
                    )
                }
                K::DoubleColon => {
                    self.bump()?;
                    let name = self.simple_variable()?;
                    expr(
                        ExprKind::StaticProperty {
                            class: Box::new(class),
                            name,
                        },
                        line,
                    )
                }
                _ => return Ok(class),
            };
        }
    }

    /// A call's argument list, from `(` to `)`.
    pub(super) fn arguments(&mut self) -> PResult<Args> {
        self.expect(K::Char(b'('))?;
        if self.eat(K::Char(b')'))?.is_some() {
            return Ok(Args::List(Vec::new()));
        }
        if self.at(K::Ellipsis)? && self.peek_second()? == K::Char(b')') {
            self.bump()?;
            self.bump()?;
            return Ok(Args::CallableConvert);
        }
        let first = self.argument()?;
        self.argument_tail(vec![first])
    }

    /// The arguments after the first, and the closing `)`.
    fn argument_tail(&mut self, mut args: Vec<Arg>) -> PResult<Args> {
        while self.eat(K::Char(b','))?.is_some() {
            if !starts_argument(self.peek()?) {
                break;
            }
            args.push(self.argument()?);
        }
        self.expect(K::Char(b')'))?;
        Ok(Args::List(args))
    }

    fn argument(&mut self) -> PResult<Arg> {
        let kind = self.peek()?;
        if kind == K::Ellipsis {
            self.bump()?;
            return Ok(Arg::Unpack(self.expr()?));
        }
        if is_identifier(kind) && self.peek_second()? == K::Char(b':') {
            let tok = self.bump()?;
            let name = self.identifier_bytes(&tok)?;
            self.bump()?;
            return Ok(Arg::Named {
                name,
                value: self.expr()?,
            });
        }
        if !starts_expression(kind) && is_identifier(kind) {
            self.bump()?;
            return self.unexpected(&[K::Char(b':')]);
        }
        Ok(Arg::Positional(self.expr()?))
    }
}

/// Tokens an array item can start with.
const fn starts_array_item(kind: K) -> bool {
    starts_expression(kind) || is_ampersand(kind) || matches!(kind, K::Ellipsis)
}

/// The class name `static`.
pub(super) fn static_name() -> Name {
    Name {
        kind: NameKind::Unqualified,
        bytes: b"static".to_vec(),
    }
}

/// Records that `value` was written in parentheses where that changes how it is printed.
fn parenthesized(mut value: Expr) -> Expr {
    match &mut value.kind {
        ExprKind::Ternary { parenthesized, .. } | ExprKind::ArrowFunction { parenthesized, .. } => {
            *parenthesized = true;
        }
        _ => {}
    }
    value
}

#[cfg(test)]
mod tests {
    use super::{Shape, binary, infix, literal_text, var_name};
    use crate::ast::{BinaryOp, Expr, ExprKind, VarName};
    use php_lexer::TokenKind as K;

    fn literal(kind: ExprKind) -> Expr {
        Expr { kind, line: 1 }
    }

    #[test]
    fn concatenated_literals_fold() {
        let folded = binary(
            BinaryOp::Concat,
            literal(ExprKind::Int(1)),
            literal(ExprKind::Float(0.5)),
        );
        assert_eq!(folded.kind, ExprKind::String(b"10.5".to_vec()));
        let kept = binary(
            BinaryOp::Add,
            literal(ExprKind::Int(1)),
            literal(ExprKind::Int(2)),
        );
        assert!(matches!(kept.kind, ExprKind::Binary { .. }));
        assert_eq!(
            literal_text(&literal(ExprKind::Magic(crate::ast::MagicConstant::Line))),
            None
        );
    }

    #[test]
    fn shapes_decide_what_follows() {
        assert!(Shape::Constant.can_index());
        assert!(!Shape::Constant.can_access_static());
        assert!(!Shape::Variable.can_call());
        assert!(Shape::New.can_call());
        assert!(!Shape::NewBare.can_index());
        assert!(Shape::Variable.is_variable());
        assert!(!Shape::Parenthesized.is_variable());
    }

    #[test]
    fn operator_table_matches_the_grammar() {
        assert!(infix(K::Pow).is_some_and(|(_, level, _)| level == super::prec::POW));
        assert!(infix(K::Char(b';')).is_none());
        assert_eq!(
            var_name(literal(ExprKind::String(b"a".to_vec()))),
            VarName::Name(b"a".to_vec())
        );
    }
}
