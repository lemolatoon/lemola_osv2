use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn;

const FONT_DATA: &str = include_str!("../resources/hankaku.txt");

#[proc_macro]
pub fn gen_font(_input: TokenStream) -> TokenStream {
    match gen_font_impl() {
        Ok(token) => token.into(),
        Err(err) => {
            let syn_error = syn::Error::new(proc_macro2::Span::call_site(), err);
            syn::Error::into_compile_error(syn_error).into()
        }
    }
}
struct Font {
    pub char_code: usize,
    pub bytes: Vec<String>,
}

fn gen_font_impl() -> anyhow::Result<TokenStream2> {
    let mut font_data: Vec<Font> = Vec::new();
    let mut current_char_code = 0;
    let mut current_char_bytes: Vec<String> = Vec::with_capacity(16);
    for (idx, line) in FONT_DATA.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        if idx % 18 == 1 {
            let str_repr = line.split_whitespace().next().unwrap();
            let Ok(char_code) = usize::from_str_radix(&str_repr[2..], 16) else {anyhow::bail!("failed to parse char code in `{}`", line)};
            current_char_code = char_code;
            continue;
        }
        let chars = line.chars();

        let chars: String = chars.map(|c| if c == '.' { '0' } else { '1' }).collect();
        anyhow::ensure!(chars.len() == 8, "invalid font data `{}`", chars);
        current_char_bytes.push(chars);
        if idx % 18 == 17 {
            font_data.push(Font {
                char_code: current_char_code,
                bytes: current_char_bytes.clone(),
            });
            current_char_bytes.clear();
        }
    }

    let mut tokens = TokenStream2::new();
    let empty: TokenStream2 = quote! {
        [0u8; 16],
    };
    let mut font_data_iter = font_data.into_iter().peekable();
    let mut array_len = 0;
    for idx in 0.. {
        array_len = idx;
        if font_data_iter.len() == 0 {
            break;
        }
        if idx != font_data_iter.peek().unwrap().char_code {
            tokens.extend(quote! {
                #empty ,
            });
            continue;
        };
        let font = font_data_iter.next().unwrap();
        let mut bit_token = TokenStream2::new();
        for bits in font.bytes {
            let binary = u8::from_str_radix(&bits, 2).unwrap();
            bit_token.extend(quote! {
                #binary ,
            });
        }
        tokens.extend(quote! {
            [#bit_token] ,
        });
    }

    Ok(quote! {
        const FONT: [[u8; 16]; #array_len] = [
            #tokens
        ];
    })
}

#[test]
fn snapshot() {
    let expanded = gen_font_impl().unwrap();
    insta::assert_display_snapshot!(expanded.to_string());
}
