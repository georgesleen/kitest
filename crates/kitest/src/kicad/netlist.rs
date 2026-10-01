//! Writing a design out as a SPICE netlist body with models bound.

use std::borrow::Cow;

use super::Design;
use super::element::element;
use super::node::node_names;
use crate::ModelLibrary;

/// First line of every netlist, which ngspice reads as the title.
const TITLE: &str = "* kitest netlist";

/// A design written as a SPICE netlist body.
#[derive(Debug, Clone, PartialEq)]
pub struct Netlist {
    /// A title line, one element line per simulated part, then model cards.
    pub text: String,
    /// Parts simulated on a default model because no library covers them.
    pub defaulted: Vec<String>,
}

impl Design {
    /// The design as a SPICE netlist body, with models bound.
    ///
    /// `libraries` are searched in order for each part's Value, so a
    /// project library listed before the bundled one takes precedence.
    pub fn netlist(
        &self,
        libraries: &[ModelLibrary],
    ) -> Result<Netlist, NetlistError> {
        let nodes = node_names(&self.nets)?;
        let mut lines = vec![TITLE.to_owned()];
        let mut cards: Vec<Cow<'_, str>> = Vec::new();
        let mut defaulted = Vec::new();
        let mut errors = Vec::new();

        let simulated = self
            .components
            .iter()
            .filter(|component| !component.excluded_from_sim && !component.dnp);
        for component in simulated {
            let resolved = self
                .part(component)
                .ok_or_else(|| NetlistError::MissingPart {
                    reference: component.reference.clone(),
                })
                .and_then(|part| element(component, part, libraries, &nodes));
            let element = match resolved {
                Ok(element) => element,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            };
            lines.push(element.line);
            if element.defaulted {
                defaulted.push(component.reference.clone());
            }
            if let Some(card) = element.card
                && !cards.contains(&card)
            {
                cards.push(card);
            }
        }
        if !errors.is_empty() {
            return Err(NetlistError::Parts(errors));
        }

        lines.extend(cards.into_iter().map(Cow::into_owned));
        Ok(Netlist {
            text: lines.join("\n"),
            defaulted,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NetlistError {
    #[error(
        "nets {first:?} and {second:?} would both be SPICE node {node:?}; \
         rename one of them in the schematic"
    )]
    NodeClash {
        first: String,
        second: String,
        node: String,
    },

    #[error(
        "{reference} has Sim.Pins {text:?}, which is not a list of \
         pin=role pairs naming each pin once, such as \"1=E 2=B 3=C\""
    )]
    SimPins { reference: String, text: String },

    #[error(
        "{reference} Sim.Pins names pin {pin}, which its symbol does not have"
    )]
    UnknownPin { reference: String, pin: String },

    #[error(
        "{reference} Sim.Pins gives a pin the role {role:?}, but its model's \
         pins are {roles}"
    )]
    UnknownRole {
        reference: String,
        role: String,
        roles: String,
    },

    #[error("{reference} has more than one pin playing {role}")]
    DuplicateRole { reference: String, role: String },

    #[error(
        "{reference} pin {pin} is named {name:?}, but its library's Sim.Pins \
         makes it {role:?}; set Sim.Pins on {reference} to say which is right"
    )]
    PinConflict {
        reference: String,
        pin: String,
        name: String,
        role: String,
    },

    #[error(
        "{reference} names its own model file with Sim.Library, which kitest \
         does not read yet; add the model to a [models] library in \
         kitest.toml instead"
    )]
    ModelFile { reference: String },

    #[error(
        "{reference} sets Sim.Params, which kitest does not read on a \
         resistor, capacitor, or inductor; give the value in its Value field"
    )]
    PassiveParams { reference: String },

    #[error(
        "{reference} is a resistor, capacitor, or inductor by its reference, \
         but its symbol has {count} pins rather than 2; set its Sim.Device, \
         or exclude it from simulation"
    )]
    PassivePins { reference: String, count: usize },

    #[error("{reference} has value {value:?}, which is not a {expected}")]
    BadValue {
        reference: String,
        value: String,
        expected: &'static str,
    },

    #[error(
        "{reference} has no simulation model: it has no Sim.Device, and no \
         model library covers its value {value:?}; add a model for it to a \
         [models] library in kitest.toml, or exclude {reference} from \
         simulation"
    )]
    NoModel { reference: String, value: String },

    #[error(
        "{reference} is a {sim_device} by its Sim.Device, but the model \
         covering {value:?} is a {model_type}"
    )]
    ModelMismatch {
        reference: String,
        sim_device: String,
        value: String,
        model_type: String,
    },

    #[error(
        "{reference} is a {device} device, which kitest cannot simulate yet; \
         exclude {reference} from simulation"
    )]
    UnsupportedDevice { reference: String, device: String },

    #[error(
        "{reference} has no pin playing {role} of its model's pins {roles}; \
         set Sim.Pins on {reference}, such as \"1=E 2=B 3=C\""
    )]
    MissingRole {
        reference: String,
        role: String,
        roles: String,
    },

    #[error("the KiCad netlist export has no net on {reference} pin {pin}")]
    MissingPinNet { reference: String, pin: String },

    #[error(
        "the KiCad netlist export has no library definition for {reference}"
    )]
    MissingPart { reference: String },

    #[error("{}", parts_message(.0))]
    Parts(Vec<NetlistError>),
}

/// One line per part that cannot be simulated, under a heading.
fn parts_message(errors: &[NetlistError]) -> String {
    let noun = if errors.len() == 1 { "part" } else { "parts" };
    let mut message = format!("cannot simulate {} {noun}:", errors.len());
    for error in errors {
        message.push_str("\n  - ");
        message.push_str(&error.to_string());
    }
    message
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::super::{
        Component, LibraryId, LibraryPart, LibraryPin, Net, PinKind,
    };
    use super::*;

    const EBC: &[(&str, &str)] = &[("1", "E"), ("2", "B"), ("3", "C")];
    const TWO_PINS: &[(&str, &str)] = &[("1", ""), ("2", "")];

    /// A placed part drawn from library part `library`, pins on `nets` in order.
    fn component(
        reference: &str,
        value: &str,
        library: &str,
        nets: &[&str],
    ) -> Component {
        Component {
            reference: reference.into(),
            value: value.into(),
            library: id(library),
            fields: BTreeMap::new(),
            pins: nets
                .iter()
                .enumerate()
                .map(|(index, net)| {
                    ((index + 1).to_string(), (*net).to_owned())
                })
                .collect(),
            excluded_from_sim: false,
            dnp: false,
        }
    }

    fn id(part: &str) -> LibraryId {
        LibraryId {
            library: "lib".into(),
            part: part.into(),
        }
    }

    fn part(pins: &[(&str, &str)]) -> LibraryPart {
        LibraryPart {
            fields: BTreeMap::new(),
            pins: pins
                .iter()
                .map(|(number, name)| LibraryPin {
                    number: (*number).to_owned(),
                    name: (*name).to_owned(),
                    kind: PinKind::Passive,
                })
                .collect(),
        }
    }

    /// A design of `components`, with every net their pins name.
    fn design(components: Vec<Component>) -> Design {
        let mut names: Vec<String> = components
            .iter()
            .flat_map(|component| component.pins.values().cloned())
            .collect();
        names.sort();
        names.dedup();
        Design {
            components,
            parts: BTreeMap::from([
                (id("R"), part(TWO_PINS)),
                (id("Q"), part(EBC)),
                (id("J"), part(TWO_PINS)),
            ]),
            nets: names
                .into_iter()
                .map(|name| Net {
                    name,
                    nodes: Vec::new(),
                })
                .collect(),
            rails: Vec::new(),
        }
    }

    fn element_lines(netlist: &Netlist) -> Vec<&str> {
        netlist
            .text
            .lines()
            .skip(1)
            .filter(|line| !line.starts_with('.') && !line.starts_with('+'))
            .collect()
    }

    #[test]
    fn the_first_line_is_a_title_not_a_part() {
        let design = design(vec![component("R1", "1k", "R", &["a", "0"])]);
        let netlist = design.netlist(&[]).expect("builds");
        assert_eq!(netlist.text.lines().next(), Some(TITLE));
        assert_eq!(element_lines(&netlist), ["R1 a 0 1e3"]);
    }

    #[test]
    fn parts_excluded_from_simulation_or_not_placed_are_left_out() {
        let mut excluded = component("R2", "2k", "R", &["a", "0"]);
        excluded.excluded_from_sim = true;
        let mut unplaced = component("R3", "3k", "R", &["a", "0"]);
        unplaced.dnp = true;
        let design = design(vec![
            component("R1", "1k", "R", &["a", "0"]),
            excluded,
            unplaced,
        ]);
        let netlist = design.netlist(&[]).expect("builds");
        assert_eq!(element_lines(&netlist), ["R1 a 0 1e3"]);
    }

    #[test]
    fn a_card_shared_by_several_parts_is_written_once() {
        let design = design(vec![
            component("Q1", "2N3904", "Q", &["e1", "b1", "c1"]),
            component("Q2", "2N3904", "Q", &["e2", "b2", "c2"]),
        ]);
        let netlist =
            design.netlist(&[ModelLibrary::bundled()]).expect("builds");
        let cards = netlist
            .text
            .lines()
            .filter(|line| line.starts_with(".model"))
            .count();
        assert_eq!(cards, 1);
        assert!(netlist.defaulted.is_empty());
    }

    #[test]
    fn parts_on_default_models_are_listed() {
        let mut defaulted = component("Q7", "BC547", "Q", &["e", "b", "c"]);
        defaulted.fields.insert("Sim.Device".into(), "NPN".into());
        let design = design(vec![defaulted]);
        let netlist = design.netlist(&[]).expect("builds");
        assert_eq!(netlist.defaulted, ["Q7"]);
    }

    #[test]
    fn reports_every_part_that_cannot_be_simulated_at_once() {
        let design = design(vec![
            component("J1", "Conn_01x02", "J", &["a", "0"]),
            component("R1", "1k", "R", &["a", "0"]),
            component("J2", "Conn_01x02", "J", &["b", "0"]),
        ]);
        let Err(NetlistError::Parts(errors)) = design.netlist(&[]) else {
            panic!("errors not collected");
        };
        assert_eq!(errors.len(), 2);
        let message = NetlistError::Parts(errors).to_string();
        assert!(message.starts_with("cannot simulate 2 parts:"), "{message}");
        assert!(
            message.contains("J1") && message.contains("J2"),
            "{message}"
        );
    }
}
