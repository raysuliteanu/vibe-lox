use std::sync::atomic::{AtomicUsize, Ordering};

use crate::ast::*;
use crate::error::CompileError;
use crate::scanner::token::{Span, Token, TokenKind};

static NEXT_EXPR_ID: AtomicUsize = AtomicUsize::new(0);

fn next_id() -> ExprId {
    NEXT_EXPR_ID.fetch_add(1, Ordering::Relaxed)
}

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
    errors: Vec<CompileError>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            current: 0,
            errors: Vec::new(),
        }
    }

    pub fn parse(mut self) -> Result<Program, Vec<CompileError>> {
        let mut declarations = Vec::new();
        while !self.is_at_end() {
            match self.declaration() {
                Ok(decl) => declarations.push(decl),
                Err(e) => {
                    self.errors.push(e);
                    self.synchronize();
                }
            }
        }
        if self.errors.is_empty() {
            Ok(Program { declarations })
        } else {
            Err(self.errors)
        }
    }

    fn declaration(&mut self) -> Result<Decl, CompileError> {
        if self.check(TokenKind::Class) {
            self.class_declaration()
        } else if self.check(TokenKind::Fun) {
            self.fun_declaration()
        } else if self.check(TokenKind::Var) {
            self.var_declaration()
        } else {
            self.statement().map(Decl::Statement)
        }
    }

    fn class_declaration(&mut self) -> Result<Decl, CompileError> {
        let start = self.current_span();
        self.advance(); // consume 'class'
        let name = self.expect_identifier("class name")?;

        let superclass = if self.match_token(TokenKind::Less) {
            Some(self.expect_identifier("superclass name")?)
        } else {
            None
        };

        self.consume(TokenKind::LeftBrace, "'{' before class body")?;

        let mut methods = Vec::new();
        while !self.check(TokenKind::RightBrace) && !self.is_at_end() {
            methods.push(self.function("method")?);
        }

        self.consume(TokenKind::RightBrace, "'}' after class body")?;

        let span = self.span_from(start);
        Ok(Decl::Class(ClassDecl {
            name,
            superclass,
            methods,
            span,
        }))
    }

    fn fun_declaration(&mut self) -> Result<Decl, CompileError> {
        let start = self.current_span();
        self.advance(); // consume 'fun'
        let function = self.function("function")?;
        let span = self.span_from(start);
        Ok(Decl::Fun(FunDecl { function, span }))
    }

    fn function(&mut self, kind: &str) -> Result<Function, CompileError> {
        let start = self.current_span();
        let name = self.expect_identifier(&format!("{kind} name"))?;

        self.consume(TokenKind::LeftParen, &format!("'(' after {kind} name"))?;
        let mut params = Vec::new();
        if !self.check(TokenKind::RightParen) {
            loop {
                if params.len() >= 255 {
                    let span = self.current_span();
                    return Err(CompileError::parse(
                        "can't have more than 255 parameters",
                        span.offset,
                        span.len,
                    ));
                }
                params.push(self.expect_identifier("parameter name")?);
                if !self.match_token(TokenKind::Comma) {
                    break;
                }
            }
        }
        self.consume(TokenKind::RightParen, "')' after parameters")?;

        self.consume(TokenKind::LeftBrace, &format!("'{{' before {kind} body"))?;
        let body = self.block_declarations()?;
        let span = self.span_from(start);

        Ok(Function {
            name,
            params,
            body,
            span,
        })
    }

    fn var_declaration(&mut self) -> Result<Decl, CompileError> {
        let start = self.current_span();
        self.advance(); // consume 'var'
        let name = self.expect_identifier("variable name")?;

        let initializer = if self.match_token(TokenKind::Equal) {
            Some(self.expression()?)
        } else {
            None
        };

        self.consume(TokenKind::Semicolon, "';' after variable declaration")?;
        let span = self.span_from(start);
        Ok(Decl::Var(VarDecl {
            name,
            initializer,
            span,
        }))
    }

    fn statement(&mut self) -> Result<Stmt, CompileError> {
        if self.check(TokenKind::Print) {
            self.print_statement()
        } else if self.check(TokenKind::Return) {
            self.return_statement()
        } else if self.check(TokenKind::LeftBrace) {
            self.block_statement()
        } else if self.check(TokenKind::If) {
            self.if_statement()
        } else if self.check(TokenKind::While) {
            self.while_statement()
        } else if self.check(TokenKind::For) {
            self.for_statement()
        } else {
            self.expression_statement()
        }
    }

    fn print_statement(&mut self) -> Result<Stmt, CompileError> {
        let start = self.current_span();
        self.advance(); // consume 'print'
        let expression = self.expression()?;
        self.consume(TokenKind::Semicolon, "';' after print value")?;
        let span = self.span_from(start);
        Ok(Stmt::Print(PrintStmt { expression, span }))
    }

    fn return_statement(&mut self) -> Result<Stmt, CompileError> {
        let start = self.current_span();
        self.advance(); // consume 'return'
        let value = if !self.check(TokenKind::Semicolon) {
            Some(self.expression()?)
        } else {
            None
        };
        self.consume(TokenKind::Semicolon, "';' after return value")?;
        let span = self.span_from(start);
        Ok(Stmt::Return(ReturnStmt { value, span }))
    }

    fn block_statement(&mut self) -> Result<Stmt, CompileError> {
        let start = self.current_span();
        self.advance(); // consume '{'
        let declarations = self.block_declarations()?;
        let span = self.span_from(start);
        Ok(Stmt::Block(BlockStmt { declarations, span }))
    }

    fn block_declarations(&mut self) -> Result<Vec<Decl>, CompileError> {
        let mut declarations = Vec::new();
        while !self.check(TokenKind::RightBrace) && !self.is_at_end() {
            declarations.push(self.declaration()?);
        }
        self.consume(TokenKind::RightBrace, "'}' after block")?;
        Ok(declarations)
    }

    fn if_statement(&mut self) -> Result<Stmt, CompileError> {
        let start = self.current_span();
        self.advance(); // consume 'if'
        self.consume(TokenKind::LeftParen, "'(' after 'if'")?;
        let condition = self.expression()?;
        self.consume(TokenKind::RightParen, "')' after if condition")?;

        let then_branch = Box::new(self.statement()?);
        let else_branch = if self.match_token(TokenKind::Else) {
            Some(Box::new(self.statement()?))
        } else {
            None
        };

        let span = self.span_from(start);
        Ok(Stmt::If(IfStmt {
            condition,
            then_branch,
            else_branch,
            span,
        }))
    }

    fn while_statement(&mut self) -> Result<Stmt, CompileError> {
        let start = self.current_span();
        self.advance(); // consume 'while'
        self.consume(TokenKind::LeftParen, "'(' after 'while'")?;
        let condition = self.expression()?;
        self.consume(TokenKind::RightParen, "')' after while condition")?;
        let body = Box::new(self.statement()?);
        let span = self.span_from(start);
        Ok(Stmt::While(WhileStmt {
            condition,
            body,
            span,
        }))
    }

    /// Desugar `for` into `while`.
    fn for_statement(&mut self) -> Result<Stmt, CompileError> {
        let start = self.current_span();
        self.advance(); // consume 'for'
        self.consume(TokenKind::LeftParen, "'(' after 'for'")?;

        let initializer = if self.match_token(TokenKind::Semicolon) {
            None
        } else if self.check(TokenKind::Var) {
            Some(self.var_declaration()?)
        } else {
            let expr = self.expression()?;
            self.consume(TokenKind::Semicolon, "';' after for initializer")?;
            let span = expr.span();
            Some(Decl::Statement(Stmt::Expression(ExprStmt {
                expression: expr,
                span,
            })))
        };

        let condition = if !self.check(TokenKind::Semicolon) {
            self.expression()?
        } else {
            Expr::Literal(LiteralExpr {
                id: next_id(),
                value: LiteralValue::Bool(true),
                span: self.current_span(),
            })
        };
        self.consume(TokenKind::Semicolon, "';' after for condition")?;

        let increment = if !self.check(TokenKind::RightParen) {
            Some(self.expression()?)
        } else {
            None
        };
        self.consume(TokenKind::RightParen, "')' after for clauses")?;

        let mut body = self.statement()?;

        // Append increment to body
        if let Some(inc) = increment {
            let inc_span = inc.span();
            body = Stmt::Block(BlockStmt {
                declarations: vec![
                    Decl::Statement(body),
                    Decl::Statement(Stmt::Expression(ExprStmt {
                        expression: inc,
                        span: inc_span,
                    })),
                ],
                span: self.span_from(start),
            });
        }

        // Wrap in while
        let while_span = self.span_from(start);
        body = Stmt::While(WhileStmt {
            condition,
            body: Box::new(body),
            span: while_span,
        });

        // Wrap with initializer
        if let Some(init) = initializer {
            let block_span = self.span_from(start);
            body = Stmt::Block(BlockStmt {
                declarations: vec![init, Decl::Statement(body)],
                span: block_span,
            });
        }

        Ok(body)
    }

    fn expression_statement(&mut self) -> Result<Stmt, CompileError> {
        let expression = self.expression()?;
        self.consume(TokenKind::Semicolon, "';' after expression")?;
        let span = expression.span();
        Ok(Stmt::Expression(ExprStmt { expression, span }))
    }

    fn expression(&mut self) -> Result<Expr, CompileError> {
        self.assignment()
    }

    fn assignment(&mut self) -> Result<Expr, CompileError> {
        let expr = self.or()?;

        if self.match_token(TokenKind::Equal) {
            let value = self.assignment()?;
            let span = Span::new(
                expr.span().offset,
                value.span().offset + value.span().len - expr.span().offset,
            );

            match expr {
                Expr::Variable(v) => {
                    return Ok(Expr::Assign(AssignExpr {
                        id: next_id(),
                        name: v.name,
                        value: Box::new(value),
                        span,
                    }));
                }
                Expr::Get(g) => {
                    return Ok(Expr::Set(SetExpr {
                        id: next_id(),
                        object: g.object,
                        name: g.name,
                        value: Box::new(value),
                        span,
                    }));
                }
                _ => {
                    return Err(CompileError::parse(
                        "invalid assignment target",
                        span.offset,
                        span.len,
                    ));
                }
            }
        }

        Ok(expr)
    }

    fn or(&mut self) -> Result<Expr, CompileError> {
        let mut expr = self.and()?;
        while self.match_token(TokenKind::Or) {
            let right = self.and()?;
            let span = Span::new(
                expr.span().offset,
                right.span().offset + right.span().len - expr.span().offset,
            );
            expr = Expr::Logical(LogicalExpr {
                id: next_id(),
                left: Box::new(expr),
                operator: LogicalOp::Or,
                right: Box::new(right),
                span,
            });
        }
        Ok(expr)
    }

    fn and(&mut self) -> Result<Expr, CompileError> {
        let mut expr = self.equality()?;
        while self.match_token(TokenKind::And) {
            let right = self.equality()?;
            let span = Span::new(
                expr.span().offset,
                right.span().offset + right.span().len - expr.span().offset,
            );
            expr = Expr::Logical(LogicalExpr {
                id: next_id(),
                left: Box::new(expr),
                operator: LogicalOp::And,
                right: Box::new(right),
                span,
            });
        }
        Ok(expr)
    }

    fn equality(&mut self) -> Result<Expr, CompileError> {
        let mut expr = self.comparison()?;
        while let Some(op) = self.match_binary_op(&[TokenKind::EqualEqual, TokenKind::BangEqual]) {
            let right = self.comparison()?;
            let span = Span::new(
                expr.span().offset,
                right.span().offset + right.span().len - expr.span().offset,
            );
            expr = Expr::Binary(BinaryExpr {
                id: next_id(),
                left: Box::new(expr),
                operator: op,
                right: Box::new(right),
                span,
            });
        }
        Ok(expr)
    }

    fn comparison(&mut self) -> Result<Expr, CompileError> {
        let mut expr = self.term()?;
        while let Some(op) = self.match_binary_op(&[
            TokenKind::Greater,
            TokenKind::GreaterEqual,
            TokenKind::Less,
            TokenKind::LessEqual,
        ]) {
            let right = self.term()?;
            let span = Span::new(
                expr.span().offset,
                right.span().offset + right.span().len - expr.span().offset,
            );
            expr = Expr::Binary(BinaryExpr {
                id: next_id(),
                left: Box::new(expr),
                operator: op,
                right: Box::new(right),
                span,
            });
        }
        Ok(expr)
    }

    fn term(&mut self) -> Result<Expr, CompileError> {
        let mut expr = self.factor()?;
        while let Some(op) = self.match_binary_op(&[TokenKind::Plus, TokenKind::Minus]) {
            let right = self.factor()?;
            let span = Span::new(
                expr.span().offset,
                right.span().offset + right.span().len - expr.span().offset,
            );
            expr = Expr::Binary(BinaryExpr {
                id: next_id(),
                left: Box::new(expr),
                operator: op,
                right: Box::new(right),
                span,
            });
        }
        Ok(expr)
    }

    fn factor(&mut self) -> Result<Expr, CompileError> {
        let mut expr = self.unary()?;
        while let Some(op) = self.match_binary_op(&[TokenKind::Star, TokenKind::Slash]) {
            let right = self.unary()?;
            let span = Span::new(
                expr.span().offset,
                right.span().offset + right.span().len - expr.span().offset,
            );
            expr = Expr::Binary(BinaryExpr {
                id: next_id(),
                left: Box::new(expr),
                operator: op,
                right: Box::new(right),
                span,
            });
        }
        Ok(expr)
    }

    fn unary(&mut self) -> Result<Expr, CompileError> {
        if self.check(TokenKind::Bang) || self.check(TokenKind::Minus) {
            let start = self.current_span();
            let op = if self.match_token(TokenKind::Bang) {
                UnaryOp::Not
            } else {
                self.advance();
                UnaryOp::Negate
            };
            let operand = self.unary()?;
            let span = Span::new(
                start.offset,
                operand.span().offset + operand.span().len - start.offset,
            );
            return Ok(Expr::Unary(UnaryExpr {
                id: next_id(),
                operator: op,
                operand: Box::new(operand),
                span,
            }));
        }
        self.call()
    }

    fn call(&mut self) -> Result<Expr, CompileError> {
        let mut expr = self.primary()?;

        loop {
            if self.match_token(TokenKind::LeftParen) {
                expr = self.finish_call(expr)?;
            } else if self.match_token(TokenKind::Dot) {
                let name = self.expect_identifier("property name")?;
                let span = Span::new(
                    expr.span().offset,
                    self.previous_span().offset + self.previous_span().len - expr.span().offset,
                );
                expr = Expr::Get(GetExpr {
                    id: next_id(),
                    object: Box::new(expr),
                    name,
                    span,
                });
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn finish_call(&mut self, callee: Expr) -> Result<Expr, CompileError> {
        let mut arguments = Vec::new();
        if !self.check(TokenKind::RightParen) {
            loop {
                if arguments.len() >= 255 {
                    let span = self.current_span();
                    return Err(CompileError::parse(
                        "can't have more than 255 arguments",
                        span.offset,
                        span.len,
                    ));
                }
                arguments.push(self.expression()?);
                if !self.match_token(TokenKind::Comma) {
                    break;
                }
            }
        }
        self.consume(TokenKind::RightParen, "')' after arguments")?;
        let span = Span::new(
            callee.span().offset,
            self.previous_span().offset + self.previous_span().len - callee.span().offset,
        );
        Ok(Expr::Call(CallExpr {
            id: next_id(),
            callee: Box::new(callee),
            arguments,
            span,
        }))
    }

    fn primary(&mut self) -> Result<Expr, CompileError> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Number => {
                self.advance();
                let value: f64 = token
                    .lexeme
                    .parse()
                    .expect("scanner guarantees valid number");
                Ok(Expr::Literal(LiteralExpr {
                    id: next_id(),
                    value: LiteralValue::Number(value),
                    span: token.span,
                }))
            }
            TokenKind::String => {
                self.advance();
                Ok(Expr::Literal(LiteralExpr {
                    id: next_id(),
                    value: LiteralValue::String(token.lexeme),
                    span: token.span,
                }))
            }
            TokenKind::True => {
                self.advance();
                Ok(Expr::Literal(LiteralExpr {
                    id: next_id(),
                    value: LiteralValue::Bool(true),
                    span: token.span,
                }))
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr::Literal(LiteralExpr {
                    id: next_id(),
                    value: LiteralValue::Bool(false),
                    span: token.span,
                }))
            }
            TokenKind::Nil => {
                self.advance();
                Ok(Expr::Literal(LiteralExpr {
                    id: next_id(),
                    value: LiteralValue::Nil,
                    span: token.span,
                }))
            }
            TokenKind::This => {
                self.advance();
                Ok(Expr::This(ThisExpr {
                    id: next_id(),
                    span: token.span,
                }))
            }
            TokenKind::Super => {
                self.advance();
                self.consume(TokenKind::Dot, "'.' after 'super'")?;
                let method = self.expect_identifier("superclass method name")?;
                let span = Span::new(
                    token.span.offset,
                    self.previous_span().offset + self.previous_span().len - token.span.offset,
                );
                Ok(Expr::Super(SuperExpr {
                    id: next_id(),
                    method,
                    span,
                }))
            }
            TokenKind::Identifier => {
                self.advance();
                Ok(Expr::Variable(VariableExpr {
                    id: next_id(),
                    name: token.lexeme,
                    span: token.span,
                }))
            }
            TokenKind::LeftParen => {
                self.advance();
                let expr = self.expression()?;
                self.consume(TokenKind::RightParen, "')' after expression")?;
                let span = Span::new(
                    token.span.offset,
                    self.previous_span().offset + self.previous_span().len - token.span.offset,
                );
                Ok(Expr::Grouping(GroupingExpr {
                    id: next_id(),
                    expression: Box::new(expr),
                    span,
                }))
            }
            _ => Err(CompileError::parse(
                format!("expected expression, found '{}'", token.lexeme),
                token.span.offset,
                token.span.len.max(1),
            )),
        }
    }

    // --- Helper methods ---

    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    fn is_at_end(&self) -> bool {
        self.peek().kind == TokenKind::Eof
    }

    fn advance(&mut self) -> &Token {
        if !self.is_at_end() {
            self.current += 1;
        }
        &self.tokens[self.current - 1]
    }

    fn check(&self, kind: TokenKind) -> bool {
        self.peek().kind == kind
    }

    fn match_token(&mut self, kind: TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn match_binary_op(&mut self, kinds: &[TokenKind]) -> Option<BinaryOp> {
        for &kind in kinds {
            if self.check(kind) {
                self.advance();
                return Some(token_to_binary_op(kind));
            }
        }
        None
    }

    fn consume(&mut self, kind: TokenKind, message: &str) -> Result<&Token, CompileError> {
        if self.check(kind) {
            Ok(self.advance())
        } else {
            let token = self.peek();
            Err(CompileError::parse(
                format!("expected {message}, found '{}'", token.lexeme),
                token.span.offset,
                token.span.len.max(1),
            ))
        }
    }

    fn expect_identifier(&mut self, context: &str) -> Result<String, CompileError> {
        if self.check(TokenKind::Identifier) {
            let token = self.advance().clone();
            Ok(token.lexeme)
        } else {
            let token = self.peek();
            Err(CompileError::parse(
                format!("expected {context}"),
                token.span.offset,
                token.span.len.max(1),
            ))
        }
    }

    fn current_span(&self) -> Span {
        self.peek().span
    }

    fn previous_span(&self) -> Span {
        self.tokens[self.current - 1].span
    }

    fn span_from(&self, start: Span) -> Span {
        let prev = self.previous_span();
        Span::new(start.offset, prev.offset + prev.len - start.offset)
    }

    fn synchronize(&mut self) {
        self.advance();
        while !self.is_at_end() {
            if self.tokens[self.current - 1].kind == TokenKind::Semicolon {
                return;
            }
            match self.peek().kind {
                TokenKind::Class
                | TokenKind::Fun
                | TokenKind::Var
                | TokenKind::For
                | TokenKind::If
                | TokenKind::While
                | TokenKind::Print
                | TokenKind::Return => return,
                _ => {
                    self.advance();
                }
            }
        }
    }
}

fn token_to_binary_op(kind: TokenKind) -> BinaryOp {
    match kind {
        TokenKind::Plus => BinaryOp::Add,
        TokenKind::Minus => BinaryOp::Subtract,
        TokenKind::Star => BinaryOp::Multiply,
        TokenKind::Slash => BinaryOp::Divide,
        TokenKind::EqualEqual => BinaryOp::Equal,
        TokenKind::BangEqual => BinaryOp::NotEqual,
        TokenKind::Less => BinaryOp::Less,
        TokenKind::LessEqual => BinaryOp::LessEqual,
        TokenKind::Greater => BinaryOp::Greater,
        TokenKind::GreaterEqual => BinaryOp::GreaterEqual,
        _ => unreachable!("only called with matched operator tokens"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner;

    fn parse_ok(source: &str) -> Program {
        let tokens = scanner::scan(source).expect("scan should succeed");
        Parser::new(tokens).parse().expect("parse should succeed")
    }

    fn parse_err(source: &str) -> Vec<CompileError> {
        let tokens = scanner::scan(source).expect("scan should succeed");
        Parser::new(tokens).parse().unwrap_err()
    }

    fn parse_sexp(source: &str) -> String {
        let program = parse_ok(source);
        crate::ast::printer::to_sexp(&program).trim().to_string()
    }

    #[test]
    fn precedence_add_mul() {
        assert_eq!(parse_sexp("1 + 2 * 3;"), "(+ 1 (* 2 3))");
    }

    #[test]
    fn precedence_group() {
        assert_eq!(parse_sexp("(1 + 2) * 3;"), "(* (group (+ 1 2)) 3)");
    }

    #[test]
    fn unary_negate() {
        assert_eq!(parse_sexp("-1;"), "(- 1)");
    }

    #[test]
    fn unary_not() {
        assert_eq!(parse_sexp("!true;"), "(! true)");
    }

    #[test]
    fn var_declaration() {
        assert_eq!(parse_sexp("var x = 42;"), "(var x 42)");
    }

    #[test]
    fn var_no_init() {
        assert_eq!(parse_sexp("var x;"), "(var x)");
    }

    #[test]
    fn if_else() {
        assert_eq!(
            parse_sexp("if (true) print 1; else print 2;"),
            "(if true (print 1) (print 2))"
        );
    }

    #[test]
    fn while_loop() {
        assert_eq!(
            parse_sexp("while (true) print 1;"),
            "(while true (print 1))"
        );
    }

    #[test]
    fn for_desugars_to_while() {
        let sexp = parse_sexp("for (var i = 0; i < 10; i = i + 1) print i;");
        assert!(sexp.contains("while"));
        assert!(sexp.contains("var i"));
    }

    #[test]
    fn function_decl() {
        assert_eq!(
            parse_sexp("fun foo(a, b) { return a + b; }"),
            "(fun foo (a b) (return (+ a b)))"
        );
    }

    #[test]
    fn class_with_methods() {
        let sexp = parse_sexp("class Foo { bar() { return 1; } }");
        assert!(sexp.starts_with("(class Foo"));
        assert!(sexp.contains("(fun bar ()"));
    }

    #[test]
    fn class_with_superclass() {
        let sexp = parse_sexp("class Foo < Bar { }");
        assert!(sexp.contains("< Bar"));
    }

    #[test]
    fn error_recovery() {
        let errors = parse_err("var x = ; var y = 1;");
        assert!(!errors.is_empty());
    }

    #[test]
    fn logical_operators() {
        assert_eq!(
            parse_sexp("true and false or true;"),
            "(or (and true false) true)"
        );
    }

    #[test]
    fn function_call() {
        assert_eq!(parse_sexp("foo(1, 2);"), "(call foo 1 2)");
    }

    #[test]
    fn property_access() {
        assert_eq!(parse_sexp("obj.field;"), "(. obj field)");
    }

    #[test]
    fn assignment() {
        assert_eq!(parse_sexp("x = 42;"), "(= x 42)");
    }

    #[test]
    fn set_property() {
        assert_eq!(parse_sexp("obj.field = 42;"), "(.= obj field 42)");
    }

    #[test]
    fn json_output_is_valid() {
        let program = parse_ok("var x = 42;");
        let json = crate::ast::printer::to_json(&program);
        let _: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    }
}
