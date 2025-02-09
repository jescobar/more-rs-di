extern crate proc_macro;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    spanned::Spanned,
    *,
};

struct ArgContext<'a> {
    type_: &'a TypePath,
    optional: bool,
    many: bool,
    lazy: bool,
}

impl<'a> ArgContext<'a> {
    fn new(type_: &'a TypePath, optional: bool, many: bool, lazy: bool) -> Self {
        Self {
            type_,
            optional,
            many,
            lazy,
        }
    }

    fn optional_of_many(&self) -> bool {
        self.optional && self.many
    }
}

struct InjectableAttribute {
    trait_: Option<Path>,
}

impl Parse for InjectableAttribute {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(Self {
            trait_: input.parse().ok(),
        })
    }
}

/// Represents the metadata used to identify an injected function.
///
/// # Remarks
///
/// The default behavior looks for an associated function with the
/// name `new`. To change this behavior, decorate the function to
/// be used with `#[inject]`. This attribute may only be applied
/// to a single function.
#[proc_macro_attribute]
pub fn inject(
    _metadata: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    // this attribute is intentionally inert
    input
}

/// Represents the metadata used to implement the `Injectable` trait.
///
/// # Arguments
///
/// * `trait` - the optional name of the trait the implementation satisfies.
///
/// # Remarks
///
/// This attribute must be applied to the `impl` of a struct. The
/// defining struct implementation must either have an associated
/// function named `new` or decorate the injected function with
/// `#[inject]`. The injected function does not have to be public.
///
/// If `trait` is not specified, then the implementation will
/// injectable as the defining struct itself.
///
/// The injected call site arguments are restricted to the same return
/// values supported by `ServiceProvider`, which can only be:
///
/// * `ServiceRef<T>`
/// * `Option<ServiceRef<T>>`
/// * `Vec<ServiceRef<T>>`
/// * `ServiceProvider`
///
/// `ServiceRef<T>` is a type alias for `Rc<T>` or `Arc<T>` depending
/// on whether the **async** feature is activated; therefore, `Rc<T>`
/// and `Arc<T>` are also allowed any place `ServiceRef<T>` is allowed.
///
/// # Examples
///
/// Injecting a struct as a trait.
///
/// ```
/// pub trait Foo {
///    fn do_work(&self);
/// }
///
/// pub struct FooImpl;
///
/// impl Foo for FooImpl {
///     fn do_work(&self) {
///         println!("Did something!");
///     }
/// }
///
/// #[injectable(Foo)]
/// impl FooImpl {
///     pub fn new() -> Self {
///         Self {}
///     }
/// }
/// ```
///
/// Injecting a struct as itself.
///
/// ```
/// pub struct Foo;
///
/// #[injectable]
/// impl Foo {
///     pub fn new() -> Self {
///         Self {}
///     }
///
///     fn do_work(&self) {
///         println!("Did something!");
///     }
/// }
/// ```
///
/// Define a custom injection function.
///
/// ```
/// pub struct Bar;
/// pub struct Foo {
///     bar: ServiceRef<Bar>
/// };
///
/// #[injectable]
/// impl Foo {
///     #[inject]
///     pub fn create(bar: ServiceRef<Bar>) -> Self {
///         Self { bar }
///     }
/// }
#[proc_macro_attribute]
pub fn injectable(
    metadata: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    proc_macro::TokenStream::from(_injectable(
        TokenStream::from(metadata),
        TokenStream::from(input),
    ))
}

fn _injectable(metadata: TokenStream, input: TokenStream) -> TokenStream {
    let mut original = TokenStream::from(input.clone());
    let result = match parse2::<InjectableAttribute>(metadata) {
        Ok(attribute) => {
            if let Ok(impl_) = parse2::<ItemImpl>(TokenStream::from(input)) {
                if let Type::Path(type_) = &*impl_.self_ty {
                    let implementation = &type_.path;
                    let service = attribute.trait_.as_ref().unwrap_or(implementation);

                    match get_injected_method(&impl_, implementation) {
                        Ok(method) => {
                            match implement_injectable(&impl_, implementation, &service, method) {
                                Ok(trait_impl) => {
                                    original.extend(trait_impl.into_iter());
                                    Ok(original)
                                }
                                Err(error) => Err(error),
                            }
                        }
                        Err(error) => Err(error),
                    }
                } else {
                    Err(Error::new(impl_.span(), "Expected implementation type."))
                }
            } else {
                Err(Error::new(
                    original.span(),
                    "Attribute can only be applied to a structure implementation block.",
                ))
            }
        }
        Err(error) => Err(error),
    };

    match result {
        Ok(output) => output,
        Err(error) => error.to_compile_error().into(),
    }
}

fn implement_injectable(
    impl_: &ItemImpl,
    implementation: &Path,
    service: &Path,
    method: &Signature,
) -> Result<TokenStream> {
    let (args, deps) = inject_argument_call_sites(method)?;
    let fn_ = &method.ident;
    let is_trait =
        implementation.segments.last().unwrap().ident != service.segments.last().unwrap().ident;
    let new = if is_trait {
        quote! { di::ServiceDescriptorBuilder::<dyn #service, Self>::new(lifetime, di::Type::of::<Self>()) }
    } else {
        quote! { di::ServiceDescriptorBuilder::<Self, Self>::new(lifetime, di::Type::of::<Self>()) }
    };
    let depends_on = quote! { #(.depends_on(#deps))* };
    let generics = &impl_.generics;
    let where_ = &generics.where_clause;
    let code = quote! {
        impl#generics di::Injectable for #implementation #where_ {
            fn inject(lifetime: di::ServiceLifetime) -> di::ServiceDescriptor {
                #new#depends_on.from(|sp: &di::ServiceProvider| di::ServiceRef::new(Self::#fn_(#(#args),*)))
            }
        }
    };
    Ok(code.into())
}

fn get_injected_method<'a>(impl_: &'a ItemImpl, path: &Path) -> Result<&'a Signature> {
    let new = Ident::new("new", Span::call_site());
    let mut convention = Option::None;
    let mut methods = Vec::new();

    for item in &impl_.items {
        if let ImplItem::Method(method) = item {
            let signature = &method.sig;

            if method.attrs.iter().any(|a| a.path.is_ident("inject")) {
                methods.push(signature);
            }

            if signature.ident == new {
                convention = Some(signature);
            }
        }
    }

    match methods.len() {
        0 => {
            if let Some(method) = convention {
                Ok(method)
            } else {
                Err(Error::new(
                    impl_.span(),
                    format!(
                        "Neither {}::new or an associated method decorated with #[inject] was found.",
                        path.segments.last().unwrap().ident
                    ),
                ))
            }
        }
        1 => Ok(methods[0]),
        _ => Err(Error::new(
            impl_.span(),
            format!(
                "{} has more than one associated method decorated with #[inject].",
                path.segments.last().unwrap().ident
            ),
        )),
    }
}

fn inject_argument_call_sites(method: &Signature) -> Result<(Vec<TokenStream>, Vec<TokenStream>)> {
    let count = method.inputs.len();

    if count == 0 {
        return Ok((Vec::with_capacity(0), Vec::with_capacity(0)));
    }

    let mut args = Vec::with_capacity(count);
    let mut deps = Vec::with_capacity(count);

    for input in method.inputs.iter() {
        let (arg, dep) = match input {
            FnArg::Typed(type_) => resolve_type(&*type_.ty)?,
            _ => return Err(Error::new(
                input.span(),
                "The argument must be ServiceRef, Rc, or Arc and optionally wrapped with Option or Vec.")),
        };

        args.push(arg);

        if let Some(d) = dep {
            deps.push(d);
        }
    }

    Ok((args, deps))
}

fn new_arg_context(arg: &Type) -> Result<ArgContext<'_>> {
    if let Type::Path(outer) = arg {
        let (type_, lazy) = if let Some(inner) = get_generic_type_arg(outer, "Lazy") {
            match inner {
                Type::Path(path) => (path, true),
                _ => (outer, false),
            }
        } else {
            (outer, false)
        };

        if let Some(inner) = get_generic_type_arg(type_, "Option") {
            if let Type::Path(path) = inner {
                Ok(ArgContext::new(path, true, false, lazy))
            } else {
                Err(Error::new(inner.span(), "Expected ServiceRef, Rc, or Arc."))
            }
        } else if let Some(inner) = get_generic_type_arg(type_, "Vec") {
            if let Type::Path(path) = inner {
                Ok(ArgContext::new(path, false, true, lazy))
            } else {
                Err(Error::new(inner.span(), "Expected ServiceRef, Rc, or Arc."))
            }
        } else {
            Ok(ArgContext::new(type_, false, false, lazy))
        }
    } else {
        Err(Error::new(arg.span(), "Expected type path."))
    }
}

fn resolve_trait_type(
    trait_: &TypeTraitObject,
    context: &ArgContext,
) -> (TokenStream, Option<TokenStream>) {
    if context.optional {
        (
            if context.lazy {
                quote! { di::lazy::zero_or_one::<#trait_>(sp.clone()) }
            } else {
                quote! { sp.get::<#trait_>() }
            },
            Some(
                quote! { di::ServiceDependency::new(di::Type::of::<#trait_>(), di::ServiceCardinality::ZeroOrOne) },
            ),
        )
    } else if context.many {
        (
            if context.lazy {
                quote! { di::lazy::zero_or_more::<#trait_>(sp.clone()) }
            } else {
                quote! { sp.get_all::<#trait_>().collect() }
            },
            Some(
                quote! { di::ServiceDependency::new(di::Type::of::<#trait_>(), di::ServiceCardinality::ZeroOrMore) },
            ),
        )
    } else {
        (
            if context.lazy {
                quote! { di::lazy::exactly_one::<#trait_>(sp.clone()) }
            } else {
                quote! { sp.get_required::<#trait_>() }
            },
            Some(
                quote! { di::ServiceDependency::new(di::Type::of::<#trait_>(), di::ServiceCardinality::ExactlyOne) },
            ),
        )
    }
}

fn resolve_struct_type(
    struct_: &TypePath,
    context: &ArgContext,
) -> (TokenStream, Option<TokenStream>) {
    if context.optional {
        (
            if context.lazy {
                quote! { di::lazy::zero_or_one::<#struct_>(sp.clone()) }
            } else {
                quote! { sp.get::<#struct_>() }
            },
            Some(
                quote! { di::ServiceDependency::new(di::Type::of::<#struct_>(), di::ServiceCardinality::ZeroOrOne) },
            ),
        )
    } else if context.many {
        (
            if context.lazy {
                quote! { di::lazy::zero_or_more::<#struct_>(sp.clone()) }
            } else {
                quote! { sp.get_all::<#struct_>().collect() }
            },
            Some(
                quote! { di::ServiceDependency::new(di::Type::of::<#struct_>(), di::ServiceCardinality::ZeroOrMore) },
            ),
        )
    } else {
        (
            if context.lazy {
                quote! { di::lazy::exactly_one::<#struct_>(sp.clone()) }
            } else {
                quote! { sp.get_required::<#struct_>() }
            },
            Some(
                quote! { di::ServiceDependency::new(di::Type::of::<#struct_>(), di::ServiceCardinality::ExactlyOne) },
            ),
        )
    }
}

fn resolve_type(arg: &Type) -> Result<(TokenStream, Option<TokenStream>)> {
    let context = new_arg_context(arg)?;

    if let Some(inner_type) = get_generic_type_arg(context.type_, "ServiceRef")
        .or(get_generic_type_arg(context.type_, "Rc"))
        .or(get_generic_type_arg(context.type_, "Arc"))
    {
        if context.optional_of_many() {
            return Err(Error::new(
                arg.span(),
                "Option<Vec> is not supported. Did you mean Vec?",
            ));
        }

        match inner_type {
            Type::TraitObject(trait_) => Ok(resolve_trait_type(trait_, &context)),
            Type::Path(struct_) => Ok(resolve_struct_type(struct_, &context)),
            _ => Err(Error::new(inner_type.span(), "Expected a trait or struct.")),
        }
    } else if context.type_.path.segments.first().unwrap().ident
        == Ident::new("ServiceProvider", Span::call_site())
    {
        Ok((quote! { sp.clone() }, None))
    } else {
        Err(Error::new(
            context.type_.span(),
            "Expected ServiceRef, Rc, or Arc.",
        ))
    }
}

fn get_generic_type_arg<'a>(type_: &'a TypePath, name: &str) -> Option<&'a Type> {
    let path = &type_.path;
    let segment = path.segments.first().unwrap();

    if segment.ident == Ident::new(name, Span::call_site()) {
        if let PathArguments::AngleBracketed(ref type_args) = segment.arguments {
            for type_arg in type_args.args.iter() {
                if let GenericArgument::Type(ref inner_type) = type_arg {
                    return Some(inner_type);
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod test {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn attribute_should_implement_injectable_by_convention() {
        // arrange
        let metadata = TokenStream::from_str(r#"Foo"#).unwrap();
        let input = TokenStream::from_str(
            r#"
            impl FooImpl {
                fn new() -> Self {
                    Self { }
                }
            }
        "#,
        )
        .unwrap();

        // act
        let result = _injectable(metadata, input);

        // assert
        let expected = concat!(
            "impl FooImpl { ",
            "fn new () -> Self { ",
            "Self { } ",
            "} ",
            "} ",
            "impl di :: Injectable for FooImpl { ",
            "fn inject (lifetime : di :: ServiceLifetime) -> di :: ServiceDescriptor { ",
            "di :: ServiceDescriptorBuilder :: < dyn Foo , Self > :: new (lifetime , di :: Type :: of :: < Self > ()) ",
            ". from (| sp : & di :: ServiceProvider | di :: ServiceRef :: new (Self :: new ())) ",
            "} ",
            "}");

        assert_eq!(expected, result.to_string());
    }

    #[test]
    fn attribute_should_implement_injectable_using_decorated_method() {
        // arrange
        let metadata = TokenStream::from_str(r#"Foo"#).unwrap();
        let input = TokenStream::from_str(
            r#"
            impl FooImpl {
                #[inject]
                fn create() -> Self {
                    Self { }
                }
            }
        "#,
        )
        .unwrap();

        // act
        let result = _injectable(metadata, input);

        // assert
        let expected = concat!(
            "impl FooImpl { ",
            "# [inject] ",
            "fn create () -> Self { ",
            "Self { } ",
            "} ",
            "} ",
            "impl di :: Injectable for FooImpl { ",
            "fn inject (lifetime : di :: ServiceLifetime) -> di :: ServiceDescriptor { ",
            "di :: ServiceDescriptorBuilder :: < dyn Foo , Self > :: new (lifetime , di :: Type :: of :: < Self > ()) ",
            ". from (| sp : & di :: ServiceProvider | di :: ServiceRef :: new (Self :: create ())) ",
            "} ",
            "}");

        assert_eq!(expected, result.to_string());
    }

    #[test]
    fn attribute_should_inject_required_dependency() {
        // arrange
        let metadata = TokenStream::from_str(r#"Foo"#).unwrap();
        let input = TokenStream::from_str(
            r#"
            impl FooImpl {
                fn new(_bar: Rc<dyn Bar>) -> Self {
                    Self { }
                }
            }
        "#,
        )
        .unwrap();

        // act
        let result = _injectable(metadata, input);

        // assert
        let expected = concat!(
            "impl FooImpl { ",
            "fn new (_bar : Rc < dyn Bar >) -> Self { ",
            "Self { } ",
            "} ",
            "} ",
            "impl di :: Injectable for FooImpl { ",
            "fn inject (lifetime : di :: ServiceLifetime) -> di :: ServiceDescriptor { ",
            "di :: ServiceDescriptorBuilder :: < dyn Foo , Self > :: new (lifetime , di :: Type :: of :: < Self > ()) ",
            ". depends_on (di :: ServiceDependency :: new (di :: Type :: of :: < dyn Bar > () , di :: ServiceCardinality :: ExactlyOne)) ",
            ". from (| sp : & di :: ServiceProvider | di :: ServiceRef :: new (Self :: new (sp . get_required :: < dyn Bar > ()))) ",
            "} ",
            "}");

        assert_eq!(expected, result.to_string());
    }

    #[test]
    fn attribute_should_inject_optional_dependency() {
        // arrange
        let metadata = TokenStream::from_str(r#"Foo"#).unwrap();
        let input = TokenStream::from_str(
            r#"
            impl FooImpl {
                fn new(_bar: Option<Rc<dyn Bar>>) -> Self {
                    Self { }
                }
            }
        "#,
        )
        .unwrap();

        // act
        let result = _injectable(metadata, input);

        // assert
        let expected = concat!(
            "impl FooImpl { ",
            "fn new (_bar : Option < Rc < dyn Bar >>) -> Self { ",
            "Self { } ",
            "} ",
            "} ",
            "impl di :: Injectable for FooImpl { ",
            "fn inject (lifetime : di :: ServiceLifetime) -> di :: ServiceDescriptor { ",
            "di :: ServiceDescriptorBuilder :: < dyn Foo , Self > :: new (lifetime , di :: Type :: of :: < Self > ()) ",
            ". depends_on (di :: ServiceDependency :: new (di :: Type :: of :: < dyn Bar > () , di :: ServiceCardinality :: ZeroOrOne)) ",
            ". from (| sp : & di :: ServiceProvider | di :: ServiceRef :: new (Self :: new (sp . get :: < dyn Bar > ()))) ",
            "} ",
            "}");

        assert_eq!(expected, result.to_string());
    }

    #[test]
    fn attribute_should_inject_dependency_collection() {
        // arrange
        let metadata = TokenStream::from_str(r#"Foo"#).unwrap();
        let input = TokenStream::from_str(
            r#"
            impl FooImpl {
                fn new(_bars: Vec<Rc<dyn Bar>>) -> Self {
                    Self { }
                }
            }
        "#,
        )
        .unwrap();

        // act
        let result = _injectable(metadata, input);

        // assert
        let expected = concat!(
            "impl FooImpl { ",
            "fn new (_bars : Vec < Rc < dyn Bar >>) -> Self { ",
            "Self { } ",
            "} ",
            "} ",
            "impl di :: Injectable for FooImpl { ",
            "fn inject (lifetime : di :: ServiceLifetime) -> di :: ServiceDescriptor { ",
            "di :: ServiceDescriptorBuilder :: < dyn Foo , Self > :: new (lifetime , di :: Type :: of :: < Self > ()) ",
            ". depends_on (di :: ServiceDependency :: new (di :: Type :: of :: < dyn Bar > () , di :: ServiceCardinality :: ZeroOrMore)) ",
            ". from (| sp : & di :: ServiceProvider | di :: ServiceRef :: new (Self :: new (sp . get_all :: < dyn Bar > () . collect ()))) ",
            "} ",
            "}");

        assert_eq!(expected, result.to_string());
    }

    #[test]
    fn attribute_should_inject_multiple_dependencies() {
        // arrange
        let metadata = TokenStream::from_str(r#"Thing"#).unwrap();
        let input = TokenStream::from_str(
            r#"
            impl ThingImpl {
                #[inject]
                fn create_new(_foo: ServiceRef<dyn Foo>, _bar: Option<ServiceRef<dyn Bar>>) -> Self {
                    Self { }
                }
            }
        "#,
        )
        .unwrap();

        // act
        let result = _injectable(metadata, input);

        // assert
        let expected = concat!(
            "impl ThingImpl { ",
            "# [inject] ",
            "fn create_new (_foo : ServiceRef < dyn Foo >, _bar : Option < ServiceRef < dyn Bar >>) -> Self { ",
            "Self { } ",
            "} ",
            "} ",
            "impl di :: Injectable for ThingImpl { ",
            "fn inject (lifetime : di :: ServiceLifetime) -> di :: ServiceDescriptor { ",
            "di :: ServiceDescriptorBuilder :: < dyn Thing , Self > :: new (lifetime , di :: Type :: of :: < Self > ()) ",
            ". depends_on (di :: ServiceDependency :: new (di :: Type :: of :: < dyn Foo > () , di :: ServiceCardinality :: ExactlyOne)) ",
            ". depends_on (di :: ServiceDependency :: new (di :: Type :: of :: < dyn Bar > () , di :: ServiceCardinality :: ZeroOrOne)) ",
            ". from (| sp : & di :: ServiceProvider | di :: ServiceRef :: new (Self :: create_new (sp . get_required :: < dyn Foo > () , sp . get :: < dyn Bar > ()))) ",
            "} ",
            "}");

        assert_eq!(expected, result.to_string());
    }

    #[test]
    fn attribute_should_implement_injectable_for_self() {
        // arrange
        let metadata = TokenStream::new();
        let input = TokenStream::from_str(
            r#"
            impl FooImpl {
                fn new() -> Self {
                    Self { }
                }
            }
        "#,
        )
        .unwrap();

        // act
        let result = _injectable(metadata, input);

        // assert
        let expected = concat!(
            "impl FooImpl { ",
            "fn new () -> Self { ",
            "Self { } ",
            "} ",
            "} ",
            "impl di :: Injectable for FooImpl { ",
            "fn inject (lifetime : di :: ServiceLifetime) -> di :: ServiceDescriptor { ",
            "di :: ServiceDescriptorBuilder :: < Self , Self > :: new (lifetime , di :: Type :: of :: < Self > ()) ",
            ". from (| sp : & di :: ServiceProvider | di :: ServiceRef :: new (Self :: new ())) ",
            "} ",
            "}");

        assert_eq!(expected, result.to_string());
    }

    #[test]
    fn attribute_should_implement_injectable_for_struct() {
        // arrange
        let metadata = TokenStream::from_str(r#"Foo"#).unwrap();
        let input = TokenStream::from_str(
            r#"
            impl FooImpl {
                fn new(_bar: Rc<Bar>) -> Self {
                    Self { }
                }
            }
        "#,
        )
        .unwrap();

        // act
        let result = _injectable(metadata, input);

        // assert
        let expected = concat!(
            "impl FooImpl { ",
            "fn new (_bar : Rc < Bar >) -> Self { ",
            "Self { } ",
            "} ",
            "} ",
            "impl di :: Injectable for FooImpl { ",
            "fn inject (lifetime : di :: ServiceLifetime) -> di :: ServiceDescriptor { ",
            "di :: ServiceDescriptorBuilder :: < dyn Foo , Self > :: new (lifetime , di :: Type :: of :: < Self > ()) ",
            ". depends_on (di :: ServiceDependency :: new (di :: Type :: of :: < Bar > () , di :: ServiceCardinality :: ExactlyOne)) ",
            ". from (| sp : & di :: ServiceProvider | di :: ServiceRef :: new (Self :: new (sp . get_required :: < Bar > ()))) ",
            "} ",
            "}");

        assert_eq!(expected, result.to_string());
    }

    #[test]
    fn attribute_should_implement_injectable_for_generic_struct() {
        // arrange
        let metadata = TokenStream::new();
        let input = TokenStream::from_str(
            r#"
            impl<T: Default> GenericBar<T> {
                fn new() -> Self {
                    Self { }
                }
            }
        "#,
        )
        .unwrap();

        // act
        let result = _injectable(metadata, input);

        // assert
        let expected = concat!(
            "impl < T : Default > GenericBar < T > { ",
            "fn new () -> Self { ",
            "Self { } ",
            "} ",
            "} ",
            "impl < T : Default > di :: Injectable for GenericBar < T > { ",
            "fn inject (lifetime : di :: ServiceLifetime) -> di :: ServiceDescriptor { ",
            "di :: ServiceDescriptorBuilder :: < Self , Self > :: new (lifetime , di :: Type :: of :: < Self > ()) ",
            ". from (| sp : & di :: ServiceProvider | di :: ServiceRef :: new (Self :: new ())) ",
            "} ",
            "}");

        assert_eq!(expected, result.to_string());
    }

    #[test]
    fn attribute_should_implement_injectable_for_generic_trait() {
        // arrange
        let metadata = TokenStream::from_str(r#"Pair<TKey, TValue>"#).unwrap();
        let input = TokenStream::from_str(
            r#"
            impl<TKey, TValue> PairImpl<TKey, TValue>
            where
                TKey: Debug,
                TValue: Debug
            {
                fn new(key: ServiceRef<TKey>, value: ServiceRef<TValue>) -> Self {
                    Self { key, value }
                }
            }
        "#,
        )
        .unwrap();

        // act
        let result = _injectable(metadata, input);

        // assert
        let expected = concat!(
            "impl < TKey , TValue > PairImpl < TKey , TValue > ",
            "where ",
            "TKey : Debug , ",
            "TValue : Debug ",
            "{ ",
            "fn new (key : ServiceRef < TKey >, value : ServiceRef < TValue >) -> Self { ",
            "Self { key , value } ",
            "} ",
            "} ",
            "impl < TKey , TValue > di :: Injectable for PairImpl < TKey , TValue > ",
            "where ",
            "TKey : Debug , ",
            "TValue : Debug ",
            "{ ",
            "fn inject (lifetime : di :: ServiceLifetime) -> di :: ServiceDescriptor { ",
            "di :: ServiceDescriptorBuilder :: < dyn Pair < TKey , TValue > , Self > :: new (lifetime , di :: Type :: of :: < Self > ()) ",
            ". depends_on (di :: ServiceDependency :: new (di :: Type :: of :: < TKey > () , di :: ServiceCardinality :: ExactlyOne)) ",
            ". depends_on (di :: ServiceDependency :: new (di :: Type :: of :: < TValue > () , di :: ServiceCardinality :: ExactlyOne)) ",
            ". from (| sp : & di :: ServiceProvider | di :: ServiceRef :: new (Self :: new (\
                sp . get_required :: < TKey > () , \
                sp . get_required :: < TValue > ()))) ",
            "} ",
            "}");

        assert_eq!(expected, result.to_string());
    }
}
