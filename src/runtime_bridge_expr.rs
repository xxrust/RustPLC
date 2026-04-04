#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExprToken<'a> {
    Number(&'a str),
    Bool(bool),
    Ident(&'a str),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Gt,
    Lt,
    Ge,
    Le,
    And,
    Or,
    Not,
    LParen,
    RParen,
    Comma,
}

fn compile_expr_program(
    state_name: &str,
    raw: &str,
    variable_indices: &HashMap<String, u16>,
) -> Result<ExprProgram, BridgeError> {
    let tokens = tokenize_expr(raw).map_err(|_| BridgeError::UnsupportedAction {
        state: state_name.to_string(),
        action: format!("expr: {raw}"),
    })?;

    let mut compiler = ExprCompiler {
        state_name,
        raw,
        variable_indices,
        tokens,
        pos: 0,
        output: [ExprOp::PushLiteral(0.0); runtime_core::MAX_EXPR_OPS],
        out_len: 0,
    };
    compiler.parse_expression()?;
    if compiler.pos != compiler.tokens.len() {
        return Err(BridgeError::UnsupportedAction {
            state: state_name.to_string(),
            action: format!("unexpected trailing expression content: {raw}"),
        });
    }

    Ok(ExprProgram {
        ops: compiler.output,
        len: compiler.out_len as u8,
    })
}

fn tokenize_expr(raw: &str) -> Result<Vec<ExprToken<'_>>, ()> {
    let mut out = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        if ch.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        if ch.is_ascii_digit() || ch == '.' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                let c = bytes[i] as char;
                if c.is_ascii_digit() || c == '.' {
                    i += 1;
                } else {
                    break;
                }
            }
            out.push(ExprToken::Number(&raw[start..i]));
            continue;
        }

        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                let c = bytes[i] as char;
                if c.is_ascii_alphanumeric() || c == '_' {
                    i += 1;
                } else {
                    break;
                }
            }
            let word = &raw[start..i];
            let lowered = word.to_ascii_lowercase();
            match lowered.as_str() {
                "true" => out.push(ExprToken::Bool(true)),
                "false" => out.push(ExprToken::Bool(false)),
                "and" => out.push(ExprToken::And),
                "or" => out.push(ExprToken::Or),
                "not" => out.push(ExprToken::Not),
                _ => out.push(ExprToken::Ident(word)),
            }
            continue;
        }

        if i + 1 < bytes.len() {
            let two = &raw[i..i + 2];
            match two {
                "==" => {
                    out.push(ExprToken::EqEq);
                    i += 2;
                    continue;
                }
                "!=" => {
                    out.push(ExprToken::NotEq);
                    i += 2;
                    continue;
                }
                ">=" => {
                    out.push(ExprToken::Ge);
                    i += 2;
                    continue;
                }
                "<=" => {
                    out.push(ExprToken::Le);
                    i += 2;
                    continue;
                }
                "&&" => {
                    out.push(ExprToken::And);
                    i += 2;
                    continue;
                }
                "||" => {
                    out.push(ExprToken::Or);
                    i += 2;
                    continue;
                }
                "<>" => {
                    out.push(ExprToken::NotEq);
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }

        match ch {
            '(' => {
                out.push(ExprToken::LParen);
                i += 1;
            }
            ')' => {
                out.push(ExprToken::RParen);
                i += 1;
            }
            '+' => {
                out.push(ExprToken::Plus);
                i += 1;
            }
            '-' => {
                out.push(ExprToken::Minus);
                i += 1;
            }
            '*' => {
                out.push(ExprToken::Star);
                i += 1;
            }
            '/' => {
                out.push(ExprToken::Slash);
                i += 1;
            }
            '%' => {
                out.push(ExprToken::Percent);
                i += 1;
            }
            '>' => {
                out.push(ExprToken::Gt);
                i += 1;
            }
            '<' => {
                out.push(ExprToken::Lt);
                i += 1;
            }
            '=' => {
                out.push(ExprToken::EqEq);
                i += 1;
            }
            '!' => {
                out.push(ExprToken::Not);
                i += 1;
            }
            ',' => {
                out.push(ExprToken::Comma);
                i += 1;
            }
            _ => return Err(()),
        }
    }

    Ok(out)
}

struct ExprCompiler<'a> {
    state_name: &'a str,
    raw: &'a str,
    variable_indices: &'a HashMap<String, u16>,
    tokens: Vec<ExprToken<'a>>,
    pos: usize,
    output: [ExprOp; runtime_core::MAX_EXPR_OPS],
    out_len: usize,
}

impl<'a> ExprCompiler<'a> {
    fn parse_expression(&mut self) -> Result<(), BridgeError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<(), BridgeError> {
        self.parse_and()?;
        while self.consume_if(ExprToken::Or) {
            self.parse_and()?;
            self.push_op(ExprOp::BoolOr)?;
        }
        Ok(())
    }

    fn parse_and(&mut self) -> Result<(), BridgeError> {
        self.parse_comparison()?;
        while self.consume_if(ExprToken::And) {
            self.parse_comparison()?;
            self.push_op(ExprOp::BoolAnd)?;
        }
        Ok(())
    }

    fn parse_comparison(&mut self) -> Result<(), BridgeError> {
        self.parse_additive()?;
        let cmp_op = if self.consume_if(ExprToken::EqEq) {
            Some(ExprOp::CmpEq)
        } else if self.consume_if(ExprToken::NotEq) {
            Some(ExprOp::CmpNe)
        } else if self.consume_if(ExprToken::Ge) {
            Some(ExprOp::CmpGe)
        } else if self.consume_if(ExprToken::Le) {
            Some(ExprOp::CmpLe)
        } else if self.consume_if(ExprToken::Gt) {
            Some(ExprOp::CmpGt)
        } else if self.consume_if(ExprToken::Lt) {
            Some(ExprOp::CmpLt)
        } else {
            None
        };
        if let Some(op) = cmp_op {
            self.parse_additive()?;
            self.push_op(op)?;
        }
        Ok(())
    }

    fn parse_additive(&mut self) -> Result<(), BridgeError> {
        self.parse_multiplicative()?;
        loop {
            let op = if self.consume_if(ExprToken::Plus) {
                Some(ExprOp::Add)
            } else if self.consume_if(ExprToken::Minus) {
                Some(ExprOp::Sub)
            } else {
                None
            };
            let Some(op) = op else { break };
            self.parse_multiplicative()?;
            self.push_op(op)?;
        }
        Ok(())
    }

    fn parse_multiplicative(&mut self) -> Result<(), BridgeError> {
        self.parse_unary()?;
        loop {
            let op = if self.consume_if(ExprToken::Star) {
                Some(ExprOp::Mul)
            } else if self.consume_if(ExprToken::Slash) {
                Some(ExprOp::Div)
            } else if self.consume_if(ExprToken::Percent) {
                Some(ExprOp::Mod)
            } else {
                None
            };
            let Some(op) = op else { break };
            self.parse_unary()?;
            self.push_op(op)?;
        }
        Ok(())
    }

    fn parse_unary(&mut self) -> Result<(), BridgeError> {
        if self.consume_if(ExprToken::Minus) {
            self.parse_unary()?;
            self.push_op(ExprOp::Neg)?;
            return Ok(());
        }
        if self.consume_if(ExprToken::Not) {
            self.parse_unary()?;
            self.push_op(ExprOp::BoolNot)?;
            return Ok(());
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<(), BridgeError> {
        let Some(token) = self.peek().copied() else {
            return self.err("unexpected end of expression");
        };

        match token {
            ExprToken::Number(raw_number) => {
                self.pos += 1;
                let parsed =
                    raw_number
                        .parse::<f32>()
                        .map_err(|_| BridgeError::UnsupportedAction {
                            state: self.state_name.to_string(),
                            action: format!("invalid number in expr: {}", self.raw),
                        })?;
                self.push_op(ExprOp::PushLiteral(parsed))
            }
            ExprToken::Bool(value) => {
                self.pos += 1;
                self.push_op(ExprOp::PushLiteral(if value { 1.0 } else { 0.0 }))
            }
            ExprToken::Ident(name) => {
                self.pos += 1;
                if self.consume_if(ExprToken::LParen) {
                    self.parse_function_call(name)
                } else {
                    let Some(idx) = self.variable_indices.get(name).copied() else {
                        return self.err(format!("undefined variable in expr: {name}"));
                    };
                    self.push_op(ExprOp::PushVariable(idx))
                }
            }
            ExprToken::LParen => {
                self.pos += 1;
                self.parse_expression()?;
                if !self.consume_if(ExprToken::RParen) {
                    return self.err("missing ')' in expression");
                }
                Ok(())
            }
            _ => self.err("unexpected token in expression"),
        }
    }

    fn parse_function_call(&mut self, name: &str) -> Result<(), BridgeError> {
        let mut arg_count = 0usize;
        if !self.consume_if(ExprToken::RParen) {
            loop {
                self.parse_expression()?;
                arg_count += 1;
                if self.consume_if(ExprToken::Comma) {
                    continue;
                }
                if self.consume_if(ExprToken::RParen) {
                    break;
                }
                return self.err("function call missing ')'");
            }
        }

        let op = match (name, arg_count) {
            ("abs", 1) => ExprOp::CallAbs,
            ("min", 2) => ExprOp::CallMin,
            ("max", 2) => ExprOp::CallMax,
            ("sin", 1) => ExprOp::CallSin,
            ("cos", 1) => ExprOp::CallCos,
            ("sqrt", 1) => ExprOp::CallSqrt,
            ("pow", 2) => ExprOp::CallPow,
            ("fmod", 2) => ExprOp::CallFmod,
            ("clamp", 3) => ExprOp::CallClamp,
            _ => {
                return self.err(format!(
                    "unsupported function call in expr: {} with {} args",
                    name, arg_count
                ));
            }
        };
        self.push_op(op)
    }

    fn push_op(&mut self, op: ExprOp) -> Result<(), BridgeError> {
        if self.out_len >= runtime_core::MAX_EXPR_OPS {
            return self.err("expression too long");
        }
        self.output[self.out_len] = op;
        self.out_len += 1;
        Ok(())
    }

    fn consume_if(&mut self, token: ExprToken<'_>) -> bool {
        if self.peek().copied() == Some(token) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<&ExprToken<'a>> {
        self.tokens.get(self.pos)
    }

    fn err<T>(&self, detail: impl Into<String>) -> Result<T, BridgeError> {
        Err(BridgeError::UnsupportedAction {
            state: self.state_name.to_string(),
            action: format!("{}: {}", detail.into(), self.raw),
        })
    }
}

fn stable_log_message_id(message: &str) -> u16 {
    // Deterministic FNV-1a hash folded to 16-bit.
    let mut h: u32 = 0x811c9dc5;
    for b in message.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    (h ^ (h >> 16)) as u16
}
