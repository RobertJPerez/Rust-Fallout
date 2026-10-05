//! Canonical PEX definition/operand tokens for an offline independent comparison.
use crate::pex::{File, Function, Value as Operand, Variable};
use serde_json::{Value, json};

pub fn tokens(file: &File<'_>) -> Vec<Value> {
    fn operand(v: &Operand, t: &mut Vec<Value>) {
        match v {
            Operand::None => t.push(json!(0)),
            Operand::Identifier(i) => t.extend([json!(1), json!(i)]),
            Operand::String(i) => t.extend([json!(2), json!(i)]),
            Operand::Integer(i) => t.extend([json!(3), json!(i)]),
            Operand::FloatBits(i) => t.extend([json!(4), json!(i)]),
            Operand::BoolByte(i) => t.extend([json!(5), json!(i)]),
        }
    }
    fn variable(v: &Variable, t: &mut Vec<Value>) {
        t.extend([json!(v.name), json!(v.type_name), json!(v.user_flags)]);
        operand(&v.initial_value, t);
        t.push(json!(v.constant));
        if let Some(d) = v.documentation {
            t.push(json!(d));
        }
    }
    fn function(f: &Function, t: &mut Vec<Value>) {
        t.extend([
            json!(f.return_type),
            json!(f.documentation),
            json!(f.user_flags),
            json!(f.flags),
        ]);
        for items in [&f.parameters, &f.locals] {
            t.push(json!(items.len()));
            for (n, ty) in items {
                t.extend([json!(n), json!(ty)]);
            }
        }
        t.push(json!(f.instructions.len()));
        for i in &f.instructions {
            t.extend([json!(i.opcode), json!(i.arguments.len())]);
            for a in &i.arguments {
                operand(a, t);
            }
            t.push(json!(i.varargs.len()));
            for a in &i.varargs {
                operand(a, t);
            }
        }
    }
    let mut t = vec![json!(file.strings.len())];
    for s in &file.strings {
        t.push(json!(
            s.iter().map(|b| format!("{b:02x}")).collect::<String>()
        ));
    }
    t.push(json!(file.user_flags.len()));
    for (n, b) in &file.user_flags {
        t.extend([json!(n), json!(b)]);
    }
    t.push(json!(file.objects.len()));
    for o in &file.objects {
        t.extend([
            json!(o.name),
            json!(o.parent),
            json!(o.documentation),
            json!(o.constant),
            json!(o.user_flags),
            json!(o.auto_state),
        ]);
        t.push(json!(o.struct_definitions.len()));
        for s in &o.struct_definitions {
            t.extend([json!(s.name), json!(s.members.len())]);
            for m in &s.members {
                variable(m, &mut t);
            }
        }
        t.push(json!(o.variable_definitions.len()));
        for v in &o.variable_definitions {
            variable(v, &mut t);
        }
        t.push(json!(o.property_definitions.len()));
        for p in &o.property_definitions {
            t.extend([
                json!(p.name),
                json!(p.type_name),
                json!(p.documentation),
                json!(p.user_flags),
                json!(p.flags),
            ]);
            if let Some(v) = p.auto_variable {
                t.push(json!(v));
            } else {
                if let Some(g) = p.getter {
                    function(&o.functions[g], &mut t);
                }
                if let Some(s) = p.setter {
                    function(&o.functions[s], &mut t);
                }
            }
        }
        t.push(json!(o.state_definitions.len()));
        for s in &o.state_definitions {
            t.extend([json!(s.name), json!(s.functions.len())]);
            for f in &o.functions[s.functions.clone()] {
                t.push(json!(f.name));
                function(f, &mut t);
            }
        }
    }
    t
}
