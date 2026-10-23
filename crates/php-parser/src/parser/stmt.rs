//! Statements: top-level declarations, block statements and control flow.

use super::expr::starts_variable;
use super::{
    Parser, is_ampersand, is_identifier, starts_expression, starts_inner_statement,
    starts_top_statement,
};
use crate::ast::{
    ArraySyntax, AttributeGroup, Case, Catch, ConstItem, Expr, ExprKind, For, Foreach, If, Stmt,
    StmtKind, TopStatement, Try, UseItem, UseKind,
};
use crate::describe::END;
use crate::error::{Severity, Warning};
use crate::literal;
use crate::stream::PResult;
use php_lexer::TokenKind as K;

const SEMICOLON: K = K::Char(b';');
const COMMA: K = K::Char(b',');
const OPEN_PAREN: K = K::Char(b'(');
const CLOSE_PAREN: K = K::Char(b')');
const OPEN_BRACE: K = K::Char(b'{');
const CLOSE_BRACE: K = K::Char(b'}');
const COLON: K = K::Char(b':');
/// The names a `use` item may start with outside a group.
const QUALIFIED: &[K] = &[K::Identifier, K::QualifiedName, K::FullyQualifiedName];
/// The names a `use` item may start with inside a group.
const UNQUALIFIED: &[K] = &[K::Identifier, K::QualifiedName];

/// One statement, or several written as one (`echo a, b;`).
fn one_or_block(mut list: Vec<Stmt>) -> StmtKind {
    if list.len() == 1
        && let Some(only) = list.pop()
    {
        return only.kind;
    }
    StmtKind::Block(list)
}

impl Parser<'_> {
    /// Top-level statements up to `close` (`None` for the end of the file).
    pub(super) fn top_statements(&mut self, close: Option<K>) -> PResult<Vec<TopStatement>> {
        let mut statements = Vec::new();
        loop {
            let kind = self.peek()?;
            if Some(kind) == close || (close.is_none() && kind == END) {
                return Ok(statements);
            }
            if !starts_top_statement(kind) {
                return self.unexpected(if close.is_none() { &[END] } else { &[] });
            }
            if kind == K::HaltCompiler {
                statements.push(self.halt_compiler()?);
                continue;
            }
            statements.push(self.top_statement()?);
        }
    }

    /// `__halt_compiler ( ) ;` at the top level; reading stops after it.
    fn halt_compiler(&mut self) -> PResult<TopStatement> {
        let line = self.bump()?.line;
        self.expect(OPEN_PAREN)?;
        self.expect(CLOSE_PAREN)?;
        let end = self.expect(SEMICOLON)?.end;
        self.stream.halt();
        self.halt_offset = Some(end);
        Ok(TopStatement::HaltCompiler { line })
    }

    fn top_statement(&mut self) -> PResult<TopStatement> {
        match self.peek()? {
            K::Namespace => self.namespace(),
            K::Use => self.use_statement(),
            K::Const => self.const_statement(Vec::new()),
            K::Attribute => {
                let attributes = self.attributes()?;
                if self.at(K::Const)? {
                    return self.const_statement(attributes);
                }
                self.attributed_statement(attributes)
                    .map(TopStatement::Statement)
            }
            _ => self.inner_statement().map(TopStatement::Statement),
        }
    }

    fn namespace(&mut self) -> PResult<TopStatement> {
        let line = self.bump()?.line;
        let kind = self.peek()?;
        let name = if kind == OPEN_BRACE {
            None
        } else if is_identifier(kind) || kind == K::QualifiedName {
            let tok = self.bump()?;
            Some(if kind == K::QualifiedName {
                self.text(&tok).to_vec()
            } else {
                self.identifier_bytes(&tok)?
            })
        } else {
            return self.unexpected(&[OPEN_BRACE]);
        };
        if name.is_some() && self.eat(SEMICOLON)?.is_some() {
            return Ok(TopStatement::Namespace {
                name,
                body: None,
                line,
            });
        }
        self.expect(OPEN_BRACE)?;
        let body = self.top_statements(Some(CLOSE_BRACE))?;
        self.bump()?;
        Ok(TopStatement::Namespace {
            name,
            body: Some(body),
            line,
        })
    }

    fn use_kind(&mut self) -> PResult<Option<UseKind>> {
        Ok(match self.peek()? {
            K::Function => {
                self.bump()?;
                Some(UseKind::Function)
            }
            K::Const => {
                self.bump()?;
                Some(UseKind::Constant)
            }
            _ => None,
        })
    }

    fn use_statement(&mut self) -> PResult<TopStatement> {
        let line = self.bump()?.line;
        let declared = self.use_kind()?;
        let kind = declared.unwrap_or(UseKind::Class);
        let first = self.use_name(true, if declared.is_some() { QUALIFIED } else { &[] })?;
        if self.eat(K::NsSeparator)?.is_some() {
            self.expect(OPEN_BRACE)?;
            let mut items = Vec::new();
            loop {
                let item_kind = if declared.is_none() {
                    self.use_kind()?
                } else {
                    None
                };
                let expected: &[K] = if item_kind.is_some() {
                    UNQUALIFIED
                } else if !items.is_empty() {
                    &[CLOSE_BRACE]
                } else if declared.is_some() {
                    UNQUALIFIED
                } else {
                    &[K::Identifier, K::QualifiedName, K::Function, K::Const]
                };
                let name = self.use_name(false, expected)?;
                let alias = self.use_alias()?;
                items.push(UseItem {
                    name,
                    alias,
                    kind: item_kind,
                });
                if self.eat(COMMA)?.is_none() || self.at(CLOSE_BRACE)? {
                    break;
                }
            }
            self.expect(CLOSE_BRACE)?;
            self.expect(SEMICOLON)?;
            return Ok(TopStatement::GroupUse {
                kind,
                prefix: first,
                items,
                line,
            });
        }
        let alias = self.use_alias()?;
        let mut items = vec![UseItem {
            name: first,
            alias,
            kind: None,
        }];
        while self.eat(COMMA)?.is_some() {
            let name = self.use_name(true, QUALIFIED)?;
            let alias = self.use_alias()?;
            items.push(UseItem {
                name,
                alias,
                kind: None,
            });
        }
        self.expect_in(SEMICOLON, &[COMMA, SEMICOLON])?;
        Ok(TopStatement::Use { kind, items, line })
    }

    /// A name in a `use` statement; `qualified` allows a leading `\`, and `expected` lists the tokens
    /// the reference names when the name is missing.
    fn use_name(&mut self, qualified: bool, expected: &[K]) -> PResult<Vec<u8>> {
        let kind = self.peek()?;
        let allowed = matches!(kind, K::Identifier | K::QualifiedName)
            || (qualified && kind == K::FullyQualifiedName);
        if !allowed {
            return self.unexpected(expected);
        }
        let tok = self.bump()?;
        Ok(literal::name(kind, self.text(&tok)).bytes)
    }

    fn use_alias(&mut self) -> PResult<Option<Vec<u8>>> {
        if self.eat(K::As)?.is_none() {
            return Ok(None);
        }
        let tok = self.expect(K::Identifier)?;
        Ok(Some(self.text(&tok).to_vec()))
    }

    fn const_statement(&mut self, attributes: Vec<AttributeGroup>) -> PResult<TopStatement> {
        let line = self.bump()?.line;
        let mut items = Vec::new();
        loop {
            let tok = self.expect(K::Identifier)?;
            let name = self.text(&tok).to_vec();
            self.expect(K::Char(b'='))?;
            let value = self.expr()?;
            items.push(ConstItem { name, value });
            if self.eat(COMMA)?.is_none() {
                break;
            }
        }
        self.expect_in(SEMICOLON, &[COMMA, SEMICOLON])?;
        Ok(TopStatement::Const {
            items,
            attributes,
            line,
        })
    }

    /// Statements up to one of `terminators`, which is left unread.
    pub(super) fn inner_statements(&mut self, terminators: &[K]) -> PResult<Vec<Stmt>> {
        let mut list = Vec::new();
        loop {
            let kind = self.peek()?;
            if terminators.contains(&kind) {
                return Ok(list);
            }
            if !starts_inner_statement(kind) {
                return self.unexpected(&[]);
            }
            list.push(self.inner_statement()?);
        }
    }

    /// Statements up to and including `}`.
    pub(super) fn block_body(&mut self) -> PResult<Vec<Stmt>> {
        let body = self.inner_statements(&[CLOSE_BRACE])?;
        self.bump()?;
        Ok(body)
    }

    fn inner_statement(&mut self) -> PResult<Stmt> {
        let kind = self.peek()?;
        match kind {
            K::Function => self.function_statement(Vec::new()),
            K::Abstract | K::Final | K::Class | K::Trait | K::Interface | K::Enum => {
                self.class_declaration(Vec::new())
            }
            K::Readonly if self.peek_second()? != OPEN_PAREN => self.class_declaration(Vec::new()),
            K::Attribute => {
                let attributes = self.attributes()?;
                self.attributed_statement(attributes)
            }
            K::HaltCompiler => {
                self.bump()?;
                self.expect(OPEN_PAREN)?;
                self.expect(CLOSE_PAREN)?;
                self.expect(SEMICOLON)?;
                self.compile_error("__HALT_COMPILER() can only be used from the outermost scope")
            }
            _ => self.statement(),
        }
    }

    /// What may follow attributes at statement level: a declaration or an attributed closure.
    fn attributed_statement(&mut self, attributes: Vec<AttributeGroup>) -> PResult<Stmt> {
        match self.peek()? {
            K::Function => self.function_statement(attributes),
            K::Abstract | K::Final | K::Readonly | K::Class | K::Trait | K::Interface | K::Enum => {
                self.class_declaration(attributes)
            }
            K::Fn => {
                let line = self.stream.peek()?.line;
                let closure = self.closure(attributes, false)?;
                self.expression_statement(closure, line)
            }
            K::Static => {
                let line = self.bump()?.line;
                if !matches!(self.peek()?, K::Function | K::Fn) {
                    return self.unexpected(&[K::Function, K::Fn]);
                }
                let closure = self.closure(attributes, true)?;
                self.expression_statement(closure, line)
            }
            _ => self.unexpected(&[]),
        }
    }

    /// An expression statement whose first operand has been parsed.
    pub(super) fn expression_statement(&mut self, first: Expr, line: u32) -> PResult<Stmt> {
        let value = self.binary_loop(first, 0)?;
        self.expect_in(SEMICOLON, &[])?;
        Ok(Stmt {
            kind: StmtKind::Expr(value),
            line,
        })
    }

    /// The `statement` rule: what may be the body of `if`, a loop or `declare`.
    pub(super) fn statement(&mut self) -> PResult<Stmt> {
        self.nested(Self::unnested_statement)
    }

    fn unnested_statement(&mut self) -> PResult<Stmt> {
        let kind = self.peek()?;
        let line = self.stream.peek()?.line;
        let kind = match kind {
            OPEN_BRACE => {
                self.bump()?;
                StmtKind::Block(self.block_body()?)
            }
            K::If => self.if_statement()?,
            K::While => self.while_statement()?,
            K::Do => self.do_while_statement()?,
            K::For => self.for_statement()?,
            K::Foreach => self.foreach_statement()?,
            K::Switch => self.switch_statement()?,
            K::Break | K::Continue | K::Return => {
                self.bump()?;
                let value = if starts_expression(self.peek()?) {
                    Some(self.expr()?)
                } else {
                    None
                };
                self.expect(SEMICOLON)?;
                match kind {
                    K::Break => StmtKind::Break(value),
                    K::Continue => StmtKind::Continue(value),
                    _ => StmtKind::Return(value),
                }
            }
            K::Global => self.global_statement()?,
            K::Static if self.peek_second()? == K::Variable => self.static_statement()?,
            K::Echo => self.echo_statement()?,
            K::InlineHtml => {
                let tok = self.bump()?;
                StmtKind::Echo(Expr {
                    kind: ExprKind::String(self.text(&tok).to_vec()),
                    line,
                })
            }
            K::Unset => self.unset_statement()?,
            K::Declare => self.declare_statement()?,
            SEMICOLON => {
                self.bump()?;
                StmtKind::Nop
            }
            K::Try => self.try_statement()?,
            K::Goto => {
                self.bump()?;
                let label = self.expect(K::Identifier)?;
                let label = self.text(&label).to_vec();
                self.expect(SEMICOLON)?;
                StmtKind::Goto(label)
            }
            K::Identifier if self.peek_second()? == COLON => {
                let label = self.bump()?;
                self.bump()?;
                StmtKind::Label(self.text(&label).to_vec())
            }
            K::VoidCast => {
                self.bump()?;
                let operand = self.expr()?;
                self.expect_in(SEMICOLON, &[])?;
                StmtKind::Expr(Expr {
                    kind: ExprKind::VoidCast(Box::new(operand)),
                    line,
                })
            }
            kind if starts_expression(kind) => {
                let value = self.expr()?;
                self.expect_in(SEMICOLON, &[])?;
                StmtKind::Expr(value)
            }
            _ => return self.unexpected(&[]),
        };
        Ok(Stmt { kind, line })
    }

    /// `( expr )` after `if`, `while` and the like.
    fn condition(&mut self) -> PResult<Expr> {
        self.expect(OPEN_PAREN)?;
        let condition = self.expr()?;
        self.expect_in(CLOSE_PAREN, &[])?;
        Ok(condition)
    }

    /// The statements of an alternative-syntax body, as one block.
    fn alternative_body(&mut self, terminators: &[K]) -> PResult<Stmt> {
        let line = self.stream.peek()?.line;
        let body = self.inner_statements(terminators)?;
        Ok(Stmt {
            kind: StmtKind::Block(body),
            line,
        })
    }

    /// `endwhile;` and the like.
    fn alternative_end(&mut self, keyword: K) -> PResult<()> {
        self.expect_in(keyword, &[])?;
        self.expect(SEMICOLON)?;
        Ok(())
    }

    fn if_statement(&mut self) -> PResult<StmtKind> {
        self.bump()?;
        let condition = self.condition()?;
        if self.eat(COLON)?.is_some() {
            return self.alternative_if(condition);
        }
        let mut branches = vec![(condition, self.statement()?)];
        while self.eat(K::Elseif)?.is_some() {
            let condition = self.condition()?;
            branches.push((condition, self.statement()?));
        }
        let otherwise = if self.eat(K::Else)?.is_some() {
            Some(self.statement()?)
        } else {
            None
        };
        Ok(StmtKind::If(Box::new(If {
            branches,
            otherwise,
        })))
    }

    fn alternative_if(&mut self, condition: Expr) -> PResult<StmtKind> {
        const ENDS: [K; 3] = [K::Elseif, K::Else, K::Endif];
        let mut branches = vec![(condition, self.alternative_body(&ENDS)?)];
        let mut otherwise = None;
        loop {
            match self.peek()? {
                K::Elseif => {
                    self.bump()?;
                    let condition = self.condition()?;
                    self.expect(COLON)?;
                    branches.push((condition, self.alternative_body(&ENDS)?));
                }
                K::Else => {
                    self.bump()?;
                    self.expect(COLON)?;
                    otherwise = Some(self.alternative_body(&[K::Endif])?);
                    break;
                }
                _ => break,
            }
        }
        self.alternative_end(K::Endif)?;
        Ok(StmtKind::If(Box::new(If {
            branches,
            otherwise,
        })))
    }

    /// The body of `while`, `for`, `foreach` or `declare`: a statement or `: … end;`.
    fn loop_body(&mut self, end: K) -> PResult<Stmt> {
        if self.eat(COLON)?.is_none() {
            return self.statement();
        }
        let body = self.alternative_body(&[end])?;
        self.alternative_end(end)?;
        Ok(body)
    }

    fn while_statement(&mut self) -> PResult<StmtKind> {
        self.bump()?;
        let condition = self.condition()?;
        let body = self.loop_body(K::Endwhile)?;
        Ok(StmtKind::While {
            condition,
            body: Box::new(body),
        })
    }

    fn do_while_statement(&mut self) -> PResult<StmtKind> {
        self.bump()?;
        let body = self.statement()?;
        self.expect(K::While)?;
        let condition = self.condition()?;
        self.expect(SEMICOLON)?;
        Ok(StmtKind::DoWhile {
            body: Box::new(body),
            condition,
        })
    }

    /// The comma-separated expressions of one `for` clause.
    /// The expressions of one `for` clause; in the `condition` a `(void)` cast cannot come last.
    fn for_clause(&mut self, condition: bool) -> PResult<Vec<Expr>> {
        let mut list = Vec::new();
        let kind = self.peek()?;
        if !starts_expression(kind) && kind != K::VoidCast {
            return Ok(list);
        }
        loop {
            let item = if self.at(K::VoidCast)? {
                let line = self.bump()?.line;
                let operand = self.expr()?;
                if condition && !self.at(COMMA)? {
                    return self.unexpected(&[COMMA]);
                }
                Expr {
                    kind: ExprKind::VoidCast(Box::new(operand)),
                    line,
                }
            } else {
                self.expr()?
            };
            list.push(item);
            if self.eat(COMMA)?.is_none() {
                return Ok(list);
            }
        }
    }

    fn for_statement(&mut self) -> PResult<StmtKind> {
        self.bump()?;
        self.expect(OPEN_PAREN)?;
        let init = self.for_clause(false)?;
        self.expect(SEMICOLON)?;
        let condition = self.for_clause(true)?;
        self.expect(SEMICOLON)?;
        let step = self.for_clause(false)?;
        self.expect(CLOSE_PAREN)?;
        let body = self.loop_body(K::Endfor)?;
        Ok(StmtKind::For(Box::new(For {
            init,
            condition,
            step,
            body,
        })))
    }

    /// A `foreach` key or value target followed by one of `followers`.
    fn foreach_target(&mut self, followers: &[K]) -> PResult<Expr> {
        let kind = self.peek()?;
        let line = self.stream.peek()?.line;
        if is_ampersand(kind) {
            self.bump()?;
            let target = self.variable_followed_by(followers)?;
            return Ok(Expr {
                kind: ExprKind::Reference(Box::new(target)),
                line,
            });
        }
        let (close, syntax) = match kind {
            K::List => (b')', ArraySyntax::List),
            K::Char(b'[') => (b']', ArraySyntax::Short),
            _ => return self.variable_followed_by(followers),
        };
        self.bump()?;
        if syntax == ArraySyntax::List {
            self.expect(OPEN_PAREN)?;
        }
        let items = self.array_items(close)?;
        let target = Expr {
            kind: ExprKind::Array { items, syntax },
            line,
        };
        if !followers.contains(&self.peek()?) {
            return self.unexpected(followers);
        }
        Ok(target)
    }

    fn foreach_statement(&mut self) -> PResult<StmtKind> {
        self.bump()?;
        self.expect(OPEN_PAREN)?;
        let subject = self.expr()?;
        self.expect_in(K::As, &[])?;
        let first = self.foreach_target(&[CLOSE_PAREN, K::DoubleArrow])?;
        let (key, value) = if self.eat(K::DoubleArrow)?.is_some() {
            (Some(first), self.foreach_target(&[CLOSE_PAREN])?)
        } else {
            (None, first)
        };
        self.expect(CLOSE_PAREN)?;
        let body = self.loop_body(K::Endforeach)?;
        Ok(StmtKind::Foreach(Box::new(Foreach {
            subject,
            key,
            value,
            body,
        })))
    }

    fn switch_statement(&mut self) -> PResult<StmtKind> {
        self.bump()?;
        let subject = self.condition()?;
        let close = match self.peek()? {
            OPEN_BRACE => CLOSE_BRACE,
            COLON => K::Endswitch,
            _ => return self.unexpected(&[COLON, OPEN_BRACE]),
        };
        self.bump()?;
        self.eat(SEMICOLON)?;
        let mut cases = Vec::new();
        loop {
            let condition = match self.peek()? {
                K::Case => {
                    self.bump()?;
                    Some(self.expr()?)
                }
                K::Default => {
                    self.bump()?;
                    None
                }
                kind if kind == close => break,
                _ => return self.unexpected(&[K::Case, K::Default, close]),
            };
            if !matches!(self.peek()?, COLON | SEMICOLON) {
                return self.unexpected(if condition.is_some() {
                    &[]
                } else {
                    &[COLON, SEMICOLON]
                });
            }
            self.bump()?;
            let body = self.inner_statements(&[K::Case, K::Default, close])?;
            cases.push(Case { condition, body });
        }
        self.bump()?;
        if close == K::Endswitch {
            self.expect(SEMICOLON)?;
        }
        Ok(StmtKind::Switch { subject, cases })
    }

    fn global_statement(&mut self) -> PResult<StmtKind> {
        self.bump()?;
        let mut list = Vec::new();
        loop {
            let var_line = self.stream.peek()?.line;
            let name = self.simple_variable()?;
            list.push(Stmt {
                kind: StmtKind::Global(Expr {
                    kind: ExprKind::Variable(name),
                    line: var_line,
                }),
                line: var_line,
            });
            if self.eat(COMMA)?.is_none() {
                break;
            }
        }
        self.expect_in(SEMICOLON, &[COMMA, SEMICOLON])?;
        Ok(one_or_block(list))
    }

    fn static_statement(&mut self) -> PResult<StmtKind> {
        self.bump()?;
        let mut list = Vec::new();
        loop {
            let tok = self.expect(K::Variable)?;
            let name = self.text(&tok)[1..].to_vec();
            let default = if self.eat(K::Char(b'='))?.is_some() {
                Some(self.expr()?)
            } else {
                None
            };
            list.push(Stmt {
                kind: StmtKind::Static { name, default },
                line: tok.line,
            });
            if self.eat(COMMA)?.is_none() {
                break;
            }
        }
        self.expect_in(SEMICOLON, &[COMMA, SEMICOLON])?;
        Ok(one_or_block(list))
    }

    fn echo_statement(&mut self) -> PResult<StmtKind> {
        self.bump()?;
        let mut list = Vec::new();
        loop {
            let value = self.expr()?;
            let value_line = value.line;
            list.push(Stmt {
                kind: StmtKind::Echo(value),
                line: value_line,
            });
            if self.eat(COMMA)?.is_none() {
                break;
            }
        }
        self.expect_in(SEMICOLON, &[COMMA, SEMICOLON])?;
        Ok(one_or_block(list))
    }

    fn unset_statement(&mut self) -> PResult<StmtKind> {
        self.bump()?;
        self.expect(OPEN_PAREN)?;
        let mut list = Vec::new();
        loop {
            let target = self.variable_followed_by(&[COMMA, CLOSE_PAREN])?;
            let target_line = target.line;
            list.push(Stmt {
                kind: StmtKind::Unset(target),
                line: target_line,
            });
            if self.eat(COMMA)?.is_none() || !starts_variable(self.peek()?) {
                break;
            }
        }
        self.expect(CLOSE_PAREN)?;
        self.expect(SEMICOLON)?;
        Ok(one_or_block(list))
    }

    fn declare_statement(&mut self) -> PResult<StmtKind> {
        self.bump()?;
        self.expect(OPEN_PAREN)?;
        let mut directives = Vec::new();
        loop {
            let tok = self.expect(K::Identifier)?;
            let name = self.text(&tok).to_vec();
            self.expect(K::Char(b'='))?;
            let value = self.expr()?;
            directives.push(ConstItem { name, value });
            if self.eat(COMMA)?.is_none() {
                break;
            }
        }
        self.expect_in(CLOSE_PAREN, &[COMMA, CLOSE_PAREN])?;
        self.check_encoding(&directives)?;
        let body = self.loop_body(K::Enddeclare)?;
        let body = match body.kind {
            StmtKind::Nop => None,
            _ => Some(Box::new(body)),
        };
        Ok(StmtKind::Declare { directives, body })
    }

    /// `declare(encoding=…)` needs a literal, and is ignored with a warning.
    fn check_encoding(&mut self, directives: &[ConstItem]) -> PResult<()> {
        for directive in directives {
            if !directive.name.eq_ignore_ascii_case(b"encoding") {
                continue;
            }
            if !matches!(
                directive.value.kind,
                ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::String(_)
            ) {
                return self.compile_error("Encoding must be a literal");
            }
            self.stream.warn(Warning::new(
                Severity::Warning,
                "declare(encoding=...) ignored because Zend multibyte feature is turned off by settings",
            ));
        }
        Ok(())
    }

    fn try_statement(&mut self) -> PResult<StmtKind> {
        self.bump()?;
        self.expect(OPEN_BRACE)?;
        let body = self.block_body()?;
        let mut catches = Vec::new();
        while self.eat(K::Catch)?.is_some() {
            self.expect(OPEN_PAREN)?;
            let mut types = vec![self.class_name()?];
            while self.eat(K::Char(b'|'))?.is_some() {
                types.push(self.class_name()?);
            }
            let variable = self
                .eat(K::Variable)?
                .map(|tok| self.text(&tok)[1..].to_vec());
            self.expect(CLOSE_PAREN)?;
            self.expect(OPEN_BRACE)?;
            let body = self.block_body()?;
            catches.push(Catch {
                types,
                variable,
                body,
            });
        }
        let finally = if self.eat(K::Finally)?.is_some() {
            self.expect(OPEN_BRACE)?;
            Some(self.block_body()?)
        } else {
            None
        };
        Ok(StmtKind::Try(Box::new(Try {
            body,
            catches,
            finally,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::one_or_block;
    use crate::ast::{Stmt, StmtKind};

    #[test]
    fn single_statements_are_not_wrapped() {
        let nop = Stmt {
            kind: StmtKind::Nop,
            line: 1,
        };
        assert_eq!(one_or_block(vec![nop.clone()]), StmtKind::Nop);
        let kind = one_or_block(vec![nop.clone(), nop]);
        assert!(matches!(kind, StmtKind::Block(ref list) if list.len() == 2));
    }
}
