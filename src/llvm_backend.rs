
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use crate::ast::{Expression, Operator, Program, Statement, UnaryOperator};
use crate::types::{StructDefinition, Type};
use crate::errors::FusionError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LlType { I64, I1, F64, Ptr, Void }

impl LlType {
    fn text(self) -> &'static str {
        match self { LlType::I64 => "i64", LlType::I1 => "i1", LlType::F64 => "double", LlType::Ptr => "ptr", LlType::Void => "void" }
    }
}

#[derive(Debug, Clone)]
struct FunctionSig {
    params: Vec<LlType>,
    ret: LlType,
}

pub struct LLVMBackend {
    module: String,
    globals: Vec<String>,
    string_ids: HashMap<String, usize>,
    functions: HashMap<String, FunctionSig>,
    structs: HashMap<String, StructDefinition>,
    next_temp: u32,
    next_block: u32,
    locals: HashMap<String, (LlType, String)>,
    current_ret: LlType,
    current_is_main: bool,
}

impl LLVMBackend {
    pub fn new() -> Self {
        Self {
            module: String::new(), globals: Vec::new(), string_ids: HashMap::new(),
            functions: HashMap::new(), structs: HashMap::new(),
            next_temp: 0, next_block: 0, locals: HashMap::new(), current_ret: LlType::Void,
            current_is_main: false,
        }
    }

    pub fn emit_program(mut self, program: &Program) -> Result<String, FusionError> {
        self.collect_declarations(program)?;
        self.emit_preamble();
        for stmt in &program.statements {
            if let Statement::Function { name, .. } = stmt {
                self.emit_function(stmt, name)?;
            } else if let Statement::Main { body, .. } = stmt {
                self.emit_main(body)?;
            }
        }
        if !self.functions.contains_key("main") {
            // Parser already enforces this, but keep backend defensive.
            return Err(self.unsupported("program has no main"));
        }
        let mut out = String::new();
        out.push_str(&self.module);
        for g in &self.globals { out.push_str(g); out.push('\n'); }
        out.push('\n');
        out.push_str("declare i32 @printf(ptr, ...)\n");
        out.push_str("declare i32 @puts(ptr)\n");
        out.push_str("declare void @llvm.trap()\n\n");
        Ok(out)
    }

    fn unsupported(&self, msg: impl Into<String>) -> FusionError {
        FusionError::Syntax { message: format!("LLVM backend: {}", msg.into()), span: crate::span::Span::new(0,0) }
    }

    fn collect_declarations(&mut self, program: &Program) -> Result<(), FusionError> {
        for s in &program.statements {
            match s {
                Statement::Function { name, parameters, return_type, body, .. } => {
                    if self.functions.contains_key(name) { continue; }
                    let params = parameters.iter().map(|p| p.type_name.as_deref().map(|p| self.llvm_type_name(p)).unwrap_or(LlType::I64)).collect();
                    let ret = match return_type.as_deref() {
                        Some("void") => LlType::Void,
                        Some(t) => self.llvm_type_name(t),
                        None => self.infer_return_type(body),
                    };
                    self.functions.insert(name.clone(), FunctionSig { params, ret });
                }
                Statement::Struct { name, fields, .. } => {
                    self.structs.insert(name.clone(), StructDefinition {
                        fields: fields.iter().map(|f| (f.name.clone(), self.type_from_name(&f.type_name))).collect()
                    });
                }
                _ => {}
            }
        }
        self.functions.insert("print".into(), FunctionSig { params: vec![LlType::I64], ret: LlType::Void });
        Ok(())
    }

    fn type_from_name(&self, n: &str) -> Type {
        match n { "num" => Type::Num, "float" => Type::Float, "bool" => Type::Bool, "string" => Type::String, x => Type::Struct(x.into()) }
    }

    fn llvm_type_name(&self, n: &str) -> LlType {
        match n {
            "float" => LlType::F64, "bool" => LlType::I1, "string" => LlType::Ptr,
            "void" => LlType::Void, _ => LlType::I64,
        }
    }

    fn infer_return_type(&self, body: &[Statement]) -> LlType {
        for s in body {
            if let Statement::Return { value: Some(e), .. } = s {
                return self.expr_type(e);
            }
            if let Statement::If { body: b, else_body, .. } = s {
                let t = self.infer_return_type(b);
                if t != LlType::Void { return t; }
                if let Some(e) = else_body { let t = self.infer_return_type(e); if t != LlType::Void { return t; } }
            }
        }
        LlType::Void
    }

    fn emit_preamble(&mut self) {
        self.module.push_str("; Fusion LLVM IR\n\n");
    }

    fn reset_function_state(&mut self, ret: LlType) {
        self.current_is_main = false;
        self.next_temp = 0; self.next_block = 0; self.locals.clear(); self.current_ret = ret;
    }

    fn temp(&mut self) -> String { let s = format!("%t{}", self.next_temp); self.next_temp += 1; s }
    fn block(&mut self, prefix: &str) -> String { let s = format!("{}.{}", prefix, self.next_block); self.next_block += 1; s }
    fn string_ptr(&mut self, value: &str) -> String {
        if let Some(id) = self.string_ids.get(value) { return format!("getelementptr inbounds ([{} x i8], ptr @.str{}, i64 0, i64 0)", value.as_bytes().len()+1, id); }
        let id = self.string_ids.len(); self.string_ids.insert(value.to_string(), id);
        let escaped = llvm_escape(value);
        self.globals.push(format!("@.str{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1", id, value.as_bytes().len()+1, escaped));
        format!("getelementptr inbounds ([{} x i8], ptr @.str{}, i64 0, i64 0)", value.as_bytes().len()+1, id)
    }

    fn emit_main(&mut self, body: &[Statement]) -> Result<(), FusionError> {
        self.reset_function_state(LlType::I64);
        self.current_is_main = true;
        self.functions.insert("main".into(), FunctionSig { params: vec![], ret: LlType::I64 });
        let mut b = String::new();
        writeln!(b, "define i32 @main() {{").unwrap();
        writeln!(b, "entry:").unwrap();
        self.emit_statements(body, &mut b)?;
        writeln!(b, "  ret i32 0").unwrap();
        writeln!(b, "}}").unwrap();
        self.module.push_str(&b);
        Ok(())
    }

    fn emit_function(&mut self, stmt: &Statement, name: &str) -> Result<(), FusionError> {
        let Statement::Function { parameters, body, .. } = stmt else { return Ok(()); };
        let sig = self.functions.get(name).cloned().unwrap();
        self.reset_function_state(sig.ret);
        let mut b = String::new();
        write!(b, "define {} @{}(", sig.ret.text(), sanitize_symbol(name)).unwrap();
        for (i, p) in sig.params.iter().enumerate() {
            if i > 0 { b.push_str(", "); }
            write!(b, "{} %arg{}", p.text(), i).unwrap();
        }
        b.push_str(") {\nentry:\n");
        for (i, p) in parameters.iter().enumerate() {
            let ty = sig.params[i];
            let slot = format!("%local_{}", sanitize_symbol(&p.name));
            writeln!(b, "  {} = alloca {}, align 8", slot, ty.text()).unwrap();
            writeln!(b, "  store {} %arg{}, ptr {}", ty.text(), i, slot).unwrap();
            self.locals.insert(p.name.clone(), (ty, slot));
        }
        self.emit_statements(body, &mut b)?;
        match sig.ret {
            LlType::Void => writeln!(b, "  ret void").unwrap(),
            LlType::I64 => writeln!(b, "  ret i64 0").unwrap(),
            LlType::I1 => writeln!(b, "  ret i1 false").unwrap(),
            LlType::F64 => writeln!(b, "  ret double 0.0").unwrap(),
            LlType::Ptr => writeln!(b, "  ret ptr null").unwrap(),
        }
        writeln!(b, "}}").unwrap();
        self.module.push_str(&b);
        Ok(())
    }

    fn emit_statements(&mut self, body: &[Statement], out: &mut String) -> Result<(), FusionError> {
        for s in body {
            let terminates = matches!(s, Statement::Return { .. });
            self.emit_statement(s, out)?;
            if terminates { break; }
        }
        Ok(())
    }

    fn emit_statement(&mut self, s: &Statement, out: &mut String) -> Result<(), FusionError> {
        match s {
            Statement::VariableDeclarations { declarations, .. } => {
                for d in declarations {
                    let value = if let Some(v) = &d.value { self.emit_expr(v, out)? } else { self.zero_for(self.llvm_type_name(d.declared_type.as_deref().unwrap_or("num"))) };
                    let ty = self.expr_type(d.value.as_ref().unwrap_or(&Expression::Number { value: 0, span: crate::span::Span::new(0,0) }));
                    let ty = d.declared_type.as_deref().map(|x| self.llvm_type_name(x)).unwrap_or(ty);
                    let slot = format!("%local_{}_{}", sanitize_symbol(&d.name), self.next_temp);
                    writeln!(out, "  {} = alloca {}, align 8", slot, ty.text()).unwrap();
                    writeln!(out, "  store {} {}, ptr {}", ty.text(), value, slot).unwrap();
                    self.locals.insert(d.name.clone(), (ty, slot));
                }
            }
            Statement::ConstDeclaration { name, value, declared_type, .. } => {
                let ty = declared_type.as_deref().map(|x| self.llvm_type_name(x)).unwrap_or_else(|| self.expr_type(value));
                let value = self.emit_expr(value, out)?;
                let slot = format!("%local_{}_{}", sanitize_symbol(name), self.next_temp);
                writeln!(out, "  {} = alloca {}, align 8", slot, ty.text()).unwrap();
                writeln!(out, "  store {} {}, ptr {}", ty.text(), value, slot).unwrap();
                self.locals.insert(name.clone(), (ty, slot));
            }
            Statement::Assignment { target, value, .. } => {
    let Expression::Identifier { name, .. } = target else {
        return Err(self.unsupported("only variable assignment is currently lowered"));
    };

    if let Some((ty, slot)) = self.locals.get(name).cloned() {
        // Existing variable: evaluate the new value and store it.
        let v = self.emit_expr(value, out)?;
        writeln!(out, "  store {} {}, ptr {}", ty.text(), v, slot).unwrap();
    } else {
        // New variable: Fusion treats `x = value` as an inferred declaration.
        let ty = self.expr_type(value);
        let v = self.emit_expr(value, out)?;
        let slot = format!("%local_{}_{}", sanitize_symbol(name), self.next_temp);

        writeln!(out, "  {} = alloca {}, align 8", slot, ty.text()).unwrap();
        writeln!(out, "  store {} {}, ptr {}", ty.text(), v, slot).unwrap();

        self.locals.insert(name.clone(), (ty, slot));
    }
}
            Statement::Expression { expression, .. } | Statement::Call { expression, .. } => { let _ = self.emit_expr(expression, out)?; }
            Statement::Return { value, .. } => {
                if self.current_is_main {
                    return Err(self.unsupported("main cannot contain return; process termination is owned by the runtime"));
                }
                if let Some(v) = value {
                    let x = self.emit_expr(v, out)?;
                    writeln!(out, "  ret {} {}", self.current_ret.text(), x).unwrap();
                } else { writeln!(out, "  ret void").unwrap(); }
            }
            Statement::If { condition, body, else_body, .. } => self.emit_if(condition, body, else_body.as_deref(), out)?,
            Statement::While { condition, body, .. } => self.emit_while(condition, body, out)?,
            Statement::For { variable, start, end, body, .. } => self.emit_for(variable, start, end, body, out)?,
            Statement::Break { .. } | Statement::Continue { .. } => return Err(self.unsupported("break/continue need loop CFG lowering; use interpreter until CFG backend lands")),
            Statement::Defer { .. } => return Err(self.unsupported("defer requires cleanup CFG lowering")),
            Statement::ForEach { .. } => return Err(self.unsupported("for-each requires collection runtime lowering")),
            Statement::Match { .. } => return Err(self.unsupported("match requires pattern CFG lowering")),
            Statement::Struct { .. } | Statement::Enum { .. } | Statement::Trait { .. } | Statement::Impl { .. } | Statement::Main { .. } | Statement::Function { .. } => {}
        }
        Ok(())
    }

    fn emit_if(&mut self, condition: &Expression, body: &[Statement], else_body: Option<&[Statement]>, out: &mut String) -> Result<(), FusionError> {
        if body.iter().any(|s| matches!(s, Statement::Return { .. })) ||
           else_body.map_or(false, |b| b.iter().any(|s| matches!(s, Statement::Return { .. }))) {
            return Err(self.unsupported("return inside if is not yet lowered to cleanup-aware CFG"));
        }
        let c = self.emit_expr(condition, out)?;
        let then_b = self.block("then"); let else_b = self.block("else"); let end_b = self.block("endif");
        writeln!(out, "  br i1 {}, label %{}, label %{}", c, then_b, else_b).unwrap();
        writeln!(out, "{}:", then_b).unwrap();
        self.emit_statements(body, out)?;
        writeln!(out, "  br label %{}", end_b).unwrap();
        writeln!(out, "{}:", else_b).unwrap();
        if let Some(e) = else_body { self.emit_statements(e, out)?; }
        writeln!(out, "  br label %{}", end_b).unwrap();
        writeln!(out, "{}:", end_b).unwrap();
        Ok(())
    }

    fn emit_while(&mut self, condition: &Expression, body: &[Statement], out: &mut String) -> Result<(), FusionError> {
        if body.iter().any(|s| matches!(s, Statement::Return { .. })) {
            return Err(self.unsupported("return inside while is not yet lowered to cleanup-aware CFG"));
        }
        let cond_b = self.block("while.cond"); let body_b = self.block("while.body"); let end_b = self.block("while.end");
        writeln!(out, "  br label %{}", cond_b).unwrap();
        writeln!(out, "{}:", cond_b).unwrap();
        let c = self.emit_expr(condition, out)?;
        writeln!(out, "  br i1 {}, label %{}, label %{}", c, body_b, end_b).unwrap();
        writeln!(out, "{}:", body_b).unwrap();
        self.emit_statements(body, out)?;
        writeln!(out, "  br label %{}", cond_b).unwrap();
        writeln!(out, "{}:", end_b).unwrap();
        Ok(())
    }

    fn emit_for(&mut self, variable: &str, start: &Expression, end: &Expression, body: &[Statement], out: &mut String) -> Result<(), FusionError> {
        if body.iter().any(|s| matches!(s, Statement::Return { .. })) {
            return Err(self.unsupported("return inside for is not yet lowered to cleanup-aware CFG"));
        }
        let startv = self.emit_expr(start, out)?; let endv = self.emit_expr(end, out)?;
        let slot = format!("%local_{}_{}", sanitize_symbol(variable), self.next_temp);
        writeln!(out, "  {} = alloca i64, align 8", slot).unwrap();
        writeln!(out, "  store i64 {}, ptr {}", startv, slot).unwrap();
        self.locals.insert(variable.to_string(), (LlType::I64, slot.clone()));
        let cond_b = self.block("for.cond"); let body_b = self.block("for.body"); let end_b = self.block("for.end");
        writeln!(out, "  br label %{}", cond_b).unwrap();
        writeln!(out, "{}:", cond_b).unwrap();
        let cur = self.temp(); writeln!(out, "  {} = load i64, ptr {}", cur, slot).unwrap();
        let cmp = self.temp(); writeln!(out, "  {} = icmp slt i64 {}, {}", cmp, cur, endv).unwrap();
        writeln!(out, "  br i1 {}, label %{}, label %{}", cmp, body_b, end_b).unwrap();
        writeln!(out, "{}:", body_b).unwrap();
        self.emit_statements(body, out)?;
        let cur2 = self.temp(); writeln!(out, "  {} = load i64, ptr {}", cur2, slot).unwrap();
        let inc = self.temp(); writeln!(out, "  {} = add i64 {}, 1", inc, cur2).unwrap();
        writeln!(out, "  store i64 {}, ptr {}", inc, slot).unwrap();
        writeln!(out, "  br label %{}", cond_b).unwrap();
        writeln!(out, "{}:", end_b).unwrap();
        Ok(())
    }

    fn emit_expr(&mut self, e: &Expression, out: &mut String) -> Result<String, FusionError> {
        match e {
            Expression::Number { value, .. } => Ok(value.to_string()),
            Expression::Float { value, .. } => Ok(format!("{:.17e}", value)),
            Expression::Boolean { value, .. } => Ok(if *value { "true".into() } else { "false".into() }),
            Expression::String { value, .. } => Ok(self.string_ptr(value)),
            Expression::Identifier { name, .. } => {
                let (ty, slot) = self.locals.get(name).cloned().ok_or_else(|| self.unsupported(format!("unknown local '{}'", name)))?;
                let t = self.temp(); writeln!(out, "  {} = load {}, ptr {}", t, ty.text(), slot).unwrap(); Ok(t)
            }
            Expression::Binary { left, operator, right, .. } => {
                let l = self.emit_expr(left, out)?; let r = self.emit_expr(right, out)?;
                let ty = self.expr_type(left);
                let t = self.temp();
                match (operator, ty) {
                    (Operator::Plus, LlType::I64) => writeln!(out, "  {} = add i64 {}, {}", t, l, r),
                    (Operator::Minus, LlType::I64) => writeln!(out, "  {} = sub i64 {}, {}", t, l, r),
                    (Operator::Multiply, LlType::I64) => writeln!(out, "  {} = mul i64 {}, {}", t, l, r),
                    (Operator::Divide, LlType::I64) => writeln!(out, "  {} = sdiv i64 {}, {}", t, l, r),
                    (Operator::Plus, LlType::F64) => writeln!(out, "  {} = fadd double {}, {}", t, l, r),
                    (Operator::Minus, LlType::F64) => writeln!(out, "  {} = fsub double {}, {}", t, l, r),
                    (Operator::Multiply, LlType::F64) => writeln!(out, "  {} = fmul double {}, {}", t, l, r),
                    (Operator::Divide, LlType::F64) => writeln!(out, "  {} = fdiv double {}, {}", t, l, r),
                    (Operator::Equal, LlType::I64) => writeln!(out, "  {} = icmp eq i64 {}, {}", t, l, r),
                    (Operator::NotEqual, LlType::I64) => writeln!(out, "  {} = icmp ne i64 {}, {}", t, l, r),
                    (Operator::Less, LlType::I64) => writeln!(out, "  {} = icmp slt i64 {}, {}", t, l, r),
                    (Operator::LessEqual, LlType::I64) => writeln!(out, "  {} = icmp sle i64 {}, {}", t, l, r),
                    (Operator::Greater, LlType::I64) => writeln!(out, "  {} = icmp sgt i64 {}, {}", t, l, r),
                    (Operator::GreaterEqual, LlType::I64) => writeln!(out, "  {} = icmp sge i64 {}, {}", t, l, r),
                    (Operator::And, LlType::I1) => writeln!(out, "  {} = and i1 {}, {}", t, l, r),
                    (Operator::Or, LlType::I1) => writeln!(out, "  {} = or i1 {}, {}", t, l, r),
                    (Operator::Equal, LlType::F64) => writeln!(out, "  {} = fcmp oeq double {}, {}", t, l, r),
                    (Operator::NotEqual, LlType::F64) => writeln!(out, "  {} = fcmp one double {}, {}", t, l, r),
                    (Operator::Less, LlType::F64) => writeln!(out, "  {} = fcmp olt double {}, {}", t, l, r),
                    (Operator::LessEqual, LlType::F64) => writeln!(out, "  {} = fcmp ole double {}, {}", t, l, r),
                    (Operator::Greater, LlType::F64) => writeln!(out, "  {} = fcmp ogt double {}, {}", t, l, r),
                    (Operator::GreaterEqual, LlType::F64) => writeln!(out, "  {} = fcmp oge double {}, {}", t, l, r),
                    _ => return Err(self.unsupported("invalid LLVM binary operation")),
                }.map_err(|e| self.unsupported(e.to_string()))?;
                Ok(t)
            }
            Expression::Unary { operator, expression, .. } => {
                let x = self.emit_expr(expression, out)?; let ty = self.expr_type(expression); let t = self.temp();
                match (operator, ty) {
                    (UnaryOperator::Negate, LlType::I64) => writeln!(out, "  {} = sub i64 0, {}", t, x),
                    (UnaryOperator::Negate, LlType::F64) => writeln!(out, "  {} = fneg double {}", t, x),
                    (UnaryOperator::Not, LlType::I1) => writeln!(out, "  {} = xor i1 {}, true", t, x),
                    _ => return Err(self.unsupported("invalid unary operation")),
                }.map_err(|e| self.unsupported(e.to_string()))?;
                Ok(t)
            }
            Expression::Call { name, arguments, .. } => {
                if name == "print" {
                    if arguments.len() != 1 { return Err(self.unsupported("print currently takes exactly one argument")); }
                    let e = &arguments[0]; let v = self.emit_expr(e, out); let ty = self.expr_type(e);
                    match ty {
                        LlType::I64 => {
                            let fmt = self.string_ptr("%ld\n"); let f = self.temp();
                            writeln!(out, "  {} = call i32 (ptr, ...) @printf(ptr {}, i64 {})", f, fmt, v.unwrap()).unwrap();
                        }
                        LlType::F64 => {
                            let fmt = self.string_ptr("%f\n"); let f = self.temp();
                            writeln!(out, "  {} = call i32 (ptr, ...) @printf(ptr {}, double {})", f, fmt, v.unwrap()).unwrap();
                        }
                        LlType::I1 => {
                            let truep = self.string_ptr("true\n"); let falsep = self.string_ptr("false\n");
                            let sel = self.temp(); writeln!(out, "  {} = select i1 {}, ptr {}, ptr {}", sel, v.unwrap(), truep, falsep).unwrap();
                            let f = self.temp(); writeln!(out, "  {} = call i32 @puts(ptr {})", f, sel).unwrap();
                        }
                        LlType::Ptr => {
                            let f = self.temp(); writeln!(out, "  {} = call i32 @puts(ptr {})", f, v.unwrap()).unwrap();
                        }
                        LlType::Void => return Err(self.unsupported("cannot print void")),
                    }
                    Ok("0".into())
                } else {
                    let sig = self.functions.get(name).cloned().ok_or_else(|| self.unsupported(format!("unknown function '{}'", name)))?;
                    if sig.params.len() != arguments.len() { return Err(self.unsupported(format!("function '{}' expects {} arguments", name, sig.params.len()))); }
                    let mut args = Vec::new();
                    for (i, a) in arguments.iter().enumerate() {
                        let v = self.emit_expr(a, out)?;
                        args.push(format!("{} {}", sig.params[i].text(), v));
                    }
                    if sig.ret == LlType::Void {
                        writeln!(out, "  call void @{}({})", sanitize_symbol(name), args.join(", ")).unwrap();
                        Ok("0".into())
                    } else {
                        let t = self.temp(); writeln!(out, "  {} = call {} @{}({})", t, sig.ret.text(), sanitize_symbol(name), args.join(", ")).unwrap(); Ok(t)
                    }
                }
            }
            _ => Err(self.unsupported("expression is not yet lowered by the LLVM backend")),
        }
    }

    fn expr_type(&self, e: &Expression) -> LlType {
        match e {
            Expression::Number { .. } => LlType::I64,
            Expression::Float { .. } => LlType::F64,
            Expression::Boolean { .. } => LlType::I1,
            Expression::String { .. } => LlType::Ptr,
            Expression::Binary { left, operator, .. } => match operator {
                Operator::Equal | Operator::NotEqual | Operator::Less | Operator::LessEqual | Operator::Greater | Operator::GreaterEqual | Operator::And | Operator::Or => LlType::I1,
                _ => self.expr_type(left),
            },
            Expression::Unary { operator, expression, .. } => match operator { UnaryOperator::Not => LlType::I1, _ => self.expr_type(expression) },
            Expression::Identifier { name, .. } => self.locals.get(name).map(|x| x.0).unwrap_or(LlType::I64),
            Expression::Call { name, .. } => self.functions.get(name).map(|s| s.ret).unwrap_or(LlType::I64),
            _ => LlType::I64,
        }
    }

    fn zero_for(&self, t: LlType) -> String {
        match t { LlType::I64 => "0".into(), LlType::I1 => "false".into(), LlType::F64 => "0.0".into(), LlType::Ptr => "null".into(), LlType::Void => "0".into() }
    }
}

fn sanitize_symbol(s: &str) -> String {
    let mut out = String::from("_F");
    for c in s.chars() { if c.is_ascii_alphanumeric() || c == '_' { out.push(c); } else { out.push('_'); } }
    out
}

fn llvm_escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match *b {
            b'\\' => out.push_str("\\5C"),
            b'"' => out.push_str("\\22"),
            b'\n' => out.push_str("\\0A"),
            b'\r' => out.push_str("\\0D"),
            b'\t' => out.push_str("\\09"),
            0x20..=0x7e => out.push(*b as char),
            b => write!(&mut out, "\\{:02X}", b).unwrap(),
        }
    }
    out
}
