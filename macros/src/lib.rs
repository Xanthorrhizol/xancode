use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    Data, DataEnum, DataStruct, DeriveInput, Fields, GenericArgument, PathArguments, Type,
    parse_macro_input,
};

#[proc_macro_derive(Codec)]
pub fn derive_codec(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_codec_impl(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn derive_codec_impl(input: DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;

    let (encode_body, decode_body) = match &input.data {
        Data::Struct(data) => struct_bodies(&input, data)?,
        Data::Enum(data) => enum_bodies(&input, data)?,
        Data::Union(u) => {
            return Err(syn::Error::new_spanned(
                &u.union_token,
                "Codec can only be derived for structs and enums, not unions",
            ));
        }
    };

    Ok(quote! {
        impl ::xancode::Codec for #name {
            type Error = ::std::boxed::Box<dyn ::std::error::Error>;

            fn encode(&self) -> ::xancode::Bytes {
                let mut buf: ::std::vec::Vec<u8> = ::std::vec![0u8; 4]; // length placeholder
                #encode_body
                let len = (buf.len() - 4) as u32;
                buf[0..4].copy_from_slice(&len.to_be_bytes());
                ::xancode::Bytes::from(buf)
            }

            fn decode(data: &::xancode::Bytes) -> ::std::result::Result<Self, Self::Error> {
                let mut pos = 4usize; // skip length
                #decode_body
            }
        }
    })
}

fn struct_bodies(
    input: &DeriveInput,
    data: &DataStruct,
) -> syn::Result<(TokenStream2, TokenStream2)> {
    let fields = match &data.fields {
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

    Ok((
        quote! { #(#encode_stmts)* },
        quote! { ::std::result::Result::Ok(Self { #(#decode_stmts,)* }) },
    ))
}

fn enum_bodies(
    input: &DeriveInput,
    data: &DataEnum,
) -> syn::Result<(TokenStream2, TokenStream2)> {
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "Codec cannot be derived for empty enums",
        ));
    }
    if data.variants.len() > 256 {
        return Err(syn::Error::new_spanned(
            &input.ident,
            format!(
                "Codec supports at most 256 enum variants (got {})",
                data.variants.len()
            ),
        ));
    }

    let mut encode_arms = Vec::new();
    let mut decode_arms = Vec::new();

    for (idx, variant) in data.variants.iter().enumerate() {
        let tag = idx as u8;
        let vname = &variant.ident;

        match &variant.fields {
            Fields::Unit => {
                encode_arms.push(quote! {
                    Self::#vname => { buf.push(#tag); }
                });
                decode_arms.push(quote! {
                    #tag => ::std::result::Result::Ok(Self::#vname),
                });
            }
            Fields::Unnamed(fields) => {
                let bindings: Vec<_> = (0..fields.unnamed.len())
                    .map(|i| format_ident!("__f{}", i))
                    .collect();
                let encode_each = fields
                    .unnamed
                    .iter()
                    .zip(&bindings)
                    .map(|(f, b)| encode_for_type(quote!((*#b)), &f.ty))
                    .collect::<syn::Result<Vec<_>>>()?;
                let decode_each = fields
                    .unnamed
                    .iter()
                    .zip(&bindings)
                    .map(|(f, b)| -> syn::Result<TokenStream2> {
                        let decode = decode_for_type(&f.ty)?;
                        Ok(quote! { let #b = #decode; })
                    })
                    .collect::<syn::Result<Vec<_>>>()?;
                encode_arms.push(quote! {
                    Self::#vname(#(#bindings),*) => {
                        buf.push(#tag);
                        #(#encode_each)*
                    }
                });
                decode_arms.push(quote! {
                    #tag => {
                        #(#decode_each)*
                        ::std::result::Result::Ok(Self::#vname(#(#bindings),*))
                    },
                });
            }
            Fields::Named(fields) => {
                let names: Vec<_> = fields
                    .named
                    .iter()
                    .map(|f| f.ident.clone().expect("named field has ident"))
                    .collect();
                let encode_each = fields
                    .named
                    .iter()
                    .zip(&names)
                    .map(|(f, n)| encode_for_type(quote!((*#n)), &f.ty))
                    .collect::<syn::Result<Vec<_>>>()?;
                let decode_each = fields
                    .named
                    .iter()
                    .zip(&names)
                    .map(|(f, n)| -> syn::Result<TokenStream2> {
                        let decode = decode_for_type(&f.ty)?;
                        Ok(quote! { let #n = #decode; })
                    })
                    .collect::<syn::Result<Vec<_>>>()?;
                encode_arms.push(quote! {
                    Self::#vname { #(#names),* } => {
                        buf.push(#tag);
                        #(#encode_each)*
                    }
                });
                decode_arms.push(quote! {
                    #tag => {
                        #(#decode_each)*
                        ::std::result::Result::Ok(Self::#vname { #(#names),* })
                    },
                });
            }
        }
    }

    let encode_body = quote! {
        match self {
            #(#encode_arms)*
        }
    };
    let decode_body = quote! {
        if pos + 1 > data.len() {
            return ::std::result::Result::Err("unexpected EOF reading enum tag".into());
        }
        let __tag = data[pos];
        pos += 1;
        match __tag {
            #(#decode_arms)*
            __other => ::std::result::Result::Err(
                ::std::format!("invalid enum tag: {}", __other).into(),
            ),
        }
    };
    Ok((encode_body, decode_body))
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
        "HashSet" | "BTreeSet" => {
            let inner = extract_single_generic(&last.arguments)?;
            let inner_encode = encode_for_type(quote!((*__elem)), inner)?;
            quote! {
                {
                    let __set = &#expr;
                    let __count = __set.len() as u32;
                    buf.extend_from_slice(&__count.to_be_bytes());
                    for __elem in __set.iter() {
                        #inner_encode
                    }
                }
            }
        }
        "HashMap" | "BTreeMap" => {
            let (k_ty, v_ty) = extract_two_generics(&last.arguments)?;
            let k_encode = encode_for_type(quote!((*__k)), k_ty)?;
            let v_encode = encode_for_type(quote!((*__v)), v_ty)?;
            quote! {
                {
                    let __map = &#expr;
                    let __count = __map.len() as u32;
                    buf.extend_from_slice(&__count.to_be_bytes());
                    for (__k, __v) in __map.iter() {
                        #k_encode
                        #v_encode
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
        "HashSet" | "BTreeSet" => {
            let inner = extract_single_generic(&last.arguments)?;
            let inner_decode = decode_for_type(inner)?;
            quote! {
                {
                    if pos + 4 > data.len() {
                        return ::std::result::Result::Err("unexpected EOF while decoding Set length".into());
                    }
                    let __count = u32::from_be_bytes(
                        (&data[pos..pos + 4]).try_into()?,
                    ) as usize;
                    pos += 4;
                    let mut __set = <#ty>::new();
                    for _ in 0..__count {
                        __set.insert(#inner_decode);
                    }
                    __set
                }
            }
        }
        "HashMap" | "BTreeMap" => {
            let (k_ty, v_ty) = extract_two_generics(&last.arguments)?;
            let k_decode = decode_for_type(k_ty)?;
            let v_decode = decode_for_type(v_ty)?;
            quote! {
                {
                    if pos + 4 > data.len() {
                        return ::std::result::Result::Err("unexpected EOF while decoding Map length".into());
                    }
                    let __count = u32::from_be_bytes(
                        (&data[pos..pos + 4]).try_into()?,
                    ) as usize;
                    pos += 4;
                    let mut __map = <#ty>::new();
                    for _ in 0..__count {
                        let __k = #k_decode;
                        let __v = #v_decode;
                        __map.insert(__k, __v);
                    }
                    __map
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

fn extract_two_generics(args: &PathArguments) -> syn::Result<(&Type, &Type)> {
    let PathArguments::AngleBracketed(ab) = args else {
        return Err(syn::Error::new_spanned(
            args,
            "expected exactly two angle-bracketed type arguments (e.g. HashMap<K, V>)",
        ));
    };
    let mut types = ab.args.iter().filter_map(|a| match a {
        GenericArgument::Type(t) => Some(t),
        _ => None,
    });
    let first = types
        .next()
        .ok_or_else(|| syn::Error::new_spanned(ab, "expected exactly two type arguments"))?;
    let second = types
        .next()
        .ok_or_else(|| syn::Error::new_spanned(ab, "expected exactly two type arguments"))?;
    if types.next().is_some() {
        return Err(syn::Error::new_spanned(
            ab,
            "expected exactly two type arguments",
        ));
    }
    Ok((first, second))
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
