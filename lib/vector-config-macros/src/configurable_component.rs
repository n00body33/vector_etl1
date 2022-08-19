use darling::{util::path_to_string, Error, FromMeta};
use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input, parse_quote, parse_quote_spanned, punctuated::Punctuated, token::Comma,
    AttributeArgs, DeriveInput, Ident, Lit, Meta, NestedMeta, Path,
};

#[derive(Copy, Clone)]
struct AttributeIdent(&'static str);

const ENRICHMENT_TABLE: AttributeIdent = AttributeIdent("enrichment_table");
const PROVIDER: AttributeIdent = AttributeIdent("provider");
const SINK: AttributeIdent = AttributeIdent("sink");
const SOURCE: AttributeIdent = AttributeIdent("source");
const TRANSFORM: AttributeIdent = AttributeIdent("transform");
const NO_SER: AttributeIdent = AttributeIdent("no_ser");
const NO_DESER: AttributeIdent = AttributeIdent("no_deser");

impl PartialEq<AttributeIdent> for Ident {
    fn eq(&self, word: &AttributeIdent) -> bool {
        self == word.0
    }
}

impl<'a> PartialEq<AttributeIdent> for &'a Ident {
    fn eq(&self, word: &AttributeIdent) -> bool {
        *self == word.0
    }
}

impl PartialEq<AttributeIdent> for Path {
    fn eq(&self, word: &AttributeIdent) -> bool {
        self.is_ident(word.0)
    }
}

impl<'a> PartialEq<AttributeIdent> for &'a Path {
    fn eq(&self, word: &AttributeIdent) -> bool {
        self.is_ident(word.0)
    }
}

fn path_matches(path: &Path, haystack: &[AttributeIdent]) -> bool {
    haystack.iter().any(|p| path == p)
}

#[derive(Clone, Debug)]
struct TypedComponent {
    component_type: ComponentType,
    component_name: String,
}

impl TypedComponent {
    pub fn get_registration_block(&self, input: &DeriveInput) -> proc_macro2::TokenStream {
        let config_ty = &input.ident;
        let component_name = self.component_name.as_str();
        let desc_ty: syn::Type = match self.component_type {
            ComponentType::EnrichmentTable => {
                parse_quote! { ::vector_config::component::EnrichmentTableDescription }
            }
            ComponentType::Provider => {
                parse_quote! { ::vector_config::component::ProviderDescription }
            }
            ComponentType::Sink => parse_quote! { ::vector_config::component::SinkDescription },
            ComponentType::Source => {
                parse_quote! { ::vector_config::component::SourceDescription }
            }
            ComponentType::Transform => {
                parse_quote! { ::vector_config::component::TransformDescription }
            }
        };

        quote! {
            ::inventory::submit! {
                #desc_ty::new::<#config_ty>(#component_name)
            }
        }
    }
}

#[derive(Clone, Debug)]
enum ComponentType {
    EnrichmentTable,
    Provider,
    Sink,
    Source,
    Transform,
}

impl<'a> From<&'a Path> for ComponentType {
    fn from(path: &'a Path) -> Self {
        let path_str = path_to_string(path);
        match path_str.as_str() {
            "enrichment_table" => Self::EnrichmentTable,
            "provider" => Self::Provider,
            "sink" => Self::Sink,
            "source" => Self::Source,
            "transform" => Self::Transform,
            _ => unreachable!("should not be used unless path is validated"),
        }
    }
}

impl TypedComponent {
    /// Creates a new `TypedComponent`.
    pub const fn new(component_type: ComponentType, component_name: String) -> Self {
        Self {
            component_type,
            component_name,
        }
    }

    /// Gets the type of this component as a string.
    fn as_type_str(&self) -> &'static str {
        match self.component_type {
            ComponentType::EnrichmentTable => "enrichment_table",
            ComponentType::Provider => "provider",
            ComponentType::Sink => "sink",
            ComponentType::Source => "source",
            ComponentType::Transform => "transform",
        }
    }

    /// Gets the name of this component.
    fn as_name_str(&self) -> &str {
        self.component_name.as_str()
    }
}

#[derive(Debug)]
struct Options {
    /// Component type details, if specified.
    ///
    /// While the macro `#[configurable_component]` sort of belies an implication that any item
    /// being annotated is a component, we only consider sources, transforms, and sinks a true
    /// "component", in the context of a component in a Vector topology.
    typed_component: Option<TypedComponent>,

    /// Whether to disable the automatic derive for `serde::Serialize`.
    no_ser: bool,

    /// Whether to disable the automatic derive for `serde::Deserialize`.
    no_deser: bool,
}

impl FromMeta for Options {
    fn from_list(items: &[syn::NestedMeta]) -> darling::Result<Self> {
        let mut typed_component = None;
        let mut no_ser = None;
        let mut no_deser = None;

        let mut errors = Error::accumulator();

        for nm in items {
            match nm {
                // Disable automatically deriving `serde::Serialize`.
                NestedMeta::Meta(Meta::Path(p)) if p == NO_SER => {
                    if no_ser.is_some() {
                        errors.push(Error::duplicate_field_path(p));
                    } else {
                        no_ser = Some(());
                    }
                }

                // Disable automatically deriving `serde::Deserialize`.
                NestedMeta::Meta(Meta::Path(p)) if p == NO_DESER => {
                    if no_deser.is_some() {
                        errors.push(Error::duplicate_field_path(p));
                    } else {
                        no_deser = Some(());
                    }
                }

                // Marked as a component.
                NestedMeta::Meta(Meta::List(ml))
                    if path_matches(
                        &ml.path,
                        &[ENRICHMENT_TABLE, PROVIDER, SINK, SOURCE, TRANSFORM],
                    ) =>
                {
                    if typed_component.is_some() {
                        errors.push(Error::custom("already marked as a typed component; `source(..)`, `transform(..)`, and `sink(..)` are mutually exclusive").with_span(ml));
                    } else {
                        match ml.nested.first() {
                            Some(NestedMeta::Lit(Lit::Str(component_name))) => {
                                typed_component = Some(TypedComponent::new(
                                    ComponentType::from(&ml.path),
                                    component_name.value(),
                                ));
                            }
                            _ => {
                                let path_nice = path_to_string(&ml.path);
                                let error = format!("`{}` must have only one parameter, the name of the component (i.e. `{}(\"name\")`)", path_nice, path_nice);
                                errors.push(Error::custom(&error).with_span(ml))
                            }
                        }
                    }
                }

                NestedMeta::Meta(m) => {
                    let error = "expected one of: `source(\"...\")`, `transform(\"..\")`, `sink(\"..\")`, `no_ser`, or `no_deser`";
                    errors.push(Error::custom(error).with_span(m));
                }

                NestedMeta::Lit(lit) => errors.push(Error::unexpected_lit_type(lit)),
            }
        }

        errors.finish().map(|()| Self {
            typed_component,
            no_ser: no_ser.is_some(),
            no_deser: no_deser.is_some(),
        })
    }
}

impl Options {
    fn typed_component(&self) -> Option<TypedComponent> {
        self.typed_component.clone()
    }

    fn should_derive_ser(&self) -> bool {
        !self.no_ser
    }

    fn should_derive_deser(&self) -> bool {
        !self.no_deser
    }
}

pub fn configurable_component_impl(args: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as AttributeArgs);
    let input = parse_macro_input!(item as DeriveInput);

    let options = match Options::from_list(&args) {
        Ok(v) => v,
        Err(e) => {
            return TokenStream::from(e.write_errors());
        }
    };

    // If the component is typed -- source, transform, sink -- we do a few additional things:
    // - we add a metadata attribute to indicate the component type
    // - we add an attribute so the component's configuration type becomes "named", which drives
    //   the component config trait impl (i.e. `SourceConfig`) and will eventually drive the value
    //   that `serde` uses to deserialize the given component variant in the Big Enum model
    // - we automatically generate the call to register the component config type via `inventory`p
    //   which powers the `vector generate` subcommand by maintaining a name -> config type map
    let component_type = options.typed_component().map(|tc| {
        let component_type = tc.as_type_str();
        let component_name = tc.as_name_str();

        quote! {
            #[configurable(metadata(component_type = #component_type))]
            #[::vector_config::component_name(#component_name)]
        }
    });

    let maybe_component_desc = options
        .typed_component()
        .map(|tc| tc.get_registration_block(&input));

    // Generate and apply all of the necessary derives.
    let mut derives = Punctuated::<Path, Comma>::new();
    derives.push(parse_quote_spanned! {input.ident.span()=>
        ::vector_config_macros::Configurable
    });

    if options.should_derive_ser() {
        derives.push(parse_quote_spanned! {input.ident.span()=>
            ::serde::Serialize
        });
    }

    if options.should_derive_deser() {
        derives.push(parse_quote_spanned! {input.ident.span()=>
            ::serde::Deserialize
        });
    }

    // Final assembly.
    let derived = quote! {
        #[derive(#derives)]
        #component_type
        #input
        #maybe_component_desc
    };

    derived.into()
}
