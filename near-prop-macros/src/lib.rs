use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, punctuated::Punctuated, token::Comma, AttributeArgs, Error, ItemFn,
    NestedMeta, 
};

/// Mark an async function to be fuzz-tested using [near-prop], within a tokio
/// executor.
///
/// # Usage
///
/// ```
/// #[near_prop::test]
/// async fn fuzz_me(_ctx: PropContext, fuzz_arg: String) -> bool {
///     fuzz_arg != "fuzzed".to_owned()
/// }
/// ```
///
/// # Attribute arguments
///
/// Arguments to this attribute are passed through to [tokio::test].
///
/// ```
/// #[near_prop::test(core_threads = 3)]
/// async fn fuzz_me(_ctx: PropContext, fuzz_arg: String) -> bool {
///     fuzz_arg != "fuzzed".to_owned()
/// }
/// ```
/// [near-prop]: https://github.com/austinabell/near-prop
/// [tokio::test]: https://docs.rs/tokio/latest/tokio/attr.test.html
#[proc_macro_attribute]
pub fn test(args: TokenStream, item: TokenStream) -> TokenStream {
    let fn_item = parse_macro_input!(item as ItemFn);

    for attr in &fn_item.attrs {
        if attr.path.is_ident("test") {
            return Error::new_spanned(&fn_item, "multiple #[test] attributes were supplied")
                .to_compile_error()
                .into();
        }
    }

    if fn_item.sig.asyncness.is_none() {
        return Error::new_spanned(&fn_item, "test fn must be async")
            .to_compile_error()
            .into();
    }

    let p_args = parse_macro_input!(args as AttributeArgs);
    let attrib: Punctuated<NestedMeta, Comma> = p_args.into_iter().collect();

    let call_by = format_ident!("{}", fn_item.sig.ident);

    quote! (
        #[::tokio::test(#attrib)]
        async fn #call_by() {
            #fn_item

            ::near_prop::prop_test(#call_by as fn(PropContext, _) -> _).await
        }
    )
    .into()
}
