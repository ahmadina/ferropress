//! Classes: instance properties, instance methods and `new`.

use php_parser::ast::{
    ClassKind, ClassLike, ClassMember, Expr, ExprKind, Function, Member, Modifiers, Name, NameKind,
    PropertyGroup,
};

use super::{Emit, Emitter, Scope};
use crate::bytecode::{ClassDecl, Instruction, Literal, MethodDecl, Opcode, Operand, PropertyDecl};

/// Modifiers this slice does not compile on a class or member.
const UNSUPPORTED_MEMBER: Modifiers = Modifiers::STATIC
    .union(Modifiers::PRIVATE)
    .union(Modifiers::PROTECTED)
    .union(Modifiers::ABSTRACT)
    .union(Modifiers::READONLY)
    .union(Modifiers::SET_VISIBILITY);

impl Emitter<'_> {
    /// Compiles an unconditional top-level class into the unit.
    pub(super) fn class_declaration(&mut self, class: &ClassLike) -> Emit {
        self.builder.line = class.line;
        if class.kind != ClassKind::Class {
            return self.unsupported(match class.kind {
                ClassKind::Interface => "interfaces",
                ClassKind::Trait => "traits",
                ClassKind::Enum => "enums",
                ClassKind::Class => "classes",
            });
        }
        let Some(name) = class.name.as_deref() else {
            return self.unsupported("anonymous classes");
        };
        if class
            .modifiers
            .intersects(Modifiers::ABSTRACT.union(Modifiers::READONLY))
        {
            return self.unsupported("abstract and readonly classes");
        }
        if !class.attributes.is_empty() {
            return self.unsupported("attributes");
        }
        if class.extends.is_some() || !class.implements.is_empty() {
            return self.unsupported("inheritance");
        }
        let mut properties = Vec::new();
        let mut methods = Vec::new();
        self.ctx.current_methods.clear();
        for member in &class.members {
            match member {
                ClassMember::Property(group) => self.property_group(group, &mut properties)?,
                ClassMember::Method(function) => {
                    self.ctx
                        .current_methods
                        .push(function.name.as_slice().into());
                    methods.push(self.method(name, function)?);
                }
                ClassMember::Constant(_) => return self.unsupported("class constants"),
                ClassMember::EnumCase(_) => return self.unsupported("enums"),
                ClassMember::TraitUse(_) => return self.unsupported("traits"),
            }
        }
        self.ctx.classes.push(ClassDecl {
            name: name.into(),
            line: class.line,
            properties,
            methods,
        });
        self.ctx.current_methods.clear();
        Ok(())
    }

    fn property_group(&mut self, group: &PropertyGroup, out: &mut Vec<PropertyDecl>) -> Emit {
        self.builder.line = group.line;
        if group.modifiers.intersects(UNSUPPORTED_MEMBER) {
            return self.unsupported("static, private, typed or hooked properties");
        }
        if group.property_type.is_some() {
            return self.unsupported("property types");
        }
        if !group.attributes.is_empty() {
            return self.unsupported("attributes");
        }
        for property in &group.properties {
            if property.hooks.is_some() {
                return self.unsupported("property hooks");
            }
            out.push(PropertyDecl {
                name: property.name.as_slice().into(),
                default: literal_default(property.default.as_ref()).ok_or(
                    super::Failure::Unsupported {
                        construct: "property defaults",
                        line: property.line,
                    },
                )?,
            });
        }
        Ok(())
    }

    fn method(&mut self, class: &[u8], function: &Function) -> Emit<MethodDecl> {
        self.builder.line = function.line;
        if function.modifiers.intersects(UNSUPPORTED_MEMBER) {
            return self.unsupported("static, private or abstract methods");
        }
        let mut code_name = class.to_vec();
        code_name.extend_from_slice(b"->");
        code_name.extend_from_slice(&function.name);
        let code = {
            let mut inner = Emitter::child(
                self.ctx,
                &code_name,
                Scope::Method {
                    class: class.into(),
                    name: function.name.as_slice().into(),
                },
            );
            inner.compile_code(function, &[])?
        };
        Ok(MethodDecl {
            name: function.name.as_slice().into(),
            line: function.line,
            code,
        })
    }

    /// `new Class(args)`.
    pub(super) fn new_object(
        &mut self,
        class: &Expr,
        args: &php_parser::ast::Args,
        needed: bool,
    ) -> Emit<Operand> {
        let ExprKind::Name(name) = &class.kind else {
            return self.unsupported("dynamic class names");
        };
        if name.kind == NameKind::Relative {
            return self.unsupported("namespace-relative names");
        }
        let php_parser::ast::Args::List(args) = args else {
            return self.unsupported("first-class callables");
        };
        let dest = self.builder.temp()?;
        let class_name = self.builder.string(&name.bytes)?;
        self.builder
            .op(Opcode::New, class_name, Operand::UNUSED, dest)?;
        self.construct(dest, name, args)?;
        Ok(if needed { dest } else { Operand::UNUSED })
    }

    fn construct(&mut self, object: Operand, class: &Name, args: &[php_parser::ast::Arg]) -> Emit {
        let implicit = !self.has_constructor(&class.bytes);
        if implicit && args.is_empty() {
            return Ok(());
        }
        let method = self.builder.string(b"__construct")?;
        let written = self.builder.string(b"__construct")?;
        let mut init = Instruction::new(Opcode::InitMethodCall);
        init.a = object;
        init.b = method;
        init.c = written;
        init.flags = u8::from(implicit);
        self.builder.emit(init)?;
        self.send_args(args)?;
        self.builder
            .op(
                Opcode::Call,
                Operand::UNUSED,
                Operand::UNUSED,
                Operand::UNUSED,
            )
            .map(drop)
    }

    /// `$object->name`.
    pub(super) fn property(&mut self, object: &Expr, name: &Member, needed: bool) -> Emit<Operand> {
        let Member::Name(name) = name else {
            return self.unsupported("computed property names");
        };
        let object = self.expr(object)?;
        let prop = self.builder.string(name)?;
        let dest = self.result(needed)?;
        self.builder.op(Opcode::LoadProp, object, prop, dest)?;
        Ok(dest)
    }

    /// `$object->name = value` and compound forms.
    pub(super) fn store_property(
        &mut self,
        object: &Expr,
        name: &Member,
        value: Operand,
        needed: bool,
    ) -> Emit<Operand> {
        let Member::Name(name) = name else {
            return self.unsupported("computed property names");
        };
        let object = self.expr(object)?;
        let prop = self.builder.string(name)?;
        let mut store = Instruction::new(Opcode::StoreProp);
        store.a = object;
        store.b = prop;
        store.c = value;
        self.builder.emit(store)?;
        if needed {
            Ok(value)
        } else {
            Ok(Operand::UNUSED)
        }
    }

    /// `$object->name(args)`.
    pub(super) fn method_call(
        &mut self,
        object: &Expr,
        name: &Member,
        args: &php_parser::ast::Args,
        needed: bool,
    ) -> Emit<Operand> {
        let Member::Name(name) = name else {
            return self.unsupported("computed method names");
        };
        let php_parser::ast::Args::List(args) = args else {
            return self.unsupported("first-class callables");
        };
        let object = self.expr(object)?;
        let method = self.builder.string(&name.to_ascii_lowercase())?;
        let written = self.builder.string(name)?;
        let mut init = Instruction::new(Opcode::InitMethodCall);
        init.a = object;
        init.b = method;
        init.c = written;
        self.builder.emit(init)?;
        self.send_args(args)?;
        let dest = self.result(needed)?;
        self.builder
            .op(Opcode::Call, Operand::UNUSED, Operand::UNUSED, dest)?;
        Ok(dest)
    }

    fn has_constructor(&self, class: &[u8]) -> bool {
        let named = |name: &[u8]| name.eq_ignore_ascii_case(b"__construct");
        self.ctx.current_methods.iter().any(|name| named(name))
            || self
                .ctx
                .classes
                .iter()
                .rev()
                .find(|decl| decl.name.eq_ignore_ascii_case(class))
                .is_some_and(|decl| decl.methods.iter().any(|method| named(&method.name)))
    }
}

/// A property default the emitter stores as a literal, or `None` when it is not a constant value.
fn literal_default(expr: Option<&Expr>) -> Option<Literal> {
    match expr.map(|expr| &expr.kind) {
        None => Some(Literal::Null),
        Some(ExprKind::Int(value)) => Some(Literal::Int(*value)),
        Some(ExprKind::Float(value)) => Some(Literal::Float(*value)),
        Some(ExprKind::String(bytes)) => Some(Literal::from(bytes.as_slice())),
        Some(ExprKind::Constant(name)) => super::expr::keyword_constant(name),
        _ => None,
    }
}
