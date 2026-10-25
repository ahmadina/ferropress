//! Classes, interfaces, traits and enums: members, `new`, `instanceof` and enum cases.

use php_parser::ast::{
    ClassKind as AstKind, ClassLike, ClassMember, Expr, ExprKind, Function, FunctionBody, Member,
    Modifiers, Name, NameKind, PropertyGroup,
};

use super::{Emit, Emitter, Scope};
use crate::bytecode::{
    ClassDecl, ClassKind, EnumCaseDecl, Instruction, Literal, MethodDecl, Opcode, Operand,
    PropertyDecl,
};

/// Modifiers this slice does not compile on a class or member.
const UNSUPPORTED_MEMBER: Modifiers = Modifiers::STATIC
    .union(Modifiers::PRIVATE)
    .union(Modifiers::PROTECTED)
    .union(Modifiers::ABSTRACT)
    .union(Modifiers::READONLY)
    .union(Modifiers::SET_VISIBILITY);

impl Emitter<'_> {
    /// Compiles an unconditional top-level class-like into the unit.
    pub(super) fn class_declaration(&mut self, class: &ClassLike) -> Emit {
        self.builder.line = class.line;
        self.reject_class_header(class)?;
        let Some(name) = class.name.as_deref() else {
            return self.unsupported("anonymous classes");
        };
        let implements = self.implemented(class)?;
        let mut collected = Collected::default();
        self.ctx.current_methods.clear();
        for member in &class.members {
            self.class_member(name, member, &mut collected)?;
        }
        let deferred = !matches!(self.scope, Scope::File) || self.conditional != 0;
        let index = self.ctx.classes.len();
        self.ctx.classes.push(ClassDecl {
            name: name.into(),
            kind: class_kind(class.kind),
            line: class.line,
            properties: collected.properties,
            methods: collected.methods,
            implements,
            uses: collected.uses,
            cases: collected.cases,
            deferred,
        });
        self.ctx.current_methods.clear();
        if !deferred {
            return Ok(());
        }
        let index = i64::try_from(index)
            .map_err(|_overflow| super::Failure::TooLarge { line: class.line })?;
        let index = self.builder.literal(Literal::Int(index))?;
        self.builder
            .op(Opcode::BindClass, index, Operand::UNUSED, Operand::UNUSED)
            .map(drop)
    }

    fn reject_class_header(&self, class: &ClassLike) -> Emit {
        if class
            .modifiers
            .intersects(Modifiers::ABSTRACT.union(Modifiers::READONLY))
        {
            return self.unsupported("abstract and readonly classes");
        }
        if !class.attributes.is_empty() {
            return self.unsupported("attributes");
        }
        if class.extends.is_some() && class.kind == AstKind::Class {
            return self.unsupported("inheritance");
        }
        if class.kind == AstKind::Enum && class.backing_type.is_some() {
            return self.unsupported("backed enums");
        }
        Ok(())
    }

    fn implemented(&self, class: &ClassLike) -> Emit<Vec<Box<[u8]>>> {
        let mut names = Vec::with_capacity(class.implements.len());
        for name in &class.implements {
            if name.kind == NameKind::Relative {
                return self.unsupported("namespace-relative names");
            }
            names.push(name.bytes.clone().into());
        }
        Ok(names)
    }

    fn class_member(&mut self, class: &[u8], member: &ClassMember, out: &mut Collected) -> Emit {
        match member {
            ClassMember::Property(group) => self.property_group(group, &mut out.properties),
            ClassMember::Method(function) => {
                if matches!(function.body, FunctionBody::Absent) {
                    return Ok(());
                }
                self.ctx
                    .current_methods
                    .push(function.name.as_slice().into());
                out.methods.push(self.method(class, function)?);
                Ok(())
            }
            ClassMember::Constant(_) => self.unsupported("class constants"),
            ClassMember::EnumCase(item) => {
                if item.value.is_some() {
                    return self.unsupported("backed enums");
                }
                if !item.attributes.is_empty() {
                    return self.unsupported("attributes");
                }
                out.cases.push(EnumCaseDecl {
                    name: item.name.as_slice().into(),
                });
                Ok(())
            }
            ClassMember::TraitUse(used) => self.trait_use(used, &mut out.uses),
        }
    }

    fn trait_use(&mut self, used: &php_parser::ast::TraitUse, names: &mut Vec<Box<[u8]>>) -> Emit {
        self.builder.line = used.line;
        if used
            .adaptations
            .as_ref()
            .is_some_and(|items| !items.is_empty())
        {
            return self.unsupported("trait adaptations");
        }
        for name in &used.traits {
            if name.kind == NameKind::Relative {
                return self.unsupported("namespace-relative names");
            }
            names.push(name.bytes.clone().into());
        }
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

    /// `expr instanceof Name`.
    pub(super) fn instanceof(&mut self, expr: &Expr, class: &Expr) -> Emit<Operand> {
        let ExprKind::Name(name) = &class.kind else {
            return self.unsupported("dynamic class names");
        };
        if name.kind == NameKind::Relative {
            return self.unsupported("namespace-relative names");
        }
        let value = self.expr(expr)?;
        let class_name = self.builder.string(&name.bytes)?;
        let dest = self.builder.temp()?;
        self.builder
            .op(Opcode::InstanceOf, value, class_name, dest)?;
        Ok(dest)
    }

    /// `Class::NAME`.
    pub(super) fn class_const(&mut self, class: &Expr, name: &Member) -> Emit<Operand> {
        let ExprKind::Name(class_name) = &class.kind else {
            return self.unsupported("dynamic class names");
        };
        if class_name.kind == NameKind::Relative {
            return self.unsupported("namespace-relative names");
        }
        let Member::Name(member) = name else {
            return self.unsupported("computed constant names");
        };
        let dest = self.builder.temp()?;
        let class_op = self.builder.string(&class_name.bytes)?;
        let member_op = self.builder.string(member)?;
        self.builder
            .op(Opcode::ClassConst, class_op, member_op, dest)?;
        Ok(dest)
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

    /// `Class::name(args)`.
    pub(super) fn static_call(
        &mut self,
        class: &Expr,
        name: &Member,
        args: &php_parser::ast::Args,
        needed: bool,
    ) -> Emit<Operand> {
        let ExprKind::Name(class_name) = &class.kind else {
            return self.unsupported("dynamic class names");
        };
        if class_name.kind == NameKind::Relative {
            return self.unsupported("namespace-relative names");
        }
        let Member::Name(name) = name else {
            return self.unsupported("computed method names");
        };
        let php_parser::ast::Args::List(args) = args else {
            return self.unsupported("first-class callables");
        };
        let class_op = self.builder.string(&class_name.bytes)?;
        let method = self.builder.string(&name.to_ascii_lowercase())?;
        let written = self.builder.string(name)?;
        let mut init = Instruction::new(Opcode::InitStaticCall);
        init.a = class_op;
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

#[derive(Default)]
struct Collected {
    properties: Vec<PropertyDecl>,
    methods: Vec<MethodDecl>,
    uses: Vec<Box<[u8]>>,
    cases: Vec<EnumCaseDecl>,
}

fn class_kind(kind: AstKind) -> ClassKind {
    match kind {
        AstKind::Class => ClassKind::Class,
        AstKind::Interface => ClassKind::Interface,
        AstKind::Trait => ClassKind::Trait,
        AstKind::Enum => ClassKind::Enum,
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
