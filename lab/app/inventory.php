<?php
/*
 * Ferropress lab inventory: prints the interoperability facts of the installed reference platform as JSON.
 *
 * Scans the platform's PHP sources for declared functions, classes, hooks, constants and globals, then boots
 * the installed site and lists its REST routes, option names and database tables. Only names, signatures,
 * defaults, schema and source locations are emitted; no code or documentation text.
 *
 * Run by `cargo xtask lab inventory` on a freshly reset lab; never part of Ferropress.
 */

declare(strict_types=1);

const INVENTORY_ROOT = '/var/www/html';
const INVENTORY_SCHEMA = 1;

/** Hook-firing functions: kind, and whether callback arguments follow the name or sit in an array. */
const HOOK_CALLS = [
	'do_action'                => array( 'action', 'direct' ),
	'do_action_ref_array'      => array( 'action', 'array' ),
	'do_action_deprecated'     => array( 'action', 'array' ),
	'apply_filters'            => array( 'filter', 'direct' ),
	'apply_filters_ref_array'  => array( 'filter', 'array' ),
	'apply_filters_deprecated' => array( 'filter', 'array' ),
];

const OPEN_BRACES = array( '{', '(', '[' );
const CLOSE_BRACES = array( '}', ')', ']' );
const MODIFIERS = array(
	T_PUBLIC, T_PROTECTED, T_PRIVATE, T_STATIC, T_ABSTRACT, T_FINAL, T_READONLY, T_VAR,
	T_PUBLIC_SET, T_PROTECTED_SET, T_PRIVATE_SET,
);

final class SourceScanner {
	/** @var list<PhpToken> */
	private array $tokens = array();
	private string $location_file = '';
	private string $namespace = '';
	/** @var array<string, string> lowercase alias => qualified name */
	private array $imports = array();

	public array $functions = array();
	public array $classes = array();
	public array $hooks = array();
	public array $constants = array();
	public array $globals = array();

	public function scan( string $relative, string $code ): void {
		$this->tokens = array_values(
			array_filter(
				PhpToken::tokenize( $code ),
				static fn ( PhpToken $t ): bool => ! $t->is(
					array( T_WHITESPACE, T_COMMENT, T_DOC_COMMENT, T_OPEN_TAG, T_CLOSE_TAG, T_INLINE_HTML )
				)
			)
		);
		$this->location_file = $relative;
		$this->namespace     = '';
		$this->imports       = array();

		/** @var list<array{kind: string, class?: ?string}> $scopes */
		$scopes  = array();
		$pending = null;
		$count   = count( $this->tokens );

		for ( $i = 0; $i < $count; $i++ ) {
			$t   = $this->tokens[ $i ];
			$top = $scopes === array() ? null : $scopes[ count( $scopes ) - 1 ];

			if ( $t->text === '{' || $t->is( array( T_CURLY_OPEN, T_DOLLAR_OPEN_CURLY_BRACES ) ) ) {
				if ( $t->text === '{' && $pending !== null ) {
					$scopes[] = $pending;
					$pending  = null;
				} else {
					$scopes[] = array( 'kind' => 'block' );
				}
				continue;
			}
			if ( $t->text === '}' ) {
				array_pop( $scopes );
				continue;
			}
			if ( $t->text === ';' ) {
				if ( $pending !== null && $pending['kind'] === 'function' ) {
					$pending = null;
				}
				continue;
			}

			$in_class    = $top !== null && $top['kind'] === 'class';
			$in_anon     = $top !== null && $top['kind'] === 'anon';
			$in_function = $this->inside( $scopes, 'function' );

			switch ( true ) {
				case $t->is( T_NAMESPACE ):
					$next = $this->tokens[ $i + 1 ] ?? null;
					if ( $next !== null && $next->is( array( T_STRING, T_NAME_QUALIFIED ) ) ) {
						$this->namespace = $next->text;
						$this->imports   = array();
					} elseif ( $next !== null && $next->text === '{' ) {
						$this->namespace = '';
						$this->imports   = array();
					}
					break;

				case $t->is( T_USE ):
					if ( $in_class ) {
						$i = $this->trait_uses( $i, $top['class'] );
					} elseif ( ! $in_function && ! $in_anon ) {
						$i = $this->imports_statement( $i );
					}
					break;

				case $t->is( array( T_CLASS, T_INTERFACE, T_TRAIT, T_ENUM ) ):
					$previous = $this->tokens[ $i - 1 ] ?? null;
					if ( $previous !== null && $previous->is( T_DOUBLE_COLON ) ) {
						break;
					}
					if ( $previous !== null && $previous->is( T_NEW ) ) {
						$pending = array( 'kind' => 'anon' );
						break;
					}
					[ $i, $class ] = $this->class_header( $i );
					$pending       = array( 'kind' => 'class', 'class' => $class );
					break;

				case $t->is( T_FUNCTION ):
					$j = $i + 1;
					if ( $this->is_ampersand( $j ) ) {
						$j++;
					}
					$name = $this->tokens[ $j ] ?? null;
					if ( $name === null || $name->text === '(' ) {
						$pending = array( 'kind' => 'function' );
						break;
					}
					[ $end, $signature ] = $this->signature( $j );
					$signature['by_ref'] = $j !== $i + 1;
					if ( $in_class ) {
						$this->add_method( $top['class'], $name->text, $signature, $this->modifiers_before( $i ) );
					} elseif ( ! $in_anon ) {
						$this->add_function( $this->qualify( $name->text ), $signature, $name->line );
					}
					$pending = array( 'kind' => 'function' );
					$i       = $end - 1;
					break;

				case $t->is( T_CONST ):
					$names = $this->constant_names( $i );
					if ( $in_class ) {
						foreach ( $names as $constant ) {
							$this->classes[ $top['class'] ]['constants'][ $constant ] = true;
						}
					} elseif ( ! $in_function && ! $in_anon ) {
						foreach ( $names as $constant ) {
							$this->add_constant( $this->qualify( $constant ), $t->line );
						}
					}
					break;

				case $in_class && $t->is( MODIFIERS ):
					$i = $this->property_statement( $i, $top['class'] );
					break;

				case $t->is( T_GLOBAL ):
					for ( $j = $i + 1; $j < $count && $this->tokens[ $j ]->text !== ';'; $j++ ) {
						if ( $this->tokens[ $j ]->is( T_VARIABLE ) && $this->tokens[ $j - 1 ]->text !== '$' ) {
							$this->globals[ substr( $this->tokens[ $j ]->text, 1 ) ][ $this->location_file ] = true;
						}
					}
					break;

				case $t->is( array( T_STRING, T_NAME_FULLY_QUALIFIED ) ) && $this->is_call( $i ):
					$function = strtolower( ltrim( $t->text, '\\' ) );
					if ( $function === 'define' ) {
						$this->define_call( $i );
					} elseif ( isset( HOOK_CALLS[ $function ] ) ) {
						$this->hook_call( $i, $function );
					}
					break;
			}
		}
	}

	private function inside( array $scopes, string $kind ): bool {
		foreach ( $scopes as $scope ) {
			if ( $scope['kind'] === $kind ) {
				return true;
			}
		}
		return false;
	}

	private function is_ampersand( int $i ): bool {
		$t = $this->tokens[ $i ] ?? null;
		return $t !== null && ( $t->text === '&' );
	}

	private function is_call( int $i ): bool {
		$next     = $this->tokens[ $i + 1 ] ?? null;
		$previous = $this->tokens[ $i - 1 ] ?? null;
		return $next !== null && $next->text === '('
			&& ! ( $previous !== null && $previous->is( array( T_OBJECT_OPERATOR, T_NULLSAFE_OBJECT_OPERATOR, T_DOUBLE_COLON, T_FUNCTION, T_NEW, T_CONST ) ) );
	}

	private function location( int $line ): string {
		return $this->location_file . ':' . $line;
	}

	private function qualify( string $name ): string {
		return $this->namespace === '' ? $name : $this->namespace . '\\' . $name;
	}

	/** Resolves a class name as written to its fully qualified form. */
	private function resolve( string $name ): string {
		if ( str_starts_with( $name, '\\' ) ) {
			return substr( $name, 1 );
		}
		if ( str_starts_with( strtolower( $name ), 'namespace\\' ) ) {
			return $this->qualify( substr( $name, 10 ) );
		}
		$parts = explode( '\\', $name, 2 );
		$alias = strtolower( $parts[0] );
		if ( isset( $this->imports[ $alias ] ) ) {
			return $this->imports[ $alias ] . ( isset( $parts[1] ) ? '\\' . $parts[1] : '' );
		}
		return $this->qualify( $name );
	}

	/** Whether the token opens a bracket pair: `{ ( [`, `#[` or a string interpolation brace. */
	private static function opens( PhpToken $t ): bool {
		return in_array( $t->text, OPEN_BRACES, true ) || $t->is( array( T_CURLY_OPEN, T_DOLLAR_OPEN_CURLY_BRACES, T_ATTRIBUTE ) );
	}

	/** The tokens without `#[...]` attribute groups. */
	private static function without_attributes( array $tokens ): array {
		$kept  = array();
		$depth = 0;
		foreach ( $tokens as $t ) {
			if ( $depth > 0 ) {
				if ( self::opens( $t ) ) {
					$depth++;
				} elseif ( in_array( $t->text, CLOSE_BRACES, true ) ) {
					$depth--;
				}
				continue;
			}
			if ( $t->is( T_ATTRIBUTE ) ) {
				$depth = 1;
				continue;
			}
			$kept[] = $t;
		}
		return $kept;
	}

	/** Index of the bracket closing the one at `$open`. */
	private function closing( int $open ): int {
		$depth = 0;
		$count = count( $this->tokens );
		for ( $i = $open; $i < $count; $i++ ) {
			$t = $this->tokens[ $i ];
			if ( self::opens( $t ) ) {
				$depth++;
			} elseif ( in_array( $t->text, CLOSE_BRACES, true ) ) {
				$depth--;
				if ( $depth === 0 ) {
					return $i;
				}
			}
		}
		return $count - 1;
	}

	/** Splits the tokens strictly between `$open` and `$close` at top-level commas. */
	private function split( int $open, int $close ): array {
		$parts   = array();
		$current = array();
		$depth   = 0;
		for ( $i = $open + 1; $i < $close; $i++ ) {
			$t = $this->tokens[ $i ];
			if ( self::opens( $t ) ) {
				$depth++;
			} elseif ( in_array( $t->text, CLOSE_BRACES, true ) ) {
				$depth--;
			}
			if ( $depth === 0 && $t->text === ',' ) {
				$parts[] = $current;
				$current = array();
				continue;
			}
			$current[] = $t;
		}
		if ( $current !== array() ) {
			$parts[] = $current;
		}
		return $parts;
	}

	/** Joins tokens into compact canonical text, with a space only between two word-like tokens. */
	public static function text( array $tokens ): string {
		$out = '';
		foreach ( $tokens as $t ) {
			$piece = $t->text;
			if ( $out !== '' && preg_match( '/[\w$\\\\]$/', $out ) && preg_match( '/^[\w$\\\\]/', $piece ) ) {
				$out .= ' ';
			}
			$out .= $piece;
		}
		return $out;
	}

	private static function literal( PhpToken $t ): ?string {
		if ( ! $t->is( T_CONSTANT_ENCAPSED_STRING ) ) {
			return null;
		}
		$inner = substr( $t->text, 1, -1 );
		return $t->text[0] === "'" ? strtr( $inner, array( "\\'" => "'", '\\\\' => '\\' ) ) : stripcslashes( $inner );
	}

	/** @return array{0: int, 1: array} index after the signature, and the signature. */
	private function signature( int $name_index ): array {
		$open   = $name_index + 1;
		$close  = $this->closing( $open );
		$params = array();
		foreach ( $this->split( $open, $close ) as $tokens ) {
			$params[] = $this->parameter( $tokens );
		}
		$end         = $close + 1;
		$return_type = null;
		if ( ( $this->tokens[ $end ]->text ?? '' ) === ':' ) {
			$start = $end + 1;
			for ( $end = $start; isset( $this->tokens[ $end ] ) && ! in_array( $this->tokens[ $end ]->text, array( '{', ';' ), true ); $end++ );
			$return_type = self::text( array_slice( $this->tokens, $start, $end - $start ) );
		}
		return array( $end, array( 'params' => $params, 'return_type' => $return_type ) );
	}

	private function parameter( array $tokens ): array {
		$param = array(
			'name'     => '',
			'type'     => null,
			'default'  => null,
			'by_ref'   => false,
			'variadic' => false,
			'promoted' => false,
		);
		$type   = array();
		$tokens = self::without_attributes( $tokens );
		foreach ( $tokens as $k => $t ) {
			if ( $t->is( T_VARIABLE ) ) {
				$param['name'] = substr( $t->text, 1 );
				$rest          = array_slice( $tokens, $k + 1 );
				if ( $rest !== array() && $rest[0]->text === '=' ) {
					$param['default'] = self::text( array_slice( $rest, 1 ) );
				}
				break;
			}
			if ( $t->is( MODIFIERS ) ) {
				$param['promoted'] = true;
			} elseif ( $t->text === '&' ) {
				$param['by_ref'] = true;
			} elseif ( $t->is( T_ELLIPSIS ) ) {
				$param['variadic'] = true;
			} else {
				$type[] = $t;
			}
		}
		$param['type'] = $type === array() ? null : self::text( $type );
		return $param;
	}

	/** Lowercase modifier keywords before the token at `$index`, back to the previous statement boundary. */
	private function modifiers_before( int $index ): array {
		$found = array();
		for ( $i = $index - 1; $i >= 0; $i-- ) {
			$t = $this->tokens[ $i ];
			if ( $t->is( MODIFIERS ) ) {
				$found[] = strtolower( $t->text );
			} elseif ( ! $t->is( T_ATTRIBUTE ) && $t->text !== ']' ) {
				break;
			}
		}
		return $found;
	}

	private static function visibility( array $modifiers ): string {
		foreach ( array( 'private', 'protected' ) as $visibility ) {
			if ( in_array( $visibility, $modifiers, true ) ) {
				return $visibility;
			}
		}
		return 'public';
	}

	private function add_function( string $name, array $signature, int $line ): void {
		$key                                       = strtolower( $name );
		$this->functions[ $key ]['name']         ??= $name;
		$this->functions[ $key ]['declarations'][] = array( 'location' => $this->location( $line ) ) + $signature;
	}

	private function add_method( string $class, string $name, array $signature, array $modifiers ): void {
		$key = strtolower( $name );
		if ( isset( $this->classes[ $class ]['methods'][ $key ] ) ) {
			return;
		}
		$this->classes[ $class ]['methods'][ $key ] = array(
			'name'       => $name,
			'visibility' => self::visibility( $modifiers ),
			'static'     => in_array( 'static', $modifiers, true ),
			'abstract'   => in_array( 'abstract', $modifiers, true ),
			'final'      => in_array( 'final', $modifiers, true ),
		) + $signature;
	}

	private function add_constant( string $name, int $line ): void {
		$this->constants[ $name ][] = $this->location( $line );
	}

	/** @return array{0: int, 1: string} index before the body, and the class key. */
	private function class_header( int $i ): array {
		$kind      = strtolower( $this->tokens[ $i ]->text );
		$name      = $this->qualify( $this->tokens[ $i + 1 ]->text );
		$modifiers = $this->modifiers_before( $i );
		$extends   = array();
		$implements = array();
		$target    = null;
		for ( $j = $i + 2; isset( $this->tokens[ $j ] ) && $this->tokens[ $j ]->text !== '{'; $j++ ) {
			$t = $this->tokens[ $j ];
			if ( $t->is( T_EXTENDS ) ) {
				$target = 'extends';
			} elseif ( $t->is( T_IMPLEMENTS ) ) {
				$target = 'implements';
			} elseif ( $t->text === ':' ) {
				$target = null;
			} elseif ( $target !== null && $t->is( array( T_STRING, T_NAME_QUALIFIED, T_NAME_FULLY_QUALIFIED, T_NAME_RELATIVE ) ) ) {
				${$target}[] = $this->resolve( $t->text );
			}
		}
		$key = strtolower( $name );
		if ( ! isset( $this->classes[ $key ] ) ) {
			$this->classes[ $key ] = array(
				'name'         => $name,
				'kind'         => $kind,
				'abstract'     => in_array( 'abstract', $modifiers, true ),
				'final'        => in_array( 'final', $modifiers, true ),
				'extends'      => $extends,
				'implements'   => $implements,
				'traits'       => array(),
				'constants'    => array(),
				'properties'   => array(),
				'methods'      => array(),
				'declarations' => array(),
			);
		}
		$this->classes[ $key ]['declarations'][] = $this->location( $this->tokens[ $i ]->line );
		return array( $j - 1, $key );
	}

	private function imports_statement( int $i ): int {
		$j    = $i + 1;
		$kind = 'class';
		if ( $this->tokens[ $j ]->is( array( T_FUNCTION, T_CONST ) ) ) {
			$kind = 'other';
			$j++;
		}
		$name  = null;
		$alias = null;
		for ( ; isset( $this->tokens[ $j ] ); $j++ ) {
			$t = $this->tokens[ $j ];
			if ( $t->text === ';' || $t->text === ',' ) {
				if ( $kind === 'class' && $name !== null ) {
					$short = $alias ?? substr( strrchr( '\\' . $name, '\\' ), 1 );
					$this->imports[ strtolower( $short ) ] = $name;
				}
				$name  = null;
				$alias = null;
				if ( $t->text === ';' ) {
					break;
				}
			} elseif ( $t->text === '{' ) {
				$j = $this->closing( $j );
			} elseif ( $t->is( T_AS ) ) {
				$alias = $this->tokens[ ++$j ]->text;
			} elseif ( $t->is( array( T_STRING, T_NAME_QUALIFIED, T_NAME_FULLY_QUALIFIED ) ) ) {
				$name = ltrim( $t->text, '\\' );
			}
		}
		return $j;
	}

	private function trait_uses( int $i, string $class ): int {
		for ( $j = $i + 1; isset( $this->tokens[ $j ] ); $j++ ) {
			$t = $this->tokens[ $j ];
			if ( $t->text === ';' ) {
				return $j;
			}
			if ( $t->text === '{' ) {
				return $this->closing( $j );
			}
			if ( $t->is( array( T_STRING, T_NAME_QUALIFIED, T_NAME_FULLY_QUALIFIED ) ) ) {
				$this->classes[ $class ]['traits'][] = $this->resolve( $t->text );
			}
		}
		return $j;
	}

	/** Names declared by a `const` statement: each identifier directly followed by `=` at top level. */
	private function constant_names( int $i ): array {
		$names = array();
		$depth = 0;
		for ( $j = $i + 1; isset( $this->tokens[ $j ] ) && ! ( $depth === 0 && $this->tokens[ $j ]->text === ';' ); $j++ ) {
			$t = $this->tokens[ $j ];
			if ( in_array( $t->text, OPEN_BRACES, true ) ) {
				$depth++;
			} elseif ( in_array( $t->text, CLOSE_BRACES, true ) ) {
				$depth--;
			} elseif ( $depth === 0 && ( $this->tokens[ $j + 1 ]->text ?? '' ) === '=' && preg_match( '/^\w+$/', $t->text ) ) {
				$names[] = $t->text;
			}
		}
		return $names;
	}

	/** A property declaration starting at a modifier; returns where scanning continues. */
	private function property_statement( int $i, string $class ): int {
		$modifiers = array();
		$type      = array();
		for ( $j = $i; isset( $this->tokens[ $j ] ); $j++ ) {
			$t = $this->tokens[ $j ];
			if ( $t->is( array( T_FUNCTION, T_CONST, T_USE ) ) || $t->text === ';' && $type === array() && $modifiers === array() ) {
				return $i;
			}
			if ( $t->is( MODIFIERS ) ) {
				$modifiers[] = strtolower( $t->text );
				continue;
			}
			if ( $t->is( T_VARIABLE ) ) {
				break;
			}
			$type[] = $t;
		}
		if ( ! isset( $this->tokens[ $j ] ) || ! $this->tokens[ $j ]->is( T_VARIABLE ) ) {
			return $i;
		}
		$depth = 0;
		$first = true;
		for ( ; isset( $this->tokens[ $j ] ); $j++ ) {
			$t = $this->tokens[ $j ];
			if ( $depth === 0 && $t->text === ';' ) {
				break;
			}
			if ( in_array( $t->text, OPEN_BRACES, true ) ) {
				$depth++;
			} elseif ( in_array( $t->text, CLOSE_BRACES, true ) ) {
				$depth--;
			} elseif ( $depth === 0 && $t->is( T_VARIABLE ) && ( $first || $this->tokens[ $j - 1 ]->text === ',' ) ) {
				$first = false;
				$name  = substr( $t->text, 1 );
				$this->classes[ $class ]['properties'][ $name ] = array(
					'name'       => $name,
					'visibility' => self::visibility( $modifiers ),
					'static'     => in_array( 'static', $modifiers, true ),
					'readonly'   => in_array( 'readonly', $modifiers, true ),
					'type'       => $type === array() ? null : self::text( $type ),
				);
			}
		}
		return $j;
	}

	private function define_call( int $i ): void {
		$close = $this->closing( $i + 1 );
		$args  = $this->split( $i + 1, $close );
		if ( isset( $args[0] ) && count( $args[0] ) === 1 ) {
			$name = self::literal( $args[0][0] );
			if ( $name !== null ) {
				$this->add_constant( $name, $this->tokens[ $i ]->line );
			}
		}
	}

	private function hook_call( int $i, string $function ): void {
		[ $kind, $style ] = HOOK_CALLS[ $function ];
		$close            = $this->closing( $i + 1 );
		$args             = $this->split( $i + 1, $close );
		if ( $args === array() ) {
			return;
		}
		[ $name, $dynamic ] = self::hook_name( $args[0] );
		$callback_args      = $style === 'direct' ? count( $args ) - 1 : self::array_length( $args[1] ?? array() );
		$key                = $kind . ' ' . $name;
		$this->hooks[ $key ] ??= array(
			'kind'       => $kind,
			'name'       => $name,
			'dynamic'    => $dynamic,
			'deprecated' => false,
			'sites'      => array(),
		);
		if ( str_ends_with( $function, '_deprecated' ) ) {
			$this->hooks[ $key ]['deprecated'] = true;
		}
		$this->hooks[ $key ]['sites'][] = array(
			'location' => $this->location( $this->tokens[ $i ]->line ),
			'args'     => $callback_args,
		);
	}

	/** Number of elements in a literal array expression, or null when it is not a literal array. */
	private static function array_length( array $tokens ): ?int {
		if ( $tokens === array() ) {
			return null;
		}
		$scanner         = new self();
		$scanner->tokens = $tokens;
		if ( $tokens[0]->is( T_ARRAY ) && ( $tokens[1]->text ?? '' ) === '(' ) {
			$open = 1;
		} elseif ( $tokens[0]->text === '[' ) {
			$open = 0;
		} else {
			return null;
		}
		$close = $scanner->closing( $open );
		if ( $close !== count( $tokens ) - 1 ) {
			return null;
		}
		return count( $scanner->split( $open, $close ) );
	}

	/** @return array{0: string, 1: bool} hook name with `{expression}` for computed parts, and whether any part is computed. */
	private static function hook_name( array $tokens ): array {
		$parts   = array( array() );
		$depth   = 0;
		$quoted  = false;
		foreach ( $tokens as $t ) {
			if ( $t->text === '"' ) {
				$quoted = ! $quoted;
			} elseif ( ! $quoted && in_array( $t->text, OPEN_BRACES, true ) ) {
				$depth++;
			} elseif ( ! $quoted && in_array( $t->text, CLOSE_BRACES, true ) ) {
				$depth--;
			}
			if ( ! $quoted && $depth === 0 && $t->text === '.' ) {
				$parts[] = array();
				continue;
			}
			$parts[ count( $parts ) - 1 ][] = $t;
		}
		$name    = '';
		$dynamic = false;
		foreach ( $parts as $part ) {
			$literal = count( $part ) === 1 ? self::literal( $part[0] ) : null;
			if ( $literal !== null ) {
				$name .= $literal;
			} elseif ( $part !== array() && $part[0]->text === '"' && end( $part )->text === '"' ) {
				$name   .= self::interpolated( array_slice( $part, 1, -1 ) );
				$dynamic = true;
			} else {
				$name   .= '{' . self::text( $part ) . '}';
				$dynamic = true;
			}
		}
		return array( $name, $dynamic );
	}

	/** Renders the inside of an interpolated string with every embedded expression as `{expression}`. */
	private static function interpolated( array $tokens ): string {
		$out   = '';
		$count = count( $tokens );
		for ( $k = 0; $k < $count; $k++ ) {
			$t = $tokens[ $k ];
			if ( $t->is( T_ENCAPSED_AND_WHITESPACE ) ) {
				$out .= $t->text;
			} elseif ( $t->is( array( T_CURLY_OPEN, T_DOLLAR_OPEN_CURLY_BRACES ) ) ) {
				$inner = array();
				for ( $depth = 1, $k++; $k < $count; $k++ ) {
					if ( $tokens[ $k ]->text === '}' && --$depth === 0 ) {
						break;
					}
					if ( $tokens[ $k ]->text === '{' ) {
						$depth++;
					}
					$inner[] = $tokens[ $k ];
				}
				$out .= '{' . self::text( $inner ) . '}';
			} else {
				$inner = array( $t );
				while ( $k + 1 < $count && ! $tokens[ $k + 1 ]->is( array( T_ENCAPSED_AND_WHITESPACE, T_CURLY_OPEN, T_DOLLAR_OPEN_CURLY_BRACES ) ) ) {
					$inner[] = $tokens[ ++$k ];
				}
				$out .= '{' . self::text( $inner ) . '}';
			}
		}
		return $out;
	}
}

/** @return list<string> PHP files of the platform, relative to the root, excluding site content and config. */
function inventory_files(): array {
	$files    = array();
	$iterator = new RecursiveIteratorIterator(
		new RecursiveDirectoryIterator( INVENTORY_ROOT, FilesystemIterator::SKIP_DOTS )
	);
	foreach ( $iterator as $file ) {
		$relative = substr( $file->getPathname(), strlen( INVENTORY_ROOT ) + 1 );
		if ( str_ends_with( $relative, '.php' ) && ! str_starts_with( $relative, 'wp-content/' ) && $relative !== 'wp-config.php' ) {
			$files[] = $relative;
		}
	}
	sort( $files, SORT_STRING );
	return $files;
}

$scanner = new SourceScanner();
foreach ( inventory_files() as $relative ) {
	$scanner->scan( $relative, (string) file_get_contents( INVENTORY_ROOT . '/' . $relative ) );
}

$_SERVER['HTTP_HOST']   = '127.0.0.1:8080';
$_SERVER['REQUEST_URI'] = '/';
require INVENTORY_ROOT . '/wp-load.php';

$runtime_constants = get_defined_constants( true )['user'] ?? array();
$runtime_globals   = array_flip( array_keys( $GLOBALS ) );

$routes = array();
foreach ( rest_get_server()->get_routes() as $route => $handlers ) {
	$methods = array();
	foreach ( $handlers as $handler ) {
		$methods += $handler['methods'] ?? array();
	}
	$options  = rest_get_server()->get_route_options( $route );
	$routes[] = array(
		'route'     => $route,
		'namespace' => (string) ( $options['namespace'] ?? '' ),
		'methods'   => array_keys( $methods ),
	);
}

$options = array();
foreach ( $wpdb->get_results( "SELECT option_name, autoload FROM {$wpdb->options}", ARRAY_A ) as $row ) {
	if ( ! str_starts_with( $row['option_name'], '_transient_' ) && ! str_starts_with( $row['option_name'], '_site_transient_' ) ) {
		$options[] = array( 'name' => $row['option_name'], 'autoload' => $row['autoload'] );
	}
}

$tables = array();
$scopes = array(
	'multisite' => array_values( $wpdb->tables( 'ms_global', false ) ),
	'global'    => array_values( array_diff( $wpdb->tables( 'global', false ), $wpdb->tables( 'ms_global', false ) ) ),
	'site'      => array_values( $wpdb->tables( 'blog', false ) ),
);
foreach ( $scopes as $scope => $names ) {
	foreach ( $names as $table ) {
		$full    = $wpdb->prefix . $table;
		$exists  = $wpdb->get_var( $wpdb->prepare( 'SHOW TABLES LIKE %s', $full ) ) === $full;
		$columns = array();
		$indexes = array();
		if ( $exists ) {
			foreach ( $wpdb->get_results( "SHOW COLUMNS FROM `{$full}`", ARRAY_A ) as $column ) {
				$columns[] = array(
					'name'     => $column['Field'],
					'type'     => $column['Type'],
					'nullable' => $column['Null'] === 'YES',
					'default'  => $column['Default'],
					'extra'    => $column['Extra'],
				);
			}
			foreach ( $wpdb->get_results( "SHOW INDEX FROM `{$full}`", ARRAY_A ) as $index ) {
				$indexes[ $index['Key_name'] ]['name']      = $index['Key_name'];
				$indexes[ $index['Key_name'] ]['unique']    = (int) $index['Non_unique'] === 0;
				$indexes[ $index['Key_name'] ]['columns'][] = $index['Column_name'] . ( $index['Sub_part'] !== null ? '(' . $index['Sub_part'] . ')' : '' );
			}
		}
		$tables[] = array(
			'name'    => $table,
			'scope'   => $scope,
			'created' => $exists,
			'columns' => $columns,
			'indexes' => array_values( $indexes ),
		);
	}
}

$constants = array();
foreach ( $scanner->constants as $name => $declarations ) {
	$constants[ $name ] = array( 'name' => $name, 'declarations' => $declarations, 'defined_at_boot' => isset( $runtime_constants[ $name ] ) );
}

$globals = array();
foreach ( $scanner->globals as $name => $files ) {
	$globals[] = array( 'name' => (string) $name, 'files' => array_keys( $files ), 'set_at_boot' => isset( $runtime_globals[ $name ] ) );
}

$classes = array();
foreach ( $scanner->classes as $class ) {
	$class['constants']  = array_keys( $class['constants'] );
	$class['properties'] = array_values( $class['properties'] );
	$class['methods']    = array_values( $class['methods'] );
	$classes[]           = $class;
}

echo json_encode(
	array(
		'schema'           => INVENTORY_SCHEMA,
		'platform_version' => $wp_version,
		'functions'        => array_values( $scanner->functions ),
		'classes'          => $classes,
		'hooks'            => array_values( $scanner->hooks ),
		'constants'        => array_values( $constants ),
		'globals'          => $globals,
		'rest_routes'      => $routes,
		'options'          => $options,
		'tables'           => $tables,
	),
	JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR | JSON_INVALID_UTF8_SUBSTITUTE
);
