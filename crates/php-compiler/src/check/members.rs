//! Class-like declarations and their members other than methods: properties, class constants
//! and enum cases.

use std::collections::HashSet;

use php_parser::ast::{
    ClassKind, ClassLike, ClassMember, ConstantGroup, HookBody, Property, PropertyGroup,
};
use php_value::Value;

use super::types::{ANONYMOUS_CLASS, Mask};
use super::{Checker, ClassScope, Code, Flow, Frame, types};

impl<'a> Checker<'a> {
    pub(super) fn class(&mut self, class: &'a ClassLike) -> Flow {
        self.line = class.line;
        if let Some(name) = &class.name {
            self.class_declaration(class.kind, name)?;
        }
        let parent = match &class.extends {
            Some(parent) => Some(self.declared_class(parent, "class name")?),
            None => None,
        };
        let scope = ClassScope {
            name: class.name.as_deref().map(|name| self.qualify(name)),
            is_trait: class.kind == ClassKind::Trait,
            parent,
        };
        let outer = self.class.replace(scope);
        let result = self.class_body(class);
        self.class = outer;
        result
    }

    fn class_body(&mut self, class: &'a ClassLike) -> Flow {
        self.attributes(&class.attributes)?;
        for interface in &class.implements {
            self.declared_class(interface, "interface name")?;
        }
        let mut methods = HashSet::new();
        for member in &class.members {
            match member {
                ClassMember::Method(method) => self.method(class, method, &mut methods)?,
                ClassMember::Property(group) => {
                    self.line = group.line;
                    for property in &group.properties {
                        let outer = self.frame.property.replace(property.name.clone());
                        let result = self.property(group, property);
                        self.frame.property = outer;
                        result?;
                    }
                }
                ClassMember::Constant(group) => {
                    self.line = group.line;
                    self.class_constants(group)?;
                }
                ClassMember::EnumCase(case) => {
                    self.line = case.line;
                    if let Some(value) = &case.value {
                        self.constant(value, false)?;
                    }
                    self.attributes(&case.attributes)?;
                }
                ClassMember::TraitUse(trait_use) => {
                    self.line = trait_use.line;
                    self.trait_use(class.kind, trait_use)?;
                }
            }
        }
        Ok(())
    }

    /// The class being compiled, as messages name it.
    pub(super) fn class_label(&self) -> Vec<u8> {
        self.class
            .as_ref()
            .map_or(ANONYMOUS_CLASS, ClassScope::label)
            .to_vec()
    }

    fn class_constants(&mut self, group: &'a ConstantGroup) -> Flow {
        for item in &group.items {
            let declared = match &group.constant_type {
                Some(declared) => Some(self.declared_type(declared, false)?.0),
                None => None,
            };
            let mut subject = self.class_label();
            subject.extend_from_slice(b"::");
            subject.extend_from_slice(&item.name);
            if let Some(declared) = &declared {
                let mask = declared.mask();
                let forbidden = Mask::CALLABLE.union(Mask::VOID).union(Mask::NEVER);
                if mask != Mask::ANY && mask.intersects(forbidden) {
                    let mut message = b"Class constant ".to_vec();
                    message.extend_from_slice(&subject);
                    message.extend_from_slice(b" cannot have type ");
                    message.extend(declared.display());
                    return self.fatal(message);
                }
            }
            let value = self.constant(&item.value, false)?;
            if let (Some(declared), Some(value)) = (&declared, &value)
                && !declared.accepts(value)
            {
                let mut message = format!(
                    "Cannot use {} as value for class constant ",
                    types::type_name(value)
                )
                .into_bytes();
                message.extend_from_slice(&subject);
                message.extend_from_slice(b" of type ");
                message.extend(declared.display());
                return self.fatal(message);
            }
            self.attributes(&group.attributes)?;
        }
        Ok(())
    }

    fn property(&mut self, group: &'a PropertyGroup, property: &'a Property) -> Flow {
        let declared = match &group.property_type {
            Some(declared) => Some(self.declared_type(declared, false)?.0),
            None => None,
        };
        let mut subject = self.class_label();
        subject.extend_from_slice(b"::$");
        subject.extend_from_slice(&property.name);
        if let Some(declared) = &declared {
            let forbidden = Mask::VOID.union(Mask::NEVER).union(Mask::CALLABLE);
            if declared.mask().intersects(forbidden) {
                let mut message = b"Property ".to_vec();
                message.extend_from_slice(&subject);
                message.extend_from_slice(b" cannot have type ");
                message.extend(declared.display());
                return self.fatal(message);
            }
        }
        if let Some(default) = &property.default {
            let value = self.constant(default, false)?;
            if let (Some(declared), Some(value)) = (&declared, &value)
                && !declared.accepts(value)
            {
                let message = if matches!(value, Value::Null) && !declared.is_intersection() {
                    let mut message = b"Default value for property of type ".to_vec();
                    message.extend(declared.display());
                    message.extend_from_slice(b" may not be null. Use the nullable type ");
                    message.extend(declared.nullable().display());
                    message.extend_from_slice(b" to allow null default value");
                    message
                } else {
                    let mut message = format!(
                        "Cannot use {} as default value for property ",
                        types::value_name(value)
                    )
                    .into_bytes();
                    message.extend_from_slice(&subject);
                    message.extend_from_slice(b" of type ");
                    message.extend(declared.display());
                    message
                };
                return self.fatal(message);
            }
        }
        for hook in property.hooks.iter().flatten() {
            let mut name = b"$".to_vec();
            name.extend_from_slice(&property.name);
            name.extend_from_slice(b"::");
            name.extend_from_slice(&hook.name);
            let code = Code::Method {
                class: self.class.as_ref().and_then(|scope| scope.name.clone()),
                name,
            };
            self.hook_body(&hook.body, code)?;
        }
        self.attributes(&group.attributes)
    }

    fn hook_body(&mut self, body: &'a HookBody, code: Code) -> Flow {
        let property = self.frame.property.clone();
        let outer = std::mem::replace(
            &mut self.frame,
            Frame {
                property,
                ..Frame::new(code)
            },
        );
        let result = match body {
            HookBody::Absent => Ok(()),
            HookBody::Expr(expr) => self.expr(expr),
            HookBody::Block(stmts) => self.stmts(stmts),
        };
        self.frame = outer;
        result
    }
}
