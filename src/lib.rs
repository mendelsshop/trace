//! A procedural macro for tracing the execution of functions.
//!
//! Adding `#[trace]` to the top of any function will insert `println!` statements at the beginning
//! and the end of that function, notifying you of when that function was entered and exited and
//! printing the argument and return values.  This is useful for quickly debugging whether functions
//! that are supposed to be called are actually called without manually inserting print statements.
//!
//! Note that this macro requires all arguments to the function and the return value to have types
//! that implement `Debug`. You can disable the printing of certain arguments if necessary.
//!
//! You can also add `#[trace]` to `impl`s and `mod`s to enable tracing for all functions in the
//! `impl` or `mod`. If you use `#[trace]` on a `mod` or `impl` as well as on a method or function
//! inside one of those elements, then only the outermost `#[trace]` is used.
//!
//! `#[trace]` takes a few optional arguments that configure things like the prefixes to use,
//! enabling/disabling particular arguments or functions, and more. See the
//! [documentation](macro@trace) for details.
//!
//! ## Example
//!
//! See the examples in `examples/`. You can run the following example with
//! `cargo run --example example_prefix`.
//! ```
//! use trace::trace;
//!
//! trace::init_depth_var!();
//!
//! fn main() {
//!     foo(1, 2);
//! }
//!
//! #[trace]
//! fn foo(a: i32, b: i32) {
//!     println!("I'm in foo!");
//!     bar((a, b));
//! }
//!
//! #[trace(prefix_enter="[ENTER]", prefix_exit="[EXIT]")]
//! fn bar((a, b): (i32, i32)) -> i32 {
//!     println!("I'm in bar!");
//!     if a == 1 {
//!         2
//!     } else {
//!         b
//!     }
//! }
//! ```
//!
//! Output:
//! ```text
//! [+] Entering foo(a = 1, b = 2)
//! I'm in foo!
//!  [ENTER] Entering bar(a = 1, b = 2)
//! I'm in bar!
//!  [EXIT] Exiting bar = 2
//! [-] Exiting foo = ()
//! ```
//!
//! Note the convenience [`trace::init_depth_var!()`](macro@init_depth_var) macro which declares and
//! initializes the thread-local `DEPTH` variable that is used for indenting the output. Calling
//! `trace::init_depth_var!()` is equivalent to writing:
//! ```
//! use std::cell::Cell;
//!
//! thread_local! {
//!     static DEPTH: Cell<usize> = Cell::new(0);
//! }
//! ```
//!
//! The only time it can be omitted is when `#[trace]` is applied to `mod`s as it's defined for you
//! automatically (see `examples/example_mod.rs`). Note that the `DEPTH` variable isn't shared
//! between `mod`s, so indentation won't be perfect when tracing functions in multiple `mod`s. Also
//! note that using trace as an inner attribute (`#![trace]`) is not supported at this time.

mod args;

use std::{iter::Peekable, str::Chars};

use proc_macro2::{Span, TokenStream};
use quote::{quote, ToTokens};
use syn::{
    parse::{Parse, Parser},
    parse_quote,
};

/// A convenience macro for declaring the `DEPTH` variable used for indenting the output
///
/// Calling this macro is equivalent to:
/// ```
/// use std::cell::Cell;
///
/// thread_local! {
///     static DEPTH: Cell<usize> = Cell::new(0);
/// }
/// ```
///
/// It is required to declare a `DEPTH` variable unless using `#[trace]` on a `mod`, in which case
/// the variable is declared for you.
#[proc_macro]
pub fn init_depth_var(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let output = if input.is_empty() {
        quote! {
            ::std::thread_local! {
                static DEPTH: ::std::cell::Cell<usize> = ::std::cell::Cell::new(0);
            }
        }
    } else {
        let input2 = proc_macro2::TokenStream::from(input);
        syn::Error::new_spanned(input2, "`init_depth_var` takes no arguments").to_compile_error()
    };

    output.into()
}

/// Enables tracing the execution of functions
///
/// It supports the following optional arguments (see the `examples` folder for examples of using
/// each of these):
///
/// - `prefix_enter` - The prefix of the `println!` statement when a function is entered. Defaults
/// to `[+]`.
///
/// - `prefix_exit` - The prefix of the `println!` statement when a function is exited. Defaults to
/// `[-]`.
///
/// - `enable` - When applied to a `mod` or `impl`, `enable` takes a list of function names to
/// print, not printing any functions that are not part of this list. All functions are enabled by
/// default. When applied to an `impl` method or a function, `enable` takes a list of arguments to
/// print, not printing any arguments that are not part of the list. All arguments are enabled by
/// default.
///
/// - `disable` - When applied to a `mod` or `impl`, `disable` takes a list of function names to not
/// print, printing all other functions in the `mod` or `impl`. No functions are disabled by
/// default. When applied to an `impl` method or a function, `disable` takes a list of arguments to
/// not print, printing all other arguments. No arguments are disabled by default.
///
/// - `pause` - When given as an argument to `#[trace]`, execution is paused after each line of
/// tracing output until enter is pressed. This allows you to trace through a program step by
/// step. Disabled by default.
///
/// - `pretty` - Pretty print the output (use `{:#?}` instead of `{:?}`). Disabled by default.
///
/// - `logging` - Use `log::trace!` from the `log` crate instead of `println`. Disabled by default.
///
/// - `format_enter` - The format (anything after the prefix) of `println!` statements when a function
/// is entered. Allows parameter interpolation like:
/// ```rust
/// #[trace(format_enter = "i is {i}")]
/// fn foo(i: i32) {
///     println!("foo")
/// }
/// ```
/// Interpolation follows the same rules as `format!()` besides for the fact that there is no pretty printing,
/// that is anything interpolated will be debug formatted. Disabled by default.

/// - `format_exit` - The format (anything after the prefix) of `println!` statements when a function
/// is exited. To interpolate the return value use `{r}`:
/// ```rust
/// #[trace(format_exit = "returning {r}")]
/// fn foo() -> i32 {
///     1
/// }
/// ```
/// Otherwise formatting follows the same rules as `format_enter`. Disabled by default.
///
/// Note that `enable` and `disable` cannot be used together, and doing so will result in an error.
///
/// Further note that `format_enter` or `format_exit` cannot be used together with with `pretty`, and doing so will result in an error.
#[proc_macro_attribute]
pub fn trace(
    args: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let raw_args = syn::parse_macro_input!(args as syn::AttributeArgs);
    let args = match args::Args::from_raw_args(raw_args) {
        Ok(args) => args,
        Err(errors) => {
            return errors
                .iter()
                .map(syn::Error::to_compile_error)
                .collect::<proc_macro2::TokenStream>()
                .into()
        }
    };

    let output = if let Ok(item) = syn::Item::parse.parse(input.clone()) {
        expand_item(&args, item)
    } else if let Ok(impl_item) = syn::ImplItem::parse.parse(input.clone()) {
        expand_impl_item(&args, impl_item)
    } else {
        let input2 = proc_macro2::TokenStream::from(input);
        syn::Error::new_spanned(input2, "expected one of: `fn`, `impl`, `mod`").to_compile_error()
    };

    output.into()
}

#[derive(Clone, Copy)]
enum AttrApplied {
    Directly,
    Indirectly,
}

fn expand_item(args: &args::Args, mut item: syn::Item) -> proc_macro2::TokenStream {
    transform_item(args, AttrApplied::Directly, &mut item);

    match item {
        syn::Item::Fn(_) | syn::Item::Mod(_) | syn::Item::Impl(_) => item.into_token_stream(),
        _ => syn::Error::new_spanned(item, "#[trace] is not supported for this item")
            .to_compile_error(),
    }
}

fn expand_impl_item(args: &args::Args, mut impl_item: syn::ImplItem) -> proc_macro2::TokenStream {
    transform_impl_item(args, AttrApplied::Directly, &mut impl_item);

    match impl_item {
        syn::ImplItem::Method(_) => impl_item.into_token_stream(),
        _ => syn::Error::new_spanned(impl_item, "#[trace] is not supported for this impl item")
            .to_compile_error(),
    }
}

fn transform_item(args: &args::Args, attr_applied: AttrApplied, item: &mut syn::Item) {
    match *item {
        syn::Item::Fn(ref mut item_fn) => transform_fn(args, attr_applied, item_fn),
        syn::Item::Mod(ref mut item_mod) => transform_mod(args, attr_applied, item_mod),
        syn::Item::Impl(ref mut item_impl) => transform_impl(args, attr_applied, item_impl),
        _ => (),
    }
}

fn transform_fn(args: &args::Args, attr_applied: AttrApplied, item_fn: &mut syn::ItemFn) {
    item_fn.block = Box::new(construct_traced_block(
        args,
        attr_applied,
        &item_fn.sig,
        &item_fn.block,
    ));
}

fn transform_mod(args: &args::Args, attr_applied: AttrApplied, item_mod: &mut syn::ItemMod) {
    assert!(
        (item_mod.content.is_some() && item_mod.semi.is_none())
            || (item_mod.content.is_none() && item_mod.semi.is_some())
    );

    if item_mod.semi.is_some() {
        unimplemented!();
    }

    if let Some((_, items)) = item_mod.content.as_mut() {
        items.iter_mut().for_each(|item| {
            if let AttrApplied::Directly = attr_applied {
                match *item {
                    syn::Item::Fn(syn::ItemFn {
                        sig: syn::Signature { ref ident, .. },
                        ..
                    })
                    | syn::Item::Mod(syn::ItemMod { ref ident, .. }) => match args.filter {
                        args::Filter::Enable(ref idents) if !idents.contains(ident) => {
                            return;
                        }
                        args::Filter::Disable(ref idents) if idents.contains(ident) => {
                            return;
                        }
                        _ => (),
                    },
                    _ => (),
                }
            }

            transform_item(args, AttrApplied::Indirectly, item);
        });

        items.insert(
            0,
            parse_quote! {
                ::std::thread_local! {
                    static DEPTH: ::std::cell::Cell<usize> = ::std::cell::Cell::new(0);
                }
            },
        );
    }
}

fn transform_impl(args: &args::Args, attr_applied: AttrApplied, item_impl: &mut syn::ItemImpl) {
    item_impl.items.iter_mut().for_each(|impl_item| {
        if let syn::ImplItem::Method(ref mut impl_item_method) = *impl_item {
            if let AttrApplied::Directly = attr_applied {
                let ident = &impl_item_method.sig.ident;

                match args.filter {
                    args::Filter::Enable(ref idents) if !idents.contains(ident) => {
                        return;
                    }
                    args::Filter::Disable(ref idents) if idents.contains(ident) => {
                        return;
                    }
                    _ => (),
                }
            }

            impl_item_method.block = construct_traced_block(
                args,
                AttrApplied::Indirectly,
                &impl_item_method.sig,
                &impl_item_method.block,
            );
        }
    });
}

fn transform_impl_item(
    args: &args::Args,
    attr_applied: AttrApplied,
    impl_item: &mut syn::ImplItem,
) {
    // Will probably add more cases in the future
    #[allow(clippy::single_match)]
    match *impl_item {
        syn::ImplItem::Method(ref mut impl_item_method) => {
            transform_method(args, attr_applied, impl_item_method)
        }
        _ => (),
    }
}

fn transform_method(
    args: &args::Args,
    attr_applied: AttrApplied,
    impl_item_method: &mut syn::ImplItemMethod,
) {
    impl_item_method.block = construct_traced_block(
        args,
        attr_applied,
        &impl_item_method.sig,
        &impl_item_method.block,
    );
}

fn construct_traced_block(
    args: &args::Args,
    attr_applied: AttrApplied,
    sig: &syn::Signature,
    original_block: &syn::Block,
) -> syn::Block {
    let arg_idents = extract_arg_idents(args, attr_applied, sig)
        .iter()
        .map(|ident| ident.to_token_stream())
        .collect();
    let (enter_format, arg_idents) = if let Some(fmt_str) = &args.format_enter {
        parse_fmt_str(fmt_str, arg_idents)
    } else {
        (
            Ok(arg_idents
                .iter()
                .map(|arg_ident| format!("{} = {{:?}}", arg_ident))
                .collect::<Vec<_>>()
                .join(", ")),
            arg_idents,
        )
    };
    // we set set exit val to be a vector with one element which is Ident called r
    // this means that the format parser can indentify when then return value should be interprolated
    // so if we want to use a different symbol to denote return value interpolation we just need to change the symbol in the following quote
    // ie: `let exit_val = vec![quote!(return_value)];` if we wanted to use return_value to denote return value interpolation
    let exit_val = vec![quote!(r)];
    let (exit_format, exit_val) = if let Some(fmt_str) = &args.format_exit {
        parse_fmt_str(fmt_str, exit_val)
    } else if args.pretty {
        (Ok("{:#?}".to_string()), exit_val)
    } else {
        (Ok("{:?}".to_string()), exit_val)
    };
    let should_interpolate = !exit_val.is_empty();
    let entering_format = format!(
        "{{:depth$}}{} Entering {}({})",
        args.prefix_enter,
        sig.ident,
        match enter_format {
            Ok(ok) => ok,
            Err(e) => {
                let error = e.into_compile_error();
                return parse_quote! {{#error}};
            }
        }
    );
    let exiting_format = format!(
        "{{:depth$}}{} Exiting {} = {}",
        args.prefix_exit,
        sig.ident,
        match exit_format {
            Ok(ok) => ok,
            Err(e) => {
                let error = e.into_compile_error();
                return parse_quote! {{#error}};
            }
        }
    );

    let pause_stmt = if args.pause {
        quote! {{
            use std::io::{self, BufRead};
            let stdin = io::stdin();
            stdin.lock().lines().next();
        }}
    } else {
        quote!()
    };

    let printer = if args.logging {
        quote! { log::trace! }
    } else {
        quote! { println! }
    };
    let print_exit = if should_interpolate {
        quote! {{#printer(#exiting_format, "",fn_return_value, depth = DEPTH.with(|d| d.get()));}}
    } else {
        quote!(#printer(#exiting_format, "", depth = DEPTH.with(|d| d.get()));)
    };
    parse_quote! {{
        #printer(#entering_format, "", #(#arg_idents,)* depth = DEPTH.with(|d| d.get()));
        #pause_stmt
        DEPTH.with(|d| d.set(d.get() + 1));
        let fn_return_value = #original_block;
        DEPTH.with(|d| d.set(d.get() - 1));
        #print_exit
        #pause_stmt
        fn_return_value
    }}
}
// how interpolation parsing works:
// we get a format string, we scan until we find a {,
// once we find a { we check if we find another { right after for just escaping the interpolation
// if its just a single {, we scan until we find the closing }, then
// we see if there is any custom formatting options like :? or whatever, and we verify that the
// ident is bound by the parameters to the function.
// a side note: the way we format is with indexes ie: format("{0} {1}", foo, bar)
// too facilitate this we keep to list the arg_idents and the keep_arg_idents
// arg_idents represent what is initially the list of parameters
// keep_arg_idents represent the parameters that are actually going to shown, this is how we
// maintain the order for formatting
// if it is there are two cases:
// 1. This parameter is not part of the format string (its in arg_idents)
//    so we add it to keep_arg_idents, and remove it from arg_idents
//    we put as the index for this part of the interpolation the length of keep_arg_idents before
//    adding
// 2. It's already in keep_arg_idents its already been interpolated once
//    so we just put as the index the index of the ident from keep_arg_idents
// if there is any custom formatting information we put that right after the index in the
// interpolation
// otherwise if we are not in interpolation we didn't find a { we just add the char to the string
// we are outputting
fn parse_fmt_str(
    fmt_str: &str,
    mut arg_idents: Vec<TokenStream>,
) -> (Result<String, syn::Error>, Vec<TokenStream>) {
    let mut fixed_format_str = String::new();
    let mut kept_arg_idents = Vec::new();
    let mut fmt_iter = fmt_str.chars().peekable();
    while let Some(fmt_char) = fmt_iter.next() {
        match fmt_char {
            '{' => {
                if let Some('{') = fmt_iter.peek() {
                    fixed_format_str.push_str("{{");
                    fmt_iter.next();
                } else {
                    match parse_interpolated(&mut fmt_iter, &mut arg_idents, &mut kept_arg_idents) {
                        Ok(interpolated) => fixed_format_str.push_str(&interpolated),
                        Err(e) => return (Err(e), kept_arg_idents),
                    }
                }
            }
            '}' => {
                if fmt_iter.next() != Some('}') {
                    return (Err(syn::Error::new(
                            Span::call_site(),
                            "invalid format string: unmatched `}` found\nif you intended to print `}`, you can escape it using `}}`"
                        )), kept_arg_idents);
                }

                fixed_format_str.push_str("}}")
            }
            _ => fixed_format_str.push(fmt_char),
        }
    }
    (Ok(fixed_format_str), kept_arg_idents)
}

fn fix_interpolated(
    last_char: char,
    ident: String,
    arg_idents: &mut Vec<TokenStream>,
    kept_arg_idents: &mut Vec<TokenStream>,
) -> Result<String, syn::Error> {
    if last_char != '}' {
        return Err(syn::Error::new(
            Span::call_site(),
            "invalid format string: expected `'}}'` but string was terminated\nif you intended to print `{{`, you can escape it using `{{`.",
        ));
    }
    // just parsing to colon means we are relying on the format! macro to do the actual custom
    // formatting stuff
    let custom_format = ident.split_once(":");
    let (ident, custom_format) = custom_format.unwrap_or((&ident, ""));
    let predicate = |arg_ident: &TokenStream| arg_ident.to_string() == ident;

    // we always put colon even if there is not custom format string, because we do not have to do
    // any actual checking for custom format string after splitting on the format string
    // because format! does allow for dangling colon when there is not format string
    if let Some(index) = kept_arg_idents.iter().position(predicate) {
        Ok(format!("{{{}:{}}}", index + 1, custom_format))
    } else if let Some(index) = arg_idents.iter().position(predicate) {
        kept_arg_idents.push(arg_idents.remove(index));
        Ok(format!("{{{}:{}}}", kept_arg_idents.len(), custom_format))
    } else {
        Err(syn::Error::new(
            Span::call_site(),
            // TODO: better error message
            format!("cannot find `{ident}` in this scope."),
        ))
    }
}

fn parse_interpolated(
    fmt_iter: &mut Peekable<Chars>,
    arg_idents: &mut Vec<TokenStream>,
    kept_arg_idents: &mut Vec<TokenStream>,
) -> Result<String, syn::Error> {
    let mut last_char = ' ';
    let mut ident = String::new();
    while let Some(ident_char) = fmt_iter.next() {
        match ident_char {
            '}' => {
                last_char = '}';
                break;
            }
            _ => {
                last_char = ident_char;
                if !ident_char.is_whitespace() {
                    ident.push(ident_char);
                } else {
                    skip_whitespace_and_check(fmt_iter, &mut last_char, ident_char)?;
                }
            }
        }
    }
    // we do not actually verify that ident is a valid rust ident, because
    // inf fix_interpolated we will check that has the same string representation as one of the
    // functions parameters, but if we did this is how we would do it
    // syn::parse_str::<syn::Ident>(&ident)?;
    fix_interpolated(last_char, ident, arg_idents, kept_arg_idents)
}

fn skip_whitespace_and_check(
    fmt_iter: &mut Peekable<Chars>,
    last_char: &mut char,
    ident_char: char,
) -> Result<(), syn::Error> {
    for blank_char in fmt_iter.by_ref() {
        match blank_char {
            '}' => {
                *last_char = '}';
                break;
            }
            c if c.is_whitespace() => {
                *last_char = ident_char;
            }
            _ => {
                return Err(syn::Error::new(
                    Span::call_site(),
                    format!("invalid format string: expected `'}}'`, found `'{blank_char}'`\nif you intended to print `{{`, you can escape it using `{{`."),
                ))
            }
        }
    }
    Ok(())
}

fn extract_arg_idents(
    args: &args::Args,
    attr_applied: AttrApplied,
    sig: &syn::Signature,
) -> Vec<proc_macro2::Ident> {
    fn process_pat(
        args: &args::Args,
        attr_applied: AttrApplied,
        pat: &syn::Pat,
        arg_idents: &mut Vec<proc_macro2::Ident>,
    ) {
        match *pat {
            syn::Pat::Ident(ref pat_ident) => {
                let ident = &pat_ident.ident;

                if let AttrApplied::Directly = attr_applied {
                    match args.filter {
                        args::Filter::Enable(ref idents) if !idents.contains(ident) => {
                            return;
                        }
                        args::Filter::Disable(ref idents) if idents.contains(ident) => {
                            return;
                        }
                        _ => (),
                    }
                }

                arg_idents.push(ident.clone());
            }
            syn::Pat::Tuple(ref pat_tuple) => {
                pat_tuple.elems.iter().for_each(|pat| {
                    process_pat(args, attr_applied, pat, arg_idents);
                });
            }
            _ => unimplemented!(),
        }
    }

    let mut arg_idents = vec![];

    for input in &sig.inputs {
        match input {
            syn::FnArg::Receiver(_) => (), // ignore `self`
            syn::FnArg::Typed(arg_typed) => {
                process_pat(args, attr_applied, &arg_typed.pat, &mut arg_idents);
            }
        }
    }

    arg_idents
}
