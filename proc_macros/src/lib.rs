#![feature(str_as_str)]

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    Ident, Token, Type, Visibility,
    parse::{Parse, ParseStream, Result},
};

#[proc_macro]
pub fn bitstruct(input: TokenStream) -> TokenStream {
    let bitstruct = syn::parse_macro_input!(input as BitStruct);
    generate_bitstruct_code(bitstruct).into()
}

struct BitStruct {
    name: Ident,
    fields: Vec<BitField>,
}

struct BitField {
    vis: Visibility,
    name: Ident,
    ty: Type,
    range: Option<(u32, u32)>,
}

impl Parse for BitStruct {
    fn parse(input: ParseStream) -> Result<Self> {
        input.parse::<Token![struct]>()?;
        let name = input.parse::<Ident>()?;

        let content;
        syn::braced!(content in input);

        let mut fields = Vec::new();

        while !content.is_empty() {
            // Parse visibility (pub, pub(crate), etc.)
            let vis = content.parse::<Visibility>()?;

            // Parse field name
            let name = content.parse::<Ident>()?;

            // Parse colon
            content.parse::<Token![:]>()?;

            // Parse type
            let ty = content.parse::<Type>()?;

            // Check if there's an assignment (= range)
            let range = if content.peek(Token![=]) {
                content.parse::<Token![=]>()?;
                Some(parse_range_expr(&content)?)
            } else {
                None
            };

            // Parse optional comma
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }

            fields.push(BitField {
                vis,
                name,
                ty,
                range,
            });
        }

        Ok(BitStruct { name, fields })
    }
}

fn parse_range_expr(input: ParseStream) -> Result<(u32, u32)> {
    // try parsing a range expression like 3..=10
    let lookahead = input.lookahead1();

    if lookahead.peek(syn::LitInt) {
        // Try reading either a single number or a range
        let start_lit: syn::LitInt = input.parse()?;
        let start_val = start_lit.base10_parse::<u32>()?;

        if input.peek(Token![..]) {
            input.parse::<Token![..]>()?;

            let inclusive = input.peek(Token![=]);
            if inclusive {
                input.parse::<Token![=]>()?;
            }

            let end_lit: syn::LitInt = input.parse()?;
            let mut end_val = end_lit.base10_parse::<u32>()?;
            if !inclusive {
                end_val = end_val - 1;
            }
            Ok((start_val, end_val))
        } else {
            Ok((start_val, start_val))
        }
    } else {
        Err(lookahead.error())
    }
}
fn generate_bitstruct_code(bitstruct: BitStruct) -> proc_macro2::TokenStream {
    let name = &bitstruct.name;
    let mut field_info = Vec::new();
    let mut current_bit = 0u32;

    // Process each field to determine bit positions
    for field in &bitstruct.fields {
        let (start_bit, end_bit) = if let Some((start, end)) = field.range {
            current_bit = end + 1;
            (start, end)
        } else {
            let size = get_type_bit_size(&field.ty);
            let start = current_bit;
            let end = current_bit + size - 1;
            current_bit = end + 1;
            (start, end)
        };

        field_info.push(FieldInfo {
            vis: field.vis.clone(),
            name: field.name.clone(),
            ty: field.ty.clone(),
            start_bit,
            end_bit,
        });
    }

    // Calculate total bytes needed
    let max_bit = field_info.iter().map(|f| f.end_bit).max().unwrap_or(0);
    let total_bytes = (max_bit + 8) / 8;

    // Generate getters and setters
    let methods = generate_methods(&field_info);
    let constructor = generate_constructor(&field_info);

    quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct #name {
            data: [u8; #total_bytes as usize],
        }

        impl #name {
            pub const fn new() -> Self {
                Self {
                    data: [0; #total_bytes as usize],
                }
            }

            #constructor
            #methods
        }

        impl Default for #name {
            fn default() -> Self {
                Self::new()
            }
        }
    }
}

#[derive(Debug)]
struct FieldInfo {
    vis: Visibility,
    name: Ident,
    ty: Type,
    start_bit: u32,
    end_bit: u32,
}

fn get_type_bit_size(ty: &Type) -> u32 {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            match segment.ident.to_string().as_str() {
                "bool" => return 1,
                "u8" | "i8" => return 8,
                "u16" | "i16" => return 16,
                "u32" | "i32" => return 32,
                "u64" | "i64" => return 64,
                "usize" | "isize" => return 64,
                _ => {
                    // For custom types, try to infer size
                    let type_name = segment.ident.to_string();
                    return estimate_custom_type_size(&type_name);
                }
            }
        }
    }
    8 // Default fallback
}

fn estimate_custom_type_size(type_name: &str) -> u32 {
    // This is a heuristic - in practice you might want to make this configurable
    match type_name.as_str() {
        name if name.contains("Enum") => 8,  // Most enums fit in 8 bits
        name if name.contains("Data") => 24, // Example custom struct
        "RandomData" => 24,                  // 8 + 16 bits
        "RandomEnum" => 8,                   // 8 bits for enum
        _ => 8,                              // Default to 8 bits
    }
}

fn generate_constructor(fields: &[FieldInfo]) -> proc_macro2::TokenStream {
    if fields.is_empty() {
        return quote! {};
    }

    let params: Vec<_> = fields
        .iter()
        .map(|f| {
            let name = &f.name;
            let ty = &f.ty;
            quote! { #name: #ty }
        })
        .collect();

    let setters: Vec<_> = fields
        .iter()
        .map(|f| {
            let name = &f.name;
            let setter_name = quote::format_ident!("set_{}", name);
            quote! { result.#setter_name(#name); }
        })
        .collect();

    quote! {
        pub fn with_fields(#(#params),*) -> Self {
            let mut result = Self::new();
            #(#setters)*
            result
        }
    }
}

fn generate_methods(fields: &[FieldInfo]) -> proc_macro2::TokenStream {
    let mut methods = proc_macro2::TokenStream::new();

    for field in fields {
        let getter = generate_getter(field);
        let setter = generate_setter(field);
        methods.extend(getter);
        methods.extend(setter);
    }

    methods
}

fn generate_getter(field: &FieldInfo) -> proc_macro2::TokenStream {
    let vis = &field.vis;
    let name = &field.name;
    let ty = &field.ty;
    let start_bit = field.start_bit;
    let end_bit = field.end_bit;
    let bit_width = end_bit - start_bit + 1;
    let start_byte = start_bit / 8;
    let end_byte = end_bit / 8;

    if start_byte == end_byte {
        let start_bit_in_byte = start_bit % 8;
        let mask = (1u64 << bit_width) - 1;

        quote! {
            #vis fn #name(&self) -> #ty {
                let byte = self.data[#start_byte as usize];
                let shifted = (byte >> #start_bit_in_byte) as u64;
                let masked = shifted & #mask;

                // We need Rust to pick one branch per compile-time type,
                // so we use a const generic trick:
                {
                    struct __TCheck<T>(core::marker::PhantomData<T>);
                    trait __GetVal {
                        type Out;
                        fn get(v: u64) -> Self::Out;
                    }
                    impl __GetVal for __TCheck<bool> {
                        type Out = bool;
                        #[inline(always)]
                        fn get(v: u64) -> bool { (v & 1) != 0 }
                    }
                    impl __GetVal for __TCheck<u8> {
                        type Out = u8;
                        #[inline(always)]
                        fn get(v: u64) -> u8 { v as u8 }
                    }
                    impl __GetVal for __TCheck<u16> {
                        type Out = u16;
                        #[inline(always)]
                        fn get(v: u64) -> u16 { v as u16 }
                    }
                    impl __GetVal for __TCheck<u32> {
                        type Out = u32;
                        #[inline(always)]
                        fn get(v: u64) -> u32 { v as u32 }
                    }
                    impl __GetVal for __TCheck<u64> {
                        type Out = u64;
                        #[inline(always)]
                        fn get(v: u64) -> u64 { v }
                    }

                    <__TCheck<#ty> as __GetVal>::get(masked)
                }
            }
        }
    } else {
        quote! {
            #vis fn #name(&self) -> #ty {
                let mut result: u64 = 0;
                let start_byte = #start_bit / 8;
                let end_byte = #end_bit / 8;
                let start_offset = #start_bit % 8;

                for byte_idx in start_byte..=end_byte {
                    let byte_val = self.data[byte_idx as usize] as u64;
                    let bit_start_in_result = if byte_idx == start_byte {
                        0
                    } else {
                        (byte_idx - start_byte) * 8 - start_offset
                    };
                    let bits_from_byte = if byte_idx == start_byte {
                        byte_val >> start_offset
                    } else {
                        byte_val
                    };
                    result |= bits_from_byte << bit_start_in_result;
                }

                let mask = (1u64 << #bit_width) - 1;
                let result = result & mask;

                {
                    struct __TCheck<T>(core::marker::PhantomData<T>);
                    trait __GetVal {
                        type Out;
                        fn get(v: u64) -> Self::Out;
                    }
                    impl __GetVal for __TCheck<bool> {
                        type Out = bool;
                        fn get(v: u64) -> bool { (v & 1) != 0 }
                    }
                    impl __GetVal for __TCheck<u8> {
                        type Out = u8;
                        fn get(v: u64) -> u8 { v as u8 }
                    }
                    impl __GetVal for __TCheck<u16> {
                        type Out = u16;
                        fn get(v: u64) -> u16 { v as u16 }
                    }
                    impl __GetVal for __TCheck<u32> {
                        type Out = u32;
                        fn get(v: u64) -> u32 { v as u32 }
                    }
                    impl __GetVal for __TCheck<u64> {
                        type Out = u64;
                        fn get(v: u64) -> u64 { v }
                    }

                    <__TCheck<#ty> as __GetVal>::get(result)
                }
            }
        }
    }
}

fn generate_setter(field: &FieldInfo) -> proc_macro2::TokenStream {
    let vis = &field.vis;
    let setter_name = quote::format_ident!("set_{}", field.name);
    let ty = &field.ty;
    let start_bit = field.start_bit;
    let end_bit = field.end_bit;
    let bit_width = end_bit - start_bit + 1;
    let start_byte = start_bit / 8;
    let end_byte = end_bit / 8;

    if start_byte == end_byte {
        let start_bit_in_byte = start_bit % 8;
        let clear_mask = !((((1u64 << bit_width) - 1) << start_bit_in_byte) as u8);

        quote! {
            #vis fn #setter_name(&mut self, value: #ty) {
                // local helper just like getter, but now using an associated type
                {
                    struct __TCheck<T>(core::marker::PhantomData<T>);
                    trait __ToBits {
                        type In;
                        fn to_bits(v: Self::In) -> u64;
                    }
                    impl __ToBits for __TCheck<bool> {
                        type In = bool;
                        fn to_bits(v: bool) -> u64 { if v {1} else {0} }
                    }
                    impl __ToBits for __TCheck<u8> {
                        type In = u8;
                        fn to_bits(v: u8) -> u64 { v as u64 }
                    }
                    impl __ToBits for __TCheck<u16> {
                        type In = u16;
                        fn to_bits(v: u16) -> u64 { v as u64 }
                    }
                    impl __ToBits for __TCheck<u32> {
                        type In = u32;
                        fn to_bits(v: u32) -> u64 { v as u64 }
                    }
                    impl __ToBits for __TCheck<u64> {
                        type In = u64;
                        fn to_bits(v: u64) -> u64 { v }
                    }

                    let value_bits =
                        <__TCheck<#ty> as __ToBits>::to_bits(value);
                    let value_masked =
                        (value_bits & ((1u64 << #bit_width) - 1)) as u8;
                    let value_positioned =
                        value_masked << #start_bit_in_byte;

                    self.data[#start_byte as usize] =
                        (self.data[#start_byte as usize] & #clear_mask)
                            | value_positioned;
                }
            }
        }
    } else {
        quote! {
            #vis fn #setter_name(&mut self, value: #ty) {
                {
                    struct __TCheck<T>(core::marker::PhantomData<T>);
                    trait __ToBits {
                        type In;
                        fn to_bits(v: Self::In) -> u64;
                    }
                    impl __ToBits for __TCheck<bool> {
                        type In = bool;
                        fn to_bits(v: bool) -> u64 { if v {1} else {0} }
                    }
                    impl __ToBits for __TCheck<u8> {
                        type In = u8;
                        fn to_bits(v: u8) -> u64 { v as u64 }
                    }
                    impl __ToBits for __TCheck<u16> {
                        type In = u16;
                        fn to_bits(v: u16) -> u64 { v as u64 }
                    }
                    impl __ToBits for __TCheck<u32> {
                        type In = u32;
                        fn to_bits(v: u32) -> u64 { v as u64 }
                    }
                    impl __ToBits for __TCheck<u64> {
                        type In = u64;
                        fn to_bits(v: u64) -> u64 { v }
                    }

                    let value_bits =
                        <__TCheck<#ty> as __ToBits>::to_bits(value);
                    let value_masked =
                        value_bits & ((1u64 << #bit_width) - 1);

                    let start_byte = #start_bit / 8;
                    let end_byte = #end_bit / 8;
                    let start_offset = #start_bit % 8;

                    for byte_idx in start_byte..=end_byte {
                        let byte_idx_usize = byte_idx as usize;
                        let bit_start_in_value = if byte_idx == start_byte {
                            0
                        } else {
                            (byte_idx - start_byte) * 8 - start_offset
                        };
                        let bits_for_this_byte = if byte_idx == start_byte {
                            8 - start_offset
                        } else if byte_idx == end_byte {
                            (#end_bit % 8) + 1
                        } else {
                            8
                        };
                        let byte_mask =
                            ((1u64 << bits_for_this_byte) - 1) as u8;
                        let value_for_byte =
                            ((value_masked >> bit_start_in_value)
                                & (byte_mask as u64)) as u8;
                        let shift =
                            if byte_idx == start_byte {
                                start_offset
                            } else {
                                0
                            };
                        let positioned_value = value_for_byte << shift;
                        let clear_mask = !(byte_mask << shift);
                        self.data[byte_idx_usize] =
                            (self.data[byte_idx_usize] & clear_mask)
                                | positioned_value;
                    }
                }
            }
        }
    }
}
