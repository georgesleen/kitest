//! Which pin of a part plays each role in its model, such as a BJT's collector.

use std::collections::{BTreeMap, BTreeSet};

use super::netlist::NetlistError;
use super::{Component, LibraryPart};

/// The field mapping symbol pins to model roles, as in `1=E 2=B 3=C`.
const SIM_PINS: &str = "Sim.Pins";

/// The pin number playing each of `roles` on `component`, by role.
///
/// The placed part's `Sim.Pins` wins outright. Otherwise the library pin
/// names give roles, and the library's `Sim.Pins` must agree with them and
/// fills any pin whose name is not a role. A role no pin plays is absent.
pub(crate) fn pin_roles<'a>(
    component: &'a Component,
    part: &'a LibraryPart,
    roles: &[&'a str],
) -> Result<BTreeMap<&'a str, &'a str>, NetlistError> {
    let mut assigned = BTreeMap::new();

    if let Some(text) = component.fields.get(SIM_PINS) {
        for (pin, role) in sim_pins(component, part, text, roles)? {
            assign(&mut assigned, component, role, pin)?;
        }
        return Ok(assigned);
    }

    let listed: BTreeMap<&str, &str> = match part.fields.get(SIM_PINS) {
        Some(text) => sim_pins(component, part, text, roles)?
            .into_iter()
            .collect(),
        None => BTreeMap::new(),
    };
    for pin in &part.pins {
        let named = canonical(&pin.name, roles);
        let role = match (named, listed.get(pin.number.as_str()).copied()) {
            (Some(named), Some(listed)) if named != listed => {
                return Err(NetlistError::PinConflict {
                    reference: component.reference.clone(),
                    pin: pin.number.clone(),
                    name: pin.name.clone(),
                    role: listed.to_owned(),
                });
            }
            (Some(role), _) | (None, Some(role)) => role,
            (None, None) => continue,
        };
        assign(&mut assigned, component, role, &pin.number)?;
    }
    Ok(assigned)
}

/// The `(pin, role)` pairs a `Sim.Pins` field lists, each pin once.
fn sim_pins<'a>(
    component: &Component,
    part: &LibraryPart,
    text: &'a str,
    roles: &[&'a str],
) -> Result<Vec<(&'a str, &'a str)>, NetlistError> {
    let malformed = || NetlistError::SimPins {
        reference: component.reference.clone(),
        text: text.to_owned(),
    };
    let mut seen = BTreeSet::new();
    let mut pairs = Vec::new();
    for token in text.split_whitespace() {
        let (pin, role_text) = token.split_once('=').ok_or_else(malformed)?;
        if pin.is_empty() || !seen.insert(pin) {
            return Err(malformed());
        }
        if part.pin(pin).is_none() {
            return Err(NetlistError::UnknownPin {
                reference: component.reference.clone(),
                pin: pin.to_owned(),
            });
        }
        let role = canonical(role_text, roles).ok_or_else(|| {
            NetlistError::UnknownRole {
                reference: component.reference.clone(),
                role: role_text.to_owned(),
                roles: roles.join(" "),
            }
        })?;
        pairs.push((pin, role));
    }
    Ok(pairs)
}

/// The role in `roles` that `name` spells, ignoring ASCII case.
fn canonical<'a>(name: &str, roles: &[&'a str]) -> Option<&'a str> {
    roles
        .iter()
        .find(|role| role.eq_ignore_ascii_case(name))
        .copied()
}

fn assign<'a>(
    assigned: &mut BTreeMap<&'a str, &'a str>,
    component: &Component,
    role: &'a str,
    pin: &'a str,
) -> Result<(), NetlistError> {
    match assigned.insert(role, pin) {
        Some(_) => Err(NetlistError::DuplicateRole {
            reference: component.reference.clone(),
            role: role.to_owned(),
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{LibraryId, LibraryPin, PinKind};
    use super::*;

    const BJT: &[&str] = &["C", "B", "E"];

    fn part(pins: &[(&str, &str)], sim_pins: Option<&str>) -> LibraryPart {
        LibraryPart {
            fields: sim_pins
                .map(|text| (SIM_PINS.to_owned(), text.to_owned()))
                .into_iter()
                .collect(),
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

    fn placed(sim_pins: Option<&str>) -> Component {
        Component {
            reference: "Q1".into(),
            value: "2N3904".into(),
            library: LibraryId {
                library: "Transistor_BJT".into(),
                part: "2N3904".into(),
            },
            fields: sim_pins
                .map(|text| (SIM_PINS.to_owned(), text.to_owned()))
                .into_iter()
                .collect(),
            pins: BTreeMap::new(),
            excluded_from_sim: false,
            dnp: false,
        }
    }

    fn roles<'a>(
        component: &'a Component,
        part: &'a LibraryPart,
    ) -> Result<BTreeMap<&'a str, &'a str>, NetlistError> {
        pin_roles(component, part, BJT)
    }

    const EBC: &[(&str, &str)] = &[("1", "E"), ("2", "B"), ("3", "C")];

    #[test]
    fn library_pin_names_give_the_roles() {
        let (component, part) = (placed(None), part(EBC, None));
        let roles = roles(&component, &part).expect("resolves");
        assert_eq!(roles, BTreeMap::from([("C", "3"), ("B", "2"), ("E", "1")]));
    }

    #[test]
    fn pin_names_match_roles_ignoring_case() {
        let pins = [("1", "e"), ("2", "b"), ("3", "c")];
        let (component, part) = (placed(None), part(&pins, None));
        assert_eq!(roles(&component, &part).expect("resolves")["C"], "3");
    }

    #[test]
    fn the_placed_parts_sim_pins_win_over_pin_names() {
        let component = placed(Some("1=C 2=B 3=E"));
        let part = part(EBC, Some("1=E 2=B 3=C"));
        let roles = roles(&component, &part).expect("resolves");
        assert_eq!((roles["C"], roles["E"]), ("1", "3"));
    }

    #[test]
    fn the_librarys_sim_pins_fill_pins_whose_names_are_not_roles() {
        let (component, part) = (
            placed(None),
            part(&[("1", ""), ("2", "~")], Some("1=A 2=K")),
        );
        let roles =
            pin_roles(&component, &part, &["A", "K"]).expect("resolves");
        assert_eq!(roles, BTreeMap::from([("A", "1"), ("K", "2")]));
    }

    #[test]
    fn a_role_no_pin_plays_is_absent() {
        let (component, part) = (placed(None), part(&[("1", "C")], None));
        let roles = roles(&component, &part).expect("resolves");
        assert_eq!(roles, BTreeMap::from([("C", "1")]));
    }

    #[test]
    fn rejects_library_sim_pins_that_contradict_the_pin_names() {
        let (component, part) = (placed(None), part(EBC, Some("1=C 2=B 3=E")));
        let Err(NetlistError::PinConflict {
            pin, name, role, ..
        }) = roles(&component, &part)
        else {
            panic!("conflict not reported");
        };
        assert_eq!(
            (pin.as_str(), name.as_str(), role.as_str()),
            ("1", "E", "C")
        );
    }

    #[test]
    fn rejects_a_role_the_model_does_not_have() {
        let (component, part) = (placed(Some("1=E 2=B 3=X")), part(EBC, None));
        assert!(matches!(
            roles(&component, &part),
            Err(NetlistError::UnknownRole { role, .. }) if role == "X"
        ));
    }

    #[test]
    fn rejects_a_pin_the_symbol_does_not_have() {
        let (component, part) = (placed(Some("1=E 2=B 9=C")), part(EBC, None));
        assert!(matches!(
            roles(&component, &part),
            Err(NetlistError::UnknownPin { pin, .. }) if pin == "9"
        ));
    }

    #[test]
    fn rejects_sim_pins_that_are_not_pin_role_pairs() {
        for text in ["1E 2=B 3=C", "1=E 1=B 3=C", "=E 2=B"] {
            let (component, part) = (placed(Some(text)), part(EBC, None));
            assert!(
                matches!(
                    roles(&component, &part),
                    Err(NetlistError::SimPins { .. })
                ),
                "{text:?}"
            );
        }
    }

    #[test]
    fn rejects_two_pins_playing_one_role() {
        let (component, part) = (placed(Some("1=C 2=B 3=C")), part(EBC, None));
        assert!(matches!(
            roles(&component, &part),
            Err(NetlistError::DuplicateRole { role, .. }) if role == "C"
        ));
    }
}
