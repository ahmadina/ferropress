//! Declarations: functions, closures, parameters, types, attributes, classes and their members.

use super::expr::{prec, static_name};
use super::{
    Parser, is_ampersand, is_identifier, is_member_modifier, is_name, is_reserved_non_modifier,
};
use crate::ast::{
    Attribute, AttributeGroup, ClassKind, ClassLike, ClassMember, ClosureUse, ConstItem,
    ConstantGroup, EnumCase, Expr, ExprKind, Function, FunctionBody, Hook, HookBody,
    MethodReference, Modifiers, Name, Param, Property, PropertyGroup, SimpleType, Stmt, StmtKind,
    TraitAdaptation, TraitUse, Type,
};
use crate::literal;
use crate::stream::PResult;
use php_lexer::TokenKind as K;

const SEMICOLON: K = K::Char(b';');
const COMMA: K = K::Char(b',');
const OPEN_PAREN: K = K::Char(b'(');
const CLOSE_PAREN: K = K::Char(b')');
const OPEN_BRACE: K = K::Char(b'{');
const CLOSE_BRACE: K = K::Char(b'}');

/// What a modifier list applies to, which decides the modifiers allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Property,
    Method,
    Constant,
    Parameter,
    Hook,
}

impl Target {
    const fn noun(self) -> &'static str {
        match self {
            Self::Property => "property",
            Self::Method => "method",
            Self::Constant => "class constant",
            Self::Parameter => "parameter",
            Self::Hook => "property hook",
        }
    }
}

/// The spelling of a modifier keyword in messages.
const fn modifier_word(kind: K) -> &'static str {
    match kind {
        K::Public => "public",
        K::Protected => "protected",
        K::Private => "private",
        K::Static => "static",
        K::Abstract => "abstract",
        K::Final => "final",
        K::Readonly => "readonly",
        K::PublicSet => "public(set)",
        K::ProtectedSet => "protected(set)",
        _ => "private(set)",
    }
}

/// The flag a modifier sets on `target`, or `None` where it is not allowed.
const fn modifier_flag(target: Target, kind: K) -> Option<Modifiers> {
    use Target as T;
    let flag = match kind {
        K::Public => Modifiers::PUBLIC,
        K::Protected => Modifiers::PROTECTED,
        K::Private => Modifiers::PRIVATE,
        K::Static => Modifiers::STATIC,
        K::Abstract => Modifiers::ABSTRACT,
        K::Final => Modifiers::FINAL,
        K::Readonly => Modifiers::READONLY,
        K::PublicSet => Modifiers::PUBLIC_SET,
        K::ProtectedSet => Modifiers::PROTECTED_SET,
        _ => Modifiers::PRIVATE_SET,
    };
    let allowed = match kind {
        K::Public | K::Protected | K::Private => !matches!(target, T::Hook),
        K::Readonly | K::PublicSet | K::ProtectedSet | K::PrivateSet => {
            matches!(target, T::Property | T::Parameter)
        }
        K::Abstract | K::Static => matches!(target, T::Method | T::Property),
        _ => true,
    };
    if allowed { Some(flag) } else { None }
}

/// Adds one modifier to `flags`, or explains why it cannot be added.
fn add_modifier(target: Target, flags: Modifiers, kind: K) -> Result<Modifiers, String> {
    let Some(flag) = modifier_flag(target, kind) else {
        return Err(format!(
            "Cannot use the {} modifier on a {}",
            modifier_word(kind),
            target.noun()
        ));
    };
    let combined = flags.union(flag);
    if flags.intersects(Modifiers::VISIBILITY) && flag.intersects(Modifiers::VISIBILITY) {
        return Err("Multiple access type modifiers are not allowed".to_owned());
    }
    if combined.contains(Modifiers::ABSTRACT) && combined.contains(Modifiers::FINAL) {
        match target {
            Target::Method => {
                return Err("Cannot use the final modifier on an abstract method".to_owned());
            }
            Target::Property => {
                return Err("Cannot use the final modifier on an abstract property".to_owned());
            }
            _ => {}
        }
    }
    if matches!(target, Target::Property | Target::Parameter)
        && flags.intersects(Modifiers::SET_VISIBILITY)
        && flag.intersects(Modifiers::SET_VISIBILITY)
    {
        return Err("Multiple access type modifiers are not allowed".to_owned());
    }
    if flags.intersects(flag) {
        return Err(format!(
            "Multiple {} modifiers are not allowed",
            modifier_word(kind)
        ));
    }
    Ok(combined)
}

/// The flags of a modifier list, checked in order as the reference does.
fn modifier_list(target: Target, tokens: &[K]) -> Result<Modifiers, String> {
    tokens.iter().try_fold(Modifiers::NONE, |flags, &kind| {
        add_modifier(target, flags, kind)
    })
}

/// Tokens a type can start with.
const fn starts_type(kind: K, allow_static: bool) -> bool {
    is_name(kind)
        || matches!(kind, K::Char(b'?' | b'(') | K::Array | K::Callable)
        || (allow_static && matches!(kind, K::Static))
}

impl Parser<'_> {
    /// Member modifiers read up to the first other token.
    fn modifier_tokens(&mut self) -> PResult<Vec<K>> {
        let mut tokens = Vec::new();
        while is_member_modifier(self.peek()?) {
            tokens.push(self.bump()?.kind);
        }
        Ok(tokens)
    }

    /// Converts modifier tokens once the token after them has been read.
    fn modifiers(&mut self, target: Target, tokens: &[K]) -> PResult<Modifiers> {
        if tokens.is_empty() {
            return Ok(Modifiers::NONE);
        }
        self.peek()?;
        match modifier_list(target, tokens) {
            Ok(flags) => Ok(flags),
            Err(message) => self.compile_error(message),
        }
    }

    fn eat_ampersand(&mut self) -> PResult<bool> {
        if is_ampersand(self.peek()?) {
            self.bump()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// `T_STATIC` or a name.
    pub(super) fn class_name(&mut self) -> PResult<Name> {
        let kind = self.peek()?;
        if kind == K::Static {
            self.bump()?;
            return Ok(static_name());
        }
        if !is_name(kind) {
            return self.unexpected(&[]);
        }
        let tok = self.bump()?;
        Ok(literal::name(kind, self.text(&tok)))
    }

    pub(super) fn class_name_list(&mut self) -> PResult<Vec<Name>> {
        let mut names = vec![self.class_name()?];
        while self.eat(COMMA)?.is_some() {
            names.push(self.class_name()?);
        }
        Ok(names)
    }

    /// Attribute groups, possibly none.
    pub(super) fn attributes(&mut self) -> PResult<Vec<AttributeGroup>> {
        let mut groups = Vec::new();
        while self.eat(K::Attribute)?.is_some() {
            let mut group = Vec::new();
            loop {
                let name = self.class_name()?;
                let args = if self.at(OPEN_PAREN)? {
                    Some(self.arguments()?)
                } else {
                    None
                };
                group.push(Attribute { name, args });
                if self.eat(COMMA)?.is_none() {
                    break;
                }
                let next = self.peek()?;
                if next != K::Static && !is_name(next) {
                    break;
                }
            }
            self.expect(K::Char(b']'))?;
            groups.push(group);
        }
        Ok(groups)
    }

    /// A declared type.
    pub(super) fn type_expr(&mut self, allow_static: bool) -> PResult<Type> {
        if self.eat(K::Char(b'?'))?.is_some() {
            let kind = self.simple_type(allow_static)?;
            return Ok(Type::Simple {
                kind,
                nullable: true,
            });
        }
        if self.at(OPEN_PAREN)? {
            let first = self.parenthesized_intersection(allow_static)?;
            self.expect(K::Char(b'|'))?;
            return self.union_rest(first, allow_static);
        }
        let first = Type::Simple {
            kind: self.simple_type(allow_static)?,
            nullable: false,
        };
        match self.peek()? {
            K::Char(b'|') => {
                self.bump()?;
                self.union_rest(first, allow_static)
            }
            K::AmpersandNotFollowedByVarOrVararg => {
                let mut types = vec![first];
                while self.eat(K::AmpersandNotFollowedByVarOrVararg)?.is_some() {
                    types.push(Type::Simple {
                        kind: self.simple_type(allow_static)?,
                        nullable: false,
                    });
                }
                Ok(Type::Intersection(types))
            }
            _ => Ok(first),
        }
    }

    /// The elements of a union after its first `|`.
    fn union_rest(&mut self, first: Type, allow_static: bool) -> PResult<Type> {
        let mut types = vec![first];
        loop {
            let element = if self.at(OPEN_PAREN)? {
                self.parenthesized_intersection(allow_static)?
            } else {
                Type::Simple {
                    kind: self.simple_type(allow_static)?,
                    nullable: false,
                }
            };
            types.push(element);
            if self.eat(K::Char(b'|'))?.is_none() {
                return Ok(Type::Union(types));
            }
        }
    }

    /// `(A&B…)` inside a union.
    fn parenthesized_intersection(&mut self, allow_static: bool) -> PResult<Type> {
        self.bump()?;
        let mut types = vec![Type::Simple {
            kind: self.simple_type(allow_static)?,
            nullable: false,
        }];
        loop {
            self.expect(K::AmpersandNotFollowedByVarOrVararg)?;
            types.push(Type::Simple {
                kind: self.simple_type(allow_static)?,
                nullable: false,
            });
            if self.eat(CLOSE_PAREN)?.is_some() {
                return Ok(Type::Intersection(types));
            }
        }
    }

    fn simple_type(&mut self, allow_static: bool) -> PResult<SimpleType> {
        let kind = self.peek()?;
        let simple = match kind {
            K::Array => SimpleType::Array,
            K::Callable => SimpleType::Callable,
            K::Static if allow_static => SimpleType::Static,
            kind if is_name(kind) => {
                let tok = self.bump()?;
                return Ok(SimpleType::Name(literal::name(kind, self.text(&tok))));
            }
            _ => return self.unexpected(&[]),
        };
        self.bump()?;
        Ok(simple)
    }

    fn optional_type(&mut self, allow_static: bool) -> PResult<Option<Type>> {
        if starts_type(self.peek()?, allow_static) {
            self.type_expr(allow_static).map(Some)
        } else {
            Ok(None)
        }
    }

    /// `: type` after a parameter list.
    fn return_type(&mut self) -> PResult<Option<Type>> {
        if self.eat(K::Char(b':'))?.is_some() {
            self.type_expr(true).map(Some)
        } else {
            Ok(None)
        }
    }

    /// Parameters after `(`, leaving the `)` unread.
    fn parameter_list(&mut self) -> PResult<Vec<Param>> {
        let mut params = Vec::new();
        if self.at(CLOSE_PAREN)? {
            return Ok(params);
        }
        loop {
            params.push(self.parameter()?);
            if self.eat(COMMA)?.is_none() || self.at(CLOSE_PAREN)? {
                return Ok(params);
            }
        }
    }

    /// `( parameters )`.
    fn parenthesized_parameters(&mut self) -> PResult<Vec<Param>> {
        self.expect(OPEN_PAREN)?;
        let params = self.parameter_list()?;
        self.expect(CLOSE_PAREN)?;
        Ok(params)
    }

    fn parameter(&mut self) -> PResult<Param> {
        let attributes = self.attributes()?;
        let tokens = self.modifier_tokens()?;
        let modifiers = self.modifiers(Target::Parameter, &tokens)?;
        let type_line = self.stream.peek()?.line;
        let declared_type = self.optional_type(false)?;
        let by_ref = self.eat(K::AmpersandFollowedByVarOrVararg)?.is_some();
        let variadic = self.eat(K::Ellipsis)?.is_some();
        let name = self.expect(K::Variable)?;
        let line = if declared_type.is_some() {
            type_line
        } else {
            name.line
        };
        let name = self.text(&name)[1..].to_vec();
        let default = if self.eat(K::Char(b'='))?.is_some() {
            Some(self.expr()?)
        } else {
            None
        };
        let hooks = if self.at(OPEN_BRACE)? {
            Some(self.hook_list(&name)?)
        } else {
            None
        };
        Ok(Param {
            name,
            declared_type,
            default,
            by_ref,
            variadic,
            modifiers,
            hooks,
            attributes,
            line,
        })
    }

    /// A closure or arrow function starting at `function` or `fn`.
    pub(super) fn closure(
        &mut self,
        attributes: Vec<AttributeGroup>,
        is_static: bool,
    ) -> PResult<Expr> {
        let keyword = self.bump()?;
        let by_ref = self.eat_ampersand()?;
        let modifiers = if is_static {
            Modifiers::STATIC
        } else {
            Modifiers::NONE
        };
        if keyword.kind == K::Fn {
            let params = self.parenthesized_parameters()?;
            let return_type = self.return_type()?;
            self.expect(K::DoubleArrow)?;
            let (body, is_generator) =
                self.function_body(|parser| parser.expr_bp(prec::ARROW_FUNCTION))?;
            let function = Function {
                name: Vec::new(),
                by_ref,
                params,
                uses: Vec::new(),
                return_type,
                body: FunctionBody::Expr(Box::new(body)),
                is_generator,
                modifiers,
                attributes,
                line: keyword.line,
            };
            return Ok(Expr {
                kind: ExprKind::ArrowFunction {
                    function: Box::new(function),
                    parenthesized: false,
                },
                line: keyword.line,
            });
        }
        let function = self.closure_rest(keyword.line, by_ref, attributes, modifiers)?;
        Ok(Expr {
            kind: ExprKind::Closure(Box::new(function)),
            line: keyword.line,
        })
    }

    /// A closure after `function [&]`.
    fn closure_rest(
        &mut self,
        line: u32,
        by_ref: bool,
        attributes: Vec<AttributeGroup>,
        modifiers: Modifiers,
    ) -> PResult<Function> {
        let params = self.parenthesized_parameters()?;
        let uses = if self.eat(K::Use)?.is_some() {
            self.closure_uses()?
        } else {
            Vec::new()
        };
        let return_type = self.return_type()?;
        self.expect(OPEN_BRACE)?;
        let (body, is_generator) = self.function_body(Self::block_body)?;
        Ok(Function {
            name: Vec::new(),
            by_ref,
            params,
            uses,
            return_type,
            body: FunctionBody::Block(body),
            is_generator,
            modifiers,
            attributes,
            line,
        })
    }

    fn closure_uses(&mut self) -> PResult<Vec<ClosureUse>> {
        self.expect(OPEN_PAREN)?;
        let mut uses = Vec::new();
        loop {
            let by_ref = self.eat_ampersand()?;
            let name = self.expect(K::Variable)?;
            uses.push(ClosureUse {
                name: self.text(&name)[1..].to_vec(),
                by_ref,
            });
            if self.eat(COMMA)?.is_none() {
                break;
            }
            let next = self.peek()?;
            if next != K::Variable && !is_ampersand(next) {
                break;
            }
        }
        self.expect(CLOSE_PAREN)?;
        Ok(uses)
    }

    /// `function` at statement level: a declaration, or a closure starting an expression.
    pub(super) fn function_statement(&mut self, attributes: Vec<AttributeGroup>) -> PResult<Stmt> {
        let keyword = self.bump()?;
        let line = keyword.line;
        let by_ref = self.eat_ampersand()?;
        if !matches!(self.peek()?, K::Identifier | K::Readonly) {
            let function = self.closure_rest(line, by_ref, attributes, Modifiers::NONE)?;
            let closure = Expr {
                kind: ExprKind::Closure(Box::new(function)),
                line,
            };
            return self.expression_statement(closure, line);
        }
        let name = self.bump()?;
        let name = self.text(&name).to_vec();
        let params = self.parenthesized_parameters()?;
        let return_type = self.return_type()?;
        self.expect(OPEN_BRACE)?;
        let (body, is_generator) = self.function_body(Self::block_body)?;
        let function = Function {
            name,
            by_ref,
            params,
            uses: Vec::new(),
            return_type,
            body: FunctionBody::Block(body),
            is_generator,
            modifiers: Modifiers::NONE,
            attributes,
            line,
        };
        Ok(Stmt {
            kind: StmtKind::Function(Box::new(function)),
            line,
        })
    }

    /// A class, interface, trait or enum declaration.
    pub(super) fn class_declaration(&mut self, attributes: Vec<AttributeGroup>) -> PResult<Stmt> {
        let start = self.stream.peek()?.line;
        let (kind, modifiers) = match self.peek()? {
            K::Trait => (ClassKind::Trait, Modifiers::NONE),
            K::Interface => (ClassKind::Interface, Modifiers::NONE),
            K::Enum => (ClassKind::Enum, Modifiers::NONE),
            _ => (ClassKind::Class, self.class_modifiers()?),
        };
        let line = self.bump()?.line;
        let name = self.expect(K::Identifier)?;
        let name = self.text(&name).to_vec();
        let mut backing_type = None;
        let mut extends = None;
        let mut implements = Vec::new();
        match kind {
            ClassKind::Class => {
                if self.eat(K::Extends)?.is_some() {
                    extends = Some(self.class_name()?);
                }
                if self.eat(K::Implements)?.is_some() {
                    implements = self.class_name_list()?;
                }
            }
            ClassKind::Interface => {
                if self.eat(K::Extends)?.is_some() {
                    implements = self.class_name_list()?;
                }
            }
            ClassKind::Enum => {
                if self.eat(K::Char(b':'))?.is_some() {
                    backing_type = Some(self.type_expr(true)?);
                }
                if self.eat(K::Implements)?.is_some() {
                    implements = self.class_name_list()?;
                }
            }
            ClassKind::Trait => {}
        }
        self.expect(OPEN_BRACE)?;
        let members = self.class_body()?;
        let class = ClassLike {
            kind,
            name: Some(name),
            modifiers,
            extends,
            implements,
            backing_type,
            members,
            attributes,
            line,
        };
        Ok(Stmt {
            kind: StmtKind::Class(Box::new(class)),
            line: start,
        })
    }

    /// `abstract`, `final` and `readonly` before `class`, leaving `class` unread.
    fn class_modifiers(&mut self) -> PResult<Modifiers> {
        let mut flags = Modifiers::NONE;
        let mut first = true;
        loop {
            let (flag, word) = match self.peek()? {
                K::Abstract => (Modifiers::ABSTRACT, "abstract"),
                K::Final => (Modifiers::FINAL, "final"),
                K::Readonly => (Modifiers::READONLY, "readonly"),
                K::Class => return Ok(flags),
                _ => {
                    return self.unexpected(&[K::Abstract, K::Final, K::Readonly, K::Class]);
                }
            };
            self.bump()?;
            if !first {
                if flags.intersects(flag) {
                    return self
                        .compile_error(format!("Multiple {word} modifiers are not allowed"));
                }
                let combined = flags.union(flag);
                if combined.contains(Modifiers::ABSTRACT) && combined.contains(Modifiers::FINAL) {
                    return self
                        .compile_error("Cannot use the final modifier on an abstract class");
                }
            }
            flags = flags.union(flag);
            first = false;
        }
    }

    /// Class members up to and including `}`.
    pub(super) fn class_body(&mut self) -> PResult<Vec<ClassMember>> {
        let mut members = Vec::new();
        while self.eat(CLOSE_BRACE)?.is_none() {
            members.push(self.class_member()?);
        }
        Ok(members)
    }

    fn class_member(&mut self) -> PResult<ClassMember> {
        if self.at(K::Use)? {
            return self.trait_use();
        }
        let attributes = self.attributes()?;
        let line = self.stream.peek()?.line;
        match self.peek()? {
            K::Case => return self.enum_case(attributes),
            K::Var => {
                self.bump()?;
                return self.property_group(attributes, Modifiers::PUBLIC);
            }
            K::Const | K::Function => {}
            kind if is_member_modifier(kind) => {}
            _ => return self.unexpected(&[K::Function]),
        }
        let tokens = self.modifier_tokens()?;
        match self.peek()? {
            K::Const => {
                let flags = self.modifiers(Target::Constant, &tokens)?;
                self.constant_group(attributes, with_default_visibility(flags))
            }
            K::Function => {
                let flags = self.modifiers(Target::Method, &tokens)?;
                self.method(attributes, with_default_visibility(flags), line)
            }
            _ => {
                let flags = self.modifiers(Target::Property, &tokens)?;
                self.property_group(attributes, flags)
            }
        }
    }

    /// The group's line is that of its type, or of its first property when untyped.
    fn property_group(
        &mut self,
        attributes: Vec<AttributeGroup>,
        modifiers: Modifiers,
    ) -> PResult<ClassMember> {
        let group_line = self.stream.peek()?.line;
        let property_type = self.optional_type(false)?;
        let mut properties = Vec::new();
        loop {
            let name = self.expect(K::Variable)?;
            let line = name.line;
            let name = self.text(&name)[1..].to_vec();
            let default = if self.eat(K::Char(b'='))?.is_some() {
                Some(self.expr()?)
            } else {
                None
            };
            if properties.is_empty() && self.at(OPEN_BRACE)? {
                let hooks = Some(self.hook_list(&name)?);
                properties.push(Property {
                    name,
                    default,
                    hooks,
                    line,
                });
                return Ok(ClassMember::Property(PropertyGroup {
                    modifiers,
                    property_type,
                    properties,
                    attributes,
                    line: group_line,
                }));
            }
            properties.push(Property {
                name,
                default,
                hooks: None,
                line,
            });
            if self.eat(COMMA)?.is_none() {
                break;
            }
        }
        self.expect_in(SEMICOLON, &[COMMA, SEMICOLON])?;
        Ok(ClassMember::Property(PropertyGroup {
            modifiers,
            property_type,
            properties,
            attributes,
            line: group_line,
        }))
    }

    /// `{ hooks }` of the property `property`.
    fn hook_list(&mut self, property: &[u8]) -> PResult<Vec<Hook>> {
        self.bump()?;
        let mut hooks = Vec::new();
        while self.eat(CLOSE_BRACE)?.is_none() {
            hooks.push(self.hook(property)?);
        }
        Ok(hooks)
    }

    fn hook(&mut self, property: &[u8]) -> PResult<Hook> {
        let attributes = self.attributes()?;
        let tokens = self.modifier_tokens()?;
        let modifiers = self.modifiers(Target::Hook, &tokens)?;
        let by_ref = self.eat_ampersand()?;
        let name = self.expect(K::Identifier)?;
        let line = name.line;
        let name = self.text(&name).to_vec();
        let params = if self.at(OPEN_PAREN)? {
            Some(self.parenthesized_parameters()?)
        } else {
            None
        };
        let (body, is_generator, fetched) =
            self.function_body_fetching(|parser| match parser.peek()? {
                SEMICOLON => {
                    parser.bump()?;
                    Ok(HookBody::Absent)
                }
                OPEN_BRACE => {
                    parser.bump()?;
                    Ok(HookBody::Block(parser.block_body()?))
                }
                K::DoubleArrow => {
                    parser.bump()?;
                    let value = parser.expr()?;
                    parser.expect_in(SEMICOLON, &[])?;
                    Ok(HookBody::Expr(value))
                }
                _ => parser.unexpected(&[K::DoubleArrow, SEMICOLON, OPEN_BRACE]),
            })?;
        Ok(Hook {
            name,
            by_ref,
            modifiers,
            params,
            body,
            is_generator,
            uses_property: fetched.iter().any(|name| name == property),
            attributes,
            line,
            end_line: self.stream.consumed_line(),
        })
    }

    fn method(
        &mut self,
        attributes: Vec<AttributeGroup>,
        modifiers: Modifiers,
        line: u32,
    ) -> PResult<ClassMember> {
        self.bump()?;
        let by_ref = self.eat_ampersand()?;
        if !is_identifier(self.peek()?) {
            return self.unexpected(&[]);
        }
        let name = self.bump()?;
        let name = self.identifier_bytes(&name)?;
        let params = self.parenthesized_parameters()?;
        let return_type = self.return_type()?;
        let (body, is_generator) = self.function_body(|parser| match parser.peek()? {
            SEMICOLON => {
                parser.bump()?;
                Ok(FunctionBody::Absent)
            }
            OPEN_BRACE => {
                parser.bump()?;
                Ok(FunctionBody::Block(parser.block_body()?))
            }
            _ => parser.unexpected(&[SEMICOLON, OPEN_BRACE]),
        })?;
        Ok(ClassMember::Method(Box::new(Function {
            name,
            by_ref,
            params,
            uses: Vec::new(),
            return_type,
            body,
            is_generator,
            modifiers,
            attributes,
            line,
        })))
    }

    /// The group's line is that of its first constant's name.
    fn constant_group(
        &mut self,
        attributes: Vec<AttributeGroup>,
        modifiers: Modifiers,
    ) -> PResult<ClassMember> {
        self.bump()?;
        let kind = self.peek()?;
        let constant_type = if is_identifier(kind) && self.peek_second()? == K::Char(b'=') {
            None
        } else {
            self.optional_type(true)?
        };
        let mut items = Vec::new();
        let mut line = 0;
        loop {
            if !is_identifier(self.peek()?) {
                return self.unexpected(&[]);
            }
            let name = self.bump()?;
            if items.is_empty() {
                line = name.line;
            }
            let name_line = name.line;
            let name = self.identifier_bytes(&name)?;
            self.expect(K::Char(b'='))?;
            let value = self.expr()?;
            items.push(ConstItem {
                name,
                line: name_line,
                value,
            });
            if self.eat(COMMA)?.is_none() {
                break;
            }
        }
        self.expect_in(SEMICOLON, &[COMMA, SEMICOLON])?;
        Ok(ClassMember::Constant(ConstantGroup {
            modifiers,
            constant_type,
            items,
            attributes,
            line,
        }))
    }

    fn enum_case(&mut self, attributes: Vec<AttributeGroup>) -> PResult<ClassMember> {
        self.bump()?;
        if !is_identifier(self.peek()?) {
            return self.unexpected(&[]);
        }
        let name = self.bump()?;
        let line = name.line;
        let name = self.identifier_bytes(&name)?;
        let value = if self.eat(K::Char(b'='))?.is_some() {
            Some(self.expr()?)
        } else {
            None
        };
        self.expect(SEMICOLON)?;
        Ok(ClassMember::EnumCase(EnumCase {
            name,
            value,
            attributes,
            line,
        }))
    }

    fn trait_use(&mut self) -> PResult<ClassMember> {
        let line = self.bump()?.line;
        let traits = self.class_name_list()?;
        let adaptations = match self.peek()? {
            SEMICOLON => {
                self.bump()?;
                None
            }
            OPEN_BRACE => {
                self.bump()?;
                if self.eat(CLOSE_BRACE)?.is_some() {
                    None
                } else {
                    let mut list = Vec::new();
                    while self.eat(CLOSE_BRACE)?.is_none() {
                        list.push(self.trait_adaptation()?);
                    }
                    Some(list)
                }
            }
            _ => return self.unexpected(&[COMMA, SEMICOLON, OPEN_BRACE]),
        };
        Ok(ClassMember::TraitUse(TraitUse {
            traits,
            adaptations,
            line,
        }))
    }

    fn trait_adaptation(&mut self) -> PResult<TraitAdaptation> {
        let kind = self.peek()?;
        let qualified = (is_name(kind) && kind != K::Identifier)
            || (matches!(kind, K::Identifier | K::Static) && self.peek_second()? == K::DoubleColon);
        let method = if qualified {
            let class = self.class_name()?;
            self.expect(K::DoubleColon)?;
            if !is_identifier(self.peek()?) {
                return self.unexpected(&[]);
            }
            let method = self.bump()?;
            let method = self.identifier_bytes(&method)?;
            let method = MethodReference {
                class: Some(class),
                method,
            };
            if self.eat(K::Insteadof)?.is_some() {
                let instead_of = self.class_name_list()?;
                self.expect(SEMICOLON)?;
                return Ok(TraitAdaptation::Precedence { method, instead_of });
            }
            method
        } else if is_identifier(kind) {
            let method = self.bump()?;
            let method = MethodReference {
                class: None,
                method: self.identifier_bytes(&method)?,
            };
            if kind == K::Identifier && !self.at(K::As)? {
                return self.unexpected(&[K::DoubleColon]);
            }
            method
        } else {
            return self.unexpected(&[]);
        };
        self.expect(K::As)?;
        let kind = self.peek()?;
        let (modifier, alias) = if kind == K::Identifier || is_reserved_non_modifier(kind) {
            let alias = self.bump()?;
            (Modifiers::NONE, Some(self.identifier_bytes(&alias)?))
        } else if is_member_modifier(kind) {
            self.bump()?;
            let alias = if is_identifier(self.peek()?) {
                let alias = self.bump()?;
                Some(self.identifier_bytes(&alias)?)
            } else {
                None
            };
            let Some(modifier) = modifier_flag(Target::Method, kind) else {
                return self.compile_error(format!(
                    "Cannot use the {} modifier on a method",
                    modifier_word(kind)
                ));
            };
            (modifier, alias)
        } else {
            return self.unexpected(&[]);
        };
        self.expect(SEMICOLON)?;
        Ok(TraitAdaptation::Alias {
            method,
            modifier,
            alias,
        })
    }
}

/// Methods and class constants without a visibility are public.
const fn with_default_visibility(flags: Modifiers) -> Modifiers {
    if flags.intersects(Modifiers::VISIBILITY) {
        flags
    } else {
        flags.union(Modifiers::PUBLIC)
    }
}

#[cfg(test)]
mod tests {
    use super::{Target, modifier_list, with_default_visibility};
    use crate::ast::{
        ClassMember, Expr, ExprKind, Function, FunctionBody, Modifiers, StmtKind, TopStatement,
    };
    use php_lexer::TokenKind as K;

    fn function(source: &str) -> Function {
        let Ok(parsed) = crate::parse(source.as_bytes(), crate::ParseOptions::default()) else {
            panic!("source fits");
        };
        let Ok(mut program) = parsed.result else {
            panic!("{source} parses");
        };
        match program.statements.remove(0) {
            TopStatement::Statement(stmt) => match stmt.kind {
                StmtKind::Function(function) => *function,
                StmtKind::Class(mut class) => match class.members.remove(0) {
                    ClassMember::Method(method) => *method,
                    other => panic!("not a method: {other:?}"),
                },
                other => panic!("not a declaration: {other:?}"),
            },
            other => panic!("not a statement: {other:?}"),
        }
    }

    fn nested(function: &Function) -> &Function {
        let FunctionBody::Block(body) = &function.body else {
            panic!("block body");
        };
        let StmtKind::Expr(Expr {
            kind:
                ExprKind::Closure(inner)
                | ExprKind::ArrowFunction {
                    function: inner, ..
                },
            ..
        }) = &body[0].kind
        else {
            panic!("closure statement");
        };
        inner
    }

    #[test]
    fn a_yield_marks_only_the_innermost_body_as_a_generator() {
        assert!(function("<?php function f() { yield; }").is_generator);
        assert!(function("<?php function f() { yield from g(); }").is_generator);
        assert!(!function("<?php function f() { return 1; }").is_generator);

        let outer = function("<?php function f() { function () { yield 1; }; }");
        assert!(!outer.is_generator);
        assert!(nested(&outer).is_generator);
        let outer = function("<?php function f() { fn() => yield 1 => 2; yield; }");
        assert!(outer.is_generator);
        assert!(nested(&outer).is_generator);

        assert!(function("<?php class C { function m() { yield; } }").is_generator);
        let Ok(parsed) = crate::parse(
            b"<?php class C { public $p { get { yield; } } }",
            crate::ParseOptions::default(),
        ) else {
            panic!("source fits");
        };
        let Ok(program) = parsed.result else {
            panic!("hooked property parses");
        };
        let TopStatement::Statement(stmt) = &program.statements[0] else {
            panic!("statement");
        };
        let StmtKind::Class(class) = &stmt.kind else {
            panic!("class");
        };
        let ClassMember::Property(group) = &class.members[0] else {
            panic!("property");
        };
        let hooks = group.properties[0].hooks.as_deref().unwrap_or_default();
        assert!(hooks[0].is_generator);
    }

    #[test]
    fn modifier_lists_follow_the_reference_checks() {
        assert_eq!(
            modifier_list(Target::Method, &[K::Public, K::Static]),
            Ok(Modifiers::PUBLIC.union(Modifiers::STATIC))
        );
        assert_eq!(
            modifier_list(Target::Method, &[K::Public, K::Public]),
            Err("Multiple access type modifiers are not allowed".to_owned())
        );
        assert_eq!(
            modifier_list(Target::Property, &[K::Static, K::Static]),
            Err("Multiple static modifiers are not allowed".to_owned())
        );
        assert_eq!(
            modifier_list(Target::Method, &[K::Abstract, K::Final]),
            Err("Cannot use the final modifier on an abstract method".to_owned())
        );
        assert_eq!(
            modifier_list(Target::Constant, &[K::Readonly]),
            Err("Cannot use the readonly modifier on a class constant".to_owned())
        );
        assert_eq!(
            modifier_list(Target::Hook, &[K::Public]),
            Err("Cannot use the public modifier on a property hook".to_owned())
        );
        assert_eq!(
            modifier_list(Target::Parameter, &[K::PrivateSet, K::PublicSet]),
            Err("Multiple access type modifiers are not allowed".to_owned())
        );
        assert_eq!(
            with_default_visibility(Modifiers::STATIC),
            Modifiers::STATIC.union(Modifiers::PUBLIC)
        );
        assert_eq!(
            with_default_visibility(Modifiers::PRIVATE),
            Modifiers::PRIVATE
        );
    }

    #[test]
    fn hooks_record_property_use_and_lines() {
        let source = b"<?php class C { public $p {\n get => [$this->p, fn() => $this->q];\n set {\n $this->q = $this?->r;\n }\n } }";
        let Ok(parsed) = crate::parse(source, crate::ParseOptions::default()) else {
            panic!("source fits");
        };
        let Ok(program) = parsed.result else {
            panic!("hooked property parses");
        };
        let TopStatement::Statement(stmt) = &program.statements[0] else {
            panic!("statement");
        };
        let StmtKind::Class(class) = &stmt.kind else {
            panic!("class");
        };
        let ClassMember::Property(group) = &class.members[0] else {
            panic!("property");
        };
        let hooks = group.properties[0].hooks.as_deref().unwrap_or_default();
        let facts: Vec<(bool, u32, u32)> = hooks
            .iter()
            .map(|hook| (hook.uses_property, hook.line, hook.end_line))
            .collect();
        assert_eq!(facts, [(true, 2, 2), (false, 3, 5)]);
    }

    #[test]
    fn parenthesized_static_properties_are_marked() {
        let Ok(parsed) = crate::parse(
            b"<?php (A::$a)::f(); A::$b::f();",
            crate::ParseOptions::default(),
        ) else {
            panic!("source fits");
        };
        let Ok(program) = parsed.result else {
            panic!("calls parse");
        };
        let marks: Vec<bool> = program
            .statements
            .iter()
            .map(|statement| match statement {
                TopStatement::Statement(stmt) => match &stmt.kind {
                    StmtKind::Expr(call) => match &call.kind {
                        ExprKind::StaticCall { class, .. } => match &class.kind {
                            ExprKind::StaticProperty { parenthesized, .. } => *parenthesized,
                            _ => panic!("static property"),
                        },
                        _ => panic!("static call"),
                    },
                    _ => panic!("expression statement"),
                },
                _ => panic!("statement"),
            })
            .collect();
        assert_eq!(marks, [true, false]);
    }

    #[test]
    fn member_groups_record_the_reference_lines() {
        let source = b"<?php enum E {\n public\n int\n $a,\n $b;\n var\n $c { get; }\n const\n int\n A = 1,\n B = 2;\n case\n C;\n function\n f() {}\n}";
        let Ok(parsed) = crate::parse(source, crate::ParseOptions::default()) else {
            panic!("source fits");
        };
        let Ok(program) = parsed.result else {
            panic!("enum parses");
        };
        let TopStatement::Statement(stmt) = &program.statements[0] else {
            panic!("statement");
        };
        let StmtKind::Class(class) = &stmt.kind else {
            panic!("class");
        };
        let lines: Vec<u32> = class
            .members
            .iter()
            .map(|member| match member {
                ClassMember::Property(group) => group.line,
                ClassMember::Constant(group) => group.line,
                ClassMember::EnumCase(case) => case.line,
                ClassMember::Method(method) => method.line,
                ClassMember::TraitUse(trait_use) => trait_use.line,
            })
            .collect();
        assert_eq!(lines, [3, 7, 10, 13, 14]);
    }

    #[test]
    fn method_calls_record_the_line_of_their_name() {
        let Ok(parsed) = crate::parse(b"<?php $a\n?->b\n->\nc();", crate::ParseOptions::default())
        else {
            panic!("source fits");
        };
        let Ok(program) = parsed.result else {
            panic!("call parses");
        };
        let [TopStatement::Statement(stmt)] = program.statements.as_slice() else {
            panic!("one statement");
        };
        let StmtKind::Expr(call) = &stmt.kind else {
            panic!("expression statement");
        };
        let ExprKind::MethodCall {
            name_line, object, ..
        } = &call.kind
        else {
            panic!("method call");
        };
        assert_eq!((call.line, *name_line, object.line), (1, 4, 1));
    }
}
