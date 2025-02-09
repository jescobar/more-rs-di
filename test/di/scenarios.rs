use crate::*;
use di::*;

#[test]
fn inject_should_implement_trait_for_struct() {
    // arrange
    let provider = ServiceCollection::new()
        .add(traits::BarImpl::transient())
        .build_provider()
        .unwrap();

    // act
    let bar = provider.get_required::<dyn traits::Bar>();

    // assert
    assert_eq!("Success!", bar.echo());
}

#[test]
fn inject_should_implement_trait_for_struct_with_dependency() {
    // arrange
    let provider = ServiceCollection::new()
        .add(traits::FooImpl::singleton())
        .add(traits::BarImpl::transient())
        .build_provider()
        .unwrap();

    // act
    let foo = provider.get_required::<dyn traits::Foo>();

    // assert
    assert_eq!("Success!", foo.echo());
}

#[test]
fn inject_should_implement_struct_for_self() {
    // arrange
    let provider = ServiceCollection::new()
        .add(structs::Bar::transient())
        .build_provider()
        .unwrap();

    // act
    let bar = provider.get_required::<structs::Bar>();

    // assert
    assert_eq!("Success!", bar.echo());
}

#[test]
fn inject_should_implement_struct_for_self_with_dependency() {
    // arrange
    let provider = ServiceCollection::new()
        .add(structs::Foo::singleton())
        .add(structs::Bar::transient())
        .build_provider()
        .unwrap();

    // act
    let foo = provider.get_required::<structs::Foo>();

    // assert
    assert_eq!("Success!", foo.echo());
}

#[test]
#[allow(clippy::vtable_address_comparisons)]
fn inject_should_clone_service_provider_and_return_same_singleton() {
    // arrange
    let provider = ServiceCollection::new()
        .add(traits::FooImpl::singleton())
        .add(traits::BarImpl::transient())
        .add(containers::Container::transient())
        .build_provider()
        .unwrap();
    let container = provider.get_required::<containers::Container>();

    // act
    let svc1 = container.foo();
    let svc2 = provider.get_required::<dyn traits::Foo>();

    // assert
    assert!(ServiceRef::ptr_eq(&svc1, &svc2));
}

#[test]
#[allow(clippy::vtable_address_comparisons)]
fn inject_should_clone_service_provider_and_return_different_scoped_instance() {
    // arrange
    let provider = ServiceCollection::new()
        .add(traits::FooImpl::scoped())
        .add(traits::BarImpl::transient())
        .add(containers::ScopedContainer::transient())
        .build_provider()
        .unwrap();
    let container = provider.get_required::<containers::ScopedContainer>();

    // act
    let svc1 = container.foo();
    let svc2 = provider.get_required::<dyn traits::Foo>();

    // assert
    assert!(!ServiceRef::ptr_eq(&svc1, &svc2));
}

#[test]
fn inject_should_add_dependencies_for_validation() {
    // arrange
    let mut services = ServiceCollection::new();

    services.add(traits::FooImpl::transient());

    // act
    let result = services.build_provider();

    // assert
    assert!(result.is_err());
}

#[test]
fn inject_should_implement_generic_struct_with_dependency() {
    // arrange
    let provider = ServiceCollection::new()
        .add(structs::GenericFoo::<u8>::singleton())
        .add(structs::GenericBar::<u8>::transient())
        .build_provider()
        .unwrap();

    // act
    let foo = provider.get_required::<structs::GenericFoo<u8>>();

    // assert
    assert_eq!(u8::default(), foo.echo());
}

#[test]
fn inject_should_implement_generic_trait_for_generic_struct() {
    // arrange
    let provider = ServiceCollection::new()
        .add(traits::PairImpl::<u8, u8>::transient())
        .build_provider()
        .unwrap();

    // act
    let pair = provider.get_required::<dyn traits::Pair<u8, u8>>();

    // assert
    assert_eq!(&u8::default(), pair.key());
    assert_eq!(&u8::default(), pair.value());
}

#[test]
fn inject_should_implement_lazy_struct() {
    // arrange
    let provider = ServiceCollection::new()
        .add(structs::Bar::transient())
        .add(structs::LazyFoo::transient())
        .build_provider()
        .unwrap();

    // act
    let foo = provider.get_required::<structs::LazyFoo>();

    // assert
    assert_eq!("Success!", foo.echo())
}

#[test]
fn inject_should_implement_required_lazy_trait() {
    // arrange
    let provider = ServiceCollection::new()
        .add(traits::BarImpl::transient())
        .add(traits::OneLazyFoo::transient())
        .build_provider()
        .unwrap();

    // act
    let foo = provider.get_required::<dyn traits::Foo>();

    // assert
    assert_eq!("Success!", foo.echo())
}

#[test]
fn inject_should_implement_optional_lazy_trait() {
    // arrange
    let provider = ServiceCollection::new()
        .add(traits::BarImpl::transient())
        .add(traits::MaybeLazyFoo::transient())
        .build_provider()
        .unwrap();

    // act
    let foo = provider.get_required::<dyn traits::Foo>();

    // assert
    assert_eq!("Success!", foo.echo())
}

#[test]
fn inject_should_handle_implement_optional_lazy_trait() {
    // arrange
    let provider = ServiceCollection::new()
        .add(traits::MaybeLazyFoo::transient())
        .build_provider()
        .unwrap();

    // act
    let foo = provider.get_required::<dyn traits::Foo>();

    // assert
    assert_eq!("", foo.echo())
}

#[test]
fn inject_should_implement_many_lazy_trait() {
    // arrange
    let provider = ServiceCollection::new()
        .add(traits::BarImpl::transient())
        .add(traits::ManyLazyFoo::transient())
        .build_provider()
        .unwrap();

    // act
    let foo = provider.get_required::<dyn traits::Foo>();

    // assert
    assert_eq!("Success!", foo.echo())
}
