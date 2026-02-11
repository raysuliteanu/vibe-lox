use crate::ast::*;

pub fn to_sexp(program: &Program) -> String {
    let mut buf = String::new();
    for decl in &program.declarations {
        sexp_decl(&mut buf, decl);
        buf.push('\n');
    }
    buf
}

pub fn to_json(program: &Program) -> String {
    serde_json::to_string_pretty(program).expect("AST should be serializable")
}

fn sexp_decl(buf: &mut String, decl: &Decl) {
    match decl {
        Decl::Class(c) => {
            buf.push_str("(class ");
            buf.push_str(&c.name);
            if let Some(ref superclass) = c.superclass {
                buf.push_str(" < ");
                buf.push_str(superclass);
            }
            for method in &c.methods {
                buf.push(' ');
                sexp_function(buf, method);
            }
            buf.push(')');
        }
        Decl::Fun(f) => sexp_function(buf, &f.function),
        Decl::Var(v) => {
            buf.push_str("(var ");
            buf.push_str(&v.name);
            if let Some(ref init) = v.initializer {
                buf.push(' ');
                sexp_expr(buf, init);
            }
            buf.push(')');
        }
        Decl::Statement(s) => sexp_stmt(buf, s),
    }
}

fn sexp_function(buf: &mut String, f: &Function) {
    buf.push_str("(fun ");
    buf.push_str(&f.name);
    buf.push_str(" (");
    for (i, param) in f.params.iter().enumerate() {
        if i > 0 {
            buf.push(' ');
        }
        buf.push_str(param);
    }
    buf.push(')');
    for decl in &f.body {
        buf.push(' ');
        sexp_decl(buf, decl);
    }
    buf.push(')');
}

fn sexp_stmt(buf: &mut String, stmt: &Stmt) {
    match stmt {
        Stmt::Expression(e) => sexp_expr(buf, &e.expression),
        Stmt::Print(p) => {
            buf.push_str("(print ");
            sexp_expr(buf, &p.expression);
            buf.push(')');
        }
        Stmt::Return(r) => {
            buf.push_str("(return");
            if let Some(ref val) = r.value {
                buf.push(' ');
                sexp_expr(buf, val);
            }
            buf.push(')');
        }
        Stmt::Block(b) => {
            buf.push_str("(block");
            for decl in &b.declarations {
                buf.push(' ');
                sexp_decl(buf, decl);
            }
            buf.push(')');
        }
        Stmt::If(i) => {
            buf.push_str("(if ");
            sexp_expr(buf, &i.condition);
            buf.push(' ');
            sexp_stmt(buf, &i.then_branch);
            if let Some(ref else_branch) = i.else_branch {
                buf.push(' ');
                sexp_stmt(buf, else_branch);
            }
            buf.push(')');
        }
        Stmt::While(w) => {
            buf.push_str("(while ");
            sexp_expr(buf, &w.condition);
            buf.push(' ');
            sexp_stmt(buf, &w.body);
            buf.push(')');
        }
    }
}

fn sexp_expr(buf: &mut String, expr: &Expr) {
    match expr {
        Expr::Binary(b) => {
            buf.push('(');
            buf.push_str(&b.operator.to_string());
            buf.push(' ');
            sexp_expr(buf, &b.left);
            buf.push(' ');
            sexp_expr(buf, &b.right);
            buf.push(')');
        }
        Expr::Unary(u) => {
            buf.push('(');
            buf.push_str(&u.operator.to_string());
            buf.push(' ');
            sexp_expr(buf, &u.operand);
            buf.push(')');
        }
        Expr::Literal(l) => match &l.value {
            LiteralValue::Number(n) => buf.push_str(&format!("{n}")),
            LiteralValue::String(s) => {
                buf.push('"');
                buf.push_str(s);
                buf.push('"');
            }
            LiteralValue::Bool(b) => buf.push_str(if *b { "true" } else { "false" }),
            LiteralValue::Nil => buf.push_str("nil"),
        },
        Expr::Grouping(g) => {
            buf.push_str("(group ");
            sexp_expr(buf, &g.expression);
            buf.push(')');
        }
        Expr::Variable(v) => buf.push_str(&v.name),
        Expr::Assign(a) => {
            buf.push_str("(= ");
            buf.push_str(&a.name);
            buf.push(' ');
            sexp_expr(buf, &a.value);
            buf.push(')');
        }
        Expr::Logical(l) => {
            buf.push('(');
            buf.push_str(&l.operator.to_string());
            buf.push(' ');
            sexp_expr(buf, &l.left);
            buf.push(' ');
            sexp_expr(buf, &l.right);
            buf.push(')');
        }
        Expr::Call(c) => {
            buf.push_str("(call ");
            sexp_expr(buf, &c.callee);
            for arg in &c.arguments {
                buf.push(' ');
                sexp_expr(buf, arg);
            }
            buf.push(')');
        }
        Expr::Get(g) => {
            buf.push_str("(. ");
            sexp_expr(buf, &g.object);
            buf.push(' ');
            buf.push_str(&g.name);
            buf.push(')');
        }
        Expr::Set(s) => {
            buf.push_str("(.= ");
            sexp_expr(buf, &s.object);
            buf.push(' ');
            buf.push_str(&s.name);
            buf.push(' ');
            sexp_expr(buf, &s.value);
            buf.push(')');
        }
        Expr::This(_) => buf.push_str("this"),
        Expr::Super(s) => {
            buf.push_str("(super ");
            buf.push_str(&s.method);
            buf.push(')');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sexp_binary_expression() {
        let program = Program {
            declarations: vec![Decl::Statement(Stmt::Expression(ExprStmt {
                expression: Expr::Binary(BinaryExpr {
                    id: 0,
                    left: Box::new(Expr::Literal(LiteralExpr {
                        id: 1,
                        value: LiteralValue::Number(1.0),
                        span: Span::new(0, 1),
                    })),
                    operator: BinaryOp::Add,
                    right: Box::new(Expr::Binary(BinaryExpr {
                        id: 2,
                        left: Box::new(Expr::Literal(LiteralExpr {
                            id: 3,
                            value: LiteralValue::Number(2.0),
                            span: Span::new(4, 1),
                        })),
                        operator: BinaryOp::Multiply,
                        right: Box::new(Expr::Literal(LiteralExpr {
                            id: 4,
                            value: LiteralValue::Number(3.0),
                            span: Span::new(8, 1),
                        })),
                        span: Span::new(4, 5),
                    })),
                    span: Span::new(0, 9),
                }),
                span: Span::new(0, 10),
            }))],
        };
        let result = to_sexp(&program);
        assert_eq!(result.trim(), "(+ 1 (* 2 3))");
    }

    #[test]
    fn json_output_is_valid() {
        let program = Program {
            declarations: vec![Decl::Var(VarDecl {
                name: "x".to_string(),
                initializer: Some(Expr::Literal(LiteralExpr {
                    id: 0,
                    value: LiteralValue::Number(42.0),
                    span: Span::new(8, 2),
                })),
                span: Span::new(0, 11),
            })],
        };
        let json = to_json(&program);
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("JSON output should be valid");
        assert_eq!(parsed["declarations"][0]["name"], "x");
    }
}
