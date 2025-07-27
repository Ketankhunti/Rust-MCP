use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse::Parser,
    parse_macro_input, punctuated::Punctuated, FnArg, GenericArgument, ItemFn, Lit, Meta, Pat,
    PathArguments, ReturnType, Token, Type,
};


// Detect whether a type is Option<T>
fn type_is_option(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            return segment.ident == "Option";
        }
    }
    false
}

// Convert a Rust type to its JSON schema expression using schemars
fn type_to_schema_expr(ty: &Type) -> proc_macro2::TokenStream {
    quote! {
        <#ty as ::schemars::JsonSchema>::json_schema(
            &mut ::schemars::SchemaGenerator::default()
        )
    }
}

#[proc_macro_attribute]
pub fn tool(args: TokenStream, input: TokenStream) -> TokenStream {
    let attr_args = match Punctuated::<Meta, Token![,]>::parse_terminated.parse(args.into()) {
        Ok(args) => args,
        Err(e) => return e.to_compile_error().into(),
    };
    let input_fn = parse_macro_input!(input as ItemFn);
    let fn_name = &input_fn.sig.ident;

    // --- Extract attributes ---
    let mut title = String::new();
    let mut description = String::new();

    for meta in attr_args {
        if let Meta::NameValue(nv) = meta {
            let ident = nv.path.get_ident().unwrap().to_string();
            if let syn::Expr::Lit(expr_lit) = nv.value {
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

    // --- Inputs ---
    let mut param_names = vec![];
    let mut param_types = vec![];
    let mut required_fields = vec![];
    let mut schema_properties = vec![];

    for arg in &input_fn.sig.inputs {
        if let FnArg::Typed(pat_type) = arg {
            let name = if let Pat::Ident(pat_ident) = &*pat_type.pat {
                pat_ident.ident.clone()
            } else {
                panic!("Expected ident in function parameter");
            };
            let ty = &pat_type.ty;
            let name_str = name.to_string();

            let schema_expr = type_to_schema_expr(ty);

            param_names.push(name.clone());
            param_types.push(ty.clone());
            if !type_is_option(ty) {
                required_fields.push(quote! { #name_str });
            }
            schema_properties.push(quote! {
                (#name_str.to_string(), serde_json::to_value(#schema_expr).unwrap())
            });            
        }
    }

    // --- Output ---
    let output_schema = match &input_fn.sig.output {
        ReturnType::Type(_, ty) => {
            if let Type::Path(type_path) = &**ty {
                if let Some(segment) = type_path.path.segments.last() {
                    if segment.ident == "Result" {
                        if let PathArguments::AngleBracketed(args) = &segment.arguments {
                            if let Some(GenericArgument::Type(ok_type)) = args.args.first() {
                                let ok_schema = type_to_schema_expr(ok_type);
                                quote! { #ok_schema }
                            } else {
                                panic!("Tool Result must have Ok type");
                            }
                        } else {
                            panic!("Tool Result must be Result<T, E>");
                        }
                    } else {
                        panic!("Tool must return Result<T, E>");
                    }
                } else {
                    panic!("Unsupported return type");
                }
            } else {
                panic!("Unsupported return type");
            }
        }
        ReturnType::Default => panic!("Tool must return a Result"),
    };

    // --- Tool Registration Code ---
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

            let tool = ::rust_mcp_sdk::Tool {
                name: stringify!(#fn_name).to_string(),
                title: Some(#title.to_string()),
                description: #description.to_string(),
                input_schema: ::rust_mcp_sdk::InputSchema::Inline(input_schema),
                output_schema: Some(::rust_mcp_sdk::InputSchema::Inline(serde_json::json!({
                    "type": "object",
                    "properties": { "value": #output_schema }
                }))),
                requires_auth: Some(false),
                annotations: None,
                experimental: None,
            };

            let handler: ::rust_mcp_sdk::server::ToolExecutionHandler = std::sync::Arc::new(|params, _, _| {
                Box::pin(async move {
                    #(
                        let #param_names: #param_types = serde_json::from_value(
                            params.get(stringify!(#param_names)).cloned().unwrap_or(serde_json::Value::Null)
                        ).map_err(|e| ::rust_mcp_sdk::McpError::ProtocolError(format!("Invalid argument '{}': {}", stringify!(#param_names), e)))?;
                    )*

                    match #fn_name(#(#param_names),*).await {
                        Ok(val) => Ok(serde_json::json!({ "value": val })),
                        Err(e) => Err(::rust_mcp_sdk::McpError::ProtocolError(format!("{:?}", e))),
                    }
                })
            });

            ::rust_mcp_sdk::TOOL_REGISTRY.lock().unwrap().push((tool, handler));
        }
    };

    TokenStream::from(expanded)
}


#[proc_macro_attribute]
pub fn prompt(args: TokenStream, input: TokenStream) -> TokenStream {
    let attr_args = match Punctuated::<Meta, Token![,]>::parse_terminated.parse(args.into()) {
        Ok(args) => args,
        Err(e) => return e.to_compile_error().into(),
    };
    let input_fn = parse_macro_input!(input as ItemFn);
    let fn_name = &input_fn.sig.ident;

    // --- Extract attributes ---
    let mut name = String::new();
    let mut title = String::new();
    let mut description = String::new();

    for meta in attr_args {
        if let Meta::NameValue(nv) = meta {
            let ident = nv.path.get_ident().unwrap().to_string();
            if let syn::Expr::Lit(expr_lit) = nv.value {
                if let Lit::Str(lit) = expr_lit.lit {
                    match ident.as_str() {
                        "name" => name = lit.value(),
                        "title" => title = lit.value(),
                        "description" => description = lit.value(),
                        _ => panic!("Unknown prompt attribute: {}", ident),
                    }
                }
            }
        }
    }
    if name.is_empty() || title.is_empty() || description.is_empty() {
        panic!("#[prompt] requires `name`, `title`, and `description` attributes.");
    }

    // --- Inputs ---
    let mut param_names = vec![];
    let mut param_types = vec![];
    let mut prompt_arguments = vec![];

    for arg in &input_fn.sig.inputs {
        if let FnArg::Typed(pat_type) = arg {
            let param_name = if let Pat::Ident(pat_ident) = &*pat_type.pat {
                pat_ident.ident.clone()
            } else {
                panic!("Unsupported parameter pattern in prompt function.");
            };
            // FIX: Correctly get the type from the pattern.
            let param_ty = &pat_type.ty;
            let name_str = param_name.to_string();

            param_names.push(param_name);
            param_types.push(param_ty.clone());

            let required = !type_is_option(param_ty);

            prompt_arguments.push(quote! {
                ::rust_mcp_sdk::PromptArgument {
                    name: #name_str.to_string(),
                    description: None,
                    required: Some(#required),
                }
            });
            
        }
    }

    // --- Prompt Registration Code ---
    let registration_fn_name = format_ident!("__register_prompt_{}", fn_name);

    let expanded = quote! {
        #input_fn

        #[ctor::ctor]
        fn #registration_fn_name() {
            let prompt = ::rust_mcp_sdk::Prompt {
                name: #name.to_string(),
                title: Some(#title.to_string()),
                description: Some(#description.to_string()),
                arguments: Some(vec![#(#prompt_arguments),*]),
            };

            let handler: ::rust_mcp_sdk::server::PromptHandler = std::sync::Arc::new(|params| {
                Box::pin(async move {
                    #(
                        let #param_names: #param_types = serde_json::from_value(
                            params.get(stringify!(#param_names)).cloned().unwrap_or(serde_json::Value::Null)
                        ).map_err(|e| format!("Invalid argument '{}': {}", stringify!(#param_names), e))?;
                    )*

                    #fn_name(#(#param_names),*).await
                })
            });

            ::rust_mcp_sdk::PROMPT_REGISTRY.lock().unwrap().push((prompt, handler));
        }
    };

    TokenStream::from(expanded)
}