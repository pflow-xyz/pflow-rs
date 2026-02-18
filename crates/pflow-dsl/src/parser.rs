//! S-expression parser producing AST nodes.

use crate::ast::*;
use crate::lexer::{Lexer, Token, TokenType};

/// Parser for the S-expression DSL.
pub struct Parser {
    lexer: Lexer,
    cur: Token,
    peek: Token,
}

impl Parser {
    pub fn new(input: &str) -> Self {
        let mut lexer = Lexer::new(input);
        let cur = lexer.next_token();
        let peek = lexer.next_token();
        Self { lexer, cur, peek }
    }

    fn next_token(&mut self) {
        self.cur = self.peek.clone();
        self.peek = self.lexer.next_token();
    }

    fn expect(&self, t: TokenType) -> Result<(), String> {
        if self.cur.typ != t {
            Err(format!(
                "expected {}, got {} at position {}",
                t, self.cur.typ, self.cur.pos
            ))
        } else {
            Ok(())
        }
    }

    fn expect_symbol(&self, s: &str) -> Result<(), String> {
        if self.cur.typ != TokenType::Symbol || self.cur.literal != s {
            Err(format!(
                "expected symbol {:?}, got {} {:?} at position {}",
                s, self.cur.typ, self.cur.literal, self.cur.pos
            ))
        } else {
            Ok(())
        }
    }

    fn expect_identifier(&mut self) -> Result<String, String> {
        if self.cur.typ == TokenType::Symbol {
            let lit = self.cur.literal.clone();
            self.next_token();
            Ok(lit)
        } else {
            Err(format!(
                "expected identifier, got {} at position {}",
                self.cur.typ, self.cur.pos
            ))
        }
    }

    fn expect_guard_expr(&mut self) -> Result<String, String> {
        if self.cur.typ == TokenType::Guard {
            let lit = self.cur.literal.clone();
            self.next_token();
            Ok(lit)
        } else {
            Err(format!(
                "expected guard expression, got {} at position {}",
                self.cur.typ, self.cur.pos
            ))
        }
    }

    fn parse_schema(&mut self) -> Result<SchemaNode, String> {
        self.expect(TokenType::LParen)?;
        self.next_token();

        self.expect_symbol("schema")?;
        self.next_token();

        let name = self.expect_identifier()?;
        let mut node = SchemaNode {
            name,
            version: String::new(),
            states: Vec::new(),
            actions: Vec::new(),
            arcs: Vec::new(),
            constraints: Vec::new(),
        };

        while self.cur.typ != TokenType::RParen && self.cur.typ != TokenType::Eof {
            if self.cur.typ != TokenType::LParen {
                return Err(format!(
                    "expected '(' for clause, got {} at position {}",
                    self.cur.typ, self.cur.pos
                ));
            }
            self.next_token();

            if self.cur.typ != TokenType::Symbol {
                return Err(format!(
                    "expected clause type, got {} at position {}",
                    self.cur.typ, self.cur.pos
                ));
            }

            match self.cur.literal.as_str() {
                "version" => {
                    self.next_token();
                    node.version = self.expect_identifier()?;
                    self.expect(TokenType::RParen)?;
                    self.next_token();
                }
                "states" => {
                    self.next_token();
                    node.states = self.parse_states()?;
                    self.expect(TokenType::RParen)?;
                    self.next_token();
                }
                "actions" => {
                    self.next_token();
                    node.actions = self.parse_actions()?;
                    self.expect(TokenType::RParen)?;
                    self.next_token();
                }
                "arcs" => {
                    self.next_token();
                    node.arcs = self.parse_arcs()?;
                    self.expect(TokenType::RParen)?;
                    self.next_token();
                }
                "constraints" => {
                    self.next_token();
                    node.constraints = self.parse_constraints()?;
                    self.expect(TokenType::RParen)?;
                    self.next_token();
                }
                other => {
                    return Err(format!(
                        "unknown clause type {:?} at position {}",
                        other, self.cur.pos
                    ));
                }
            }
        }

        Ok(node)
    }

    fn parse_states(&mut self) -> Result<Vec<StateNode>, String> {
        let mut states = Vec::new();

        while self.cur.typ == TokenType::LParen {
            self.next_token();
            self.expect_symbol("state")?;
            self.next_token();

            let id = self.expect_identifier()?;
            let mut state = StateNode {
                id,
                typ: String::new(),
                kind: "data".into(),
                initial: None,
                exported: false,
            };

            while self.cur.typ == TokenType::Keyword {
                let keyword = self.cur.literal.clone();
                self.next_token();

                match keyword.as_str() {
                    ":type" => {
                        state.typ = self.expect_identifier()?;
                    }
                    ":kind" => {
                        self.expect(TokenType::Symbol)?;
                        state.kind = self.cur.literal.clone();
                        self.next_token();
                    }
                    ":initial" => {
                        state.initial = Some(self.parse_value()?);
                    }
                    ":exported" => {
                        state.exported = true;
                    }
                    other => {
                        return Err(format!(
                            "unknown state keyword {:?} at position {}",
                            other, self.cur.pos
                        ));
                    }
                }
            }

            self.expect(TokenType::RParen)?;
            self.next_token();
            states.push(state);
        }

        Ok(states)
    }

    fn parse_actions(&mut self) -> Result<Vec<ActionNode>, String> {
        let mut actions = Vec::new();

        while self.cur.typ == TokenType::LParen {
            self.next_token();
            self.expect_symbol("action")?;
            self.next_token();

            let id = self.expect_identifier()?;
            let mut action = ActionNode {
                id,
                guard: String::new(),
            };

            while self.cur.typ == TokenType::Keyword {
                let keyword = self.cur.literal.clone();
                self.next_token();

                match keyword.as_str() {
                    ":guard" => {
                        action.guard = self.expect_guard_expr()?;
                    }
                    other => {
                        return Err(format!(
                            "unknown action keyword {:?} at position {}",
                            other, self.cur.pos
                        ));
                    }
                }
            }

            self.expect(TokenType::RParen)?;
            self.next_token();
            actions.push(action);
        }

        Ok(actions)
    }

    fn parse_arcs(&mut self) -> Result<Vec<ArcNode>, String> {
        let mut arcs = Vec::new();

        while self.cur.typ == TokenType::LParen {
            self.next_token();
            self.expect_symbol("arc")?;
            self.next_token();

            let source = self.expect_identifier()?;

            self.expect(TokenType::Arrow)?;
            self.next_token();

            let target = self.expect_identifier()?;

            let mut arc = ArcNode {
                source,
                target,
                keys: Vec::new(),
                value: String::new(),
            };

            while self.cur.typ == TokenType::Keyword {
                let keyword = self.cur.literal.clone();
                self.next_token();

                match keyword.as_str() {
                    ":keys" => {
                        arc.keys = self.parse_identifier_list()?;
                    }
                    ":value" => {
                        arc.value = self.expect_identifier()?;
                    }
                    other => {
                        return Err(format!(
                            "unknown arc keyword {:?} at position {}",
                            other, self.cur.pos
                        ));
                    }
                }
            }

            self.expect(TokenType::RParen)?;
            self.next_token();
            arcs.push(arc);
        }

        Ok(arcs)
    }

    fn parse_constraints(&mut self) -> Result<Vec<ConstraintNode>, String> {
        let mut constraints = Vec::new();

        while self.cur.typ == TokenType::LParen {
            self.next_token();
            self.expect_symbol("constraint")?;
            self.next_token();

            let id = self.expect_identifier()?;
            let expr = self.expect_guard_expr()?;

            self.expect(TokenType::RParen)?;
            self.next_token();
            constraints.push(ConstraintNode { id, expr });
        }

        Ok(constraints)
    }

    fn parse_identifier_list(&mut self) -> Result<Vec<String>, String> {
        self.expect(TokenType::LParen)?;
        self.next_token();

        let mut ids = Vec::new();
        while self.cur.typ == TokenType::Symbol {
            ids.push(self.cur.literal.clone());
            self.next_token();
        }

        self.expect(TokenType::RParen)?;
        self.next_token();

        Ok(ids)
    }

    fn parse_value(&mut self) -> Result<InitialValue, String> {
        match self.cur.typ {
            TokenType::Number => {
                let n: i64 = self
                    .cur
                    .literal
                    .parse()
                    .map_err(|e| format!("invalid number: {}", e))?;
                self.next_token();
                Ok(InitialValue::Int(n))
            }
            TokenType::Str => {
                let s = self.cur.literal.clone();
                self.next_token();
                Ok(InitialValue::Str(s))
            }
            TokenType::Symbol if self.cur.literal == "nil" => {
                self.next_token();
                Ok(InitialValue::Nil)
            }
            _ => Err(format!(
                "unexpected token {} in value position at {}",
                self.cur.typ, self.cur.pos
            )),
        }
    }
}

/// Parse S-expression DSL input into a SchemaNode.
pub fn parse(input: &str) -> Result<SchemaNode, String> {
    let mut p = Parser::new(input);
    p.parse_schema()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_schema() {
        let input = r#"(schema ERC-020
  (version v1.0.0)

  (states
    (state totalSupply :type uint256)
    (state balances :type map[address]uint256 :exported)
  )

  (actions
    (action transfer :guard {balances[from] >= amount})
  )

  (arcs
    (arc balances -> transfer :keys (from))
    (arc transfer -> balances :keys (to))
  )

  (constraints
    (constraint conservation {sum(balances) == totalSupply})
  )
)"#;

        let node = parse(input).unwrap();
        assert_eq!(node.name, "ERC-020");
        assert_eq!(node.version, "v1.0.0");
        assert_eq!(node.states.len(), 2);
        assert_eq!(node.actions.len(), 1);
        assert_eq!(node.arcs.len(), 2);
        assert_eq!(node.constraints.len(), 1);

        assert_eq!(node.states[0].id, "totalSupply");
        assert_eq!(node.states[1].id, "balances");
        assert!(node.states[1].exported);

        assert_eq!(node.actions[0].id, "transfer");
        assert_eq!(node.actions[0].guard, "balances[from] >= amount");

        assert_eq!(node.arcs[0].source, "balances");
        assert_eq!(node.arcs[0].target, "transfer");
        assert_eq!(node.arcs[0].keys, vec!["from"]);

        assert_eq!(node.constraints[0].id, "conservation");
        assert_eq!(node.constraints[0].expr, "sum(balances) == totalSupply");
    }

    #[test]
    fn test_parse_token_state() {
        let input = r#"(schema counter
  (states
    (state count :kind token :initial 5)
  )
  (actions
    (action inc)
  )
  (arcs
    (arc inc -> count)
  )
)"#;

        let node = parse(input).unwrap();
        assert_eq!(node.states[0].kind, "token");
        match &node.states[0].initial {
            Some(InitialValue::Int(5)) => {}
            other => panic!("expected Int(5), got {:?}", other),
        }
    }
}
