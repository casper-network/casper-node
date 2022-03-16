//! Parser for reactor macro.
//!
//! Contains the `Parse` implementations for the intermediate representation of the macro, which is
//! `ReactorDefinition`. Many functions required by the code generator are also included here as
//! methods in this representation.

use std::{
    convert::{TryFrom, TryInto},
    fmt::{self, Debug, Formatter},
};

use indexmap::{IndexMap, IndexSet};
use inflector::cases::pascalcase::to_pascal_case;
use quote::quote;
use syn::{
    braced, bracketed, parenthesized,
    parse::{Parse, ParseStream, Result},
    punctuated::Punctuated,
    Expr, Ident, ItemType, Path, Token, Type,
};

use crate::{rust_type::RustType, util::to_ident};
use proc_macro2::TokenStream;

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

impl ReactorDefinition {
    /// Returns the reactor's type's identifier (e.g. `ExampleReactor`).
    pub(crate) fn reactor_ident(&self) -> Ident {
        self.reactor_type_ident.clone()
    }

    /// Returns the reactor's associated event type's identifier (e.g. `ExampleReactorEvent`).
    pub(crate) fn event_ident(&self) -> Ident {
        let mut event_str = self.reactor_ident().to_string();
        event_str.push_str("Event");
        to_ident(&event_str)
    }

    /// Returns the reactor's associated error type's identifier (e.g. `ExampleReactorError`).
    pub(crate) fn error_ident(&self) -> Ident {
        let mut event_str = self.reactor_ident().to_string();
        event_str.push_str("Error");
        to_ident(&event_str)
    }

    /// Returns an iterator over all announcement mappings.
    pub(crate) fn announcements(&self) -> impl Iterator<Item = &AnnouncementDefinition> {
        self.announcements.iter()
    }

    /// Returns an iterator over all component definitions.
    pub(crate) fn components(&self) -> impl Iterator<Item = &ComponentDefinition> {
        self.components.values()
    }

    /// Returns the configuration type.
    pub(crate) fn config_type(&self) -> &RustType {
        &self.config_type
    }

    /// Returns an iterator over all request mappings.
    pub(crate) fn requests(&self) -> impl Iterator<Item = &RequestDefinition> {
        self.requests.iter()
    }

    /// Returns the a full component by ident.
    pub(crate) fn component(&self, ident: &Ident) -> &ComponentDefinition {
        &self.components[ident]
    }

    /// Returns the type for the event associated with a specific component.
    pub(crate) fn component_event(&self, component: &ComponentDefinition) -> TokenStream {
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

    /// Update a parsed reactor to include control announcements.
    pub(crate) fn inject_control_announcements(&mut self) {
        // For now, we allow no manual control announcements implementation.
        let ty: Type = syn::parse_str("crate::effect::announcements::ControlAnnouncement")
            .expect("Hardcoded ControlAnnouncement type could not be parsed");
        self.announcements.push(AnnouncementDefinition {
            announcement_type: ty
                .try_into()
                .expect("could not convert hardcoded `ControlAnnouncement` to `RustType`"),
            targets: vec![Target::Panic],
        })
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

        let requests: Vec<_> = requests_content
            .parse_terminated::<RequestDefinition, Token!(;)>(RequestDefinition::parse)?
            .into_iter()
            .collect();

        // Announcements.
        let announcements_content;
        let _: kw::announcements = content.parse()?;
        let _: Token!(:) = content.parse()?;
        braced!(announcements_content in content);
        let announcements: Vec<_> = announcements_content
            .parse_terminated::<AnnouncementDefinition, Token!(;)>(AnnouncementDefinition::parse)?
            .into_iter()
            .collect();

        // We can now perform some rudimentary checks. Component keys are converted to strings, so
        // rid them of their span information.
        let component_keys: IndexSet<_> =
            components.keys().map(|ident| ident.to_string()).collect();

        // Ensure that the `events` section does not point to non-existing components.
        let events_keys: IndexSet<_> = events.keys().collect();

        // We cannot use the `difference` function, because equal idents compare different based on
        // their span.
        for key in &events_keys {
            if !component_keys.contains(&key.to_string()) {
                return Err(syn::Error::new_spanned(
                    key,
                    format!("An event entry points to a non-existing component: {}", key),
                ));
            }
        }

        // Ensure that requests are not routed to non-existing events.
        let request_target_keys: IndexSet<_> = requests
            .iter()
            .filter_map(|req| req.target.as_dest())
            .collect();

        for key in &request_target_keys {
            if !component_keys.contains(&key.to_string()) {
                return Err(syn::Error::new_spanned(
                    key,
                    format!("An request route to a non-existing component: {}", key),
                ));
            }
        }

        // Ensure that requests are not routed to non-existing events.
        let announce_target_keys: IndexSet<_> = announcements
            .iter()
            .flat_map(|ann| ann.targets.iter())
            .filter_map(Target::as_dest)
            .collect();

        for key in &announce_target_keys {
            if !component_keys.contains(&key.to_string()) {
                return Err(syn::Error::new_spanned(
                    key,
                    format!("An announcement route to a non-existing component: {}", key),
                ));
            }
        }

        Ok(ReactorDefinition {
            reactor_type_ident,
            config_type: RustType::try_from(config.ty.as_ref().clone())
                .map_err(|err| syn::parse::Error::new_spanned(config.ty, err))?,
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
    /// Whether or not the component has actual effects when constructed.
    has_effects: bool,
    /// Whether or not the component's `new` function returns a component instead of a `Result`.
    is_infallible: bool,
}

impl ComponentDefinition {
    /// Returns the component construction arguments.
    pub(crate) fn component_arguments(&self) -> &[Expr] {
        self.component_arguments.as_slice()
    }

    /// Returns an ident identifying the component that is suitable for a struct field, e.g. `net`.
    pub(crate) fn field_ident(&self) -> &Ident {
        &self.name
    }

    /// Returns an ident identifying the component that is suitable for a variant, e.g. `Net`.
    pub(crate) fn variant_ident(&self) -> Ident {
        to_ident(&to_pascal_case(&self.field_ident().to_string()))
    }

    /// Returns the type of the component.
    pub(crate) fn component_type(&self) -> &RustType {
        &self.component_type
    }

    /// Returns the full path for a component by prefixing it with `crate::components::`, e.g.
    /// `crate::components::small_net::SmallNet<NodeId>`
    pub(crate) fn full_component_type(&self) -> TokenStream {
        let component_type = self.component_type();
        let module_ident = component_type.module_ident();
        let ty = component_type.ty();
        quote!(crate::components::#module_ident::#ty)
    }

    /// Returns the full path for a component's event e.g. `crate::components::small_net::Error`
    pub(crate) fn full_error_type(&self, reactor_event_type: TokenStream) -> TokenStream {
        let comp_type = self.full_component_type();
        quote!(<#comp_type as crate::components::Component<#reactor_event_type>>::ConstructionError)
    }

    /// Returns whether or not the component returns effects upon instantiation.
    pub(crate) fn has_effects(&self) -> bool {
        self.has_effects
    }

    /// Returns whether the component always returns a component directly instead of a `Result`.
    pub(crate) fn is_infallible(&self) -> bool {
        self.is_infallible
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

        let has_effects = if input.peek(kw::has_effects) {
            let _: kw::has_effects = input.parse()?;
            true
        } else {
            false
        };

        let is_infallible = if input.peek(kw::infallible) {
            let _: kw::infallible = input.parse()?;
            true
        } else {
            false
        };

        let ty: Path = input.parse()?;

        // Parse arguments
        let content;
        parenthesized!(content in input);

        let args: Punctuated<Expr, Token!(,)> = content.parse_terminated(Expr::parse)?;
        Ok(ComponentDefinition {
            name,
            component_type: RustType::new(ty),
            component_arguments: args.into_iter().collect(),
            has_effects,
            is_infallible,
        })
    }
}

/// An event-definition
///
/// Typically only used to override tricky event definitions.
#[derive(Debug)]
pub(crate) struct EventDefinition {
    /// Identifier of the components.
    pub(crate) name: Ident,
    /// Event type to use.
    pub(crate) event_type: RustType,
}

impl Parse for EventDefinition {
    fn parse(input: ParseStream) -> Result<Self> {
        // Parse left hand side and type def.
        let name: Ident = input.parse()?;
        let _: Token!(=) = input.parse()?;
        let ty: Path = input.parse()?;

        Ok(EventDefinition {
            name,
            event_type: RustType::new(ty),
        })
    }
}

#[derive(Debug)]
/// A definition of a request routing.
pub(crate) struct RequestDefinition {
    pub(crate) request_type: RustType,
    pub(crate) target: Target,
}

impl RequestDefinition {
    /// Returns an ident identifying the request that is suitable for a variant, e.g.
    /// `NetworkRequest`.
    pub(crate) fn variant_ident(&self) -> Ident {
        self.request_type.stringified_ident()
    }

    /// Returns the type of the request.
    pub(crate) fn request_type(&self) -> &RustType {
        &self.request_type
    }

    /// Returns the target of the request.
    pub(crate) fn target(&self) -> &Target {
        &self.target
    }

    /// Returns the full path for a request.
    pub(crate) fn full_request_type(&self) -> TokenStream {
        let request_type = self.request_type();
        let ty = request_type.ty();
        quote!(crate::effect::requests::#ty)
    }
}

impl Parse for RequestDefinition {
    fn parse(input: ParseStream) -> Result<Self> {
        let request_type = RustType::new(input.parse()?);
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
    pub(crate) announcement_type: RustType,
    pub(crate) targets: Vec<Target>,
}

impl AnnouncementDefinition {
    /// Returns the type of the announcement.
    pub(crate) fn announcement_type(&self) -> &RustType {
        &self.announcement_type
    }

    /// Returns the full path for an announcement.
    pub(crate) fn full_announcement_type(&self) -> TokenStream {
        let announcement_type = self.announcement_type();
        let ty = announcement_type.ty();

        if announcement_type.ident().to_string().ends_with("Incoming") {
            quote!(crate::effect::incoming::#ty)
        } else {
            quote!(crate::effect::announcements::#ty)
        }
    }

    /// Returns an iterator over the targets of the announcement.
    pub(crate) fn targets(&self) -> impl Iterator<Item = &Target> {
        self.targets.iter()
    }

    /// Returns an ident identifying the announcement that is suitable for a variant, e.g.
    /// `NetworkAnnouncement`.
    pub(crate) fn variant_ident(&self) -> Ident {
        self.announcement_type.stringified_ident()
    }
}

impl Parse for AnnouncementDefinition {
    fn parse(input: ParseStream) -> Result<Self> {
        let announcement_type = RustType::new(input.parse()?);
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

/// A routing target.
pub(crate) enum Target {
    /// Discard whatever is being routed.
    Discard,
    /// When anything is routed to this target, panic.
    Panic,
    /// Forward to destination.
    Dest(Ident),
    /// Dispatch using a method.
    Dispatch(Ident),
}

impl Target {
    /// Returns a reference to the destination identifier if the target is a destination, or `None`.
    fn as_dest(&self) -> Option<&Ident> {
        match self {
            Target::Discard | Target::Panic | Target::Dispatch(_) => None,
            Target::Dest(ident) => Some(ident),
        }
    }
}

impl Debug for Target {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Target::Discard => write!(f, "#"),
            Target::Panic => write!(f, "!"),
            Target::Dest(id) => write!(f, "{}", id),
            Target::Dispatch(id) => write!(f, "{}()", id),
        }
    }
}

impl Parse for Target {
    fn parse(input: ParseStream) -> Result<Self> {
        if input.peek(Token!(!)) {
            let _: Token!(!) = input.parse()?;
            Ok(Target::Panic)
        } else if input.peek(Token!(#)) {
            let _: Token!(#) = input.parse()?;
            Ok(Target::Discard)
        } else if input.peek(Token!(fn)) {
            let _: Token!(fn) = input.parse()?;
            let dispatch = input.parse()?;

            Ok(Target::Dispatch(dispatch))
        } else {
            input.parse().map(Target::Dest)
        }
    }
}

/// Custom keywords.
///
/// This module groups custom keywords used by the parser.
mod kw {
    syn::custom_keyword!(components);
    syn::custom_keyword!(events);
    syn::custom_keyword!(requests);
    syn::custom_keyword!(announcements);
    syn::custom_keyword!(infallible);
    syn::custom_keyword!(has_effects);
}
