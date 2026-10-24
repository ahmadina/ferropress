//! Constant expressions: folding, and the checks on what a constant expression may contain.
//!
//! The reference folds an expression in place before compiling it: operators over known operands,
//! `true`/`false`/`null`, magic constants, array literals and casts become values, and `??` or
//! `?:` with a known left side become the operand they select. [`Checker::folded`] records each
//! replacement, so every later pass sees the tree the reference sees. A constant expression is then
//! checked node by node for what it may contain.
//!
//! Not folded yet: constants other than `true`/`false`/`null` (they need the registry of built-in
//! constants), class constants, `X::class` for a class name (it needs import resolution), and the
//! magic constants that depend on the file path or a generated name (`__FILE__`, `__DIR__`, and
//! names inside closures and anonymous classes).

use std::cmp::Ordering;
use std::ptr;

use php_parser::ast::{
    Arg, Args, ArrayItem, ArraySyntax, AttributeGroup, BinaryOp, CastKind, Expr, ExprKind,
    MagicConstant, Member, Modifiers, Name, NameKind, UnaryOp,
};
use php_value::ops::{self, Collect, Number, OpError};
use php_value::{ArrayKey, PhpArray, PhpString, Slot, Value};

use super::{Checker, Code, Flow};
use crate::diagnostic::Severity;

/// What folding made of a node.
#[derive(Debug)]
pub(super) enum Folded<'a> {
    /// The node is this value.
    Value(Value),
    /// The node was replaced by one of its operands.
    Replaced(&'a Expr),
}

/// `2^63`, the first float above the integer range.
const INT_END: f64 = 9_223_372_036_854_775_808.0;

/// Whether `value` is an integer in the integer range, so converting it loses nothing.
fn is_long_compatible(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0 && (-INT_END..INT_END).contains(&value)
}

/// Whether an operand converts to an integer without a diagnostic.
fn is_operand_long_compatible(value: &Value) -> bool {
    match value {
        Value::Array(_) => false,
        Value::Float(number) => is_long_compatible(*number),
        Value::String(text) => {
            ops::whole_numeric(text.as_bytes()).is_some_and(|prefix| match prefix.number {
                Number::Int(_) => true,
                Number::Float(number) => is_long_compatible(number),
            })
        }
        Value::Null | Value::Bool(_) | Value::Int(_) | Value::Object(_) => true,
    }
}

/// Whether a string operand is non-numeric, which numeric operators reject.
fn is_non_numeric(value: &Value) -> bool {
    matches!(value, Value::String(text) if ops::whole_numeric(text.as_bytes()).is_none())
}

/// A conversion that raises nothing for the operands it is used on.
fn quiet_float(value: &Value) -> f64 {
    value.to_float().unwrap_or_default()
}

fn quiet_int(value: &Value) -> i64 {
    value.to_int(&mut Collect::default()).unwrap_or_default()
}

/// Whether evaluating `op` would throw or warn, so it is left to run time.
fn binary_produces_error(op: BinaryOp, left: &Value, right: &Value) -> bool {
    let is_array = |value: &Value| matches!(value, Value::Array(_));
    if op == BinaryOp::Concat {
        return is_array(left) || is_array(right);
    }
    let bitwise = matches!(op, BinaryOp::BitOr | BinaryOp::BitAnd | BinaryOp::BitXor);
    let shift = matches!(op, BinaryOp::ShiftLeft | BinaryOp::ShiftRight);
    let numeric = bitwise
        || shift
        || matches!(
            op,
            BinaryOp::Add
                | BinaryOp::Sub
                | BinaryOp::Mul
                | BinaryOp::Div
                | BinaryOp::Mod
                | BinaryOp::Pow
        );
    if !numeric {
        return false;
    }
    if is_array(left) || is_array(right) {
        return !(op == BinaryOp::Add && is_array(left) && is_array(right));
    }
    if bitwise && matches!((left, right), (Value::String(_), Value::String(_))) {
        return false;
    }
    if is_non_numeric(left) || is_non_numeric(right) {
        return true;
    }
    let long_compatible = is_operand_long_compatible(left) && is_operand_long_compatible(right);
    match op {
        _ if (bitwise || shift) && !long_compatible => true,
        BinaryOp::Div => quiet_float(right) == 0.0,
        BinaryOp::Mod => !long_compatible || quiet_int(right) == 0,
        BinaryOp::Pow => quiet_float(left) == 0.0 && quiet_float(right) < 0.0,
        BinaryOp::ShiftLeft | BinaryOp::ShiftRight => quiet_int(right) < 0,
        _ => false,
    }
}

/// Evaluates a binary operator; `None` for `&&`, `||`, `??` and `|>`, which fold by their own
/// rules or not at all.
fn binary(
    op: BinaryOp,
    left: &Value,
    right: &Value,
    cx: &mut Collect,
) -> Option<Result<Value, OpError>> {
    let bool_of = |result: Result<bool, OpError>| result.map(Value::Bool);
    Some(match op {
        BinaryOp::Add => ops::add(left, right, cx),
        BinaryOp::Sub => ops::sub(left, right, cx),
        BinaryOp::Mul => ops::mul(left, right, cx),
        BinaryOp::Div => ops::div(left, right, cx),
        BinaryOp::Mod => ops::modulo(left, right, cx),
        BinaryOp::Pow => ops::pow(left, right, cx),
        BinaryOp::Concat => ops::concat(left, right, cx),
        BinaryOp::ShiftLeft => ops::shift_left(left, right, cx),
        BinaryOp::ShiftRight => ops::shift_right(left, right, cx),
        BinaryOp::BitOr => ops::bit_or(left, right, cx),
        BinaryOp::BitAnd => ops::bit_and(left, right, cx),
        BinaryOp::BitXor => ops::bit_xor(left, right, cx),
        BinaryOp::Xor => left
            .to_bool(cx)
            .and_then(|left| Ok(Value::Bool(left ^ right.to_bool(cx)?))),
        BinaryOp::Equal => bool_of(ops::loose_equals(left, right, cx)),
        BinaryOp::NotEqual => bool_of(ops::loose_equals(left, right, cx).map(|equal| !equal)),
        BinaryOp::Identical => bool_of(ops::identical(left, right)),
        BinaryOp::NotIdentical => bool_of(ops::identical(left, right).map(|same| !same)),
        BinaryOp::Less => bool_of(ops::less(left, right, cx)),
        BinaryOp::LessOrEqual => bool_of(ops::less_or_equal(left, right, cx)),
        BinaryOp::Greater => bool_of(ops::less(right, left, cx)),
        BinaryOp::GreaterOrEqual => bool_of(ops::less_or_equal(right, left, cx)),
        BinaryOp::Spaceship => ops::compare(left, right, cx).map(|order| {
            Value::Int(match order {
                Ordering::Less => -1,
                Ordering::Equal => 0,
                Ordering::Greater => 1,
            })
        }),
        BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce | BinaryOp::Pipe => return None,
    })
}

/// A cast the reference evaluates at compile time; `None` leaves it to run time.
fn cast(kind: CastKind, value: &Value, cx: &mut Collect) -> Option<Value> {
    if matches!(value, Value::Float(number) if number.is_nan()) {
        return None;
    }
    let result = match kind {
        CastKind::Bool => value.to_bool(cx).map(Value::Bool),
        CastKind::Int => match value {
            Value::Float(number) if !(-INT_END..INT_END).contains(number) => return None,
            _ => value.to_int(cx).map(Value::Int),
        },
        CastKind::Float => value.to_float().map(Value::Float),
        CastKind::String => match value {
            Value::Array(_) | Value::Float(_) => return None,
            _ => value.to_php_string(cx).map(Value::String),
        },
        CastKind::Array => value.to_array(cx).map(Value::Array),
        CastKind::Object | CastKind::Unset => return None,
    };
    result.ok()
}

/// `self`, `parent` or `static`, as a class reference names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fetch {
    SelfClass,
    Parent,
    Static,
}

impl Fetch {
    pub(super) const fn keyword(self) -> &'static str {
        match self {
            Self::SelfClass => "self",
            Self::Parent => "parent",
            Self::Static => "static",
        }
    }
}

/// The special class `class` names, when it is written as a name or string.
fn fetch_of(class: &Expr) -> Option<Fetch> {
    let (ExprKind::Name(Name {
        kind: NameKind::Unqualified,
        bytes,
    })
    | ExprKind::String(bytes)) = &class.kind
    else {
        return None;
    };
    [Fetch::SelfClass, Fetch::Parent, Fetch::Static]
        .into_iter()
        .find(|fetch| bytes.eq_ignore_ascii_case(fetch.keyword().as_bytes()))
}

/// Whether `expr` is a literal number, which no class or function can be named by.
const fn is_number(expr: &Expr) -> bool {
    matches!(expr.kind, ExprKind::Int(_) | ExprKind::Float(_))
}

fn text(bytes: &[u8]) -> Value {
    Value::String(PhpString::new(bytes))
}

impl<'a> Checker<'a> {
    /// Compiles `expr` as a constant expression; `allow_dynamic` permits `new` and object casts,
    /// whose value may differ on each evaluation. Returns the value it folded to, if any.
    pub(super) fn constant(&mut self, expr: &'a Expr, allow_dynamic: bool) -> Flow<Option<Value>> {
        let value = self.fold(expr)?;
        self.validate(expr, allow_dynamic)?;
        Ok(value)
    }

    /// Compiles the arguments of `groups`' attributes.
    pub(super) fn attributes(&mut self, groups: &'a [AttributeGroup]) -> Flow {
        for attribute in groups.iter().flatten() {
            let args = match &attribute.args {
                None => continue,
                Some(Args::CallableConvert) => {
                    return self.fatal("Cannot create Closure as attribute argument");
                }
                Some(Args::List(args)) => args,
            };
            let mut named = false;
            for (index, arg) in args.iter().enumerate() {
                let value = match arg {
                    Arg::Unpack(_) => {
                        return self.fatal("Cannot use unpacking in attribute argument list");
                    }
                    Arg::Named { name, value } => {
                        let repeated = args[..index].iter().any(
                            |earlier| matches!(earlier, Arg::Named { name: other, .. } if other == name),
                        );
                        if repeated {
                            let mut message = b"Duplicate named parameter $".to_vec();
                            message.extend_from_slice(name);
                            return self.fatal(message);
                        }
                        named = true;
                        value
                    }
                    Arg::Positional(value) => {
                        if named {
                            return self
                                .fatal("Cannot use positional argument after named argument");
                        }
                        value
                    }
                };
                self.constant(value, true)?;
            }
        }
        Ok(())
    }

    /// What `expr` stands for after folding: the node, or the operand that replaced it.
    pub(super) fn resolve(&self, mut expr: &'a Expr) -> &'a Expr {
        while let Some(Folded::Replaced(by)) = self.folded.get(&ptr::from_ref(expr)) {
            expr = by;
        }
        expr
    }

    /// The value of `expr` when it is a literal or was folded to one.
    fn value_of(&self, expr: &'a Expr) -> Option<Value> {
        let expr = self.resolve(expr);
        match &expr.kind {
            ExprKind::Int(number) => Some(Value::Int(*number)),
            ExprKind::Float(number) => Some(Value::Float(*number)),
            ExprKind::String(bytes) => Some(text(bytes)),
            _ => match self.folded.get(&ptr::from_ref(expr)) {
                Some(Folded::Value(value)) => Some(value.clone()),
                _ => None,
            },
        }
    }

    /// Whether `expr` is a value or a name once folded, the operands the reference keeps as is.
    fn is_known(&self, expr: &'a Expr) -> bool {
        matches!(self.resolve(expr).kind, ExprKind::Name(_)) || self.value_of(expr).is_some()
    }

    /// Runs `operation` and reports its diagnostics, unless it fails.
    fn evaluate<T>(
        &mut self,
        operation: impl FnOnce(&mut Collect) -> Result<T, OpError>,
    ) -> Option<T> {
        let mut cx = Collect::default();
        let result = operation(&mut cx).ok()?;
        for diagnostic in cx.diagnostics {
            let severity = match diagnostic.severity() {
                ops::Severity::Warning => Severity::Warning,
                ops::Severity::Deprecated => Severity::Deprecated,
            };
            self.report(severity, diagnostic.message());
        }
        Some(result)
    }

    fn truthy(&mut self, value: &Value) -> bool {
        self.evaluate(|cx| value.to_bool(cx)).unwrap_or_default()
    }

    /// Folds `expr` and returns its value when it has one.
    pub(super) fn fold(&mut self, expr: &'a Expr) -> Flow<Option<Value>> {
        let key = ptr::from_ref(expr);
        match self.folded.get(&key) {
            Some(Folded::Value(value)) => return Ok(Some(value.clone())),
            Some(Folded::Replaced(by)) => {
                let by = *by;
                return self.fold(by);
            }
            None => {}
        }
        let value = match &expr.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::String(_) => {
                return Ok(self.value_of(expr));
            }
            ExprKind::Binary {
                op: BinaryOp::Coalesce,
                left,
                right,
            } => return self.fold_coalesce(expr, left, right),
            ExprKind::Ternary {
                condition,
                then,
                otherwise,
                ..
            } => return self.fold_ternary(expr, condition, then.as_deref(), otherwise),
            ExprKind::Binary { op, left, right } => self.fold_binary(*op, left, right)?,
            ExprKind::Unary { op, operand } => self.fold_unary(*op, operand)?,
            ExprKind::Cast { kind, operand } => {
                if *kind == CastKind::Unset {
                    return self.fatal("The (unset) cast is no longer supported");
                }
                match self.fold(operand)? {
                    Some(value) => self.evaluate(|cx| Ok(cast(*kind, &value, cx))).flatten(),
                    None => None,
                }
            }
            ExprKind::Dim { base, index } => self.fold_dim(base, index.as_deref())?,
            ExprKind::Array { items, syntax } => self.fold_array(items, *syntax)?,
            ExprKind::Magic(magic) => self.magic(*magic, expr.line),
            ExprKind::Constant(name) => self.special_constant(name),
            ExprKind::ClassConstant { class, name } => self.fold_class_constant(class, name)?,
            ExprKind::ClassName(class) => self.fold_class_name(class)?,
            ExprKind::New { class, args } => {
                self.fold(class)?;
                self.fold_args(args)?;
                None
            }
            ExprKind::Property { object, name, .. } => {
                self.fold(object)?;
                self.fold_member(name)?;
                None
            }
            _ => None,
        };
        if let Some(value) = &value {
            self.folded.insert(key, Folded::Value(value.clone()));
        }
        Ok(value)
    }

    /// Records that `expr` became `chosen`, or `chosen`'s value.
    fn replace(&mut self, expr: &'a Expr, chosen: &'a Expr) -> Flow<Option<Value>> {
        let value = self.fold(chosen)?;
        let folded = value
            .clone()
            .map_or(Folded::Replaced(chosen), Folded::Value);
        self.folded.insert(ptr::from_ref(expr), folded);
        Ok(value)
    }

    fn fold_coalesce(
        &mut self,
        expr: &'a Expr,
        left: &'a Expr,
        right: &'a Expr,
    ) -> Flow<Option<Value>> {
        match self.fold(left)? {
            None => {
                self.fold(right)?;
                Ok(None)
            }
            Some(Value::Null) => self.replace(expr, right),
            Some(_) => self.replace(expr, left),
        }
    }

    fn fold_ternary(
        &mut self,
        expr: &'a Expr,
        condition: &'a Expr,
        then: Option<&'a Expr>,
        otherwise: &'a Expr,
    ) -> Flow<Option<Value>> {
        let Some(value) = self.fold(condition)? else {
            if let Some(then) = then {
                self.fold(then)?;
            }
            self.fold(otherwise)?;
            return Ok(None);
        };
        let chosen = if self.truthy(&value) {
            then.unwrap_or(condition)
        } else {
            otherwise
        };
        self.replace(expr, chosen)
    }

    fn fold_binary(
        &mut self,
        op: BinaryOp,
        left: &'a Expr,
        right: &'a Expr,
    ) -> Flow<Option<Value>> {
        if op == BinaryOp::Pipe {
            return Ok(None);
        }
        let left = self.fold(left)?;
        let right = self.fold(right)?;
        if let BinaryOp::And | BinaryOp::Or = op {
            let is_or = op == BinaryOp::Or;
            let Some(left) = left else {
                return Ok(None);
            };
            let left = self.truthy(&left);
            if left == is_or {
                return Ok(Some(Value::Bool(is_or)));
            }
            let Some(right) = right else {
                return Ok(None);
            };
            return Ok(Some(Value::Bool(self.truthy(&right))));
        }
        let (Some(left), Some(right)) = (left, right) else {
            return Ok(None);
        };
        Ok(self.fold_operator(op, &left, &right))
    }

    fn fold_operator(&mut self, op: BinaryOp, left: &Value, right: &Value) -> Option<Value> {
        if binary_produces_error(op, left, right) {
            return None;
        }
        self.evaluate(|cx| binary(op, left, right, cx).transpose())
            .flatten()
    }

    fn fold_unary(&mut self, op: UnaryOp, operand: &'a Expr) -> Flow<Option<Value>> {
        if op == UnaryOp::Silence {
            return Ok(None);
        }
        let Some(value) = self.fold(operand)? else {
            return Ok(None);
        };
        Ok(match op {
            UnaryOp::Plus => self.fold_operator(BinaryOp::Mul, &value, &Value::Int(1)),
            UnaryOp::Minus => self.fold_operator(BinaryOp::Mul, &value, &Value::Int(-1)),
            UnaryOp::Not => match value {
                Value::Float(_) => None,
                _ => Some(Value::Bool(!self.truthy(&value))),
            },
            UnaryOp::BitNot => match value {
                Value::Null | Value::Bool(_) => None,
                Value::String(_) => self.evaluate(|cx| ops::bit_not(&value, cx)),
                _ if !is_operand_long_compatible(&value) => None,
                _ => self.evaluate(|cx| ops::bit_not(&value, cx)),
            },
            UnaryOp::Silence => None,
        })
    }

    fn fold_dim(&mut self, base: &'a Expr, index: Option<&'a Expr>) -> Flow<Option<Value>> {
        let Some(index) = index else {
            return self.fatal("Cannot use [] for reading");
        };
        let container = self.fold(base)?;
        let dim = self.fold(index)?;
        let (Some(container), Some(dim)) = (container, dim) else {
            return Ok(None);
        };
        Ok(match (container, dim) {
            (Value::Array(array), Value::Int(key)) => {
                array.get(&ArrayKey::Int(key)).map(Slot::value)
            }
            (Value::Array(array), Value::String(key)) => {
                array.get(&ArrayKey::from_string(key)).map(Slot::value)
            }
            (Value::String(string), dim) => {
                let offset = match dim {
                    Value::Int(offset) => offset,
                    Value::String(offset) => match ops::numeric_prefix(offset.as_bytes()) {
                        Some(prefix) => match prefix.number {
                            Number::Int(offset) => offset,
                            Number::Float(_) => return Ok(None),
                        },
                        None => return Ok(None),
                    },
                    _ => return Ok(None),
                };
                usize::try_from(offset)
                    .ok()
                    .and_then(|offset| string.as_bytes().get(offset))
                    .map(|byte| text(&[*byte]))
            }
            _ => None,
        })
    }

    fn fold_array(
        &mut self,
        items: &'a [Option<ArrayItem>],
        syntax: ArraySyntax,
    ) -> Flow<Option<Value>> {
        if syntax == ArraySyntax::List {
            return self.fatal("Cannot use list() as standalone expression");
        }
        let mut constant = true;
        let mut last_line = None;
        for item in items {
            match item {
                None => {
                    if let Some(line) = last_line {
                        self.line = line;
                    }
                    return self.fatal("Cannot use empty array elements in arrays");
                }
                Some(ArrayItem::Value { key, value, by_ref }) => {
                    let known = self.fold(value)?.is_some();
                    let key_known = match key {
                        Some(key) => self.fold(key)?.is_some(),
                        None => true,
                    };
                    constant &= known && key_known && !by_ref;
                    last_line = Some(value.line);
                }
                Some(ArrayItem::Spread(value)) => {
                    constant &= self.fold(value)?.is_some();
                    last_line = Some(value.line);
                }
            }
        }
        if !constant {
            return Ok(None);
        }
        let mut array = PhpArray::with_capacity(items.len());
        for item in items.iter().flatten() {
            match item {
                ArrayItem::Spread(value) => match self.value_of(value) {
                    Some(Value::Array(spread)) => {
                        for (key, slot) in spread.iter() {
                            match key {
                                ArrayKey::String(_) => {
                                    array.assign(key.clone(), slot.value());
                                }
                                ArrayKey::Int(_) => {
                                    if array.append(slot.value()).is_err() {
                                        return Ok(None);
                                    }
                                }
                            }
                        }
                    }
                    Some(other) => {
                        return self.fatal(format!(
                            "Only arrays and Traversables can be unpacked, {} given",
                            other.value_name()
                        ));
                    }
                    None => return Ok(None),
                },
                ArrayItem::Value { key, value, .. } => {
                    let Some(value) = self.value_of(value) else {
                        return Ok(None);
                    };
                    let Some(key) = key else {
                        if array.append(value).is_err() {
                            return Ok(None);
                        }
                        continue;
                    };
                    let key = match self.value_of(key) {
                        Some(Value::Int(key)) => ArrayKey::Int(key),
                        Some(Value::String(key)) => ArrayKey::from_string(key),
                        Some(Value::Float(key)) if is_long_compatible(key) => {
                            ArrayKey::Int(quiet_int(&Value::Float(key)))
                        }
                        Some(Value::Bool(key)) => ArrayKey::Int(i64::from(key)),
                        Some(Value::Array(_)) => return self.fatal("Illegal offset type"),
                        _ => return Ok(None),
                    };
                    array.assign(key, value);
                }
            }
        }
        Ok(Some(Value::Array(array)))
    }

    fn fold_args(&mut self, args: &'a Args) -> Flow {
        if let Args::List(args) = args {
            for arg in args {
                if let Arg::Positional(value) | Arg::Named { value, .. } = arg {
                    self.fold(value)?;
                }
            }
        }
        Ok(())
    }

    fn fold_member(&mut self, member: &'a Member) -> Flow<Option<Value>> {
        match member {
            Member::Name(name) => Ok(Some(text(name))),
            Member::Expr(expr) => self.fold(expr),
        }
    }

    fn fold_class_constant(&mut self, class: &'a Expr, name: &'a Member) -> Flow<Option<Value>> {
        self.fold(class)?;
        if !matches!(self.fold_member(name)?, Some(Value::String(_))) {
            return Ok(None);
        }
        match self.value_of(class) {
            Some(Value::String(_)) | None => Ok(None),
            Some(_) => self.fatal("Illegal class name"),
        }
    }

    fn fold_class_name(&mut self, class: &'a Expr) -> Flow<Option<Value>> {
        if is_number(class) {
            return self.fatal("Illegal class name");
        }
        let fetch = fetch_of(class);
        if let Some(fetch) = fetch {
            self.ensure_valid_fetch(fetch)?;
        }
        Ok(match (fetch, &class.kind) {
            (Some(Fetch::SelfClass), _) if self.is_scope_known() => self
                .class
                .as_ref()
                .and_then(|scope| scope.name.as_deref())
                .map(text),
            (None, ExprKind::String(bytes)) => Some(text(bytes)),
            _ => None,
        })
    }

    /// Whether `self` and `parent` name a class known while compiling: not in closures, which can
    /// be rebound, nor in traits, nor at the top level of a file, which runs in its includer's scope.
    pub(super) fn is_scope_known(&self) -> bool {
        match (&self.frame.code, &self.class) {
            (Code::Closure(_), _) => false,
            (code, None) => !matches!(code, Code::File),
            (_, Some(class)) => !class.is_trait,
        }
    }

    pub(super) fn ensure_valid_fetch(&self, fetch: Fetch) -> Flow {
        if !self.is_scope_known() {
            return Ok(());
        }
        match &self.class {
            None => self.fatal(format!(
                "Cannot use \"{}\" when no class scope is active",
                fetch.keyword()
            )),
            Some(class) if fetch == Fetch::Parent && class.parent.is_none() => {
                self.fatal("Cannot use \"parent\" when current class scope has no parent")
            }
            Some(_) => Ok(()),
        }
    }

    /// `true`, `false` or `null`, which fold however they are cased, also unqualified in a
    /// namespace.
    pub(super) fn special_constant(&self, name: &Name) -> Option<Value> {
        let candidate = match name.kind {
            NameKind::Unqualified | NameKind::FullyQualified => true,
            NameKind::Relative => self.namespace.is_none(),
        };
        let bytes = name.bytes.as_slice();
        if !candidate {
            None
        } else if bytes.eq_ignore_ascii_case(b"true") {
            Some(Value::Bool(true))
        } else if bytes.eq_ignore_ascii_case(b"false") {
            Some(Value::Bool(false))
        } else if bytes.eq_ignore_ascii_case(b"null") {
            Some(Value::Null)
        } else {
            None
        }
    }

    fn magic(&self, magic: MagicConstant, line: u32) -> Option<Value> {
        let class = self.class.as_ref();
        let name: &[u8] = match magic {
            MagicConstant::Line => return Some(Value::Int(i64::from(line))),
            MagicConstant::File | MagicConstant::Dir => return None,
            MagicConstant::Function => match &self.frame.code {
                Code::File => b"",
                Code::Function(name) | Code::Method { name, .. } => name,
                Code::Closure(name) if name.complete => &name.text,
                Code::Closure(_) => return None,
            },
            MagicConstant::Method => match &self.frame.code {
                Code::File => b"",
                Code::Function(_) if class.is_some() => b"",
                Code::Function(name) => name,
                Code::Method {
                    class: Some(class),
                    name,
                } => {
                    let mut full = class.clone();
                    full.extend_from_slice(b"::");
                    full.extend_from_slice(name);
                    return Some(text(&full));
                }
                Code::Closure(name) if name.complete => &name.text,
                Code::Method { class: None, .. } | Code::Closure(_) => return None,
            },
            MagicConstant::Class => match class {
                None => b"",
                Some(class) if class.is_trait => return None,
                Some(class) => class.name.as_deref()?,
            },
            MagicConstant::Trait => match class {
                Some(class) if class.is_trait => class.name.as_deref()?,
                _ => b"",
            },
            MagicConstant::Namespace => self.namespace.as_deref().unwrap_or_default(),
            MagicConstant::Property => self.frame.property.as_deref().unwrap_or_default(),
        };
        Some(text(name))
    }

    fn invalid<T>(&self) -> Flow<T> {
        self.fatal("Constant expression contains invalid operations")
    }

    /// Checks that folded `expr` holds only what a constant expression may contain.
    fn validate(&mut self, expr: &'a Expr, allow_dynamic: bool) -> Flow {
        let expr = self.resolve(expr);
        if self.is_known(expr) {
            return Ok(());
        }
        match &expr.kind {
            ExprKind::Binary {
                op: BinaryOp::Pipe, ..
            }
            | ExprKind::Unary {
                op: UnaryOp::Silence,
                ..
            } => self.invalid(),
            ExprKind::Binary { left, right, .. } => {
                self.validate(left, allow_dynamic)?;
                self.validate(right, allow_dynamic)
            }
            ExprKind::Cast {
                kind: CastKind::Object,
                ..
            } if !allow_dynamic => self.fatal("Object casts are not supported in this context"),
            ExprKind::Unary { operand, .. } | ExprKind::Cast { operand, .. } => {
                self.validate(operand, allow_dynamic)
            }
            ExprKind::Ternary {
                condition,
                then,
                otherwise,
                ..
            } => {
                self.validate(condition, allow_dynamic)?;
                if let Some(then) = then {
                    self.validate(then, allow_dynamic)?;
                }
                self.validate(otherwise, allow_dynamic)
            }
            ExprKind::Dim { base, index } => {
                self.validate(base, allow_dynamic)?;
                index
                    .as_deref()
                    .map_or(Ok(()), |index| self.validate(index, allow_dynamic))
            }
            ExprKind::Array { items, .. } => {
                for item in items.iter().flatten() {
                    match item {
                        ArrayItem::Value { key, value, .. } => {
                            self.validate(value, allow_dynamic)?;
                            if let Some(key) = key {
                                self.validate(key, allow_dynamic)?;
                            }
                        }
                        ArrayItem::Spread(value) => self.validate(value, allow_dynamic)?,
                    }
                }
                Ok(())
            }
            ExprKind::Constant(_) => {
                self.line = expr.line;
                Ok(())
            }
            ExprKind::Magic(_) => Ok(()),
            ExprKind::Property { object, name, .. } => {
                self.validate(object, allow_dynamic)?;
                self.validate_member(name, allow_dynamic)
            }
            _ => self.validate_reference(expr, allow_dynamic),
        }
    }

    /// The class, `new`, closure and callable nodes of a constant expression.
    fn validate_reference(&mut self, expr: &'a Expr, allow_dynamic: bool) -> Flow {
        match &expr.kind {
            ExprKind::ClassConstant { class, name } => {
                if !self.is_known(class) {
                    return self.fatal(
                        "Dynamic class names are not allowed in compile-time class constant references",
                    );
                }
                if fetch_of(class) == Some(Fetch::Static) {
                    return self.fatal("\"static::\" is not allowed in compile-time constants");
                }
                self.validate_member(name, allow_dynamic)
            }
            ExprKind::ClassName(class) => {
                if !self.is_known(class) {
                    return self
                        .fatal("(expression)::class cannot be used in constant expressions");
                }
                if fetch_of(class) == Some(Fetch::Static) {
                    return self.fatal(
                        "static::class cannot be used for compile-time class name resolution",
                    );
                }
                Ok(())
            }
            ExprKind::New { class, args } => {
                if !allow_dynamic {
                    return self.fatal("New expressions are not supported in this context");
                }
                self.class_reference(class)?;
                let Args::List(args) = args else {
                    return self.fatal("Cannot create Closure for new expression");
                };
                self.constant_args(args, allow_dynamic)
            }
            ExprKind::NewAnonymous { .. } => {
                if allow_dynamic {
                    self.fatal("Cannot use anonymous class in constant expression")
                } else {
                    self.fatal("New expressions are not supported in this context")
                }
            }
            ExprKind::Closure(function) => {
                if !function.modifiers.contains(Modifiers::STATIC) {
                    return self.fatal("Closures in constant expressions must be static");
                }
                if !function.uses.is_empty() {
                    return self.fatal("Cannot use(...) variables in constant expression");
                }
                self.function(function, Code::Closure(self.closure_name(function.line)))
            }
            ExprKind::Call { callee, args } => {
                if *args != Args::CallableConvert {
                    return self.invalid();
                }
                match callee.kind {
                    ExprKind::Name(_) | ExprKind::String(_) => Ok(()),
                    ExprKind::Int(_) | ExprKind::Float(_) => self.fatal("Illegal function name"),
                    _ => self.fatal("Cannot use dynamic function name in constant expression"),
                }
            }
            ExprKind::StaticCall { class, name, args } => {
                if *args != Args::CallableConvert {
                    return self.invalid();
                }
                self.class_reference(class)?;
                match name {
                    Member::Expr(name) if is_number(name) => self.fatal("Illegal method name"),
                    Member::Name(_) => Ok(()),
                    Member::Expr(name) if matches!(name.kind, ExprKind::String(_)) => Ok(()),
                    Member::Expr(_) => {
                        self.fatal("Cannot use dynamic method name in constant expression")
                    }
                }
            }
            _ => self.invalid(),
        }
    }

    fn validate_member(&mut self, member: &'a Member, allow_dynamic: bool) -> Flow {
        match member {
            Member::Name(_) => Ok(()),
            Member::Expr(expr) => self.validate(expr, allow_dynamic),
        }
    }

    /// The class of `new` or of a first-class callable static call.
    fn class_reference(&self, class: &'a Expr) -> Flow {
        if !self.is_known(class) {
            return self.fatal("Cannot use dynamic class name in constant expression");
        }
        if !matches!(self.value_of(class), Some(Value::String(_)) | None) {
            return self.fatal("Illegal class name");
        }
        if fetch_of(class) == Some(Fetch::Static) {
            return self.fatal("\"static\" is not allowed in compile-time constants");
        }
        Ok(())
    }

    fn constant_args(&mut self, args: &'a [Arg], allow_dynamic: bool) -> Flow {
        let mut named = false;
        for arg in args {
            match arg {
                Arg::Unpack(_) => {
                    return self
                        .fatal("Argument unpacking in constant expressions is not supported");
                }
                Arg::Named { .. } => named = true,
                Arg::Positional(_) if named => {
                    return self.fatal("Cannot use positional argument after named argument");
                }
                Arg::Positional(_) => {}
            }
        }
        for arg in args {
            if let Arg::Positional(value) | Arg::Named { value, .. } = arg {
                self.validate(value, allow_dynamic)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use php_parser::ast::{Program, TopStatement};
    use php_parser::{ParseOptions, parse};

    use super::*;
    use crate::check::ClosureName;
    use crate::check::{ClassScope, Frame};

    fn program(source: &str) -> Program {
        parse(source.as_bytes(), ParseOptions::default())
            .unwrap()
            .result
            .unwrap()
    }

    fn show(value: &Value) -> String {
        match value {
            Value::Null => "null".to_owned(),
            Value::Bool(value) => value.to_string(),
            Value::Int(value) => value.to_string(),
            Value::Float(value) => {
                let mut out = b"float ".to_vec();
                php_value::write_float(&mut out, *value);
                String::from_utf8(out).unwrap()
            }
            Value::String(text) => format!("\"{}\"", text.as_bytes().escape_ascii()),
            Value::Array(array) => {
                let entries: Vec<String> = array
                    .iter()
                    .map(|(key, slot)| {
                        let key = match key {
                            ArrayKey::Int(key) => key.to_string(),
                            ArrayKey::String(key) => {
                                format!("\"{}\"", key.as_bytes().escape_ascii())
                            }
                        };
                        format!("{key} => {}", show(&slot.value()))
                    })
                    .collect();
                format!("[{}]", entries.join(", "))
            }
            Value::Object(_) => "object".to_owned(),
        }
    }

    /// The class, code and property a test folds in.
    #[derive(Default)]
    struct Context {
        namespace: Option<&'static str>,
        class: Option<ClassScope>,
        code: Code,
        property: Option<&'static str>,
    }

    fn class(name: Option<&str>, is_trait: bool, has_parent: bool) -> ClassScope {
        ClassScope {
            name: name.map(|name| name.as_bytes().to_vec()),
            is_trait,
            parent: has_parent.then(|| b"P".to_vec()),
        }
    }

    fn method(class: Option<&str>, name: &str) -> Code {
        Code::Method {
            class: class.map(|class| class.as_bytes().to_vec()),
            name: name.as_bytes().to_vec(),
        }
    }

    /// What `expr` folds to as the value of a top-level constant in `context`, with the warnings
    /// raised; `-` when it does not fold.
    fn folded_in(context: &Context, expr: &str) -> (String, Vec<String>) {
        let program = program(&format!("<?php const A = {expr};"));
        let TopStatement::Const { items, .. } = &program.statements[0] else {
            panic!("not a constant: {expr}");
        };
        let mut checker = Checker::new(b"");
        checker.namespace = context.namespace.map(|name| name.as_bytes().to_vec());
        checker.class.clone_from(&context.class);
        checker.frame = Frame::new(context.code.clone());
        checker.frame.property = context.property.map(|name| name.as_bytes().to_vec());
        let value = checker.fold(&items[0].value).unwrap();
        let warnings = checker
            .warnings
            .into_iter()
            .map(|warning| String::from_utf8(warning.message).unwrap())
            .collect();
        (
            value.as_ref().map_or_else(|| "-".to_owned(), show),
            warnings,
        )
    }

    fn folded(expr: &str) -> String {
        let (value, warnings) = folded_in(&Context::default(), expr);
        assert!(warnings.is_empty(), "{expr}: {warnings:?}");
        value
    }

    /// Every diagnostic of checking `source`, as `label: message @line`.
    fn diagnostics(source: &str) -> Vec<String> {
        let checked = Checker::run(&program(source), b"");
        checked
            .warnings
            .iter()
            .chain(&checked.fatal)
            .map(|diagnostic| {
                format!(
                    "{}: {} @{}",
                    diagnostic.severity.label(),
                    String::from_utf8_lossy(&diagnostic.message),
                    diagnostic.line
                )
            })
            .collect()
    }

    fn fatal(source: &str) -> String {
        let checked = Checker::run(&program(source), b"");
        let fatal = checked
            .fatal
            .unwrap_or_else(|| panic!("no error: {source}"));
        String::from_utf8(fatal.message).unwrap()
    }

    #[test]
    fn operators_fold_unless_they_would_throw_or_warn() {
        let cases = [
            ("1 + 2", "3"),
            ("'a' . 1.5", "\"a1.5\""),
            ("2 ** -1", "float 0.5"),
            ("7 % 3", "1"),
            ("6 / 4", "float 1.5"),
            ("1 << 3", "8"),
            ("'a' | 'b'", "\"c\""),
            ("[1] + [2, 3]", "[0 => 1, 1 => 3]"),
            ("1 <=> 2", "-1"),
            ("2 <=> 2", "0"),
            ("3 <=> 2", "1"),
            ("2 > 1", "true"),
            ("1 >= 2", "false"),
            ("1 == '1'", "true"),
            ("1 != 1", "false"),
            ("1 === 1.0", "false"),
            ("1 !== 1.0", "true"),
            ("1 < 2", "true"),
            ("2 <= 1", "false"),
            ("1 xor 1", "false"),
            ("true && false", "false"),
            ("false && A", "false"),
            ("true || A", "true"),
            ("false || 1", "true"),
            ("true and 1", "true"),
            ("A && true", "-"),
            ("true && A", "-"),
            ("[] . 'a'", "-"),
            ("[] - []", "-"),
            ("[] * 1", "-"),
            ("'a' + 1", "-"),
            ("1 + 'a'", "-"),
            ("1.5 | 1", "-"),
            ("'1.5' & 1", "-"),
            ("1e30 >> 1", "-"),
            ("1 / 0", "-"),
            ("1 / '0.0'", "-"),
            ("1 % 0", "-"),
            ("1.5 % 1", "-"),
            ("0 ** -1", "-"),
            ("1 << -1", "-"),
            ("1 |> A", "-"),
            ("A + 1", "-"),
        ];
        for (expr, value) in cases {
            assert_eq!(folded(expr), value, "{expr}");
        }
    }

    #[test]
    fn unary_operators_and_casts_fold_like_the_reference() {
        let cases = [
            ("-'1'", "-1"),
            ("+1.5", "float 1.5"),
            ("-[]", "-"),
            ("~5", "-6"),
            ("~'a'", "\"\\x9e\""),
            ("~1.5", "-"),
            ("~true", "-"),
            ("~null", "-"),
            ("~[]", "-"),
            ("!0", "true"),
            ("!1.0", "-"),
            ("!A", "-"),
            ("(int) 1.9", "1"),
            ("(int) '12abc'", "12"),
            ("(int) 1e20", "-"),
            ("(float) '1.5'", "float 1.5"),
            ("(string) true", "\"1\""),
            ("(string) 1.5", "-"),
            ("(string) []", "-"),
            ("(bool) 'a'", "true"),
            ("(array) 1", "[0 => 1]"),
            ("(object) 1", "-"),
            ("(int) A", "-"),
        ];
        for (expr, value) in cases {
            assert_eq!(folded(expr), value, "{expr}");
        }
    }

    #[test]
    fn selections_and_reads_fold_on_known_operands() {
        let cases = [
            ("null ?? 2", "2"),
            ("1 ?? 2", "1"),
            ("A ?? 2", "-"),
            ("1 ?: 2", "1"),
            ("0 ?: 2", "2"),
            ("0 ? 1 : 2", "2"),
            ("1 ? 1 : 2", "1"),
            ("A ? 1 : 2", "-"),
            ("A ?: 2", "-"),
            ("1 ? A : 2", "-"),
            ("[1, 2][1]", "2"),
            ("['a' => 1]['a']", "1"),
            ("['1' => 1]['1']", "1"),
            ("[1][2]", "-"),
            ("[1]['b']", "-"),
            ("[1][0.5]", "-"),
            ("'abc'[1]", "\"b\""),
            ("'abc'['1x']", "\"b\""),
            ("'abc'['1.0']", "-"),
            ("'abc'['x']", "-"),
            ("'abc'[5]", "-"),
            ("'abc'[-1]", "-"),
            ("'abc'[null]", "-"),
            ("null[0]", "-"),
            ("A[0]", "-"),
        ];
        for (expr, value) in cases {
            assert_eq!(folded(expr), value, "{expr}");
        }
    }

    #[test]
    fn array_literals_fold_with_php_keys() {
        let cases = [
            ("[]", "[]"),
            ("[...[1, 2], ...['a' => 3]]", "[0 => 1, 1 => 2, \"a\" => 3]"),
            (
                "[1.0 => 'a', true => 'b', false => 'c', '2' => 'd', 'x' => 'e', 5 => 'f', 'g']",
                "[1 => \"b\", 0 => \"c\", 2 => \"d\", \"x\" => \"e\", 5 => \"f\", 6 => \"g\"]",
            ),
            ("[1.5 => 1]", "-"),
            ("[null => 1]", "-"),
            ("[A => 1]", "-"),
            ("[...A]", "-"),
            ("[PHP_INT_MAX => 1, 2]", "-"),
            ("[9223372036854775807 => 1, 2]", "-"),
            ("[9223372036854775807 => 1, ...[2]]", "-"),
        ];
        for (expr, value) in cases {
            assert_eq!(folded(expr), value, "{expr}");
        }
    }

    #[test]
    fn only_special_constants_and_known_names_fold() {
        let cases = [
            ("TRUE", "true"),
            ("\\false", "false"),
            ("Null", "null"),
            ("namespace\\true", "true"),
            ("A\\true", "-"),
            ("PHP_INT_MAX", "-"),
            ("__LINE__", "1"),
            ("__FILE__", "-"),
            ("__DIR__", "-"),
            ("A::B", "-"),
            ("A::class", "-"),
            ("'A'::class", "\"A\""),
            ("A::{'B'}", "-"),
            ("A::{B}", "-"),
            ("new A(1, b: 2 + 3)", "-"),
            ("A->b", "-"),
        ];
        for (expr, value) in cases {
            assert_eq!(folded(expr), value, "{expr}");
        }
        let namespaced = Context {
            namespace: Some("N"),
            ..Context::default()
        };
        assert_eq!(folded_in(&namespaced, "namespace\\true").0, "-");
        assert_eq!(folded_in(&namespaced, "true").0, "true");
        assert_eq!(folded_in(&namespaced, "__NAMESPACE__").0, "\"N\"");
    }

    #[test]
    fn nan_coercions_are_reported_when_folding() {
        let nan = "(1e1000 - 1e1000)";
        let warning = "unexpected NAN value was coerced to bool".to_owned();
        let context = Context::default();
        for expr in [
            format!("{nan} ?: 1"),
            format!("{nan} ? 1 : 2"),
            format!("{nan} && 1"),
            format!("{nan} || 1"),
            format!("0 || {nan}"),
        ] {
            assert_eq!(
                folded_in(&context, &expr).1,
                std::slice::from_ref(&warning),
                "{expr}"
            );
        }
        assert_eq!(
            folded_in(&context, &format!("{nan} xor {nan}")).1,
            [warning.clone(), warning]
        );
        assert_eq!(
            folded_in(&context, &format!("{nan} . ''")).1,
            ["unexpected NAN value was coerced to string"]
        );
        assert_eq!(folded_in(&context, &format!("(bool) {nan}")).0, "-");
    }

    #[test]
    fn magic_constants_follow_the_code_being_compiled() {
        let magic = "[__FUNCTION__, __METHOD__, __CLASS__, __TRAIT__, __PROPERTY__]";
        let fold = |context: Context, expr: &str| folded_in(&context, expr).0;
        assert_eq!(
            fold(Context::default(), magic),
            "[0 => \"\", 1 => \"\", 2 => \"\", 3 => \"\", 4 => \"\"]"
        );
        assert_eq!(
            fold(
                Context {
                    class: Some(class(Some("N\\C"), false, false)),
                    property: Some("p"),
                    ..Context::default()
                },
                magic
            ),
            "[0 => \"\", 1 => \"\", 2 => \"N\\\\C\", 3 => \"\", 4 => \"p\"]"
        );
        assert_eq!(
            fold(
                Context {
                    class: Some(class(Some("N\\C"), false, false)),
                    code: method(Some("N\\C"), "m"),
                    ..Context::default()
                },
                magic
            ),
            "[0 => \"m\", 1 => \"N\\\\C::m\", 2 => \"N\\\\C\", 3 => \"\", 4 => \"\"]"
        );
        let function = || Code::Function(b"f".to_vec());
        assert_eq!(
            fold(
                Context {
                    class: Some(class(Some("C"), false, false)),
                    code: function(),
                    ..Context::default()
                },
                "__FUNCTION__ . '|' . __METHOD__"
            ),
            "\"f|\""
        );
        assert_eq!(
            fold(
                Context {
                    code: function(),
                    ..Context::default()
                },
                "__FUNCTION__ . '|' . __METHOD__"
            ),
            "\"f|f\""
        );
        let in_trait = || Context {
            class: Some(class(Some("T"), true, false)),
            ..Context::default()
        };
        assert_eq!(fold(in_trait(), "__TRAIT__"), "\"T\"");
        assert_eq!(fold(in_trait(), "__CLASS__"), "-");
        let anonymous = || Context {
            class: Some(class(None, false, false)),
            code: method(None, "m"),
            ..Context::default()
        };
        assert_eq!(fold(anonymous(), "__CLASS__"), "-");
        assert_eq!(fold(anonymous(), "__METHOD__"), "-");
        let closure = || Context {
            code: Code::Closure(ClosureName {
                text: b"{closure:f():1}".to_vec(),
                complete: true,
            }),
            ..Context::default()
        };
        assert_eq!(fold(closure(), "__FUNCTION__"), "\"{closure:f():1}\"");
        assert_eq!(fold(closure(), "__METHOD__"), "\"{closure:f():1}\"");
        let unnamed = || Context {
            code: Code::Closure(ClosureName {
                text: b"{closure:class@anonymous".to_vec(),
                complete: false,
            }),
            ..Context::default()
        };
        assert_eq!(fold(unnamed(), "__FUNCTION__"), "-");
        assert_eq!(fold(unnamed(), "__METHOD__"), "-");
        assert_eq!(fold(closure(), "__PROPERTY__"), "\"\"");
    }

    #[test]
    fn self_folds_only_where_the_scope_is_known() {
        let fold = |context: Context, expr: &str| folded_in(&context, expr).0;
        let in_class = || Context {
            class: Some(class(Some("C"), false, true)),
            ..Context::default()
        };
        assert_eq!(fold(Context::default(), "self::class"), "-");
        assert_eq!(fold(in_class(), "self::class"), "\"C\"");
        assert_eq!(fold(in_class(), "parent::class"), "-");
        assert_eq!(fold(in_class(), "static::class"), "-");
        let in_closure = Context {
            code: Code::Closure(ClosureName {
                text: b"{closure:class@anonymous".to_vec(),
                complete: false,
            }),
            ..in_class()
        };
        assert_eq!(fold(in_closure, "self::class"), "-");
    }

    #[test]
    fn folding_errors_stop_compilation() {
        let cases = [
            (
                "const A = [...1];",
                "Only arrays and Traversables can be unpacked, int given",
            ),
            (
                "const A = [...true];",
                "Only arrays and Traversables can be unpacked, true given",
            ),
            ("const A = [[] => 1];", "Illegal offset type"),
            (
                "const A = [1, , 2];",
                "Cannot use empty array elements in arrays",
            ),
            ("const A = (1)::class;", "Illegal class name"),
            ("const A = (1)::B;", "Illegal class name"),
            ("const A = (true)::B;", "Illegal class name"),
            (
                "function f($a = static::class) {}",
                "Cannot use \"static\" when no class scope is active",
            ),
            (
                "class C { const A = parent::class; }",
                "Cannot use \"parent\" when current class scope has no parent",
            ),
            ("$a = [$b[]];", "Cannot use [] for reading"),
            ("$a = [&$b[], , 1];", "Cannot use [] for reading"),
        ];
        for (source, message) in cases {
            assert_eq!(fatal(&format!("<?php {source}")), message, "{source}");
        }
        assert!(diagnostics("<?php trait T { const A = parent::class; }").is_empty());
    }

    #[test]
    fn invalid_operations_are_rejected_in_any_position() {
        for source in [
            "const A = $a;",
            "const A = @1;",
            "const A = 1 |> A;",
            "const A = strlen(1);",
            "const A = A::f(1);",
            "const A = static fn() => 1;",
            "const A = A::{$b};",
            "const A = $a->b;",
            "const A = new A($a);",
            "const A = [$a];",
            "const A = [1 => $a];",
            "const A = [...$a];",
            "const A = A[$a];",
            "const A = $a ? 1 : 2;",
            "const A = A ?: $a;",
            "const A = -$a;",
            "const A = 1 + $a;",
        ] {
            assert_eq!(
                fatal(&format!("<?php {source}")),
                "Constant expression contains invalid operations",
                "{source}"
            );
        }
    }

    #[test]
    fn constant_expressions_reject_what_they_cannot_hold() {
        let cases = [
            (
                "class C { const A = new D; }",
                "New expressions are not supported in this context",
            ),
            (
                "class C { public $a = new class {}; }",
                "New expressions are not supported in this context",
            ),
            (
                "const A = new class {};",
                "Cannot use anonymous class in constant expression",
            ),
            (
                "const A = new static;",
                "\"static\" is not allowed in compile-time constants",
            ),
            (
                "const A = new $a;",
                "Cannot use dynamic class name in constant expression",
            ),
            (
                "const A = new A(...);",
                "Cannot create Closure for new expression",
            ),
            (
                "const A = new A(...[]);",
                "Argument unpacking in constant expressions is not supported",
            ),
            (
                "const A = new A(a: 1, 2);",
                "Cannot use positional argument after named argument",
            ),
            (
                "const A = static::B;",
                "\"static::\" is not allowed in compile-time constants",
            ),
            (
                "const A = $a::B;",
                "Dynamic class names are not allowed in compile-time class constant references",
            ),
            (
                "const A = $a::class;",
                "(expression)::class cannot be used in constant expressions",
            ),
            (
                "const A = static::class;",
                "static::class cannot be used for compile-time class name resolution",
            ),
            (
                "class C { const A = (object) []; }",
                "Object casts are not supported in this context",
            ),
            (
                "const A = function () {};",
                "Closures in constant expressions must be static",
            ),
            (
                "const A = static function () use ($a) {};",
                "Cannot use(...) variables in constant expression",
            ),
            (
                "const A = static function () { $this = 1; };",
                "Cannot re-assign $this",
            ),
            (
                "const A = $f(...);",
                "Cannot use dynamic function name in constant expression",
            ),
            (
                "const A = A::$f(...);",
                "Cannot use dynamic method name in constant expression",
            ),
            (
                "const A = static::f(...);",
                "\"static\" is not allowed in compile-time constants",
            ),
            (
                "const A = (unset) 1;",
                "The (unset) cast is no longer supported",
            ),
        ];
        for (source, message) in cases {
            assert_eq!(fatal(&format!("<?php {source}")), message, "{source}");
        }
    }

    #[test]
    fn constant_expressions_accept_what_they_may_hold() {
        for source in [
            "const A = (object) [];",
            "const A = static function () {};",
            "const A = strlen(...);",
            "const A = 'strlen'(...);",
            "const A = A::f(...);",
            "const A = A::{'f'}(...);",
            "const A = new A(1, b: [2]);",
            "const A = A->b?->c;",
            "const A = A::{'B'};",
            "const A = A::B[0] ?? (int) C;",
            "const A = 1 ? 2 : 3 ? 4 : 5;",
            "class C extends D { const A = self::class . parent::class; }",
            "enum E: int { case A = 1 + 1; }",
        ] {
            assert_eq!(
                diagnostics(&format!("<?php {source}")),
                Vec::<String>::new(),
                "{source}"
            );
        }
    }

    #[test]
    fn attribute_arguments_are_constant_expressions() {
        let cases = [
            (
                "#[A(...)] function f() {}",
                "Cannot create Closure as attribute argument",
            ),
            (
                "#[A(...[])] class C {}",
                "Cannot use unpacking in attribute argument list",
            ),
            (
                "#[A(a: 1, a: 2)] const B = 1;",
                "Duplicate named parameter $a",
            ),
            (
                "#[A(a: 1, 2)] class C {}",
                "Cannot use positional argument after named argument",
            ),
            (
                "#[A] const B = 1, C = 2;",
                "Cannot apply attributes to multiple constants at once",
            ),
            (
                "#[A($a)] function f() {}",
                "Constant expression contains invalid operations",
            ),
            (
                "function f(#[A($a)] $b) {}",
                "Constant expression contains invalid operations",
            ),
            (
                "class C { #[A($a)] const B = 1; }",
                "Constant expression contains invalid operations",
            ),
            (
                "class C { #[A($a)] public $b; }",
                "Constant expression contains invalid operations",
            ),
            (
                "enum E { #[A($a)] case B; }",
                "Constant expression contains invalid operations",
            ),
            ("const TRUE = 1;", "Cannot redeclare constant 'TRUE'"),
        ];
        for (source, message) in cases {
            assert_eq!(fatal(&format!("<?php {source}")), message, "{source}");
        }
        assert!(diagnostics("<?php #[A(new B, c: [1])] function f() {}").is_empty());
    }

    #[test]
    fn contexts_carry_names_and_lines_into_folding() {
        assert_eq!(
            diagnostics("<?php\nnamespace N;\nconst A = [...__NAMESPACE__];"),
            ["Fatal error: Only arrays and Traversables can be unpacked, string given @3"]
        );
        assert_eq!(
            diagnostics("<?php namespace N { } const A = [...__NAMESPACE__];"),
            ["Fatal error: Only arrays and Traversables can be unpacked, string given @1"]
        );
        assert_eq!(
            diagnostics("<?php class C { public $p = [...__PROPERTY__]; }"),
            ["Fatal error: Only arrays and Traversables can be unpacked, string given @1"]
        );
        assert_eq!(
            diagnostics("<?php class C { public $p { get => [...__PROPERTY__]; } }"),
            ["Fatal error: Only arrays and Traversables can be unpacked, string given @1"]
        );
        assert_eq!(
            diagnostics("<?php\n$a = [$b,\n(1e1000 - 1e1000) xor 1];"),
            ["Warning: unexpected NAN value was coerced to bool @2"]
        );
        assert_eq!(
            diagnostics("<?php\nconst A = [\n1,\n,\n2];"),
            ["Fatal error: Cannot use empty array elements in arrays @3"]
        );
        assert_eq!(
            diagnostics("<?php $a = [1 ? $b : $this = 1];"),
            Vec::<String>::new()
        );
    }
}
