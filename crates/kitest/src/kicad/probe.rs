//! Probes: placed `kitest:Probe` symbols that mark a net to observe.

use std::collections::BTreeMap;

use super::{Component, Design, Net};

const PROBE_LIBRARY: &str = "kitest";
const PROBE_PART: &str = "Probe";
const PROBE_PIN: &str = "1";
const EXPECT_FIELD: &str = "Expect";

/// A probe placed on a net.
#[derive(Debug, Clone, PartialEq)]
pub struct Probe<'a> {
    name: &'a str,
    net: &'a Net,
    expect: Option<&'a str>,
    reference: &'a str,
}

impl<'a> Probe<'a> {
    /// The probe's Value, or its net's name when Value is empty.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// The net the probe sits on.
    pub fn net(&self) -> &'a Net {
        self.net
    }

    /// The probe's Expect field, if it has one.
    pub fn expect(&self) -> Option<&'a str> {
        self.expect
    }

    /// The probe's reference designator, such as `PRB1`.
    pub fn reference(&self) -> &'a str {
        self.reference
    }
}

/// Problems preventing the design's probes from being read.
#[derive(Debug, thiserror::Error)]
#[error("{}", probe_message(.problems))]
pub struct ProbeError {
    pub problems: Vec<ProbeProblem>,
}

/// One problem with a probe.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ProbeProblem {
    #[error("probe {reference} is not connected to anything")]
    Unconnected { reference: String },

    #[error(
        "probe {reference} is on net {net:?}, which is not in the KiCad export"
    )]
    MissingNet { reference: String, net: String },

    #[error(
        "probes {references} are all named {name:?}; give each a unique Value"
    )]
    DuplicateName { name: String, references: String },

    #[error("no probe is named {name:?}; {}", known_probes(.known))]
    UnknownName { name: String, known: Vec<String> },
}

/// Whether `component` is a `kitest:Probe` symbol.
fn is_probe(component: &Component) -> bool {
    component.library.library == PROBE_LIBRARY
        && component.library.part == PROBE_PART
}

impl Design {
    /// Every probe in the schematic, in component order; probes marked DNP
    /// are left out. Exclusion from simulation is ignored: the probe
    /// symbol is always excluded, because it is annotation, not circuit.
    pub fn probes(&self) -> Result<Vec<Probe<'_>>, ProbeError> {
        let mut probes = Vec::new();
        let mut problems = Vec::new();
        let placed = self
            .components
            .iter()
            .filter(|component| is_probe(component) && !component.dnp);
        for component in placed {
            match self.probe_from(component) {
                Ok(probe) => probes.push(probe),
                Err(problem) => problems.push(problem),
            }
        }

        let mut by_name: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for probe in &probes {
            by_name.entry(probe.name).or_default().push(probe.reference);
        }
        for (name, references) in by_name {
            if references.len() > 1 {
                problems.push(ProbeProblem::DuplicateName {
                    name: name.to_owned(),
                    references: references.join(", "),
                });
            }
        }

        if problems.is_empty() {
            Ok(probes)
        } else {
            Err(ProbeError { problems })
        }
    }

    /// Read the probe `component`, which must already satisfy `is_probe`.
    fn probe_from<'a>(
        &'a self,
        component: &'a Component,
    ) -> Result<Probe<'a>, ProbeProblem> {
        let unconnected = || ProbeProblem::Unconnected {
            reference: component.reference.clone(),
        };

        let net_name = component.pins.get(PROBE_PIN).ok_or_else(unconnected)?;
        let net =
            self.net(net_name).ok_or_else(|| ProbeProblem::MissingNet {
                reference: component.reference.clone(),
                net: net_name.clone(),
            })?;
        if !net
            .nodes
            .iter()
            .any(|node| node.reference != component.reference)
        {
            return Err(unconnected());
        }

        let name = if component.value.is_empty() {
            net.name.as_str()
        } else {
            component.value.as_str()
        };
        let expect = component
            .fields
            .get(EXPECT_FIELD)
            .map(String::as_str)
            .filter(|text| !text.is_empty());

        Ok(Probe {
            name,
            net,
            expect,
            reference: &component.reference,
        })
    }

    /// The probe named `name`.
    pub fn probe(&self, name: &str) -> Result<Probe<'_>, ProbeError> {
        let probes = self.probes()?;
        match probes.iter().find(|p| p.name == name) {
            Some(probe) => Ok(probe.clone()),
            None => {
                let known = probes.iter().map(|p| p.name.to_owned()).collect();
                Err(ProbeError {
                    problems: vec![ProbeProblem::UnknownName {
                        name: name.to_owned(),
                        known,
                    }],
                })
            }
        }
    }
}

fn known_probes(known: &[String]) -> String {
    if known.is_empty() {
        return "the design has no probes".to_owned();
    }
    let quoted: Vec<String> =
        known.iter().map(|name| format!("{name:?}")).collect();
    format!("probes are {}", quoted.join(", "))
}

fn probe_message(problems: &[ProbeProblem]) -> String {
    let noun = if problems.len() == 1 {
        "problem"
    } else {
        "problems"
    };
    let mut message = format!("cannot read probes: {} {noun}:", problems.len());
    for problem in problems {
        message.push_str("\n  - ");
        message.push_str(&problem.to_string());
    }
    message
}

#[cfg(test)]
mod tests {
    use super::super::{LibraryId, Node, PinKind};
    use super::*;

    fn component(
        reference: &str,
        library: &str,
        part: &str,
        value: &str,
        net: Option<&str>,
    ) -> Component {
        Component {
            reference: reference.into(),
            value: value.into(),
            library: LibraryId {
                library: library.into(),
                part: part.into(),
            },
            fields: BTreeMap::new(),
            pins: net
                .map(|net| BTreeMap::from([(PROBE_PIN.into(), net.into())]))
                .unwrap_or_default(),
            excluded_from_sim: false,
            dnp: false,
        }
    }

    fn probe(reference: &str, value: &str, net: Option<&str>) -> Component {
        component(reference, PROBE_LIBRARY, PROBE_PART, value, net)
    }

    /// A design holding `components`, with nets built from their pins.
    fn design(components: Vec<Component>) -> Design {
        let mut nets: Vec<Net> = Vec::new();
        for component in &components {
            for (pin, net) in &component.pins {
                let node = Node {
                    reference: component.reference.clone(),
                    pin: pin.clone(),
                    kind: PinKind::Passive,
                };
                match nets.iter_mut().find(|candidate| &candidate.name == net) {
                    Some(existing) => existing.nodes.push(node),
                    None => nets.push(Net {
                        name: net.clone(),
                        nodes: vec![node],
                    }),
                }
            }
        }
        Design {
            components,
            parts: BTreeMap::new(),
            nets,
            rails: Vec::new(),
        }
    }

    fn resistor(reference: &str, net: &str) -> Component {
        component(reference, "Device", "R", "1k", Some(net))
    }

    #[test]
    fn is_probe_needs_both_library_and_part() {
        assert!(is_probe(&probe("PRB1", "", None)));
        assert!(!is_probe(&component("R1", "Device", "R", "1k", None)));
        assert!(!is_probe(&component("X1", "Other", PROBE_PART, "", None)));
        assert!(!is_probe(&component("X2", PROBE_LIBRARY, "R", "", None)));
    }

    #[test]
    fn probe_reads_value_net_and_expect() {
        let mut named = probe("PRB1", "OSC", Some("/OUT"));
        named.fields.insert(EXPECT_FIELD.into(), "2.5V".into());
        let design = design(vec![resistor("R1", "/OUT"), named]);

        let probes = design.probes().expect("probes read");
        assert_eq!(probes.len(), 1);
        let probe = &probes[0];
        assert_eq!(probe.name(), "OSC");
        assert_eq!(probe.net().name, "/OUT");
        assert_eq!(probe.expect(), Some("2.5V"));
        assert_eq!(probe.reference(), "PRB1");
    }

    #[test]
    fn unnamed_probe_takes_net_name_and_empty_expect_is_none() {
        let mut unnamed = probe("PRB1", "", Some("/OUT"));
        unnamed.fields.insert(EXPECT_FIELD.into(), String::new());
        let design = design(vec![resistor("R1", "/OUT"), unnamed]);

        let probes = design.probes().expect("probes read");
        assert_eq!(probes[0].name(), "/OUT");
        assert_eq!(probes[0].expect(), None);
    }

    #[test]
    fn probe_alone_on_its_net_or_without_a_pin_is_unconnected() {
        let design = design(vec![
            probe("PRB1", "A", Some("/FLOATING")),
            probe("PRB2", "B", None),
        ]);

        let error = design.probes().expect_err("both unconnected");
        assert_eq!(
            error.problems,
            vec![
                ProbeProblem::Unconnected {
                    reference: "PRB1".into()
                },
                ProbeProblem::Unconnected {
                    reference: "PRB2".into()
                },
            ]
        );
    }

    #[test]
    fn probe_on_net_missing_from_export_is_reported() {
        let mut design = design(vec![resistor("R1", "/OUT")]);
        design.components.push(probe("PRB1", "A", Some("/GONE")));

        let error = design.probes().expect_err("net missing");
        assert_eq!(
            error.problems,
            vec![ProbeProblem::MissingNet {
                reference: "PRB1".into(),
                net: "/GONE".into(),
            }]
        );
    }

    #[test]
    fn duplicate_names_are_reported_including_net_fallback() {
        let design = design(vec![
            resistor("R1", "/OUT"),
            resistor("R2", "/IN"),
            probe("PRB1", "", Some("/OUT")),
            probe("PRB2", "/OUT", Some("/IN")),
            probe("PRB3", "IN", Some("/IN")),
        ]);

        let error = design.probes().expect_err("duplicate name");
        assert_eq!(
            error.problems,
            vec![ProbeProblem::DuplicateName {
                name: "/OUT".into(),
                references: "PRB1, PRB2".into(),
            }]
        );
    }

    #[test]
    fn probe_finds_by_name_or_lists_known_names() {
        let design = design(vec![
            resistor("R1", "/OUT"),
            probe("PRB1", "VOUT", Some("/OUT")),
            probe("PRB2", "", Some("/OUT")),
        ]);

        assert_eq!(design.probe("VOUT").expect("found").reference(), "PRB1");
        assert_eq!(design.probe("/OUT").expect("found").reference(), "PRB2");

        let error = design.probe("OSC").expect_err("missing");
        assert_eq!(
            error.problems,
            vec![ProbeProblem::UnknownName {
                name: "OSC".into(),
                known: vec!["VOUT".into(), "/OUT".into()],
            }]
        );
    }

    #[test]
    fn unknown_name_message_quotes_names_or_says_none_exist() {
        let some = ProbeProblem::UnknownName {
            name: "OSC".into(),
            known: vec!["VOUT".into(), "/OUT".into()],
        };
        assert_eq!(
            some.to_string(),
            r#"no probe is named "OSC"; probes are "VOUT", "/OUT""#
        );

        let error = design(vec![resistor("R1", "/OUT")])
            .probe("OSC")
            .expect_err("no probes");
        assert_eq!(
            error.problems[0].to_string(),
            r#"no probe is named "OSC"; the design has no probes"#
        );
    }

    #[test]
    fn probe_reports_broken_probes_before_searching() {
        let design = design(vec![
            resistor("R1", "/OUT"),
            probe("PRB1", "VOUT", Some("/OUT")),
            probe("PRB2", "B", None),
        ]);

        let error = design.probe("VOUT").expect_err("PRB2 is broken");
        assert_eq!(
            error.problems,
            vec![ProbeProblem::Unconnected {
                reference: "PRB2".into()
            }]
        );
    }

    #[test]
    fn probes_excluded_from_simulation_are_read_and_dnp_left_out() {
        let mut excluded = probe("PRB1", "VOUT", Some("/OUT"));
        excluded.excluded_from_sim = true;
        let mut unplaced = probe("PRB2", "VOUT", None);
        unplaced.dnp = true;
        let design = design(vec![resistor("R1", "/OUT"), excluded, unplaced]);

        let probes = design.probes().expect("DNP probe causes no errors");
        assert_eq!(probes.len(), 1);
        assert_eq!(probes[0].reference(), "PRB1");
    }
}
