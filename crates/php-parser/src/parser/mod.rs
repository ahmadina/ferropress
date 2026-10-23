//! A recursive-descent parser that accepts exactly the programs the reference grammar accepts.
//!
//! The reference parser is an LALR(1) automaton. Two of its properties shape this one:
//!
//! - It reads one token ahead and never more, so this parser decides on the next token only (and,
//!   where the reference has already consumed a token, on the one after it).
//! - When it cannot continue, the error lists the tokens accepted in the state it stopped in, after
//!   the automaton has made every reduction that does not need the unexpected token. Each error
//!   site here names the tokens of that state; see `contracts/runtime/parser.md`.

mod decl;
mod expr;
mod stmt;

use crate::ast::Program;
use crate::describe;
use crate::error::{ErrorClass, ParseError};
use crate::stream::{PResult, Stream, Tok};
use php_lexer::TokenKind;

/// Parser state: the token stream and nothing else; the tree is built bottom-up.
#[derive(Debug)]
pub(crate) struct Parser<'s> {
    stream: Stream<'s>,
    halt_offset: Option<u32>,
}

impl<'s> Parser<'s> {
    pub(crate) const fn new(stream: Stream<'s>) -> Self {
        Self {
            stream,
            halt_offset: None,
        }
    }

    pub(crate) fn into_stream(self) -> Stream<'s> {
        self.stream
    }

    /// Parses a whole file.
    pub(crate) fn program(&mut self) -> PResult<Program> {
        let statements = self.top_statements(None)?;
        Ok(Program {
            statements,
            halt_offset: self.halt_offset,
        })
    }

    // Token access.

    fn peek(&mut self) -> PResult<TokenKind> {
        self.stream.peek_kind()
    }

    fn peek_second(&mut self) -> PResult<TokenKind> {
        self.stream.peek_second_kind()
    }

    fn at(&mut self, kind: TokenKind) -> PResult<bool> {
        Ok(self.peek()? == kind)
    }

    fn bump(&mut self) -> PResult<Tok> {
        self.stream.bump()
    }

    /// Consumes the next token if it is of `kind`.
    fn eat(&mut self, kind: TokenKind) -> PResult<Option<Tok>> {
        if self.at(kind)? {
            self.bump().map(Some)
        } else {
            Ok(None)
        }
    }

    /// Consumes a token of `kind`, which the parser is in a state accepting nothing else.
    fn expect(&mut self, kind: TokenKind) -> PResult<Tok> {
        self.expect_in(kind, &[kind])
    }

    /// Consumes a token of `kind`; otherwise fails listing `expected`, the tokens the reference
    /// state accepts there.
    fn expect_in(&mut self, kind: TokenKind, expected: &[TokenKind]) -> PResult<Tok> {
        if self.at(kind)? {
            self.bump()
        } else {
            self.unexpected(expected)
        }
    }

    fn text(&self, tok: &Tok) -> &'s [u8] {
        &self.stream.source()[tok.start as usize..tok.end as usize]
    }

    /// A syntax error at the next token.
    fn unexpected<T>(&mut self, expected: &[TokenKind]) -> PResult<T> {
        let tok = self.stream.peek()?.clone();
        let message = describe::syntax_error(tok.kind, self.text(&tok), expected);
        Err(Box::new(ParseError::new(
            ErrorClass::Parse,
            message,
            Some(tok.end_line),
        )))
    }

    /// An error raised while building the tree, reported on the line the scanner is on.
    fn compile_error<T>(&self, message: impl Into<Vec<u8>>) -> PResult<T> {
        Err(Box::new(ParseError::new(
            ErrorClass::Compile,
            message,
            Some(self.stream.line()),
        )))
    }

    /// A parse error raised while building the tree, reported on the line the scanner is on.
    fn parse_error<T>(&self, message: impl Into<Vec<u8>>) -> PResult<T> {
        Err(Box::new(ParseError::new(
            ErrorClass::Parse,
            message,
            Some(self.stream.line()),
        )))
    }

    /// The identifier a `T_STRING` or keyword token stands for where any identifier is accepted.
    fn identifier_bytes(&self, tok: &Tok) -> PResult<Vec<u8>> {
        let text = self.text(tok);
        if tok.kind == TokenKind::Identifier {
            return Ok(text.to_vec());
        }
        match crate::literal::keyword_identifier(text) {
            Some(bytes) => Ok(bytes),
            None => self.parse_error("Cannot use \"<?=\" as an identifier"),
        }
    }
}

/// Keywords that are identifiers where any identifier is accepted (method and constant names,
/// named arguments), except the member modifiers.
pub(crate) const fn is_reserved_non_modifier(kind: TokenKind) -> bool {
    use TokenKind as K;
    matches!(
        kind,
        K::Include
            | K::IncludeOnce
            | K::Eval
            | K::Require
            | K::RequireOnce
            | K::LogicalOr
            | K::LogicalXor
            | K::LogicalAnd
            | K::Instanceof
            | K::New
            | K::Clone
            | K::Exit
            | K::If
            | K::Elseif
            | K::Else
            | K::Endif
            | K::Echo
            | K::Do
            | K::While
            | K::Endwhile
            | K::For
            | K::Endfor
            | K::Foreach
            | K::Endforeach
            | K::Declare
            | K::Enddeclare
            | K::As
            | K::Try
            | K::Catch
            | K::Finally
            | K::Throw
            | K::Use
            | K::Insteadof
            | K::Global
            | K::Var
            | K::Unset
            | K::Isset
            | K::Empty
            | K::Continue
            | K::Goto
            | K::Function
            | K::Const
            | K::Return
            | K::Print
            | K::Yield
            | K::List
            | K::Switch
            | K::Endswitch
            | K::Case
            | K::Default
            | K::Break
            | K::Array
            | K::Callable
            | K::Extends
            | K::Implements
            | K::Namespace
            | K::Trait
            | K::Interface
            | K::Class
            | K::ClassMagic
            | K::TraitMagic
            | K::FunctionMagic
            | K::MethodMagic
            | K::LineMagic
            | K::FileMagic
            | K::DirMagic
            | K::NamespaceMagic
            | K::Fn
            | K::Match
            | K::Enum
            | K::PropertyMagic
    )
}

/// The member modifiers.
pub(crate) const fn is_member_modifier(kind: TokenKind) -> bool {
    use TokenKind as K;
    matches!(
        kind,
        K::Public
            | K::Protected
            | K::Private
            | K::PublicSet
            | K::ProtectedSet
            | K::PrivateSet
            | K::Static
            | K::Abstract
            | K::Final
            | K::Readonly
    )
}

/// Keywords usable as identifiers: the reserved words and the modifiers other than the set
/// visibilities.
pub(crate) const fn is_semi_reserved(kind: TokenKind) -> bool {
    use TokenKind as K;
    is_reserved_non_modifier(kind)
        || matches!(
            kind,
            K::Static
                | K::Abstract
                | K::Final
                | K::Private
                | K::Protected
                | K::Public
                | K::Readonly
        )
}

/// `T_STRING` or a keyword usable as an identifier.
pub(crate) const fn is_identifier(kind: TokenKind) -> bool {
    matches!(kind, TokenKind::Identifier) || is_semi_reserved(kind)
}

/// The four name tokens.
pub(crate) const fn is_name(kind: TokenKind) -> bool {
    use TokenKind as K;
    matches!(
        kind,
        K::Identifier | K::QualifiedName | K::FullyQualifiedName | K::RelativeName
    )
}

pub(crate) const fn is_ampersand(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::AmpersandFollowedByVarOrVararg | TokenKind::AmpersandNotFollowedByVarOrVararg
    )
}

pub(crate) const fn is_magic_constant(kind: TokenKind) -> bool {
    use TokenKind as K;
    matches!(
        kind,
        K::LineMagic
            | K::FileMagic
            | K::DirMagic
            | K::ClassMagic
            | K::TraitMagic
            | K::MethodMagic
            | K::FunctionMagic
            | K::PropertyMagic
            | K::NamespaceMagic
    )
}

pub(crate) const fn is_cast(kind: TokenKind) -> bool {
    use TokenKind as K;
    matches!(
        kind,
        K::IntCast
            | K::FloatCast
            | K::StringCast
            | K::ArrayCast
            | K::ObjectCast
            | K::BoolCast
            | K::UnsetCast
    )
}

/// Tokens an expression can start with.
pub(crate) const fn starts_expression(kind: TokenKind) -> bool {
    use TokenKind as K;
    is_name(kind)
        || is_magic_constant(kind)
        || is_cast(kind)
        || matches!(
            kind,
            K::Variable
                | K::Char(b'$' | b'[' | b'(' | b'"' | b'`' | b'!' | b'~' | b'-' | b'+' | b'@')
                | K::Static
                | K::Readonly
                | K::Array
                | K::List
                | K::Isset
                | K::Empty
                | K::Include
                | K::IncludeOnce
                | K::Require
                | K::RequireOnce
                | K::Eval
                | K::New
                | K::Clone
                | K::Exit
                | K::Print
                | K::Yield
                | K::YieldFrom
                | K::Throw
                | K::Function
                | K::Fn
                | K::Attribute
                | K::Match
                | K::Integer
                | K::Float
                | K::ConstantString
                | K::StartHeredoc
                | K::Increment
                | K::Decrement
        )
}

/// Tokens the `statement` rule can start with (the bodies of `if`, loops and so on).
pub(crate) const fn starts_statement(kind: TokenKind) -> bool {
    use TokenKind as K;
    starts_expression(kind)
        || matches!(
            kind,
            K::Char(b'{' | b';')
                | K::If
                | K::While
                | K::Do
                | K::For
                | K::Switch
                | K::Break
                | K::Continue
                | K::Return
                | K::Global
                | K::Echo
                | K::InlineHtml
                | K::Unset
                | K::Foreach
                | K::Declare
                | K::Try
                | K::Goto
                | K::VoidCast
        )
}

/// Tokens a statement inside a block or function body can start with.
pub(crate) const fn starts_inner_statement(kind: TokenKind) -> bool {
    use TokenKind as K;
    starts_statement(kind)
        || matches!(
            kind,
            K::Abstract | K::Final | K::Class | K::Trait | K::Interface | K::Enum | K::HaltCompiler
        )
}

/// Tokens a top-level statement can start with.
pub(crate) const fn starts_top_statement(kind: TokenKind) -> bool {
    use TokenKind as K;
    starts_inner_statement(kind) || matches!(kind, K::Namespace | K::Use | K::Const)
}

#[cfg(test)]
mod tests {
    use super::{
        is_identifier, is_semi_reserved, starts_expression, starts_inner_statement,
        starts_statement, starts_top_statement,
    };
    use php_lexer::TokenKind as K;

    #[test]
    fn token_classes_nest() {
        for &kind in K::NAMED {
            if starts_expression(kind) {
                assert!(starts_statement(kind), "{kind:?}");
            }
            if starts_statement(kind) {
                assert!(starts_inner_statement(kind), "{kind:?}");
            }
            if starts_inner_statement(kind) {
                assert!(starts_top_statement(kind), "{kind:?}");
            }
            if is_semi_reserved(kind) {
                assert!(is_identifier(kind), "{kind:?}");
            }
        }
        assert!(!is_semi_reserved(K::PublicSet));
        assert!(!is_semi_reserved(K::YieldFrom));
        assert!(starts_top_statement(K::Namespace));
        assert!(!starts_inner_statement(K::Namespace));
        assert!(!starts_statement(K::Class));
    }
}
