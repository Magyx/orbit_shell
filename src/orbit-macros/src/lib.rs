use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

#[proc_macro_attribute]
pub fn orbit_config(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);

    let expanded = quote! {
        #[derive(Debug, Clone)]
        #[derive(orbit_api::serde::Serialize, orbit_api::serde::Deserialize, )]
        #[serde(crate = "orbit_api::serde", rename_all = "snake_case")]
        #[derive(orbit_api::schemars::JsonSchema)]
        #[schemars(crate = "orbit_api::schemars")]
        #input
    };

    TokenStream::from(expanded)
}
