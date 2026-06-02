use core::panic;

use proc_macro::TokenStream as OGTokenStream;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{
    parse::Parse, parse_macro_input, punctuated::Punctuated, token::Paren, FnArg, GenericArgument,
    ItemFn, LitStr, PathArguments, ReturnType, Signature, Token, Type, TypeTuple,
};

#[proc_macro_attribute]
pub fn aidoku_wasm_function(_args: OGTokenStream, input: OGTokenStream) -> OGTokenStream {
    let input = parse_macro_input!(input as ItemFn);
    let Signature {
        ident,
        output,
        inputs,
        ..
    } = &input.sig;

    let internal_ident = get_internal_ident(ident);
    let register_wasm_function_ident = get_register_function_internal_ident(ident);
    let first_input = inputs
        .first()
        .expect("expected to have a first argument with the Caller type");
    let caller_store_type = get_caller_store_type(first_input)
        .expect("expected to have a first argument with the Caller type");
    let input_types: Vec<_> = inputs
        .iter()
        .skip(1)
        .map(|arg| match arg {
            FnArg::Receiver(_) => panic!("cannot have the receiver type (self) on WASM functions"),
            FnArg::Typed(pat) => pat.ty.clone(),
        })
        .collect();

    let argument_accessor_start = quote! {
        let mut params_count = 0;
    };

    let argument_setters: Vec<TokenStream> = input_types.iter()
        .enumerate()
        .map(|(idx, ty)| {
            let argument_name = Ident::new(&format!("arg{idx}"), internal_ident.span());

            quote! {
                let #argument_name = <#ty as ::wasm_shared::FromWasmValues::<#caller_store_type>>::from_wasm_values(
                    &mut caller,
                    &params[params_count..(params_count+<#ty as ::wasm_shared::FromWasmValues::<#caller_store_type>>::WASM_VALUE_COUNT)]
                );
                params_count += <#ty as ::wasm_shared::FromWasmValues<#caller_store_type>>::WASM_VALUE_COUNT;
            }
        })
        .collect();

    let function_call_parameters: Vec<TokenStream> = input_types
        .iter()
        .enumerate()
        // really
        .map(|(idx, _)| {
            let argument_name = Ident::new(&format!("arg{idx}"), internal_ident.span());

            quote! { #argument_name }
        })
        .collect();

    let wasm_parameter_types_array_definition = quote! {
        let mut wasm_parameter_types = ::std::vec::Vec::<::wasmi::ValType>::new();
    };

    let wasm_parameter_types_appenders: Vec<TokenStream> = input_types.iter()
        .map(|ty| {
            quote! { wasm_parameter_types.extend_from_slice(<#ty as ::wasm_shared::FromWasmValues::<#caller_store_type>>::get_wasm_value_types()); }
        })
        .collect();

    let actual_return_type = match output {
        ReturnType::Type(_, ty) => *ty.clone(),
        ReturnType::Default => Type::Tuple(TypeTuple {
            paren_token: Paren::default(),
            elems: Punctuated::default(),
        }),
    };

    let wasm_return_types = quote! {
        <#actual_return_type as ::wasm_shared::WasmFunctionReturnType>::WASM_TYPES.iter().cloned()
    };

    let func = quote! {
        #input

        pub fn #internal_ident(mut caller: ::wasmi::Caller<'_, #caller_store_type>, params: &[::wasmi::Val], results: &mut [::wasmi::Val]) -> ::core::result::Result<(), ::wasmi::Error> {
            use ::wasm_shared::WasmFunctionReturnType;
            #argument_accessor_start
            #(#argument_setters)*

            let result = #ident(caller, #(#function_call_parameters, )*);

            result.write_return_values(stringify!(#ident), results);

            Ok(())
        }

        pub fn #register_wasm_function_ident<'a>(
            linker: &'a mut ::wasmi::Linker<#caller_store_type>,
            module: &str,
            name: &str
        ) -> ::core::result::Result<&'a mut ::wasmi::Linker<#caller_store_type>, ::wasmi::errors::LinkerError> {
            use ::wasm_shared::ToWasmValue;

            #wasm_parameter_types_array_definition
            #(#wasm_parameter_types_appenders)*

            let function_type = ::wasmi::FuncType::new(
                wasm_parameter_types,
                #wasm_return_types
            );

            linker.func_new(module, name, function_type, #internal_ident)
        }
    };

    func.into()
}

struct RegisterWasmFunctionMacroArgs {
    linker_ident: Ident,
    module: LitStr,
    name: LitStr,
    function_name_ident: Ident,
}

impl Parse for RegisterWasmFunctionMacroArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let linker_ident: Ident = input.parse()?;
        let _: Token![,] = input.parse()?;
        let module: LitStr = input.parse()?;
        let _: Token![,] = input.parse()?;
        let name: LitStr = input.parse()?;
        let _: Token![,] = input.parse()?;
        let function_name_ident: Ident = input.parse()?;

        Ok(Self {
            linker_ident,
            module,
            name,
            function_name_ident,
        })
    }
}

#[proc_macro]
pub fn register_wasm_function(input: OGTokenStream) -> OGTokenStream {
    let RegisterWasmFunctionMacroArgs {
        linker_ident,
        module,
        name,
        function_name_ident,
    } = parse_macro_input!(input as RegisterWasmFunctionMacroArgs);
    let register_wasm_function_ident = get_register_function_internal_ident(&function_name_ident);

    quote! {
        #register_wasm_function_ident(#linker_ident, #module, #name)
    }
    .into()
}

fn get_internal_ident(ident: &Ident) -> Ident {
    Ident::new(&format!("__wasm_function_{ident}"), Span::call_site())
}

fn get_register_function_internal_ident(ident: &Ident) -> Ident {
    Ident::new(
        &format!("__register_wasm_function_{ident}"),
        Span::call_site(),
    )
}

fn get_caller_store_type(arg: &FnArg) -> Option<Type> {
    let pat = match arg {
        FnArg::Typed(pat) => Some(pat),
        _ => None,
    }?;

    let type_path = match &*pat.ty {
        Type::Path(type_path) => Some(type_path),
        _ => None,
    }?;

    let last_segment = type_path.path.segments.last()?;
    let last_generic_argument = match &last_segment.arguments {
        PathArguments::AngleBracketed(type_arguments) => Some(type_arguments.args.last()?),
        _ => None,
    }?;

    match last_generic_argument {
        GenericArgument::Type(t) => Some(t.clone()),
        _ => None,
    }
}
