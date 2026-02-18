//! Proc macros for pflow token model DSL.
//!
//! Provides `schema!` which parses and validates the S-expression DSL at
//! compile time, emitting efficient Schema construction code with zero
//! runtime parsing overhead.

use pflow_dsl::ast::*;
use proc_macro::TokenStream;
use quote::quote;

/// Parse a token model DSL string at compile time and emit a `Schema`.
///
/// # Example
///
/// ```ignore
/// use pflow::schema;
///
/// let s = schema!(r#"
///   (schema counter
///     (version v1.0.0)
///     (states
///       (state count :kind token :initial 5)
///     )
///     (actions
///       (action inc)
///     )
///     (arcs
///       (arc inc -> count)
///     )
///   )
/// "#);
/// assert_eq!(s.name, "counter");
/// ```
#[proc_macro]
pub fn schema(input: TokenStream) -> TokenStream {
    let lit = syn::parse_macro_input!(input as syn::LitStr);
    let dsl_text = lit.value();

    // Parse at compile time
    let node = match pflow_dsl::parse(&dsl_text) {
        Ok(n) => n,
        Err(e) => {
            return syn::Error::new(lit.span(), format!("DSL parse error: {e}"))
                .to_compile_error()
                .into()
        }
    };

    // Validate at compile time (interpret builds a Schema and calls validate())
    if let Err(e) = pflow_dsl::interpret::interpret(&node) {
        return syn::Error::new(lit.span(), format!("DSL validation error: {e}"))
            .to_compile_error()
            .into();
    }

    // Emit Schema construction code
    let tokens = emit_schema(&node);
    tokens.into()
}

fn emit_schema(node: &SchemaNode) -> proc_macro2::TokenStream {
    let name = &node.name;
    let version = &node.version;

    let states = node.states.iter().map(emit_state);
    let actions = node.actions.iter().map(emit_action);
    let arcs = node.arcs.iter().map(emit_arc);
    let constraints = node.constraints.iter().map(emit_constraint);

    let version_stmt = if version.is_empty() {
        quote! {}
    } else {
        quote! { schema.version = #version.into(); }
    };

    quote! {{
        let mut schema = pflow_tokenmodel::schema::Schema::new(#name);
        #version_stmt
        #( schema.add_state(#states); )*
        #( schema.add_action(#actions); )*
        #( schema.add_arc(#arcs); )*
        #( schema.add_constraint(#constraints); )*
        schema
    }}
}

fn emit_state(s: &StateNode) -> proc_macro2::TokenStream {
    let id = &s.id;
    let exported = s.exported;

    let kind = if s.kind == "token" {
        quote! { pflow_tokenmodel::schema::Kind::Token }
    } else {
        quote! { pflow_tokenmodel::schema::Kind::Data }
    };

    let typ = if s.typ.is_empty() {
        quote! { String::new() }
    } else {
        let t = &s.typ;
        quote! { #t.into() }
    };

    let initial = match &s.initial {
        Some(InitialValue::Int(n)) => {
            quote! { Some(serde_json::Value::Number((#n as i64).into())) }
        }
        Some(InitialValue::Str(v)) => {
            quote! { Some(serde_json::Value::String(#v.into())) }
        }
        Some(InitialValue::Nil) | None => {
            quote! { None }
        }
    };

    quote! {
        pflow_tokenmodel::schema::State {
            id: #id.into(),
            kind: #kind,
            typ: #typ,
            initial: #initial,
            exported: #exported,
        }
    }
}

fn emit_action(a: &ActionNode) -> proc_macro2::TokenStream {
    let id = &a.id;
    let guard = if a.guard.is_empty() {
        quote! { String::new() }
    } else {
        let g = &a.guard;
        quote! { #g.into() }
    };

    quote! {
        pflow_tokenmodel::schema::Action {
            id: #id.into(),
            guard: #guard,
            event_id: String::new(),
            event_bindings: None,
        }
    }
}

fn emit_arc(a: &ArcNode) -> proc_macro2::TokenStream {
    let source = &a.source;
    let target = &a.target;

    let keys = if a.keys.is_empty() {
        quote! { vec![] }
    } else {
        let k = &a.keys;
        quote! { vec![#( #k.into() ),*] }
    };

    let value = if a.value.is_empty() {
        quote! { String::new() }
    } else {
        let v = &a.value;
        quote! { #v.into() }
    };

    quote! {
        pflow_tokenmodel::schema::Arc {
            source: #source.into(),
            target: #target.into(),
            keys: #keys,
            value: #value,
        }
    }
}

fn emit_constraint(c: &ConstraintNode) -> proc_macro2::TokenStream {
    let id = &c.id;
    let expr = &c.expr;

    quote! {
        pflow_tokenmodel::schema::Constraint {
            id: #id.into(),
            expr: #expr.into(),
        }
    }
}
