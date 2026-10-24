//! The syntax tree.
//!
//! The tree records what PHP's own tree records: literal values are already decoded, `echo a, b;`
//! and similar lists are split into one statement per item, `isset(a, b)` is a conjunction of two
//! `isset` checks, `clone` and `exit` are calls, and two adjacent literals joined by `.` are folded
//! into one string. Statements and expressions carry the line they start on.

/// A parsed source file.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// The top-level statements.
    pub statements: Vec<TopStatement>,
    /// Offset just past `__halt_compiler();`, when the source has it.
    pub halt_offset: Option<u32>,
}

/// A statement allowed only at the top level of a file (or of a namespace block), or any other
/// statement.
#[derive(Debug, Clone, PartialEq)]
pub enum TopStatement {
    /// A statement that may also appear in a function body.
    Statement(Stmt),
    /// `__halt_compiler();`.
    HaltCompiler {
        /// Line of the statement.
        line: u32,
    },
    /// `namespace Name;` or `namespace [Name] { … }`.
    Namespace {
        /// The name, absent for the global namespace block.
        name: Option<Vec<u8>>,
        /// The block, absent for the statement form.
        body: Option<Vec<TopStatement>>,
        /// Line of the statement.
        line: u32,
    },
    /// `use A\B [as C], …;`, `use function …;` or `use const …;`.
    Use {
        /// What the names import.
        kind: UseKind,
        /// The imports.
        items: Vec<UseItem>,
        /// Line of the statement.
        line: u32,
    },
    /// `use A\{B, function c};`.
    GroupUse {
        /// What the names import unless an item says otherwise.
        kind: UseKind,
        /// The shared prefix.
        prefix: Vec<u8>,
        /// The imports.
        items: Vec<UseItem>,
        /// Line of the statement.
        line: u32,
    },
    /// `const A = 1, B = 2;`.
    Const {
        /// The constants.
        items: Vec<ConstItem>,
        /// Attributes written before `const`.
        attributes: Vec<AttributeGroup>,
        /// Line of the statement.
        line: u32,
    },
}

/// What a `use` import refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseKind {
    /// A class, interface, trait, enum or namespace.
    Class,
    /// A function.
    Function,
    /// A constant.
    Constant,
}

/// One import of a `use` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UseItem {
    /// The imported name (without a leading `\`).
    pub name: Vec<u8>,
    /// The `as` alias.
    pub alias: Option<Vec<u8>>,
    /// The kind given inside a group (`use A\{function b}`), when one is.
    pub kind: Option<UseKind>,
}

/// A statement.
#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    /// What the statement is.
    pub kind: StmtKind,
    /// Line it starts on.
    pub line: u32,
}

/// The kinds of statement.
#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    /// `;`.
    Nop,
    /// `{ … }`, or several statements written as one (`echo a, b;`, `global $a, $b;`).
    Block(Vec<Stmt>),
    /// An expression followed by `;`.
    Expr(Expr),
    /// `echo expr;`, one per echoed expression; text outside the PHP tags is an echoed string.
    Echo(Expr),
    /// `if`, with its `elseif` and `else` branches.
    If(Box<If>),
    /// `while (cond) body`.
    While {
        /// The condition.
        condition: Expr,
        /// The body.
        body: Box<Stmt>,
    },
    /// `do body while (cond);`.
    DoWhile {
        /// The body.
        body: Box<Stmt>,
        /// The condition.
        condition: Expr,
    },
    /// `for (init; cond; step) body`.
    For(Box<For>),
    /// `foreach (subject as [key =>] value) body`.
    Foreach(Box<Foreach>),
    /// `switch (subject) { case …: … }`.
    Switch {
        /// The switched value.
        subject: Expr,
        /// The cases, in order.
        cases: Vec<Case>,
    },
    /// `break [levels];`.
    Break(Option<Expr>),
    /// `continue [levels];`.
    Continue(Option<Expr>),
    /// `return [value];`.
    Return(Option<Expr>),
    /// `global $name;`, one per variable; the expression is a variable.
    Global(Expr),
    /// `static $name [= default];`, one per variable.
    Static {
        /// The variable name, without `$`.
        name: Vec<u8>,
        /// The initial value.
        default: Option<Expr>,
    },
    /// `unset(variable);`, one per variable.
    Unset(Expr),
    /// `name:`.
    Label(Vec<u8>),
    /// `goto name;`.
    Goto(Vec<u8>),
    /// `try { … } catch (…) { … } finally { … }`.
    Try(Box<Try>),
    /// `declare(name = value, …) body` or `declare(…);`.
    Declare {
        /// The directives.
        directives: Vec<ConstItem>,
        /// The body; absent for `declare(…);`.
        body: Option<Box<Stmt>>,
    },
    /// A function declaration.
    Function(Box<Function>),
    /// A class, interface, trait or enum declaration.
    Class(Box<ClassLike>),
}

/// An `if` statement.
#[derive(Debug, Clone, PartialEq)]
pub struct If {
    /// The `if` and `elseif` conditions with their bodies, in order.
    pub branches: Vec<(Expr, Stmt)>,
    /// The `else` body.
    pub otherwise: Option<Stmt>,
}

/// A `for` statement.
#[derive(Debug, Clone, PartialEq)]
pub struct For {
    /// Expressions run once before the loop.
    pub init: Vec<Expr>,
    /// Expressions checked before each iteration; the last decides.
    pub condition: Vec<Expr>,
    /// Expressions run after each iteration.
    pub step: Vec<Expr>,
    /// The body.
    pub body: Stmt,
}

/// A `foreach` statement.
#[derive(Debug, Clone, PartialEq)]
pub struct Foreach {
    /// The iterated value.
    pub subject: Expr,
    /// The key target.
    pub key: Option<Expr>,
    /// The value target; [`ExprKind::Reference`] when taken by reference.
    pub value: Expr,
    /// The body.
    pub body: Stmt,
}

/// One `case` or `default` of a `switch`.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    /// The compared value; absent for `default`.
    pub condition: Option<Expr>,
    /// The statements up to the next case.
    pub body: Vec<Stmt>,
    /// Whether the label ends with `;` instead of `:`.
    pub semicolon: bool,
    /// The line of `case` or `default`.
    pub line: u32,
}

/// A `try` statement.
#[derive(Debug, Clone, PartialEq)]
pub struct Try {
    /// The guarded statements.
    pub body: Vec<Stmt>,
    /// The `catch` clauses.
    pub catches: Vec<Catch>,
    /// The `finally` statements.
    pub finally: Option<Vec<Stmt>>,
}

/// A `catch` clause.
#[derive(Debug, Clone, PartialEq)]
pub struct Catch {
    /// The caught classes.
    pub types: Vec<Name>,
    /// The variable the exception is stored in, without `$`.
    pub variable: Option<Vec<u8>>,
    /// The handler.
    pub body: Vec<Stmt>,
    /// Line of the first caught class.
    pub line: u32,
}

/// `name = value` in a constant declaration or a `declare`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstItem {
    /// The name.
    pub name: Vec<u8>,
    /// The line of the name.
    pub line: u32,
    /// The value.
    pub value: Expr,
}

/// A name as written in code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name {
    /// How the name is qualified.
    pub kind: NameKind,
    /// The name without the leading `\` or `namespace\`.
    pub bytes: Vec<u8>,
}

/// How a name is qualified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    /// `A` or `A\B`: resolved against the current namespace and imports.
    Unqualified,
    /// `\A\B`.
    FullyQualified,
    /// `namespace\A`.
    Relative,
}

/// An expression.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    /// What the expression is.
    pub kind: ExprKind,
    /// Line it starts on; for a heredoc, the line its body starts on.
    pub line: u32,
}

/// The kinds of expression.
#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    /// An integer literal.
    Int(i64),
    /// A float literal.
    Float(f64),
    /// A string literal, decoded.
    String(Vec<u8>),
    /// A string with interpolated parts.
    Interpolated(Vec<StringPart>),
    /// `` `command` ``.
    ShellExec(Vec<StringPart>),
    /// A class or function name used as a value of another node (`A` in `A::B` or `A()`).
    Name(Name),
    /// A constant read (`A`, `\A\B`, `true`).
    Constant(Name),
    /// A magic constant such as `__LINE__`.
    Magic(MagicConstant),
    /// `$name`, `$$expr` or `${expr}`.
    Variable(VarName),
    /// `[…]`, `array(…)` or `list(…)`; skipped items (`[, $b]`) are `None`.
    Array {
        /// The items.
        items: Vec<Option<ArrayItem>>,
        /// How it was written.
        syntax: ArraySyntax,
    },
    /// `base[index]`; `base[]` has no index.
    Dim {
        /// The indexed value.
        base: Box<Expr>,
        /// The index.
        index: Option<Box<Expr>>,
    },
    /// `object->name` or `object?->name`.
    Property {
        /// The object.
        object: Box<Expr>,
        /// The property.
        name: Member,
        /// Whether `?->` was used.
        nullsafe: bool,
    },
    /// `Class::$name`.
    StaticProperty {
        /// The class.
        class: Box<Expr>,
        /// The property.
        name: VarName,
        /// Whether the whole expression was in parentheses.
        parenthesized: bool,
    },
    /// `callee(args)`.
    Call {
        /// What is called: a [`ExprKind::Name`] for a function name, or any expression.
        callee: Box<Expr>,
        /// The arguments.
        args: Args,
    },
    /// `object->name(args)` or `object?->name(args)`.
    MethodCall {
        /// The object.
        object: Box<Expr>,
        /// The method.
        name: Member,
        /// The arguments.
        args: Args,
        /// Whether `?->` was used.
        nullsafe: bool,
    },
    /// `Class::name(args)`.
    StaticCall {
        /// The class.
        class: Box<Expr>,
        /// The method.
        name: Member,
        /// The arguments.
        args: Args,
    },
    /// `new Class(args)`; `new Class` has no arguments.
    New {
        /// The class.
        class: Box<Expr>,
        /// The constructor arguments.
        args: Args,
    },
    /// `new class(args) { … }`.
    NewAnonymous {
        /// The class body.
        class: Box<ClassLike>,
        /// The constructor arguments.
        args: Args,
    },
    /// `Class::NAME` or `Class::{expr}`.
    ClassConstant {
        /// The class.
        class: Box<Expr>,
        /// The constant.
        name: Member,
    },
    /// `Class::class`.
    ClassName(Box<Expr>),
    /// A prefix operator.
    Unary {
        /// The operator.
        op: UnaryOp,
        /// The operand.
        operand: Box<Expr>,
    },
    /// `(type) operand`.
    Cast {
        /// The target type.
        kind: CastKind,
        /// The operand.
        operand: Box<Expr>,
    },
    /// `(void) operand`.
    VoidCast(Box<Expr>),
    /// `++` or `--`, before or after a variable.
    IncDec {
        /// Which one and where.
        op: IncDecOp,
        /// The variable.
        operand: Box<Expr>,
    },
    /// A binary operator.
    Binary {
        /// The operator.
        op: BinaryOp,
        /// The left operand.
        left: Box<Expr>,
        /// The right operand.
        right: Box<Expr>,
    },
    /// `expr instanceof Class`.
    Instanceof {
        /// The tested value.
        expr: Box<Expr>,
        /// The class.
        class: Box<Expr>,
    },
    /// `target = value`.
    Assign {
        /// The assigned variable or destructuring array.
        target: Box<Expr>,
        /// The value.
        value: Box<Expr>,
    },
    /// `target =& value`.
    AssignRef {
        /// The assigned variable.
        target: Box<Expr>,
        /// The referenced variable.
        value: Box<Expr>,
    },
    /// `target op= value`.
    CompoundAssign {
        /// The operator.
        op: AssignOp,
        /// The assigned variable.
        target: Box<Expr>,
        /// The value.
        value: Box<Expr>,
    },
    /// `cond ? then : otherwise` or `cond ?: otherwise`.
    Ternary {
        /// The condition.
        condition: Box<Expr>,
        /// The value when true; absent for `?:`.
        then: Option<Box<Expr>>,
        /// The value when false.
        otherwise: Box<Expr>,
        /// Whether the whole expression was in parentheses.
        parenthesized: bool,
    },
    /// `isset(expr)`, one per checked expression.
    Isset(Box<Expr>),
    /// `empty(expr)`.
    Empty(Box<Expr>),
    /// `include`, `require`, their `_once` forms, or `eval`.
    Include {
        /// Which construct.
        kind: IncludeKind,
        /// The path or code.
        expr: Box<Expr>,
    },
    /// `print expr`.
    Print(Box<Expr>),
    /// `yield`, `yield value` or `yield key => value`.
    Yield {
        /// The key.
        key: Option<Box<Expr>>,
        /// The value.
        value: Option<Box<Expr>>,
    },
    /// `yield from expr`.
    YieldFrom(Box<Expr>),
    /// `throw expr`.
    Throw(Box<Expr>),
    /// `function (…) use (…) { … }`.
    Closure(Box<Function>),
    /// `fn (…) => expr`.
    ArrowFunction {
        /// The function; its body is [`FunctionBody::Expr`].
        function: Box<Function>,
        /// Whether the whole expression was in parentheses.
        parenthesized: bool,
    },
    /// `match (subject) { … }`.
    Match {
        /// The matched value.
        subject: Box<Expr>,
        /// The arms.
        arms: Vec<MatchArm>,
    },
    /// `&variable` as a `foreach` target.
    Reference(Box<Expr>),
}

/// A magic constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MagicConstant {
    /// `__LINE__`.
    Line,
    /// `__FILE__`.
    File,
    /// `__DIR__`.
    Dir,
    /// `__CLASS__`.
    Class,
    /// `__TRAIT__`.
    Trait,
    /// `__METHOD__`.
    Method,
    /// `__FUNCTION__`.
    Function,
    /// `__PROPERTY__`.
    Property,
    /// `__NAMESPACE__`.
    Namespace,
}

/// One part of an interpolated string.
#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    /// Literal text, decoded.
    Literal(Vec<u8>),
    /// An interpolated expression.
    Expr(Expr),
    /// `${name}` or `${name[index]}`, the deprecated spelling of a variable.
    BracedName(Expr),
    /// `${expr}`, the deprecated spelling of a variable variable.
    BracedExpr(Expr),
}

impl StringPart {
    /// The interpolated expression, if this part is not literal text.
    #[must_use]
    pub const fn expr(&self) -> Option<&Expr> {
        match self {
            Self::Literal(_) => None,
            Self::Expr(expr) | Self::BracedName(expr) | Self::BracedExpr(expr) => Some(expr),
        }
    }
}

/// The name of a variable.
#[derive(Debug, Clone, PartialEq)]
pub enum VarName {
    /// A name known while parsing (`$a`, `${'a'}`), without `$`.
    Name(Vec<u8>),
    /// A name computed at run time (`$$a`, `${expr}`).
    Expr(Box<Expr>),
}

/// The name of a property, method or class constant.
#[derive(Debug, Clone, PartialEq)]
pub enum Member {
    /// A name known while parsing.
    Name(Vec<u8>),
    /// A name computed at run time (`->{expr}`, `->$name`).
    Expr(Box<Expr>),
}

/// How an array literal was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArraySyntax {
    /// `[…]`.
    Short,
    /// `array(…)`.
    Long,
    /// `list(…)`.
    List,
}

/// One item of an array literal.
#[derive(Debug, Clone, PartialEq)]
pub enum ArrayItem {
    /// `[key =>] [&]value`.
    Value {
        /// The key.
        key: Option<Expr>,
        /// The value.
        value: Expr,
        /// Whether the value is taken by reference.
        by_ref: bool,
    },
    /// `...expr`.
    Spread(Expr),
}

/// The arguments of a call.
#[derive(Debug, Clone, PartialEq)]
pub enum Args {
    /// A parenthesized argument list.
    List(Vec<Arg>),
    /// `(...)`: the call creates a closure instead.
    CallableConvert,
}

/// One argument.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    /// A positional argument.
    Positional(Expr),
    /// `name: value`.
    Named {
        /// The parameter name.
        name: Vec<u8>,
        /// The value.
        value: Expr,
    },
    /// `...expr`.
    Unpack(Expr),
}

/// A prefix operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// `+`.
    Plus,
    /// `-`.
    Minus,
    /// `!`.
    Not,
    /// `~`.
    BitNot,
    /// `@`.
    Silence,
}

/// The type of a cast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastKind {
    /// `(int)`.
    Int,
    /// `(float)`.
    Float,
    /// `(string)`.
    String,
    /// `(array)`.
    Array,
    /// `(object)`.
    Object,
    /// `(bool)`.
    Bool,
    /// `(unset)`.
    Unset,
}

/// An increment or decrement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncDecOp {
    /// `++$a`.
    PreInc,
    /// `--$a`.
    PreDec,
    /// `$a++`.
    PostInc,
    /// `$a--`.
    PostDec,
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    /// `+`.
    Add,
    /// `-`.
    Sub,
    /// `*`.
    Mul,
    /// `/`.
    Div,
    /// `%`.
    Mod,
    /// `**`.
    Pow,
    /// `.`.
    Concat,
    /// `<<`.
    ShiftLeft,
    /// `>>`.
    ShiftRight,
    /// `|`.
    BitOr,
    /// `&`.
    BitAnd,
    /// `^`.
    BitXor,
    /// `&&` or `and`.
    And,
    /// `||` or `or`.
    Or,
    /// `xor`.
    Xor,
    /// `==`.
    Equal,
    /// `!=` or `<>`.
    NotEqual,
    /// `===`.
    Identical,
    /// `!==`.
    NotIdentical,
    /// `<`.
    Less,
    /// `<=`.
    LessOrEqual,
    /// `>`.
    Greater,
    /// `>=`.
    GreaterOrEqual,
    /// `<=>`.
    Spaceship,
    /// `??`.
    Coalesce,
    /// `|>`.
    Pipe,
}

/// A compound assignment operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    /// `+=`.
    Add,
    /// `-=`.
    Sub,
    /// `*=`.
    Mul,
    /// `/=`.
    Div,
    /// `%=`.
    Mod,
    /// `**=`.
    Pow,
    /// `.=`.
    Concat,
    /// `<<=`.
    ShiftLeft,
    /// `>>=`.
    ShiftRight,
    /// `|=`.
    BitOr,
    /// `&=`.
    BitAnd,
    /// `^=`.
    BitXor,
    /// `??=`.
    Coalesce,
}

/// `include`, `require` and `eval`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncludeKind {
    /// `include`.
    Include,
    /// `include_once`.
    IncludeOnce,
    /// `require`.
    Require,
    /// `require_once`.
    RequireOnce,
    /// `eval`.
    Eval,
}

/// One arm of a `match`.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    /// The compared values; absent for `default`.
    pub conditions: Option<Vec<Expr>>,
    /// The result.
    pub body: Expr,
}

/// A function, method, closure or arrow function.
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    /// The name; empty for closures and arrow functions.
    pub name: Vec<u8>,
    /// Whether it returns by reference.
    pub by_ref: bool,
    /// The parameters.
    pub params: Vec<Param>,
    /// Closure `use` variables.
    pub uses: Vec<ClosureUse>,
    /// The return type.
    pub return_type: Option<Type>,
    /// The body.
    pub body: FunctionBody,
    /// Whether the body contains `yield` outside nested functions.
    pub is_generator: bool,
    /// Method modifiers, or [`Modifiers::STATIC`] for a static closure.
    pub modifiers: Modifiers,
    /// The attributes.
    pub attributes: Vec<AttributeGroup>,
    /// Line of `function` or `fn`.
    pub line: u32,
}

/// The body of a function.
#[derive(Debug, Clone, PartialEq)]
pub enum FunctionBody {
    /// `{ … }`.
    Block(Vec<Stmt>),
    /// The expression of an arrow function.
    Expr(Box<Expr>),
    /// No body: an abstract or interface method.
    Absent,
}

/// A variable captured by a closure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosureUse {
    /// The name, without `$`.
    pub name: Vec<u8>,
    /// Whether it is captured by reference.
    pub by_ref: bool,
}

/// A function parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// The name, without `$`.
    pub name: Vec<u8>,
    /// The declared type.
    pub declared_type: Option<Type>,
    /// The default value.
    pub default: Option<Expr>,
    /// Whether it is passed by reference.
    pub by_ref: bool,
    /// Whether it is variadic.
    pub variadic: bool,
    /// Constructor promotion modifiers.
    pub modifiers: Modifiers,
    /// Hooks of a promoted property; `Some` for `{ … }` even when empty.
    pub hooks: Option<Vec<Hook>>,
    /// The attributes.
    pub attributes: Vec<AttributeGroup>,
    /// The line of the type, or of the variable when untyped.
    pub line: u32,
}

/// A declared type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// One type, possibly nullable.
    Simple {
        /// The type.
        kind: SimpleType,
        /// Whether `?` was written before it.
        nullable: bool,
    },
    /// `A|B`.
    Union(Vec<Type>),
    /// `A&B`.
    Intersection(Vec<Type>),
}

/// A type that is not a union or intersection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimpleType {
    /// `array`.
    Array,
    /// `callable`.
    Callable,
    /// `static`.
    Static,
    /// A class or built-in type name (`int`, `mixed`, `Foo`).
    Name(Name),
}

/// Member and class modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers(u16);

impl Modifiers {
    /// `public`.
    pub const PUBLIC: Self = Self(1);
    /// `protected`.
    pub const PROTECTED: Self = Self(1 << 1);
    /// `private`.
    pub const PRIVATE: Self = Self(1 << 2);
    /// `static`.
    pub const STATIC: Self = Self(1 << 3);
    /// `abstract`.
    pub const ABSTRACT: Self = Self(1 << 4);
    /// `final`.
    pub const FINAL: Self = Self(1 << 5);
    /// `readonly`.
    pub const READONLY: Self = Self(1 << 6);
    /// `public(set)`.
    pub const PUBLIC_SET: Self = Self(1 << 7);
    /// `protected(set)`.
    pub const PROTECTED_SET: Self = Self(1 << 8);
    /// `private(set)`.
    pub const PRIVATE_SET: Self = Self(1 << 9);
    /// No modifier.
    pub const NONE: Self = Self(0);
    /// The three visibilities.
    pub const VISIBILITY: Self = Self(0b111);
    /// The three set visibilities.
    pub const SET_VISIBILITY: Self = Self(0b111 << 7);

    /// Whether every modifier of `other` is present.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any modifier of `other` is present.
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Both sets of modifiers.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether no modifier is present.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// An attribute group: `#[A, B(1)]`.
pub type AttributeGroup = Vec<Attribute>;

/// One attribute.
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    /// The attribute class.
    pub name: Name,
    /// The arguments, when parentheses were written.
    pub args: Option<Args>,
}

/// A class, interface, trait or enum.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassLike {
    /// What it is.
    pub kind: ClassKind,
    /// The name; absent for an anonymous class.
    pub name: Option<Vec<u8>>,
    /// `abstract`, `final` and `readonly`.
    pub modifiers: Modifiers,
    /// The parent class.
    pub extends: Option<Name>,
    /// Implemented interfaces, or the parents of an interface.
    pub implements: Vec<Name>,
    /// The backing type of an enum.
    pub backing_type: Option<Type>,
    /// The members.
    pub members: Vec<ClassMember>,
    /// The attributes.
    pub attributes: Vec<AttributeGroup>,
    /// Line of the keyword.
    pub line: u32,
}

/// What a class-like declaration is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassKind {
    /// A class.
    Class,
    /// An interface.
    Interface,
    /// A trait.
    Trait,
    /// An enum.
    Enum,
}

/// A member of a class-like declaration.
#[derive(Debug, Clone, PartialEq)]
pub enum ClassMember {
    /// Properties declared together.
    Property(PropertyGroup),
    /// Constants declared together.
    Constant(ConstantGroup),
    /// A method.
    Method(Box<Function>),
    /// An enum case.
    EnumCase(EnumCase),
    /// `use A, B { … }`.
    TraitUse(TraitUse),
}

/// `[modifiers] [type] $a [= v], $b …;`.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyGroup {
    /// The modifiers.
    pub modifiers: Modifiers,
    /// The type.
    pub property_type: Option<Type>,
    /// The properties.
    pub properties: Vec<Property>,
    /// The attributes.
    pub attributes: Vec<AttributeGroup>,
    /// Line of the type, or of the first property when untyped.
    pub line: u32,
}

/// One property.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    /// The name, without `$`.
    pub name: Vec<u8>,
    /// The default value.
    pub default: Option<Expr>,
    /// The hooks; `Some` for `{ … }` even when empty.
    pub hooks: Option<Vec<Hook>>,
    /// The line of the variable.
    pub line: u32,
}

/// A property hook.
#[derive(Debug, Clone, PartialEq)]
pub struct Hook {
    /// The hook name as written (`get`, `set`).
    pub name: Vec<u8>,
    /// Whether it returns by reference.
    pub by_ref: bool,
    /// The modifiers.
    pub modifiers: Modifiers,
    /// The parameter list, when parentheses were written.
    pub params: Option<Vec<Param>>,
    /// The body.
    pub body: HookBody,
    /// Whether the body contains `yield` outside nested functions.
    pub is_generator: bool,
    /// Whether the body fetches the hooked property as `$this->name` outside nested functions.
    pub uses_property: bool,
    /// The attributes.
    pub attributes: Vec<AttributeGroup>,
    /// Line of the name.
    pub line: u32,
    /// Line the hook's last token ends on.
    pub end_line: u32,
}

/// The body of a property hook.
#[derive(Debug, Clone, PartialEq)]
pub enum HookBody {
    /// `;`.
    Absent,
    /// `=> expr;`.
    Expr(Expr),
    /// `{ … }`.
    Block(Vec<Stmt>),
}

/// `[modifiers] const [type] A = 1, B = 2;`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstantGroup {
    /// The modifiers.
    pub modifiers: Modifiers,
    /// The declared type.
    pub constant_type: Option<Type>,
    /// The constants.
    pub items: Vec<ConstItem>,
    /// The attributes.
    pub attributes: Vec<AttributeGroup>,
    /// Line of the first constant's name.
    pub line: u32,
}

/// `case A [= value];`.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumCase {
    /// The name.
    pub name: Vec<u8>,
    /// The backing value.
    pub value: Option<Expr>,
    /// The attributes.
    pub attributes: Vec<AttributeGroup>,
    /// Line of the name.
    pub line: u32,
}

/// `use A, B;` or `use A, B { … }` in a class body.
#[derive(Debug, Clone, PartialEq)]
pub struct TraitUse {
    /// The traits.
    pub traits: Vec<Name>,
    /// The adaptations; absent for `;` and for an empty block.
    pub adaptations: Option<Vec<TraitAdaptation>>,
    /// Line of `use`.
    pub line: u32,
}

/// A trait adaptation.
#[derive(Debug, Clone, PartialEq)]
pub enum TraitAdaptation {
    /// `A::m insteadof B, C;`.
    Precedence {
        /// The method that wins.
        method: MethodReference,
        /// The traits whose method is excluded.
        instead_of: Vec<Name>,
    },
    /// `[A::]m as [modifier] [alias];`.
    Alias {
        /// The aliased method.
        method: MethodReference,
        /// The modifier given.
        modifier: Modifiers,
        /// The new name.
        alias: Option<Vec<u8>>,
    },
}

/// `[Trait::]method` in a trait adaptation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodReference {
    /// The trait.
    pub class: Option<Name>,
    /// The method.
    pub method: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::Modifiers;

    #[test]
    fn modifiers_combine_as_sets() {
        let both = Modifiers::PUBLIC.union(Modifiers::STATIC);
        assert!(both.contains(Modifiers::PUBLIC));
        assert!(both.intersects(Modifiers::VISIBILITY));
        assert!(!both.intersects(Modifiers::SET_VISIBILITY));
        assert!(!both.contains(Modifiers::FINAL));
        assert!(Modifiers::NONE.is_empty());
        assert!(!both.is_empty());
        assert_eq!(Modifiers::default(), Modifiers::NONE);
    }
}
