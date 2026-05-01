use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Fields, GenericArgument, PathArguments, Type, parse_macro_input};

#[proc_macro_derive(Codec)]
pub fn derive_codec(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_codec_impl(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn derive_codec_impl(input: DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(f) => &f.named,
            Fields::Unnamed(f) => {
                return Err(syn::Error::new_spanned(
                    f,
                    "Codec can only be derived for structs with named fields",
                ));
            }
            Fields::Unit => {
                return Err(syn::Error::new_spanned(
                    &input.ident,
                    "Codec cannot be derived for unit structs",
                ));
            }
        },
        Data::Enum(e) => {
            return Err(syn::Error::new_spanned(
                &e.enum_token,
                "Codec can only be derived for structs, not enums",
            ));
        }
        Data::Union(u) => {
            return Err(syn::Error::new_spanned(
                &u.union_token,
                "Codec can only be derived for structs, not unions",
            ));
        }
    };

    let encode_stmts = fields
        .iter()
        .map(|f| {
            let fname = &f.ident;
            encode_for_type(quote!(self.#fname), &f.ty)
        })
        .collect::<syn::Result<Vec<_>>>()?;

    let decode_stmts = fields
        .iter()
        .map(|f| -> syn::Result<TokenStream2> {
            let fname = &f.ident;
            let decode = decode_for_type(&f.ty)?;
            Ok(quote! { #fname: #decode })
        })
        .collect::<syn::Result<Vec<_>>>()?;

    Ok(quote! {
        impl ::xancode::Codec for #name {
            type Error = ::std::boxed::Box<dyn ::std::error::Error>;

            fn encode(&self) -> ::xancode::Bytes {
                let mut buf: ::std::vec::Vec<u8> = ::std::vec![0u8; 4]; // length placeholder
                #(#encode_stmts)*
                let len = (buf.len() - 4) as u32;
                buf[0..4].copy_from_slice(&len.to_be_bytes());
                ::xancode::Bytes::from(buf)
            }

            fn decode(data: &::xancode::Bytes) -> ::std::result::Result<Self, Self::Error> {
                let mut pos = 4usize; // skip length
                ::std::result::Result::Ok(Self { #(#decode_stmts,)* })
            }
        }
    })
}

fn encode_for_type(expr: TokenStream2, ty: &Type) -> syn::Result<TokenStream2> {
    let Type::Path(tp) = ty else {
        return Err(syn::Error::new_spanned(
            ty,
            "Codec derive: only path types are supported \
             (primitives, String, Vec<T>, Option<T>, or types implementing Codec)",
        ));
    };
    let last = tp
        .path
        .segments
        .last()
        .ok_or_else(|| syn::Error::new_spanned(ty, "Codec derive: empty type path"))?;
    let name = last.ident.to_string();

    let tokens = match name.as_str() {
        "u8" | "u16" | "u32" | "u64" | "u128"
        | "i8" | "i16" | "i32" | "i64" | "i128"
        | "f32" | "f64" => {
            quote! { buf.extend_from_slice(&#expr.to_be_bytes()); }
        }
        "bool" => {
            quote! { buf.push(if #expr { 1u8 } else { 0u8 }); }
        }
        "String" => {
            quote! {
                {
                    let __bytes = #expr.as_bytes();
                    let __len = __bytes.len() as u32;
                    buf.extend_from_slice(&__len.to_be_bytes());
                    buf.extend_from_slice(__bytes);
                }
            }
        }
        "Vec" => {
            let inner = extract_single_generic(&last.arguments)?;
            let inner_encode = encode_for_type(quote!((*__elem)), inner)?;
            quote! {
                {
                    let __vec = &#expr;
                    let __count = __vec.len() as u32;
                    buf.extend_from_slice(&__count.to_be_bytes());
                    for __elem in __vec.iter() {
                        #inner_encode
                    }
                }
            }
        }
        "Option" => {
            let inner = extract_single_generic(&last.arguments)?;
            let inner_encode = encode_for_type(quote!((*__inner)), inner)?;
            quote! {
                {
                    match &#expr {
                        ::std::option::Option::None => {
                            buf.push(0u8);
                        }
                        ::std::option::Option::Some(__inner) => {
                            buf.push(1u8);
                            #inner_encode
                        }
                    }
                }
            }
        }
        _ => {
            quote! {
                {
                    let __nested = <_ as ::xancode::Codec>::encode(&#expr);
                    buf.extend_from_slice(&__nested);
                }
            }
        }
    };
    Ok(tokens)
}

fn decode_for_type(ty: &Type) -> syn::Result<TokenStream2> {
    let Type::Path(tp) = ty else {
        return Err(syn::Error::new_spanned(
            ty,
            "Codec derive: only path types are supported \
             (primitives, String, Vec<T>, Option<T>, or types implementing Codec)",
        ));
    };
    let last = tp
        .path
        .segments
        .last()
        .ok_or_else(|| syn::Error::new_spanned(ty, "Codec derive: empty type path"))?;
    let name = last.ident.to_string();

    let tokens = match name.as_str() {
        "u8" | "u16" | "u32" | "u64" | "u128"
        | "i8" | "i16" | "i32" | "i64" | "i128"
        | "f32" | "f64" => {
            let ident = &last.ident;
            quote! {
                {
                    const __SZ: usize = ::std::mem::size_of::<#ident>();
                    if pos + __SZ > data.len() {
                        return ::std::result::Result::Err("unexpected EOF while decoding primitive".into());
                    }
                    let __arr: [u8; __SZ] = (&data[pos..pos + __SZ]).try_into()?;
                    pos += __SZ;
                    #ident::from_be_bytes(__arr)
                }
            }
        }
        "bool" => {
            quote! {
                {
                    if pos + 1 > data.len() {
                        return ::std::result::Result::Err("unexpected EOF while decoding bool".into());
                    }
                    let __b = data[pos];
                    pos += 1;
                    match __b {
                        0 => false,
                        1 => true,
                        __other => {
                            return ::std::result::Result::Err(
                                ::std::format!("invalid bool value: {}", __other).into(),
                            );
                        }
                    }
                }
            }
        }
        "String" => {
            quote! {
                {
                    if pos + 4 > data.len() {
                        return ::std::result::Result::Err("unexpected EOF while decoding String length".into());
                    }
                    let __len = u32::from_be_bytes(
                        (&data[pos..pos + 4]).try_into()?,
                    ) as usize;
                    pos += 4;
                    if pos + __len > data.len() {
                        return ::std::result::Result::Err("unexpected EOF while decoding String body".into());
                    }
                    let __s = ::std::str::from_utf8(&data[pos..pos + __len])?.to_owned();
                    pos += __len;
                    __s
                }
            }
        }
        "Vec" => {
            let inner = extract_single_generic(&last.arguments)?;
            let inner_decode = decode_for_type(inner)?;
            quote! {
                {
                    if pos + 4 > data.len() {
                        return ::std::result::Result::Err("unexpected EOF while decoding Vec length".into());
                    }
                    let __count = u32::from_be_bytes(
                        (&data[pos..pos + 4]).try_into()?,
                    ) as usize;
                    pos += 4;
                    let mut __vec = ::std::vec::Vec::with_capacity(__count);
                    for _ in 0..__count {
                        __vec.push(#inner_decode);
                    }
                    __vec
                }
            }
        }
        "Option" => {
            let inner = extract_single_generic(&last.arguments)?;
            let inner_decode = decode_for_type(inner)?;
            quote! {
                {
                    if pos + 1 > data.len() {
                        return ::std::result::Result::Err("unexpected EOF while decoding Option tag".into());
                    }
                    let __tag = data[pos];
                    pos += 1;
                    match __tag {
                        0 => ::std::option::Option::None,
                        1 => ::std::option::Option::Some(#inner_decode),
                        __other => {
                            return ::std::result::Result::Err(
                                ::std::format!("invalid Option tag: {}", __other).into(),
                            );
                        }
                    }
                }
            }
        }
        _ => {
            quote! {
                {
                    if pos + 4 > data.len() {
                        return ::std::result::Result::Err("unexpected EOF while decoding nested length".into());
                    }
                    let __len = u32::from_be_bytes(
                        (&data[pos..pos + 4]).try_into()?,
                    ) as usize;
                    let __end = pos + 4 + __len;
                    if __end > data.len() {
                        return ::std::result::Result::Err("unexpected EOF while decoding nested body".into());
                    }
                    let __slice = data.slice(pos..__end);
                    let __value = <#ty as ::xancode::Codec>::decode(&__slice)?;
                    pos = __end;
                    __value
                }
            }
        }
    };
    Ok(tokens)
}

fn extract_single_generic(args: &PathArguments) -> syn::Result<&Type> {
    let PathArguments::AngleBracketed(ab) = args else {
        return Err(syn::Error::new_spanned(
            args,
            "expected exactly one angle-bracketed type argument (e.g. Vec<T>, Option<T>)",
        ));
    };
    let mut types = ab.args.iter().filter_map(|a| match a {
        GenericArgument::Type(t) => Some(t),
        _ => None,
    });
    let first = types
        .next()
        .ok_or_else(|| syn::Error::new_spanned(ab, "expected exactly one type argument"))?;
    if types.next().is_some() {
        return Err(syn::Error::new_spanned(
            ab,
            "expected exactly one type argument",
        ));
    }
    Ok(first)
}
