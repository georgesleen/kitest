//! One placed part as a SPICE element line, with the model card it needs.

use std::borrow::Cow;
use std::collections::BTreeMap;

use super::netlist::NetlistError;
use super::pins::pin_roles;
use super::value::{self, Unit};
use super::{Component, LibraryPart};
use crate::{ModelEntry, ModelKind, ModelLibrary};

/// The field naming a part's device type, such as `NPN` or `R`.
const SIM_DEVICE: &str = "Sim.Device";
/// The field holding a part's model parameters.
const SIM_PARAMS: &str = "Sim.Params";
/// The fields naming a part's own model file and the model inside it.
const SIM_MODEL_FILE: [&str; 2] = ["Sim.Library", "Sim.Name"];
/// Prefix of the model name written for a part simulated on defaults.
const DEFAULT_MODEL_PREFIX: &str = "kitest_default_";
/// Element letter for a subcircuit instance.
const SUBCIRCUIT_LETTER: char = 'X';

/// A part as SPICE: its element line and the model card that line names.
#[derive(Debug, PartialEq)]
pub(crate) struct Element<'a> {
    pub(crate) line: String,
    pub(crate) card: Option<Cow<'a, str>>,
    /// True if the card is a default one rather than a real part model.
    pub(crate) defaulted: bool,
}

/// `component` as a SPICE element, with nets named by `nodes`.
pub(crate) fn element<'a>(
    component: &Component,
    part: &LibraryPart,
    libraries: &'a [ModelLibrary],
    nodes: &BTreeMap<&str, Cow<'_, str>>,
) -> Result<Element<'a>, NetlistError> {
    if SIM_MODEL_FILE
        .iter()
        .any(|field| component.fields.contains_key(*field))
    {
        return Err(NetlistError::ModelFile {
            reference: component.reference.clone(),
        });
    }
    let sim_device = component.fields.get(SIM_DEVICE).map(String::as_str);
    if let Some(passive) = Passive::of(component, sim_device) {
        return passive.element(component, part, nodes);
    }
    let entry = libraries
        .iter()
        .find_map(|library| library.find(&component.value));
    match entry {
        Some(entry) => modelled(component, part, entry, sim_device, nodes),
        None => defaulted(component, part, sim_device, nodes),
    }
}

/// A two-terminal part whose Value is its only parameter.
#[derive(Debug, Clone, Copy)]
enum Passive {
    Resistor,
    Capacitor,
    Inductor,
}

impl Passive {
    /// The passive `component` is, by `Sim.Device` or else reference prefix.
    fn of(component: &Component, sim_device: Option<&str>) -> Option<Self> {
        let letter = match sim_device {
            Some(device) => device,
            None => component
                .reference
                .split(|character: char| !character.is_ascii_alphabetic())
                .next()
                .unwrap_or_default(),
        };
        match letter.to_ascii_uppercase().as_str() {
            "R" => Some(Self::Resistor),
            "C" => Some(Self::Capacitor),
            "L" => Some(Self::Inductor),
            _ => None,
        }
    }

    fn letter(self) -> char {
        match self {
            Self::Resistor => 'R',
            Self::Capacitor => 'C',
            Self::Inductor => 'L',
        }
    }

    fn unit(self) -> Unit {
        match self {
            Self::Resistor => Unit::Ohm,
            Self::Capacitor => Unit::Farad,
            Self::Inductor => Unit::Henry,
        }
    }

    fn element<'a>(
        self,
        component: &Component,
        part: &LibraryPart,
        nodes: &BTreeMap<&str, Cow<'_, str>>,
    ) -> Result<Element<'a>, NetlistError> {
        if component.fields.contains_key(SIM_PARAMS) {
            return Err(NetlistError::PassiveParams {
                reference: component.reference.clone(),
            });
        }
        let [first, second] = part.pins.as_slice() else {
            return Err(NetlistError::PassivePins {
                reference: component.reference.clone(),
                count: part.pins.len(),
            });
        };
        let unit = self.unit();
        let value = value::parse(&component.value, unit).ok_or_else(|| {
            NetlistError::BadValue {
                reference: component.reference.clone(),
                value: component.value.clone(),
                expected: unit.describe(),
            }
        })?;
        Ok(Element {
            line: format!(
                "{} {} {} {value:e}",
                element_name(self.letter(), &component.reference),
                node(component, &first.number, nodes)?,
                node(component, &second.number, nodes)?,
            ),
            card: None,
            defaulted: false,
        })
    }
}

/// A SPICE device that takes a `.model` card.
#[derive(Debug)]
struct Device {
    /// The type as a `.model` card spells it, such as `npn`.
    model_type: &'static str,
    /// The `Sim.Device` values this type satisfies.
    sim_devices: &'static [&'static str],
    letter: char,
    /// Pin roles in the element line's node order.
    roles: &'static [&'static str],
    /// A role that, when no pin plays it, is tied to another role's pin.
    fallback: Option<(&'static str, &'static str)>,
}

/// Every device kitest writes element lines for.
const DEVICES: &[Device] = &[
    Device {
        model_type: "npn",
        sim_devices: &["NPN"],
        letter: 'Q',
        roles: &["C", "B", "E"],
        fallback: None,
    },
    Device {
        model_type: "pnp",
        sim_devices: &["PNP"],
        letter: 'Q',
        roles: &["C", "B", "E"],
        fallback: None,
    },
    Device {
        model_type: "d",
        sim_devices: &["D"],
        letter: 'D',
        roles: &["A", "K"],
        fallback: None,
    },
    Device {
        model_type: "njf",
        sim_devices: &["NJFET"],
        letter: 'J',
        roles: &["D", "G", "S"],
        fallback: None,
    },
    Device {
        model_type: "pjf",
        sim_devices: &["PJFET"],
        letter: 'J',
        roles: &["D", "G", "S"],
        fallback: None,
    },
    Device {
        model_type: "nmos",
        sim_devices: &["NMOS"],
        letter: 'M',
        roles: &["D", "G", "S", "B"],
        fallback: Some(("B", "S")),
    },
    Device {
        model_type: "pmos",
        sim_devices: &["PMOS"],
        letter: 'M',
        roles: &["D", "G", "S", "B"],
        fallback: Some(("B", "S")),
    },
    Device {
        model_type: "vdmos",
        sim_devices: &["NMOS", "PMOS"],
        letter: 'M',
        roles: &["D", "G", "S"],
        fallback: None,
    },
];

impl Device {
    fn by_model_type(model_type: &str) -> Option<&'static Self> {
        DEVICES
            .iter()
            .find(|device| device.model_type.eq_ignore_ascii_case(model_type))
    }

    /// The device a `Sim.Device` value defaults to.
    fn by_sim_device(sim_device: &str) -> Option<&'static Self> {
        DEVICES.iter().find(|device| {
            device.sim_devices[0].eq_ignore_ascii_case(sim_device)
                && device.model_type != "vdmos"
        })
    }

    fn satisfies(&self, sim_device: &str) -> bool {
        self.sim_devices
            .iter()
            .any(|name| name.eq_ignore_ascii_case(sim_device))
    }

    /// The nodes of `component` in this device's role order.
    fn nodes<'n>(
        &self,
        component: &Component,
        part: &LibraryPart,
        nodes: &'n BTreeMap<&str, Cow<'_, str>>,
    ) -> Result<Vec<&'n str>, NetlistError> {
        let by_role = pin_roles(component, part, self.roles)?;
        self.roles
            .iter()
            .map(|role| {
                let stand_in = self
                    .fallback
                    .filter(|(missing, _)| missing == role)
                    .and_then(|(_, other)| by_role.get(other));
                let pin = by_role
                    .get(role)
                    .or(stand_in)
                    .ok_or_else(|| missing_role(component, role, self.roles))?;
                node(component, pin, nodes)
            })
            .collect()
    }
}

/// `component` bound to the library `entry` covering its Value.
fn modelled<'a>(
    component: &Component,
    part: &LibraryPart,
    entry: &'a ModelEntry,
    sim_device: Option<&str>,
    nodes: &BTreeMap<&str, Cow<'_, str>>,
) -> Result<Element<'a>, NetlistError> {
    let (letter, ordered, model) = match &entry.kind {
        ModelKind::Model { name, device } => {
            let chosen = Device::by_model_type(device).ok_or_else(|| {
                NetlistError::UnsupportedDevice {
                    reference: component.reference.clone(),
                    device: device.clone(),
                }
            })?;
            if let Some(sim_device) = sim_device
                && Device::by_sim_device(sim_device).is_some()
                && !chosen.satisfies(sim_device)
            {
                return Err(NetlistError::ModelMismatch {
                    reference: component.reference.clone(),
                    sim_device: sim_device.to_owned(),
                    value: component.value.clone(),
                    model_type: device.clone(),
                });
            }
            (chosen.letter, chosen.nodes(component, part, nodes)?, name)
        }
        ModelKind::Subckt { name, ports } => {
            let roles: Vec<&str> = ports.iter().map(String::as_str).collect();
            let by_role = pin_roles(component, part, &roles)?;
            let ordered = roles
                .iter()
                .map(|role| {
                    let pin = by_role
                        .get(role)
                        .ok_or_else(|| missing_role(component, role, &roles))?;
                    node(component, pin, nodes)
                })
                .collect::<Result<Vec<_>, _>>()?;
            (SUBCIRCUIT_LETTER, ordered, name)
        }
    };
    Ok(Element {
        line: format!(
            "{} {} {model}",
            element_name(letter, &component.reference),
            ordered.join(" "),
        ),
        card: Some(Cow::Borrowed(&entry.card)),
        defaulted: false,
    })
}

/// `component` on the default model for its `Sim.Device`, if it has one.
fn defaulted<'a>(
    component: &Component,
    part: &LibraryPart,
    sim_device: Option<&str>,
    nodes: &BTreeMap<&str, Cow<'_, str>>,
) -> Result<Element<'a>, NetlistError> {
    let Some(sim_device) = sim_device else {
        return Err(NetlistError::NoModel {
            reference: component.reference.clone(),
            value: component.value.clone(),
        });
    };
    let device = Device::by_sim_device(sim_device).ok_or_else(|| {
        NetlistError::UnsupportedDevice {
            reference: component.reference.clone(),
            device: sim_device.to_owned(),
        }
    })?;
    let model = format!("{DEFAULT_MODEL_PREFIX}{}", component.reference);
    let parameters = component
        .fields
        .get(SIM_PARAMS)
        .map(|parameters| format!(" ({parameters})"))
        .unwrap_or_default();
    Ok(Element {
        line: format!(
            "{} {} {model}",
            element_name(device.letter, &component.reference),
            device.nodes(component, part, nodes)?.join(" "),
        ),
        card: Some(Cow::Owned(format!(
            ".model {model} {}{parameters}",
            device.model_type
        ))),
        defaulted: true,
    })
}

/// The element name for `reference`, prefixed with `letter` unless it starts so.
fn element_name(letter: char, reference: &str) -> String {
    if reference
        .chars()
        .next()
        .is_some_and(|first| first.eq_ignore_ascii_case(&letter))
    {
        reference.to_owned()
    } else {
        format!("{letter}{reference}")
    }
}

/// The SPICE node on pin `pin` of `component`.
fn node<'n>(
    component: &Component,
    pin: &str,
    nodes: &'n BTreeMap<&str, Cow<'_, str>>,
) -> Result<&'n str, NetlistError> {
    component
        .pins
        .get(pin)
        .and_then(|net| nodes.get(net.as_str()))
        .map(AsRef::as_ref)
        .ok_or_else(|| NetlistError::MissingPinNet {
            reference: component.reference.clone(),
            pin: pin.to_owned(),
        })
}

fn missing_role(
    component: &Component,
    role: &str,
    roles: &[&str],
) -> NetlistError {
    NetlistError::MissingRole {
        reference: component.reference.clone(),
        role: role.to_owned(),
        roles: roles.join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{LibraryId, LibraryPin, PinKind};
    use super::*;

    /// A placed part whose pins, by number, sit on nets named after them.
    fn component(
        reference: &str,
        value: &str,
        fields: &[(&str, &str)],
        pins: &[&str],
    ) -> Component {
        Component {
            reference: reference.into(),
            value: value.into(),
            library: LibraryId {
                library: "lib".into(),
                part: "part".into(),
            },
            fields: fields
                .iter()
                .map(|(name, text)| ((*name).to_owned(), (*text).to_owned()))
                .collect(),
            pins: pins
                .iter()
                .map(|pin| ((*pin).to_owned(), format!("n{pin}")))
                .collect(),
            excluded_from_sim: false,
            dnp: false,
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

    fn nodes(component: &Component) -> BTreeMap<&str, Cow<'_, str>> {
        component
            .pins
            .values()
            .map(|net| (net.as_str(), Cow::Borrowed(net.as_str())))
            .collect()
    }

    fn resolve<'a>(
        component: &Component,
        part: &LibraryPart,
        libraries: &'a [ModelLibrary],
    ) -> Result<Element<'a>, NetlistError> {
        element(component, part, libraries, &nodes(component))
    }

    const TWO_PINS: &[(&str, &str)] = &[("1", ""), ("2", "")];
    const EBC: &[(&str, &str)] = &[("1", "E"), ("2", "B"), ("3", "C")];

    #[test]
    fn a_resistor_is_its_value_in_ohms() {
        let resistor = component("R1", "4k7", &[], &["1", "2"]);
        let element =
            resolve(&resistor, &part(TWO_PINS), &[]).expect("resolves");
        assert_eq!(element.line, "R1 n1 n2 4.7e3");
        assert_eq!(element.card, None);
    }

    #[test]
    fn a_capacitor_takes_its_unit() {
        let capacitor = component("C1", "470pF", &[], &["1", "2"]);
        let element =
            resolve(&capacitor, &part(TWO_PINS), &[]).expect("resolves");
        assert_eq!(element.line, "C1 n1 n2 4.7e-10");
    }

    #[test]
    fn a_passive_value_kitest_cannot_read_names_the_part() {
        let resistor = component("R3", "10k 1%", &[], &["1", "2"]);
        let error = resolve(&resistor, &part(TWO_PINS), &[]).unwrap_err();
        let message = error.to_string();
        assert!(matches!(error, NetlistError::BadValue { .. }));
        assert!(
            message.contains("R3") && message.contains("10k 1%"),
            "{message}"
        );
    }

    #[test]
    fn a_resistor_network_prefix_is_not_a_resistor() {
        let network = component("RN1", "4x10k", &[], &["1", "2"]);
        assert!(matches!(
            resolve(&network, &part(TWO_PINS), &[]),
            Err(NetlistError::NoModel { .. })
        ));
    }

    #[test]
    fn a_bjt_is_written_collector_base_emitter_with_its_card() {
        let libraries = [ModelLibrary::bundled()];
        let transistor = component("Q1", "2N3904", &[], &["1", "2", "3"]);
        let element =
            resolve(&transistor, &part(EBC), &libraries).expect("resolves");
        assert_eq!(element.line, "Q1 n3 n2 n1 2N3904");
        assert!(element.card.expect("card").starts_with(".model 2N3904 npn"));
        assert!(!element.defaulted);
    }

    #[test]
    fn a_reference_is_prefixed_with_the_element_letter() {
        let libraries = [ModelLibrary::bundled()];
        let transistor = component("T1", "2N3904", &[], &["1", "2", "3"]);
        let element =
            resolve(&transistor, &part(EBC), &libraries).expect("resolves");
        assert!(element.line.starts_with("QT1 "), "{}", element.line);
    }

    #[test]
    fn the_first_library_covering_a_value_wins() {
        let project = ModelLibrary::parse(
            "[[model]]\nvalues = [\"2N3904\"]\ncard = \".model mine npn (bf=50)\"\n",
        )
        .expect("parses");
        let libraries = [project, ModelLibrary::bundled()];
        let transistor = component("Q1", "2N3904", &[], &["1", "2", "3"]);
        let element =
            resolve(&transistor, &part(EBC), &libraries).expect("resolves");
        assert!(element.line.ends_with(" mine"), "{}", element.line);
    }

    #[test]
    fn a_model_contradicting_sim_device_is_rejected() {
        let libraries = [ModelLibrary::bundled()];
        let transistor = component(
            "Q1",
            "2N3904",
            &[("Sim.Device", "PNP")],
            &["1", "2", "3"],
        );
        assert!(matches!(
            resolve(&transistor, &part(EBC), &libraries),
            Err(NetlistError::ModelMismatch { .. })
        ));
    }

    #[test]
    fn a_part_with_only_sim_device_runs_on_defaults_and_says_so() {
        let transistor = component(
            "Q2",
            "BC547",
            &[("Sim.Device", "NPN"), ("Sim.Params", "bf=200")],
            &["1", "2", "3"],
        );
        let element = resolve(&transistor, &part(EBC), &[]).expect("resolves");
        assert_eq!(element.line, "Q2 n3 n2 n1 kitest_default_Q2");
        assert_eq!(
            element.card.as_deref(),
            Some(".model kitest_default_Q2 npn (bf=200)")
        );
        assert!(element.defaulted);
    }

    #[test]
    fn a_three_pin_mosfet_ties_bulk_to_source() {
        let mosfet = component(
            "Q3",
            "2N7002",
            &[("Sim.Device", "NMOS")],
            &["1", "2", "3"],
        );
        let pins = [("1", "G"), ("2", "S"), ("3", "D")];
        let element = resolve(&mosfet, &part(&pins), &[]).expect("resolves");
        assert_eq!(element.line, "MQ3 n3 n1 n2 n2 kitest_default_Q3");
    }

    #[test]
    fn a_subcircuit_is_wired_in_its_port_order() {
        let library = ModelLibrary::parse(
            "[[model]]\nvalues = [\"ESD2\"]\nports = [\"IO\", \"GND\"]\n\
             card = \".subckt ESD2 a b\\nD1 b a dz\\n.ends\"\n",
        )
        .expect("parses");
        let protection = component("U1", "ESD2", &[], &["1", "2"]);
        let pins = [("1", "GND"), ("2", "IO")];
        let element =
            resolve(&protection, &part(&pins), std::slice::from_ref(&library))
                .expect("resolves");
        assert_eq!(element.line, "XU1 n2 n1 ESD2");
    }

    #[test]
    fn a_part_with_no_model_says_how_to_fix_it() {
        let connector = component("J1", "Conn_01x02", &[], &["1", "2"]);
        let error = resolve(&connector, &part(TWO_PINS), &[]).unwrap_err();
        let message = error.to_string();
        assert!(matches!(error, NetlistError::NoModel { .. }));
        assert!(
            message.contains("J1") && message.contains("exclude"),
            "{message}"
        );
    }

    #[test]
    fn a_model_file_on_the_part_is_rejected_for_now() {
        let transistor = component(
            "Q1",
            "2N3904",
            &[("Sim.Library", "q.lib")],
            &["1", "2", "3"],
        );
        assert!(matches!(
            resolve(&transistor, &part(EBC), &[ModelLibrary::bundled()]),
            Err(NetlistError::ModelFile { .. })
        ));
    }

    #[test]
    fn a_device_kitest_does_not_write_is_rejected() {
        let switch =
            component("S1", "SW", &[("Sim.Device", "SW")], &["1", "2"]);
        assert!(matches!(
            resolve(&switch, &part(TWO_PINS), &[]),
            Err(NetlistError::UnsupportedDevice { .. })
        ));
    }

    #[test]
    fn a_role_no_pin_plays_is_rejected() {
        let transistor = component("Q1", "2N3904", &[], &["1", "2"]);
        let pins = [("1", "E"), ("2", "B")];
        assert!(matches!(
            resolve(&transistor, &part(&pins), &[ModelLibrary::bundled()]),
            Err(NetlistError::MissingRole { role, .. }) if role == "C"
        ));
    }
}
