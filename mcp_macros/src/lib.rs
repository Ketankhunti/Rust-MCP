use proc_macro::TokenStream;
use quote::{quote, format_ident};
use syn::{
    parse::Parser, // <-- FIX: Bring the Parser trait into scope
    parse_macro_input, ItemFn, Meta, Lit, FnArg, Pat, Type, ReturnType,
    PathArguments, GenericArgument, Expr, punctuated::Punctuated, Token,
};

// Helper function to map a Rust Type to a JSON schema string
fn type_to_json_schema_str(ty: &Type) -> String {
    // A simple type-to-string conversion for schema generation.
    let type_str = quote!(#ty).to_string().replace(' ', "");
    match type_str.as_str() {
        "String" | "&str" => "string".to_string(),
        "f64" | "f32" => "number".to_string(),
        "i64" | "i32" | "u64" | "u32" | "isize" | "usize" => "integer".to_string(),
        "bool" => "boolean".to_string(),
        "Value" | "serde_json::Value" => "object".to_string(), // Represents any JSON object
        _ => panic!("Unsupported argument type in tool function: {}", type_str),
    }
}

#[proc_macro_attribute]
pub fn tool(args: TokenStream, input: TokenStream) -> TokenStream {
    // This is the modern, robust way to parse attributes.
    let attr_args = match Punctuated::<Meta, Token![,]>::parse_terminated.parse(args.into()) {
        Ok(args) => args,
        Err(e) => return e.to_compile_error().into(),
    };
    let input_fn = parse_macro_input!(input as ItemFn);

    let fn_name = &input_fn.sig.ident;

    // --- 1. Parse Attributes (title, description) ---
    let mut title = String::new();
    let mut description = String::new();

    for meta in attr_args {
        if let Meta::NameValue(nv) = meta {
            let ident = nv.path.get_ident().unwrap().to_string();
            // FIX: The `value` field is an `Expr`, which can contain a `Lit`.
            // This pattern correctly extracts the string literal.
            if let Expr::Lit(expr_lit) = nv.value {
                if let Lit::Str(lit) = expr_lit.lit {
                    match ident.as_str() {
                        "title" => title = lit.value(),
                        "description" => description = lit.value(),
                        _ => panic!("Unknown attribute: {}", ident),
                    }
                }
            }
        }
    }
    if title.is_empty() || description.is_empty() {
        panic!("#[tool] requires both `title` and `description` attributes.");
    }

    // --- 2. Parse Function Inputs for Schema and Deserialization ---
    let mut param_names = vec![];
    let mut param_types = vec![];
    let mut required_fields = vec![];
    let mut schema_properties = vec![];

    for arg in &input_fn.sig.inputs {
        if let FnArg::Typed(pat_type) = arg {
            let name = if let Pat::Ident(pat_ident) = &*pat_type.pat {
                pat_ident.ident.clone()
            } else {
                panic!("Unsupported parameter pattern, expected simple identifiers like `name: Type`.");
            };
            let ty = &pat_type.ty;
            let name_str = name.to_string();

            param_names.push(name);
            param_types.push(ty.clone());
            required_fields.push(quote! { #name_str });

            let schema_type_str = type_to_json_schema_str(ty);
            schema_properties.push(quote! {
                (#name_str.to_string(), serde_json::json!({ "type": #schema_type_str }))
            });
        }
    }

    // --- 3. Parse Function Output for Schema (Dynamically) ---
    let output_schema = match &input_fn.sig.output {
        ReturnType::Type(_, ty) => {
            if let Type::Path(type_path) = &**ty {
                if let Some(segment) = type_path.path.segments.last() {
                    if segment.ident == "Result" {
                        if let PathArguments::AngleBracketed(args) = &segment.arguments {
                            if let Some(GenericArgument::Type(ok_type)) = args.args.first() {
                                let schema_type = type_to_json_schema_str(ok_type);
                                quote! { serde_json::json!({ "type": #schema_type }) }
                            } else {
                                panic!("Could not determine the `Ok` type from the function's Result.");
                            }
                        } else {
                            panic!("The tool function's `Result` must have generic arguments like `Result<f64, String>`.");
                        }
                    } else {
                        panic!("Tool function must return a `Result`.");
                    }
                } else {
                    panic!("Unsupported return type path.");
                }
            } else {
                panic!("Unsupported return type. Must be a `Result`.");
            }
        }
        ReturnType::Default => panic!("Tool function must have a return type."),
    };

    // --- 4. Generate the Registration Code ---
    let registration_fn_name = format_ident!("__register_tool_{}", fn_name);

    let expanded = quote! {
        #input_fn

        #[ctor::ctor]
        fn #registration_fn_name() {
            let input_schema_props: std::collections::HashMap<String, serde_json::Value> = [
                #(#schema_properties),*
            ].iter().cloned().collect();

            let input_schema = serde_json::json!({
                "type": "object",
                "properties": input_schema_props,
                "required": [#(#required_fields),*]
            });

            let tool = rust_mcp_sdk::Tool {
                name: stringify!(#fn_name).to_string(),
                title: Some(#title.to_string()),
                description: #description.to_string(),
                input_schema: rust_mcp_sdk::InputSchema::Inline(input_schema),
                output_schema: Some(rust_mcp_sdk::InputSchema::Inline(serde_json::json!({
                    "type": "object",
                    "properties": { "value": #output_schema }
                }))),
                requires_auth: Some(false),
                annotations: None,
                experimental: None,
            };

            let handler: rust_mcp_sdk::server::ToolExecutionHandler = std::sync::Arc::new(|params, _, _| {
                Box::pin(async move {
                    #(
                        let #param_names: #param_types = serde_json::from_value(
                            params.get(stringify!(#param_names)).cloned().unwrap_or(serde_json::Value::Null)
                        ).map_err(|e| rust_mcp_sdk::McpError::ProtocolError(format!("Invalid argument '{}': {}", stringify!(#param_names), e)))?;
                    )*

                    match #fn_name(#(#param_names),*).await {
                        Ok(val) => Ok(serde_json::json!({ "value": val })),
                        Err(e) => Err(rust_mcp_sdk::McpError::ProtocolError(format!("{:?}", e))),
                    }
                })
            });

           ::rust_mcp_sdk::TOOL_REGISTRY.lock().unwrap().push((tool, handler));
        }
    };

    TokenStream::from(expanded)
}
