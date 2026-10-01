//! Resolving power rails from a design and declared project conditions.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use super::node::node_name;
use super::rail::{is_ground, is_spice_ground, stated_volts};
use super::{Design, Net, PinKind};
use crate::{AcSupply, DcSupply, TranSource};

/// Every power rail and how it is powered.
#[derive(Debug, Clone, PartialEq)]
pub struct Power {
    rails: Vec<Rail>,
}

impl Power {
    /// Every resolved rail, sorted by full net name.
    pub fn rails(&self) -> &[Rail] {
        &self.rails
    }

    /// Every combination of sourced rail voltages.
    pub fn corners(&self) -> impl Iterator<Item = Corner<'_>> {
        Corners::new(self)
    }
}

/// One combination of power-rail voltages.
#[derive(Debug, Clone, PartialEq)]
pub struct Corner<'a> {
    values: Vec<CornerValue<'a>>,
}

impl Corner<'_> {
    /// The SPICE node and voltage of every source kitest adds.
    pub fn voltages(&self) -> impl Iterator<Item = (&str, f64)> {
        self.values
            .iter()
            .map(|value| (value.node.as_ref(), value.volts))
    }

    /// Supplies for a DC operating point.
    pub fn dc_supplies(&self) -> Vec<DcSupply> {
        self.values
            .iter()
            .map(|value| DcSupply::new(value.node.as_ref(), value.volts))
            .collect()
    }

    /// Constant sources for a transient analysis.
    pub fn tran_sources(&self) -> Vec<TranSource> {
        self.values
            .iter()
            .map(|value| TranSource::dc(value.node.as_ref(), value.volts))
            .collect()
    }

    /// Biased supplies that inject no small-signal AC stimulus.
    pub fn ac_supplies(&self) -> Vec<AcSupply> {
        self.values
            .iter()
            .map(|value| {
                AcSupply::new(value.node.as_ref())
                    .bias(value.volts)
                    .magnitude(0.0)
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
struct CornerValue<'a> {
    node: Cow<'a, str>,
    volts: f64,
}

/// The Cartesian product of every sourced rail's voltage list.
struct Corners<'a> {
    power: &'a Power,
    indices: Vec<usize>,
    lengths: Vec<usize>,
    done: bool,
}

impl<'a> Corners<'a> {
    fn new(power: &'a Power) -> Self {
        let lengths: Vec<usize> = power
            .rails
            .iter()
            .filter_map(|rail| match &rail.kind {
                RailKind::Source { voltages, .. } => Some(voltages.len()),
                _ => None,
            })
            .collect();
        Self {
            power,
            indices: vec![0; lengths.len()],
            lengths,
            done: false,
        }
    }

    fn advance(&mut self) {
        for (index, length) in self.indices.iter_mut().zip(&self.lengths).rev()
        {
            *index += 1;
            if *index < *length {
                return;
            }
            *index = 0;
        }
        self.done = true;
    }
}

impl<'a> Iterator for Corners<'a> {
    type Item = Corner<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let mut chosen = self.indices.iter();
        let values = self
            .power
            .rails
            .iter()
            .filter_map(|rail| match &rail.kind {
                RailKind::Source { voltages, .. } => {
                    let index =
                        *chosen.next().expect("one index per sourced rail");
                    Some(CornerValue {
                        node: node_name(&rail.net),
                        volts: voltages[index],
                    })
                }
                RailKind::Ground => {
                    let node = node_name(&rail.net);
                    (!is_spice_ground(node.as_ref()))
                        .then_some(CornerValue { node, volts: 0.0 })
                }
                RailKind::Driven { .. } => None,
            })
            .collect();
        self.advance();
        Some(Corner { values })
    }
}

/// One power rail in a design.
#[derive(Debug, Clone, PartialEq)]
pub struct Rail {
    net: String,
    kind: RailKind,
}

impl Rail {
    /// The rail's full net name.
    pub fn net(&self) -> &str {
        &self.net
    }

    /// How the rail is powered.
    pub fn kind(&self) -> &RailKind {
        &self.kind
    }
}

/// How a rail is powered.
#[derive(Debug, Clone, PartialEq)]
pub enum RailKind {
    /// Pins that drive the rail, such as a regulator's power output.
    Driven { by: Vec<String> },
    /// Ideal sources kitest adds, one voltage per corner.
    Source {
        voltages: Vec<f64>,
        origin: VoltageOrigin,
    },
    /// A ground name tied to SPICE ground when needed.
    Ground,
}

/// Where a sourced rail's voltage came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoltageOrigin {
    Declared,
    Inferred,
}

impl Design {
    /// Resolve power against voltages declared by full net name.
    pub fn power(
        &self,
        declared: &BTreeMap<String, Vec<f64>>,
    ) -> Result<Power, SupplyError> {
        let nets: BTreeMap<&str, &Net> = self
            .nets
            .iter()
            .map(|net| (net.name.as_str(), net))
            .collect();
        let mut problems = Vec::new();
        let mut invalid_declared = BTreeSet::new();

        for (name, voltages) in declared {
            if voltages.is_empty() {
                invalid_declared.insert(name.as_str());
                problems
                    .push(SupplyProblem::EmptyVoltages { net: name.clone() });
            } else if voltages.iter().any(|volts| !volts.is_finite()) {
                invalid_declared.insert(name.as_str());
                problems.push(SupplyProblem::NonFiniteVoltage {
                    net: name.clone(),
                });
            }
            if !nets.contains_key(name.as_str()) {
                problems.push(SupplyProblem::UnknownNet {
                    net: name.clone(),
                    candidates: candidates(name, nets.keys().copied()),
                });
            }
        }

        let mut names: BTreeSet<&str> =
            self.rails.iter().map(String::as_str).collect();
        names.extend(declared.keys().filter_map(|name| {
            nets.contains_key(name.as_str()).then_some(name.as_str())
        }));
        let mut rails = Vec::new();
        for name in names {
            if invalid_declared.contains(name) {
                continue;
            }
            let Some(net) = nets.get(name).copied() else {
                problems.push(SupplyProblem::MissingNet {
                    net: name.to_owned(),
                });
                continue;
            };
            let driven = drivers(self, net);
            let kind = if !driven.is_empty() {
                if declared.contains_key(name) {
                    problems.push(SupplyProblem::DeclaredDriven {
                        net: name.to_owned(),
                        by: driven.join(", "),
                    });
                    continue;
                }
                RailKind::Driven { by: driven }
            } else if let Some(voltages) = declared.get(name) {
                RailKind::Source {
                    voltages: voltages.clone(),
                    origin: VoltageOrigin::Declared,
                }
            } else if is_ground(name) {
                RailKind::Ground
            } else if let Some(volts) = stated_volts(name) {
                RailKind::Source {
                    voltages: vec![volts],
                    origin: VoltageOrigin::Inferred,
                }
            } else {
                problems.push(SupplyProblem::Unresolved {
                    net: name.to_owned(),
                    connections: connections(self, net).join(", "),
                });
                continue;
            };
            rails.push(Rail {
                net: name.to_owned(),
                kind,
            });
        }

        if problems.is_empty() {
            Ok(Power { rails })
        } else {
            Err(SupplyError { problems })
        }
    }
}

/// Problems preventing the project's supplies from being resolved.
#[derive(Debug, thiserror::Error)]
#[error("{}", supply_message(.problems))]
pub struct SupplyError {
    pub problems: Vec<SupplyProblem>,
}

/// One problem with a power rail or a declared supply.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SupplyProblem {
    #[error("supply {net:?} names no net{candidates}")]
    UnknownNet { net: String, candidates: String },

    #[error("supply {net:?} lists no voltages")]
    EmptyVoltages { net: String },

    #[error("supply {net:?} contains a voltage that is not finite")]
    NonFiniteVoltage { net: String },

    #[error(
        "power rail {net:?} has no net in the KiCad export; reconnect its power symbol"
    )]
    MissingNet { net: String },

    #[error(
        "{net} is driven by {by}, but kitest.toml also declares its voltage; \
         remove {net:?} from [supplies]"
    )]
    DeclaredDriven { net: String, by: String },

    #[error(
        "rail {net} has no voltage; it feeds {connections}; add {net:?} = <volts> \
         under [supplies] in kitest.toml"
    )]
    Unresolved { net: String, connections: String },
}

fn drivers(design: &Design, net: &Net) -> Vec<String> {
    net.nodes
        .iter()
        .filter(|node| node.kind == PinKind::PowerOut)
        .filter_map(|node| {
            let component = design.component(&node.reference)?;
            (!component.excluded_from_sim && !component.dnp).then(|| {
                describe_pin(design, component.reference.as_str(), &node.pin)
            })
        })
        .collect()
}

fn connections(design: &Design, net: &Net) -> Vec<String> {
    net.nodes
        .iter()
        .filter_map(|node| {
            let component = design.component(&node.reference)?;
            (!component.excluded_from_sim && !component.dnp).then(|| {
                describe_pin(design, component.reference.as_str(), &node.pin)
            })
        })
        .collect()
}

fn describe_pin(design: &Design, reference: &str, pin: &str) -> String {
    let component = design.component(reference).expect("node owner exists");
    let part = design.part(component);
    let role = part
        .and_then(|part| part.pin(pin))
        .map(|pin| pin.name.as_str())
        .filter(|name| !name.is_empty());
    match role {
        Some(role) => format!("{reference} ({}) pin {role}", component.value),
        None => format!("{reference} ({}) pin {pin}", component.value),
    }
}

fn candidates<'a>(name: &str, nets: impl Iterator<Item = &'a str>) -> String {
    let leaf = name.rsplit('/').next().unwrap_or(name);
    let candidates: Vec<&str> = nets
        .filter(|net| {
            net.rsplit('/')
                .next()
                .is_some_and(|part| part.eq_ignore_ascii_case(leaf))
        })
        .collect();
    match candidates.as_slice() {
        [] => String::new(),
        values => format!("; did you mean {}?", quoted(values)),
    }
}

fn quoted(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| format!("{value:?}"))
        .collect::<Vec<_>>()
        .join(" or ")
}

fn supply_message(problems: &[SupplyProblem]) -> String {
    let noun = if problems.len() == 1 {
        "problem"
    } else {
        "problems"
    };
    let mut message =
        format!("cannot resolve supplies: {} {noun}:", problems.len());
    for problem in problems {
        message.push_str("\n  - ");
        message.push_str(&problem.to_string());
    }
    message
}

#[cfg(test)]
mod tests {
    use super::super::{Component, LibraryId, LibraryPart, LibraryPin, Node};
    use super::*;
    use crate::stimulus::{inject, inject_tran};

    struct DesignBuilder {
        design: Design,
    }

    impl DesignBuilder {
        fn new() -> Self {
            Self {
                design: Design {
                    components: Vec::new(),
                    parts: BTreeMap::new(),
                    nets: Vec::new(),
                    rails: Vec::new(),
                },
            }
        }

        fn part(
            mut self,
            reference: &str,
            value: &str,
            pin: &str,
            role: &str,
            net: &str,
            kind: PinKind,
        ) -> Self {
            let id = LibraryId {
                library: "lib".into(),
                part: reference.into(),
            };
            self.design.parts.insert(
                id.clone(),
                LibraryPart {
                    fields: BTreeMap::new(),
                    pins: vec![LibraryPin {
                        number: pin.into(),
                        name: role.into(),
                        kind,
                    }],
                },
            );
            self.design.components.push(Component {
                reference: reference.into(),
                value: value.into(),
                library: id,
                fields: BTreeMap::new(),
                pins: BTreeMap::from([(pin.into(), net.into())]),
                excluded_from_sim: false,
                dnp: false,
            });
            let node = Node {
                reference: reference.into(),
                pin: pin.into(),
                kind,
            };
            match self
                .design
                .nets
                .iter_mut()
                .find(|candidate| candidate.name == net)
            {
                Some(existing) => existing.nodes.push(node),
                None => self.design.nets.push(Net {
                    name: net.into(),
                    nodes: vec![node],
                }),
            }
            self
        }

        fn rail(mut self, net: &str) -> Self {
            self.design.rails.push(net.into());
            self
        }

        fn build(self) -> Design {
            self.design
        }
    }

    fn voltages(kind: &RailKind) -> Vec<f64> {
        match kind {
            RailKind::Source { voltages, .. } => voltages.clone(),
            _ => Vec::new(),
        }
    }

    fn kind<'a>(power: &'a Power, net: &str) -> &'a RailKind {
        power
            .rails()
            .iter()
            .find(|rail| rail.net() == net)
            .expect("rail present")
            .kind()
    }

    #[test]
    fn resolves_declared_inferred_ground_and_driven_rails() {
        let design = DesignBuilder::new()
            .part("R1", "47k", "1", "", "VCC", PinKind::Passive)
            .part("R2", "10k", "1", "", "+3V3", PinKind::Passive)
            .part("R3", "1k", "1", "", "AGND", PinKind::Passive)
            .part("U1", "REG", "1", "VO", "VOUT", PinKind::PowerOut)
            .rail("VCC")
            .rail("+3V3")
            .rail("AGND")
            .rail("VOUT")
            .build();
        let declared = BTreeMap::from([("VCC".into(), vec![3.0, 3.3])]);
        let power = design.power(&declared).expect("resolves");
        assert_eq!(voltages(kind(&power, "+3V3")), [3.3]);
        assert_eq!(voltages(kind(&power, "VCC")), [3.0, 3.3]);
        assert!(matches!(kind(&power, "AGND"), RailKind::Ground));
        assert!(matches!(kind(&power, "VOUT"), RailKind::Driven { .. }));
    }

    #[test]
    fn a_declaration_overrides_a_voltage_in_the_name() {
        let design = DesignBuilder::new()
            .part("R1", "1k", "1", "", "+5V", PinKind::Passive)
            .rail("+5V")
            .build();
        let power = design
            .power(&BTreeMap::from([("+5V".into(), vec![4.8])]))
            .expect("resolves");
        assert_eq!(voltages(kind(&power, "+5V")), [4.8]);
    }

    #[test]
    fn a_declared_driven_rail_is_an_error() {
        let design = DesignBuilder::new()
            .part("U1", "REG", "1", "VO", "VOUT", PinKind::PowerOut)
            .rail("VOUT")
            .build();
        let error = design
            .power(&BTreeMap::from([("VOUT".into(), vec![3.3])]))
            .unwrap_err();
        assert!(matches!(
            error.problems[0],
            SupplyProblem::DeclaredDriven { .. }
        ));
    }

    #[test]
    fn an_unknown_key_suggests_local_nets_with_the_same_name() {
        let design = DesignBuilder::new()
            .part("R1", "1k", "1", "", "/A/VCC", PinKind::Passive)
            .part("R2", "1k", "1", "", "/B/VCC", PinKind::Passive)
            .build();
        let error = design
            .power(&BTreeMap::from([("VCC".into(), vec![5.0])]))
            .unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("/A/VCC") && message.contains("/B/VCC"),
            "{message}"
        );
    }

    #[test]
    fn an_unresolved_rail_names_what_it_feeds_and_the_line_to_add() {
        let design = DesignBuilder::new()
            .part("R1", "47k", "1", "", "VCC", PinKind::Passive)
            .part("Q1", "2N3904", "3", "C", "VCC", PinKind::Passive)
            .rail("VCC")
            .build();
        let error = design.power(&BTreeMap::new()).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("R1 (47k) pin 1"), "{message}");
        assert!(message.contains("Q1 (2N3904) pin C"), "{message}");
        assert!(message.contains("\"VCC\" = <volts>"), "{message}");
    }

    #[test]
    fn reports_every_supply_problem_at_once() {
        let design = DesignBuilder::new()
            .part("R1", "1k", "1", "", "VCC", PinKind::Passive)
            .part("R2", "1k", "1", "", "VBUS", PinKind::Passive)
            .rail("VCC")
            .rail("VBUS")
            .build();
        let error = design
            .power(&BTreeMap::from([("NOPE".into(), vec![1.0])]))
            .unwrap_err();
        assert_eq!(error.problems.len(), 3);
        assert!(
            error
                .to_string()
                .starts_with("cannot resolve supplies: 3 problems:")
        );
    }

    #[test]
    fn corners_are_the_cartesian_product_of_sourced_rails() {
        let power = Power {
            rails: vec![
                Rail {
                    net: "A".into(),
                    kind: RailKind::Source {
                        voltages: vec![1.0, 2.0],
                        origin: VoltageOrigin::Declared,
                    },
                },
                Rail {
                    net: "B".into(),
                    kind: RailKind::Source {
                        voltages: vec![10.0, 20.0, 30.0],
                        origin: VoltageOrigin::Declared,
                    },
                },
            ],
        };
        let corners: Vec<Vec<_>> = power
            .corners()
            .map(|corner| {
                corner
                    .voltages()
                    .map(|(node, volts)| (node.to_owned(), volts))
                    .collect()
            })
            .collect();
        assert_eq!(
            corners,
            [
                vec![(String::from("A"), 1.0), (String::from("B"), 10.0)],
                vec![(String::from("A"), 1.0), (String::from("B"), 20.0)],
                vec![(String::from("A"), 1.0), (String::from("B"), 30.0)],
                vec![(String::from("A"), 2.0), (String::from("B"), 10.0)],
                vec![(String::from("A"), 2.0), (String::from("B"), 20.0)],
                vec![(String::from("A"), 2.0), (String::from("B"), 30.0)],
            ]
        );
    }

    #[test]
    fn one_corner_exists_when_kitest_adds_no_voltage_source() {
        let power = Power {
            rails: vec![Rail {
                net: "VOUT".into(),
                kind: RailKind::Driven {
                    by: vec!["U1 (REG) pin VO".into()],
                },
            }],
        };
        let corners: Vec<_> = power.corners().collect();
        assert_eq!(corners.len(), 1);
        assert_eq!(corners[0].voltages().count(), 0);
    }

    #[test]
    fn native_ground_is_skipped_and_other_grounds_are_tied_to_zero() {
        let power = Power {
            rails: vec![
                Rail {
                    net: "GND".into(),
                    kind: RailKind::Ground,
                },
                Rail {
                    net: "AGND".into(),
                    kind: RailKind::Ground,
                },
            ],
        };
        let corner = power.corners().next().expect("one corner");
        assert_eq!(corner.voltages().collect::<Vec<_>>(), [("AGND", 0.0)]);
    }

    #[test]
    fn corners_rewrite_nodes_and_convert_to_each_analysis_supply() {
        let power = Power {
            rails: vec![Rail {
                net: "Net-(PWR)".into(),
                kind: RailKind::Source {
                    voltages: vec![9.0],
                    origin: VoltageOrigin::Declared,
                },
            }],
        };
        let corner = power.corners().next().expect("one corner");
        assert_eq!(corner.voltages().collect::<Vec<_>>(), [("Net-_PWR_", 9.0)]);
        assert_eq!(
            inject("* t", &corner.dc_supplies()),
            "* t\nVkt1 Net-_PWR_ 0 9"
        );
        assert_eq!(
            inject_tran("* t", &corner.tran_sources(), 1e-9),
            "* t\nVkt1 Net-_PWR_ 0 dc 9"
        );
        assert_eq!(
            inject("* t", &corner.ac_supplies()),
            "* t\nVkt1 Net-_PWR_ 0 dc 9 ac 0"
        );
    }

    #[test]
    fn code_cannot_construct_empty_or_non_finite_supply_corners() {
        let design = DesignBuilder::new()
            .part("R1", "1k", "1", "", "VCC", PinKind::Passive)
            .rail("VCC")
            .build();
        for (values, expected) in [
            (Vec::new(), "lists no voltages"),
            (vec![f64::NAN], "not finite"),
        ] {
            let error = design
                .power(&BTreeMap::from([("VCC".into(), values)]))
                .unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
        }
    }
}
