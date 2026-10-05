//! Parsing KiCad's `kicadxml` netlist export.

use std::collections::BTreeMap;

use roxmltree::{Document, Node as XmlNode};

use super::KicadError;
use super::design::{
    Component, LibraryId, LibraryPart, LibraryPin, Net, Node, PinKind,
};

/// What the `kicadxml` export holds: everything but rails and sim flags.
#[derive(Debug)]
pub(crate) struct Export {
    pub(crate) components: Vec<Component>,
    pub(crate) parts: BTreeMap<LibraryId, LibraryPart>,
    pub(crate) nets: Vec<Net>,
}

/// Parse a `kicadxml` document.
pub(crate) fn parse(xml: &str) -> Result<Export, KicadError> {
    let document = Document::parse(xml).map_err(KicadError::Xml)?;
    let export = document.root_element();

    let nets = section(export, "nets")
        .map(|nets| {
            elements(nets, "net")
                .map(net)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();

    let mut components = section(export, "components")
        .map(|comps| {
            elements(comps, "comp")
                .map(component)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();

    for net in &nets {
        for node in &net.nodes {
            let owner = components
                .iter_mut()
                .find(|component| component.reference == node.reference)
                .ok_or_else(|| {
                    malformed(format!(
                        "net {} names unknown part {}",
                        net.name, node.reference
                    ))
                })?;
            owner.pins.insert(node.pin.clone(), net.name.clone());
        }
    }

    let parts = section(export, "libparts")
        .map(|libparts| {
            elements(libparts, "libpart")
                .map(library_part)
                .collect::<Result<BTreeMap<_, _>, _>>()
        })
        .transpose()?
        .unwrap_or_default();

    Ok(Export {
        components,
        parts,
        nets,
    })
}

fn component(comp: XmlNode) -> Result<Component, KicadError> {
    let reference = attribute(comp, "ref")?.to_owned();
    let source = section(comp, "libsource").ok_or_else(|| {
        malformed(format!("part {reference} has no libsource"))
    })?;
    Ok(Component {
        value: section(comp, "value")
            .and_then(|value| value.text())
            .unwrap_or_default()
            .to_owned(),
        library: LibraryId {
            library: attribute(source, "lib")?.to_owned(),
            part: attribute(source, "part")?.to_owned(),
        },
        fields: fields(comp)?,
        pins: BTreeMap::new(),
        excluded_from_sim: false,
        dnp: false,
        reference,
    })
}

fn library_part(
    libpart: XmlNode,
) -> Result<(LibraryId, LibraryPart), KicadError> {
    let id = LibraryId {
        library: attribute(libpart, "lib")?.to_owned(),
        part: attribute(libpart, "part")?.to_owned(),
    };
    let pins = section(libpart, "pins")
        .map(|pins| {
            elements(pins, "pin")
                .map(|pin| {
                    Ok(LibraryPin {
                        number: attribute(pin, "num")?.to_owned(),
                        name: attribute(pin, "name")?.to_owned(),
                        kind: pin_kind(attribute(pin, "type")?)?,
                    })
                })
                .collect::<Result<Vec<_>, KicadError>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok((
        id,
        LibraryPart {
            fields: fields(libpart)?,
            pins,
        },
    ))
}

fn net(net: XmlNode) -> Result<Net, KicadError> {
    Ok(Net {
        name: attribute(net, "name")?.to_owned(),
        nodes: elements(net, "node")
            .map(|node| {
                Ok(Node {
                    reference: attribute(node, "ref")?.to_owned(),
                    pin: attribute(node, "pin")?.to_owned(),
                    kind: pin_kind(attribute(node, "pintype")?)?,
                })
            })
            .collect::<Result<_, KicadError>>()?,
    })
}

/// The `<fields>` of a part, by field name.
fn fields(parent: XmlNode) -> Result<BTreeMap<String, String>, KicadError> {
    section(parent, "fields")
        .map(|fields| {
            elements(fields, "field")
                .map(|field| {
                    Ok((
                        attribute(field, "name")?.to_owned(),
                        field.text().unwrap_or_default().to_owned(),
                    ))
                })
                .collect()
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

fn pin_kind(name: &str) -> Result<PinKind, KicadError> {
    PinKind::parse(name)
        .ok_or_else(|| malformed(format!("unknown pin type {name:?}")))
}

fn section<'a>(parent: XmlNode<'a, 'a>, name: &str) -> Option<XmlNode<'a, 'a>> {
    parent.children().find(|child| child.has_tag_name(name))
}

fn elements<'a>(
    parent: XmlNode<'a, 'a>,
    name: &'a str,
) -> impl Iterator<Item = XmlNode<'a, 'a>> + 'a {
    parent
        .children()
        .filter(move |child| child.has_tag_name(name))
}

fn attribute<'a>(
    element: XmlNode<'a, '_>,
    name: &str,
) -> Result<&'a str, KicadError> {
    element.attribute(name).ok_or_else(|| {
        malformed(format!(
            "<{}> has no {name} attribute",
            element.tag_name().name()
        ))
    })
}

fn malformed(message: String) -> KicadError {
    KicadError::MalformedExport(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<export version="E">
  <components>
    <comp ref="Q1">
      <value>2N3904</value>
      <fields>
        <field name="Footprint">Package_TO_SOT_THT:TO-92_Inline</field>
        <field name="Datasheet"/>
      </fields>
      <libsource lib="Transistor_BJT" part="2N3904" description="NPN"/>
    </comp>
    <comp ref="PRB1">
      <value>Probe</value>
      <fields>
        <field name="Expect">2.5V</field>
      </fields>
      <libsource lib="kitest" part="Probe"/>
    </comp>
  </components>
  <libparts>
    <libpart lib="Transistor_BJT" part="2N3904">
      <fields>
        <field name="Sim.Device">NPN</field>
        <field name="Sim.Pins">1=E 2=B 3=C</field>
      </fields>
      <pins>
        <pin num="1" name="E" type="passive"/>
        <pin num="2" name="B" type="input"/>
        <pin num="3" name="C" type="passive"/>
      </pins>
    </libpart>
  </libparts>
  <nets>
    <net code="1" name="/OUT" class="Default">
      <node ref="Q1" pin="1" pinfunction="E_1" pintype="passive"/>
      <node ref="PRB1" pin="1" pintype="passive"/>
    </net>
    <net code="2" name="Net-(Q1-B)" class="Default">
      <node ref="Q1" pin="2" pinfunction="B_2" pintype="input"/>
    </net>
    <net code="3" name="VCC" class="Default">
      <node ref="Q1" pin="3" pinfunction="C_3" pintype="passive"/>
    </net>
  </nets>
</export>"#;

    #[test]
    fn maps_each_pin_to_its_net() {
        let netlist = parse(EXPORT).expect("parses");
        let q1 = &netlist.components[0];
        assert_eq!(q1.reference, "Q1");
        assert_eq!(q1.value, "2N3904");
        assert_eq!(q1.pins["1"], "/OUT");
        assert_eq!(q1.pins["2"], "Net-(Q1-B)");
        assert_eq!(q1.pins["3"], "VCC");
    }

    #[test]
    fn keeps_custom_fields_on_the_placed_part() {
        let netlist = parse(EXPORT).expect("parses");
        let probe = &netlist.components[1];
        assert_eq!(probe.library.library, "kitest");
        assert_eq!(probe.library.part, "Probe");
        assert_eq!(probe.fields["Expect"], "2.5V");
    }

    #[test]
    fn reads_library_pins_and_inherited_sim_fields() {
        let netlist = parse(EXPORT).expect("parses");
        let part = &netlist.parts[&LibraryId {
            library: "Transistor_BJT".into(),
            part: "2N3904".into(),
        }];
        assert_eq!(part.fields["Sim.Pins"], "1=E 2=B 3=C");
        let names: Vec<_> =
            part.pins.iter().map(|pin| pin.name.as_str()).collect();
        assert_eq!(names, ["E", "B", "C"]);
        assert_eq!(part.pins[1].kind, PinKind::Input);
    }

    #[test]
    fn rejects_a_node_on_an_unknown_part() {
        let broken =
            EXPORT.replace(r#"ref="PRB1" pin="1""#, r#"ref="X9" pin="1""#);
        let error = parse(&broken).unwrap_err();
        assert!(error.to_string().contains("X9"), "{error}");
    }

    #[test]
    fn rejects_an_unknown_pin_type() {
        let broken =
            EXPORT.replace(r#"pintype="input""#, r#"pintype="sideways""#);
        let error = parse(&broken).unwrap_err();
        assert!(error.to_string().contains("sideways"), "{error}");
    }
}
