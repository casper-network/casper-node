use std::{
    convert::TryFrom,
    fmt::{self, Debug, Formatter},
};

use indexmap::IndexMap;
use inflector::cases::{pascalcase::to_pascal_case, snakecase::to_snake_case};
use syn::parse::{Parse, ParseStream, Result};
use syn::punctuated::Punctuated;
use syn::{
    braced, bracketed, export::quote::quote, parenthesized, Expr, Ident, ItemType, Path, Token,
    Type,
};

use crate::util::to_ident;
use proc_macro2::{Span, TokenStream};

#[derive(Debug)]
pub(crate) struct ReactorDefinition {
    /// Identifier of the reactor type.
    ///
    /// Example: `ExampleReactor`.
    reactor_type_ident: Ident,

    /// Reactor's associated configuration type.
    ///
    /// A full type that will later be the `Reactor::Config` associated type.
    config_type: RustType,

    /// Mapping of component attribute names to their types.
    ///
    /// Example: "net" maps to `crate::components::small_net::SmallNet<NodeId>`.
    components: IndexMap<Ident, ComponentDefinition>,

    /// Overrides for events of components.
    ///
    /// Example: "net" may have an event type that differs from
    /// `crate::components::small_net::Event`.
    events: IndexMap<Ident, EventDefinition>,

    /// List of request routing directives.
    requests: Vec<RequestDefinition>,

    /// List of announcement routing directives.
    announcements: Vec<AnnouncementDefinition>,
}
/// A fully pathed Rust type with type arguments, e.g. `crate::components::SmallNet<NodeId>`.
pub(crate) struct RustType(Path);

impl Debug for RustType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let path = &self.0;
        write!(f, "{}", quote!(#path).to_string())
    }
}

impl RustType {
    /// Returns the types identifier without type arguments, e.g. `SmallNet`.
    pub fn ident(&self) -> Ident {
        self.0
            .segments
            .last()
            .expect("type has no last part?")
            .ident
            .clone()
    }

    /// Returns the type without the path, but with type arguments, e.g. `SmallNet<NodeId>`.
    pub fn ty(&self) -> TokenStream {
        let ident = self.ident();
        let args = &self
            .0
            .segments
            .last()
            .expect("type has no last part?")
            .arguments;
        quote!(#ident #args)
    }

    /// Returns the full type as it was given in the macro call.
    pub fn as_given(&self) -> &Path {
        &self.0
    }

    /// Returns the module name that canonically would contain the type, e.g. `small_net`.
    pub fn module_ident(&self) -> Ident {
        to_ident(&to_snake_case(&self.ident().to_string()))
    }
}

impl TryFrom<Type> for RustType {
    type Error = String;

    fn try_from(value: Type) -> core::result::Result<Self, Self::Error> {
        match value {
            Type::Path(type_path) => Ok(RustType(type_path.path)),
            _ => Err(format!("cannot convert to RustType")),
        }
    }
}

impl ReactorDefinition {
    /// Returns the reactor's type's identifier (e.g. `ExampleReactor`).
    pub fn reactor_ident(&self) -> Ident {
        self.reactor_type_ident.clone()
    }

    /// Returns the reactor's associated event type's identifier (e.g. `ExampleReactorEvent`).
    pub fn event_ident(&self) -> Ident {
        let mut event_str = self.reactor_ident().to_string();
        event_str.push_str("Event");
        to_ident(&event_str)
    }

    /// Returns the reactor's associated error type's identifier (e.g. `ExampleReactorError`).
    pub fn error_ident(&self) -> Ident {
        let mut event_str = self.reactor_ident().to_string();
        event_str.push_str("Error");
        to_ident(&event_str)
    }

    /// Returns an iterator over all component definitions.
    pub fn components(&self) -> impl Iterator<Item = &ComponentDefinition> {
        self.components.values()
    }

    /// Returns the configuration type.
    pub fn config_type(&self) -> &RustType {
        &self.config_type
    }

    /// Returns an iterator over all request mappings.
    pub fn requests(&self) -> impl Iterator<Item = &RequestDefinition> {
        self.requests.iter()
    }

    /// Returns the type for the event associated with a specific component.
    pub fn component_event(&self, component: &ComponentDefinition) -> TokenStream {
        let component_type = component.component_type();
        let module_ident = component_type.module_ident();

        let event_ident = if let Some(event_def) = self.events.get(component.field_ident()) {
            let path = event_def.event_type.as_given();
            quote!(#path)
        } else {
            let ident = to_ident("Event");
            quote!(#ident)
        };

        quote!(crate::components::#module_ident::#event_ident)
    }
}

impl Parse for ReactorDefinition {
    fn parse(input: ParseStream) -> Result<Self> {
        let content;
        // formerly `name`
        let reactor_type_ident: Ident = input.parse()?;

        // Outer and config type.
        braced!(content in input);
        let config: ItemType = content.parse()?;

        // Components.
        let component_content;
        let _: kw::components = content.parse()?;
        let _: Token!(:) = content.parse()?;
        braced!(component_content in content);

        let mut components = IndexMap::new();
        for cdef in component_content
            .parse_terminated::<ComponentDefinition, Token!(;)>(ComponentDefinition::parse)?
        {
            components.insert(cdef.name.clone(), cdef);
        }

        // Event (-overrides)
        let event_content;
        let _: kw::events = content.parse()?;
        let _: Token!(:) = content.parse()?;
        braced!(event_content in content);

        let mut events = IndexMap::new();
        for edef in
            event_content.parse_terminated::<EventDefinition, Token!(;)>(EventDefinition::parse)?
        {
            events.insert(edef.name.clone(), edef);
        }

        // Requests.
        let requests_content;
        let _: kw::requests = content.parse()?;
        let _: Token!(:) = content.parse()?;
        braced!(requests_content in content);

        let requests = requests_content
            .parse_terminated::<RequestDefinition, Token!(;)>(RequestDefinition::parse)?
            .into_iter()
            .collect();

        // Announcements.
        let announcements_content;
        let _: kw::announcements = content.parse()?;
        let _: Token!(:) = content.parse()?;
        braced!(announcements_content in content);
        let announcements = announcements_content
            .parse_terminated::<AnnouncementDefinition, Token!(;)>(AnnouncementDefinition::parse)?
            .into_iter()
            .collect();

        Ok(ReactorDefinition {
            reactor_type_ident,
            config_type: RustType::try_from(config.ty.as_ref().clone()).map_err(|err| {
                syn::parse::Error::new(
                    Span::call_site(), // FIXME: Can we get a better span here?
                    err.to_string(),
                )
            })?,
            components,
            events,
            requests,
            announcements,
        })
    }
}

/// A definition of a component.
pub(crate) struct ComponentDefinition {
    /// The attribute-style name of the component, e.g. `net`.
    name: Ident,
    /// The components type.
    component_type: RustType,
    /// Arguments passed to the components `new` constructor when constructing.
    component_arguments: Vec<Expr>,
}

impl ComponentDefinition {
    /// Returns an ident identifying the component that is suitable for a struct field, e.g. `net`.
    pub(crate) fn field_ident(&self) -> &Ident {
        &self.name
    }

    /// Returns an ident identifying the component that is suitable for a variant, e.g. `Net`.
    pub fn variant_ident(&self) -> Ident {
        to_ident(&to_pascal_case(&self.field_ident().to_string()))
    }

    /// Returns the type of the component.
    pub(crate) fn component_type(&self) -> &RustType {
        &self.component_type
    }

    /// Returns the full path for a component by prefixing it with `crate::components::`, e.g.
    /// `crate::components::small_net::SmallNet<NodeId>`
    pub fn full_component_type(&self) -> TokenStream {
        let component_type = self.component_type();
        let module_ident = component_type.module_ident();
        let ty = component_type.ty();
        quote!(crate::components::#module_ident::#ty)
    }

    /// Returns the full path for a component's event e.g. `crate::components::small_net::Error`
    pub fn full_error_type(&self) -> TokenStream {
        // TODO: Use actual error type instead of anyhow.
        quote!(anyhow::Error)
    }
}

impl Debug for ComponentDefinition {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentDefinition")
            .field("name", &self.name.to_string())
            .field("component_type", &self.component_type)
            .field("component_arguments", &"TODO: fmtargs")
            .finish()
    }
}

impl Parse for ComponentDefinition {
    fn parse(input: ParseStream) -> Result<Self> {
        // Parse left hand side and type def.
        let name: Ident = input.parse()?;
        let _: Token!(=) = input.parse()?;
        let ty: Path = input.parse()?;

        // Parse arguments
        let content;
        parenthesized!(content in input);

        let args: Punctuated<Expr, Token!(,)> = content.parse_terminated(Expr::parse)?;
        Ok(ComponentDefinition {
            name,
            component_type: RustType(ty),
            component_arguments: args.into_iter().collect(),
        })
    }
}

/// An event-definition
///
/// Typically only used to override tricky event definitions.
#[derive(Debug)]
pub(crate) struct EventDefinition {
    /// Identifier of the components.
    pub name: Ident,
    /// Event type to use.
    pub event_type: RustType,
}

impl Parse for EventDefinition {
    fn parse(input: ParseStream) -> Result<Self> {
        // Parse left hand side and type def.
        let name: Ident = input.parse()?;
        let _: Token!(=) = input.parse()?;
        let ty: Path = input.parse()?;

        Ok(EventDefinition {
            name,
            event_type: RustType(ty),
        })
    }
}

#[derive(Debug)]
/// A definition of a request routing.
pub(crate) struct RequestDefinition {
    pub request_type: RustType,
    pub target: Target,
}

impl RequestDefinition {
    /// Returns an ident identifying the request that is suitable for a variant, e.g.
    /// `NetworkRequest`.
    pub fn variant_ident(&self) -> Ident {
        self.request_type.ident()
    }

    /// Returns the type of the request.
    pub(crate) fn request_type(&self) -> &RustType {
        &self.request_type
    }

    /// Returns the target of the request.
    pub(crate) fn target(&self) -> &Target {
        &self.target
    }

    /// Returns the full path for a component by prefixing it with `crate::components::`, e.g.
    /// `crate::components::small_net::SmallNet<NodeId>`
    pub fn full_request_type(&self) -> TokenStream {
        let request_type = self.request_type();
        let ty = request_type.ty();
        quote!(crate::effect::requests::#ty)
    }
}

impl Parse for RequestDefinition {
    fn parse(input: ParseStream) -> Result<Self> {
        let request_type = RustType(input.parse()?);
        let _: Token!(->) = input.parse()?;

        let target = input.parse()?;

        Ok(RequestDefinition {
            request_type,
            target,
        })
    }
}

#[derive(Debug)]
/// A definition of an announcement.
pub(crate) struct AnnouncementDefinition {
    pub announcement_type: RustType,
    pub targets: Vec<Target>,
}

impl Parse for AnnouncementDefinition {
    fn parse(input: ParseStream) -> Result<Self> {
        let announcement_type = RustType(input.parse()?);
        let _: Token!(->) = input.parse()?;

        let content;
        bracketed!(content in input);
        let targets = content
            .parse_terminated::<Target, Token!(,)>(Target::parse)?
            .into_iter()
            .collect();

        Ok(AnnouncementDefinition {
            announcement_type,
            targets,
        })
    }
}

pub(crate) enum Target {
    Discard,
    Dest(Ident),
}

impl Debug for Target {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Target::Discard => write!(f, "!"),
            Target::Dest(id) => write!(f, "{}", id.to_string()),
        }
    }
}

impl Parse for Target {
    fn parse(input: ParseStream) -> Result<Self> {
        if input.peek(Token!(!)) {
            let _: Token!(!) = input.parse()?;
            Ok(Target::Discard)
        } else {
            input.parse().map(Target::Dest)
        }
    }
}

mod kw {
    syn::custom_keyword!(components);
    syn::custom_keyword!(events);
    syn::custom_keyword!(requests);
    syn::custom_keyword!(announcements);
}
