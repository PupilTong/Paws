use markup5ever::{LocalName, QualName};
use std::collections::{HashMap, HashSet};
use style::properties::PropertyDeclarationBlock;
use style::servo_arc::Arc;
use style::shared_lock::{Locked, SharedRwLock};
use stylo_atoms::Atom;

#[derive(Debug, Clone)]
pub struct ElementData {
    /// The elements tag name, namespace and prefix
    pub name: QualName,

    /// The elements id attribute parsed as an atom (if it has one)
    pub id: Option<Atom>,

    /// The element's attributes
    pub attrs: HashMap<Atom, String>,

    /// The element's parsed style attribute (used by stylo)
    pub style_attribute: Option<Arc<Locked<PropertyDeclarationBlock>>>,

    /// Classes for fast lookup
    pub classes: HashSet<Atom>,

    /// Heterogeneous data that depends on the element's type.
    pub special_data: SpecialElementData,
}

#[derive(Clone, Default, Debug)]
pub enum SpecialElementData {
    /// Parley text editor (text inputs)
    TextInput,
    /// No data
    #[default]
    None,
}

impl ElementData {
    pub fn new(name: QualName, attrs: HashMap<Atom, String>) -> Self {
        let id = attrs
            .get(&Atom::from("id"))
            .map(|val| Atom::from(val.as_str()));

        let classes = if let Some(class_attr) = attrs.get(&Atom::from("class")) {
            class_attr.split_whitespace().map(Atom::from).collect()
        } else {
            HashSet::new()
        };

        // Determine special data based on tag
        let special_data = if name.local.as_ref() == "input" {
            SpecialElementData::TextInput
        } else {
            SpecialElementData::None
        };

        ElementData {
            name,
            id,
            attrs,
            style_attribute: None,
            classes,
            special_data,
        }
    }

    pub fn set_attribute(&mut self, name: &str, value: &str) {
        let atom_name = Atom::from(name);
        self.attrs.insert(atom_name.clone(), value.to_string());

        if name == "id" {
            self.id = Some(Atom::from(value));
        }

        if name == "class" {
            self.classes.clear();
            for class in value.split_whitespace() {
                self.classes.insert(Atom::from(class));
            }
        }
    }

    pub fn has_class(&self, name: &Atom) -> bool {
        self.classes.contains(name)
    }
}
