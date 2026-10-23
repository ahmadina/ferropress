//! Token kinds and tokens.

/// Declares [`TokenKind`] together with the PHP-visible name and number of each named kind, so the
/// three can never drift apart.
macro_rules! token_kinds {
    ($( $(#[$doc:meta])* $variant:ident = $name:literal, $id:literal; )*) => {
        /// What a token is. Single-character tokens carry their byte; every other kind has the name
        /// and number PHP's tokenizer reports for it ([`TokenKind::name`], [`TokenKind::id`]).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum TokenKind {
            /// A single-character token such as `;`, `(` or `"`.
            Char(u8),
            $( $(#[$doc])* $variant, )*
        }

        impl TokenKind {
            /// Every named kind, in PHP's numbering order.
            pub const NAMED: &'static [Self] = &[$(Self::$variant),*];

            /// PHP's name for the kind (`T_STRING`, …); `None` for a single-character token.
            pub const fn name(self) -> Option<&'static str> {
                match self {
                    Self::Char(_) => None,
                    $(Self::$variant => Some($name),)*
                }
            }

            /// PHP's number for the kind; a single-character token's number is its byte.
            pub const fn id(self) -> u16 {
                match self {
                    Self::Char(byte) => byte as u16,
                    $(Self::$variant => $id,)*
                }
            }
        }
    };
}

token_kinds! {
    /// An integer literal.
    Integer = "T_LNUMBER", 260;
    /// A float literal, or an integer literal too large for an integer.
    Float = "T_DNUMBER", 261;
    /// An identifier, including keywords used where only names are allowed.
    Identifier = "T_STRING", 262;
    /// A name starting with `\`.
    FullyQualifiedName = "T_NAME_FULLY_QUALIFIED", 263;
    /// A name starting with `namespace\`.
    RelativeName = "T_NAME_RELATIVE", 264;
    /// A name with namespace separators that starts with an identifier.
    QualifiedName = "T_NAME_QUALIFIED", 265;
    /// `$name`.
    Variable = "T_VARIABLE", 266;
    /// Text outside the PHP tags.
    InlineHtml = "T_INLINE_HTML", 267;
    /// Literal text inside an interpolated string.
    StringFragment = "T_ENCAPSED_AND_WHITESPACE", 268;
    /// A quoted string without interpolation.
    ConstantString = "T_CONSTANT_ENCAPSED_STRING", 269;
    /// The variable name in `"${name}"`.
    VariableName = "T_STRING_VARNAME", 270;
    /// A numeric offset in `"$a[0]"`.
    OffsetNumber = "T_NUM_STRING", 271;
    /// `include`.
    Include = "T_INCLUDE", 272;
    /// `include_once`.
    IncludeOnce = "T_INCLUDE_ONCE", 273;
    /// `eval`.
    Eval = "T_EVAL", 274;
    /// `require`.
    Require = "T_REQUIRE", 275;
    /// `require_once`.
    RequireOnce = "T_REQUIRE_ONCE", 276;
    /// `or`.
    LogicalOr = "T_LOGICAL_OR", 277;
    /// `xor`.
    LogicalXor = "T_LOGICAL_XOR", 278;
    /// `and`.
    LogicalAnd = "T_LOGICAL_AND", 279;
    /// `print`.
    Print = "T_PRINT", 280;
    /// `yield`.
    Yield = "T_YIELD", 281;
    /// `yield from`, with whatever whitespace and comments separate the words.
    YieldFrom = "T_YIELD_FROM", 282;
    /// `instanceof`.
    Instanceof = "T_INSTANCEOF", 283;
    /// `new`.
    New = "T_NEW", 284;
    /// `clone`.
    Clone = "T_CLONE", 285;
    /// `exit` or `die`.
    Exit = "T_EXIT", 286;
    /// `if`.
    If = "T_IF", 287;
    /// `elseif`.
    Elseif = "T_ELSEIF", 288;
    /// `else`.
    Else = "T_ELSE", 289;
    /// `endif`.
    Endif = "T_ENDIF", 290;
    /// `echo`.
    Echo = "T_ECHO", 291;
    /// `do`.
    Do = "T_DO", 292;
    /// `while`.
    While = "T_WHILE", 293;
    /// `endwhile`.
    Endwhile = "T_ENDWHILE", 294;
    /// `for`.
    For = "T_FOR", 295;
    /// `endfor`.
    Endfor = "T_ENDFOR", 296;
    /// `foreach`.
    Foreach = "T_FOREACH", 297;
    /// `endforeach`.
    Endforeach = "T_ENDFOREACH", 298;
    /// `declare`.
    Declare = "T_DECLARE", 299;
    /// `enddeclare`.
    Enddeclare = "T_ENDDECLARE", 300;
    /// `as`.
    As = "T_AS", 301;
    /// `switch`.
    Switch = "T_SWITCH", 302;
    /// `endswitch`.
    Endswitch = "T_ENDSWITCH", 303;
    /// `case`.
    Case = "T_CASE", 304;
    /// `default`.
    Default = "T_DEFAULT", 305;
    /// `match`.
    Match = "T_MATCH", 306;
    /// `break`.
    Break = "T_BREAK", 307;
    /// `continue`.
    Continue = "T_CONTINUE", 308;
    /// `goto`.
    Goto = "T_GOTO", 309;
    /// `function`.
    Function = "T_FUNCTION", 310;
    /// `fn`.
    Fn = "T_FN", 311;
    /// `const`.
    Const = "T_CONST", 312;
    /// `return`.
    Return = "T_RETURN", 313;
    /// `try`.
    Try = "T_TRY", 314;
    /// `catch`.
    Catch = "T_CATCH", 315;
    /// `finally`.
    Finally = "T_FINALLY", 316;
    /// `throw`.
    Throw = "T_THROW", 317;
    /// `use`.
    Use = "T_USE", 318;
    /// `insteadof`.
    Insteadof = "T_INSTEADOF", 319;
    /// `global`.
    Global = "T_GLOBAL", 320;
    /// `static`.
    Static = "T_STATIC", 321;
    /// `abstract`.
    Abstract = "T_ABSTRACT", 322;
    /// `final`.
    Final = "T_FINAL", 323;
    /// `private`.
    Private = "T_PRIVATE", 324;
    /// `protected`.
    Protected = "T_PROTECTED", 325;
    /// `public`.
    Public = "T_PUBLIC", 326;
    /// `private(set)`.
    PrivateSet = "T_PRIVATE_SET", 327;
    /// `protected(set)`.
    ProtectedSet = "T_PROTECTED_SET", 328;
    /// `public(set)`.
    PublicSet = "T_PUBLIC_SET", 329;
    /// `readonly`.
    Readonly = "T_READONLY", 330;
    /// `var`.
    Var = "T_VAR", 331;
    /// `unset`.
    Unset = "T_UNSET", 332;
    /// `isset`.
    Isset = "T_ISSET", 333;
    /// `empty`.
    Empty = "T_EMPTY", 334;
    /// `__halt_compiler`.
    HaltCompiler = "T_HALT_COMPILER", 335;
    /// `class`.
    Class = "T_CLASS", 336;
    /// `trait`.
    Trait = "T_TRAIT", 337;
    /// `interface`.
    Interface = "T_INTERFACE", 338;
    /// `enum` followed by a name.
    Enum = "T_ENUM", 339;
    /// `extends`.
    Extends = "T_EXTENDS", 340;
    /// `implements`.
    Implements = "T_IMPLEMENTS", 341;
    /// `namespace`.
    Namespace = "T_NAMESPACE", 342;
    /// `list`.
    List = "T_LIST", 343;
    /// `array`.
    Array = "T_ARRAY", 344;
    /// `callable`.
    Callable = "T_CALLABLE", 345;
    /// `__LINE__`.
    LineMagic = "T_LINE", 346;
    /// `__FILE__`.
    FileMagic = "T_FILE", 347;
    /// `__DIR__`.
    DirMagic = "T_DIR", 348;
    /// `__CLASS__`.
    ClassMagic = "T_CLASS_C", 349;
    /// `__TRAIT__`.
    TraitMagic = "T_TRAIT_C", 350;
    /// `__METHOD__`.
    MethodMagic = "T_METHOD_C", 351;
    /// `__FUNCTION__`.
    FunctionMagic = "T_FUNC_C", 352;
    /// `__PROPERTY__`.
    PropertyMagic = "T_PROPERTY_C", 353;
    /// `__NAMESPACE__`.
    NamespaceMagic = "T_NS_C", 354;
    /// `#[`.
    Attribute = "T_ATTRIBUTE", 355;
    /// `+=`.
    PlusAssign = "T_PLUS_EQUAL", 356;
    /// `-=`.
    MinusAssign = "T_MINUS_EQUAL", 357;
    /// `*=`.
    MulAssign = "T_MUL_EQUAL", 358;
    /// `/=`.
    DivAssign = "T_DIV_EQUAL", 359;
    /// `.=`.
    ConcatAssign = "T_CONCAT_EQUAL", 360;
    /// `%=`.
    ModAssign = "T_MOD_EQUAL", 361;
    /// `&=`.
    AndAssign = "T_AND_EQUAL", 362;
    /// `|=`.
    OrAssign = "T_OR_EQUAL", 363;
    /// `^=`.
    XorAssign = "T_XOR_EQUAL", 364;
    /// `<<=`.
    ShiftLeftAssign = "T_SL_EQUAL", 365;
    /// `>>=`.
    ShiftRightAssign = "T_SR_EQUAL", 366;
    /// `??=`.
    CoalesceAssign = "T_COALESCE_EQUAL", 367;
    /// `||`.
    BooleanOr = "T_BOOLEAN_OR", 368;
    /// `&&`.
    BooleanAnd = "T_BOOLEAN_AND", 369;
    /// `==`.
    Equal = "T_IS_EQUAL", 370;
    /// `!=` or `<>`.
    NotEqual = "T_IS_NOT_EQUAL", 371;
    /// `===`.
    Identical = "T_IS_IDENTICAL", 372;
    /// `!==`.
    NotIdentical = "T_IS_NOT_IDENTICAL", 373;
    /// `<=`.
    LessOrEqual = "T_IS_SMALLER_OR_EQUAL", 374;
    /// `>=`.
    GreaterOrEqual = "T_IS_GREATER_OR_EQUAL", 375;
    /// `<=>`.
    Spaceship = "T_SPACESHIP", 376;
    /// `<<`.
    ShiftLeft = "T_SL", 377;
    /// `>>`.
    ShiftRight = "T_SR", 378;
    /// `++`.
    Increment = "T_INC", 379;
    /// `--`.
    Decrement = "T_DEC", 380;
    /// `(int)` or `(integer)`.
    IntCast = "T_INT_CAST", 381;
    /// `(float)`, `(double)` or `(real)`.
    FloatCast = "T_DOUBLE_CAST", 382;
    /// `(string)` or `(binary)`.
    StringCast = "T_STRING_CAST", 383;
    /// `(array)`.
    ArrayCast = "T_ARRAY_CAST", 384;
    /// `(object)`.
    ObjectCast = "T_OBJECT_CAST", 385;
    /// `(bool)` or `(boolean)`.
    BoolCast = "T_BOOL_CAST", 386;
    /// `(unset)`.
    UnsetCast = "T_UNSET_CAST", 387;
    /// `(void)`.
    VoidCast = "T_VOID_CAST", 388;
    /// `->`.
    ObjectOperator = "T_OBJECT_OPERATOR", 389;
    /// `?->`.
    NullsafeObjectOperator = "T_NULLSAFE_OBJECT_OPERATOR", 390;
    /// `=>`.
    DoubleArrow = "T_DOUBLE_ARROW", 391;
    /// A `//`, `#` or `/* */` comment.
    Comment = "T_COMMENT", 392;
    /// A `/** */` comment.
    DocComment = "T_DOC_COMMENT", 393;
    /// `<?php` (with the following space or newline) or `<?`.
    OpenTag = "T_OPEN_TAG", 394;
    /// `<?=`.
    OpenTagWithEcho = "T_OPEN_TAG_WITH_ECHO", 395;
    /// `?>`, with a directly following newline.
    CloseTag = "T_CLOSE_TAG", 396;
    /// Spaces, tabs and newlines.
    Whitespace = "T_WHITESPACE", 397;
    /// `<<<LABEL` and its newline.
    StartHeredoc = "T_START_HEREDOC", 398;
    /// The closing label of a heredoc or nowdoc, with its indentation.
    EndHeredoc = "T_END_HEREDOC", 399;
    /// `${` inside an interpolated string.
    DollarOpenCurlyBraces = "T_DOLLAR_OPEN_CURLY_BRACES", 400;
    /// The `{` of `{$` inside an interpolated string.
    CurlyOpen = "T_CURLY_OPEN", 401;
    /// `::`.
    DoubleColon = "T_DOUBLE_COLON", 402;
    /// `\` on its own.
    NsSeparator = "T_NS_SEPARATOR", 403;
    /// `...`.
    Ellipsis = "T_ELLIPSIS", 404;
    /// `??`.
    Coalesce = "T_COALESCE", 405;
    /// `**`.
    Pow = "T_POW", 406;
    /// `**=`.
    PowAssign = "T_POW_EQUAL", 407;
    /// `|>`.
    Pipe = "T_PIPE", 408;
    /// `&` followed (after whitespace and comments) by `$` or `...`.
    AmpersandFollowedByVarOrVararg = "T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG", 409;
    /// Any other `&`.
    AmpersandNotFollowedByVarOrVararg = "T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG", 410;
    /// A byte that starts no token.
    BadCharacter = "T_BAD_CHARACTER", 411;
}

/// One token: its kind, where it lies in the source and the line it starts on.
///
/// Offsets are byte positions; `start..end` is the token's text, which may be empty (PHP reports an
/// empty string fragment when a malformed offset ends `"$a[`). Lines start at 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    /// What the token is.
    pub kind: TokenKind,
    /// Offset of the first byte.
    pub start: u32,
    /// Offset just past the last byte.
    pub end: u32,
    /// Line the token starts on.
    pub line: u32,
}

impl Token {
    /// The token's text within `source`, the input it was read from.
    ///
    /// # Panics
    ///
    /// When `source` is shorter than the input the token came from.
    pub fn text<'s>(&self, source: &'s [u8]) -> &'s [u8] {
        &source[self.start as usize..self.end as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::{Token, TokenKind};
    use std::collections::HashSet;

    #[test]
    fn names_and_numbers_follow_php() {
        assert_eq!(TokenKind::NAMED.len(), 152);
        assert_eq!(TokenKind::NAMED[0].name(), Some("T_LNUMBER"));
        assert_eq!(TokenKind::Integer.id(), 260);
        assert_eq!(TokenKind::BadCharacter.id(), 411);
        assert_eq!(TokenKind::Char(b';').id(), 59);
        assert_eq!(TokenKind::Char(b';').name(), None);
        for (index, kind) in TokenKind::NAMED.iter().enumerate() {
            assert_eq!(usize::from(kind.id()), 260 + index, "{kind:?}");
        }
        let names: HashSet<_> = TokenKind::NAMED.iter().map(|kind| kind.name()).collect();
        assert_eq!(names.len(), TokenKind::NAMED.len());
    }

    #[test]
    fn text_slices_the_source() {
        let token = Token {
            kind: TokenKind::Variable,
            start: 1,
            end: 3,
            line: 1,
        };
        assert_eq!(token.text(b" $a;"), b"$a");
    }
}
