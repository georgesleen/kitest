//! A schematic as kitest reads it: parts, nets, and power rails.

use std::collections::BTreeMap;

/// Everything kitest reads from a schematic, merged from the `kicadxml`
/// export and the schematic files themselves.
#[derive(Debug, Clone, PartialEq)]
pub struct Design {
    /// Placed parts, excluding power symbols.
    pub components: Vec<Component>,
    /// Library definitions of the parts, keyed by library and name.
    pub parts: BTreeMap<LibraryId, LibraryPart>,
    /// Every net and the pins on it.
    pub nets: Vec<Net>,
    /// Net names created by power symbols, sorted and unique.
    pub rails: Vec<String>,
}

impl Design {
    /// The placed part with reference `reference`.
    pub fn component(&self, reference: &str) -> Option<&Component> {
        self.components
            .iter()
            .find(|component| component.reference == reference)
    }

    /// The library definition a placed part was drawn from.
    pub fn part(&self, component: &Component) -> Option<&LibraryPart> {
        self.parts.get(&component.library)
    }

    /// The net named `name`.
    pub fn net(&self, name: &str) -> Option<&Net> {
        self.nets.iter().find(|net| net.name == name)
    }
}

/// A placed part.
#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    pub reference: String,
    pub value: String,
    pub library: LibraryId,
    /// Fields on the placed part, including custom ones such as `Sim.*`.
    pub fields: BTreeMap<String, String>,
    /// Net name for each pin number that is connected.
    pub pins: BTreeMap<String, String>,
    pub excluded_from_sim: bool,
    pub dnp: bool,
}

/// A symbol's library and name, as in `Transistor_BJT:2N3904`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LibraryId {
    pub library: String,
    pub part: String,
}

/// A library symbol's definition.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryPart {
    /// Fields of the definition, including inherited `Sim.*` ones.
    pub fields: BTreeMap<String, String>,
    pub pins: Vec<LibraryPin>,
}

impl LibraryPart {
    /// The pin numbered `number`.
    pub fn pin(&self, number: &str) -> Option<&LibraryPin> {
        self.pins.iter().find(|pin| pin.number == number)
    }
}

/// One pin of a library symbol.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryPin {
    pub number: String,
    pub name: String,
    pub kind: PinKind,
}

/// A net and the pins it joins.
#[derive(Debug, Clone, PartialEq)]
pub struct Net {
    pub name: String,
    pub nodes: Vec<Node>,
}

impl Net {
    /// True if any pin on the net is a power output, such as a regulator's.
    pub fn is_driven(&self) -> bool {
        self.nodes.iter().any(|node| node.kind == PinKind::PowerOut)
    }
}

/// One pin's membership of a net.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub reference: String,
    pub pin: String,
    pub kind: PinKind,
}

/// A pin's electrical type, as KiCad assigns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinKind {
    Input,
    Output,
    Bidirectional,
    TriState,
    Passive,
    Free,
    Unspecified,
    PowerIn,
    PowerOut,
    OpenCollector,
    OpenEmitter,
    NoConnect,
}

impl PinKind {
    /// Parse KiCad's name for a pin type, ignoring a `+no_connect` suffix.
    pub(crate) fn parse(name: &str) -> Option<Self> {
        let base = name.split('+').next().unwrap_or(name);
        Some(match base {
            "input" => Self::Input,
            "output" => Self::Output,
            "bidirectional" => Self::Bidirectional,
            "tri_state" => Self::TriState,
            "passive" => Self::Passive,
            "free" => Self::Free,
            "unspecified" => Self::Unspecified,
            "power_in" => Self::PowerIn,
            "power_out" => Self::PowerOut,
            "open_collector" => Self::OpenCollector,
            "open_emitter" => Self::OpenEmitter,
            "no_connect" => Self::NoConnect,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_kind_ignores_the_unconnected_suffix() {
        assert_eq!(
            PinKind::parse("passive+no_connect"),
            Some(PinKind::Passive)
        );
        assert_eq!(PinKind::parse("power_out"), Some(PinKind::PowerOut));
    }

    #[test]
    fn pin_kind_rejects_an_unknown_name() {
        assert_eq!(PinKind::parse("sideways"), None);
    }
}
