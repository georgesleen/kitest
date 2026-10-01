//! SPICE node names for KiCad nets.

use std::borrow::Cow;
use std::collections::BTreeMap;

use super::Net;
use super::netlist::NetlistError;

/// Characters other than whitespace that ngspice rejects in a node name.
const REJECTED: &[char] = &['(', ')', '{', '}', '=', ',', '\'', '"', ';'];

/// What each rejected character becomes, as in KiCad's own SPICE export.
const REPLACEMENT: &str = "_";

/// `name` with every character ngspice rejects replaced.
pub(crate) fn node_name(name: &str) -> Cow<'_, str> {
    if name.contains(is_rejected) {
        Cow::Owned(name.replace(is_rejected, REPLACEMENT))
    } else {
        Cow::Borrowed(name)
    }
}

/// The node name of each net, by net name.
pub(crate) fn node_names(
    nets: &[Net],
) -> Result<BTreeMap<&str, Cow<'_, str>>, NetlistError> {
    let mut owners: BTreeMap<String, &str> = BTreeMap::new();
    let mut names = BTreeMap::new();
    for net in nets {
        let node = node_name(&net.name);
        if let Some(first) = owners.insert(node.to_lowercase(), &net.name) {
            return Err(NetlistError::NodeClash {
                first: first.to_owned(),
                second: net.name.clone(),
                node: node.into_owned(),
            });
        }
        names.insert(net.name.as_str(), node);
    }
    Ok(names)
}

fn is_rejected(character: char) -> bool {
    character.is_whitespace() || REJECTED.contains(&character)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nets(names: &[&str]) -> Vec<Net> {
        names
            .iter()
            .map(|name| Net {
                name: (*name).to_owned(),
                nodes: Vec::new(),
            })
            .collect()
    }

    #[test]
    fn replaces_the_characters_ngspice_rejects() {
        assert_eq!(node_name("Net-(Q1-B)"), "Net-_Q1-B_");
        assert_eq!(node_name("clock in"), "clock_in");
        assert_eq!(node_name("x=1,y;{z}'\""), "x_1_y__z___");
    }

    #[test]
    fn keeps_hierarchical_names_without_copying() {
        assert!(matches!(node_name("/A/OUT"), Cow::Borrowed("/A/OUT")));
        assert!(matches!(node_name("+3V3"), Cow::Borrowed("+3V3")));
    }

    #[test]
    fn maps_each_net_to_its_node() {
        let nets = nets(&["/OUT", "Net-(Q1-B)", "GND"]);
        let names = node_names(&nets).expect("no clash");
        assert_eq!(names["/OUT"], "/OUT");
        assert_eq!(names["Net-(Q1-B)"], "Net-_Q1-B_");
        assert_eq!(names["GND"], "GND");
    }

    #[test]
    fn rejects_two_nets_that_become_one_node() {
        let nets = nets(&["Net-(A)", "Net-_A_"]);
        let Err(NetlistError::NodeClash {
            first,
            second,
            node,
        }) = node_names(&nets)
        else {
            panic!("clash not reported");
        };
        assert_eq!((first.as_str(), second.as_str()), ("Net-(A)", "Net-_A_"));
        assert_eq!(node, "Net-_A_");
    }

    #[test]
    fn rejects_nets_differing_only_in_case() {
        let nets = nets(&["/OUT", "/out"]);
        assert!(matches!(
            node_names(&nets),
            Err(NetlistError::NodeClash { .. })
        ));
    }
}
