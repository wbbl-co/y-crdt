use crate::collection::SharedCollection;
use crate::js::{Js, Shared};
use crate::transaction::YTransaction;
use crate::xml_frag::YXmlEvent;
use crate::{ImplicitTransaction, Observer};
use gloo_utils::format::JsValueSerdeExt;
use std::collections::HashMap;
use std::iter::FromIterator;
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::JsValue;
use yrs::types::TYPE_REFS_XML_ELEMENT;
use yrs::{DeepObservable, GetString, Observable, Xml, XmlElementRef, XmlFragment};

pub(crate) struct PrelimXmElement {
    pub name: String,
    pub attributes: HashMap<String, String>,
    pub children: Vec<JsValue>,
}

impl PrelimXmElement {
    fn to_string(&self, txn: &ImplicitTransaction) -> crate::Result<String> {
        let mut str = String::new();
        for js in self.children.iter() {
            let res = match Shared::from_ref(js)? {
                Shared::XmlText(c) => c.to_string(txn),
                Shared::XmlElement(c) => c.to_string(txn),
                Shared::XmlFragment(c) => c.to_string(txn),
                _ => return Err(JsValue::from_str(crate::js::errors::NOT_XML_TYPE)),
            };
            str.push_str(&res?);
        }
        Ok(str)
    }
}

/// XML element data type. It represents an XML node, which can contain key-value attributes
/// (interpreted as strings) as well as other nested XML elements or rich text (represented by
/// `YXmlText` type).
///
/// In terms of conflict resolution, `YXmlElement` uses following rules:
///
/// - Attribute updates use logical last-write-wins principle, meaning the past updates are
///   automatically overridden and discarded by newer ones, while concurrent updates made by
///   different peers are resolved into a single value using document id seniority to establish
///   an order.
/// - Child node insertion uses sequencing rules from other Yrs collections - elements are inserted
///   using interleave-resistant algorithm, where order of concurrent inserts at the same index
///   is established using peer's document id seniority.
#[wasm_bindgen]
pub struct YXmlElement(pub(crate) SharedCollection<PrelimXmElement, XmlElementRef>);

impl YXmlElement {
    pub(crate) fn try_parse_attrs(attributes: JsValue) -> Option<HashMap<String, String>> {
        let mut map = HashMap::new();
        if !attributes.is_undefined() {
            let object = js_sys::Object::from(attributes);
            let entries = js_sys::Object::entries(&object);
            for tuple in entries.iter() {
                let tuple = js_sys::Array::from(&tuple);
                let key: String = tuple.get(0).as_string()?;
                let value = tuple.get(1).as_string()?;
                map.insert(key, value);
            }
        }
        Some(map)
    }

    pub(crate) fn parse_attrs(attributes: JsValue) -> crate::Result<HashMap<String, String>> {
        match Self::try_parse_attrs(attributes) {
            Some(attrs) => Ok(attrs),
            None => Err(JsValue::from_str(crate::js::errors::INVALID_XML_ATTRS)),
        }
    }
}

#[wasm_bindgen]
impl YXmlElement {
    #[wasm_bindgen(constructor)]
    pub fn new(
        name: String,
        attributes: JsValue,
        children: Vec<JsValue>,
    ) -> crate::Result<YXmlElement> {
        let attributes = Self::parse_attrs(attributes)?;
        for child in children.iter() {
            Js::assert_xml_prelim(child)?;
        }
        Ok(YXmlElement(SharedCollection::prelim(PrelimXmElement {
            name,
            attributes,
            children,
        })))
    }

    #[wasm_bindgen(getter, js_name = type)]
    #[inline]
    pub fn get_type(&self) -> u8 {
        TYPE_REFS_XML_ELEMENT
    }

    /// Returns true if this is a preliminary instance of `YXmlElement`.
    ///
    /// Preliminary instances can be nested into other shared data types.
    /// Once a preliminary instance has been inserted this way, it becomes integrated into ywasm
    /// document store and cannot be nested again: attempt to do so will result in an exception.
    #[wasm_bindgen(getter)]
    #[inline]
    pub fn prelim(&self) -> bool {
        self.0.is_prelim()
    }

    /// Checks if current shared type reference is alive and has not been deleted by its parent collection.
    /// This method only works on already integrated shared types and will return false is current
    /// type is preliminary (has not been integrated into document).
    #[wasm_bindgen(js_name = alive)]
    #[inline]
    pub fn alive(&self, txn: &YTransaction) -> bool {
        self.0.is_alive(txn)
    }

    /// Returns a tag name of this XML node.
    #[wasm_bindgen(getter)]
    pub fn name(&self, txn: &ImplicitTransaction) -> crate::Result<String> {
        match &self.0 {
            SharedCollection::Prelim(c) => Ok(c.name.clone()),
            SharedCollection::Integrated(c) => c.readonly(txn, |c, _| Ok(c.tag().to_string())),
        }
    }

    /// Returns a number of child XML nodes stored within this `YXMlElement` instance.
    #[wasm_bindgen(js_name = length)]
    pub fn length(&self, txn: &ImplicitTransaction) -> crate::Result<u32> {
        match &self.0 {
            SharedCollection::Prelim(c) => Ok(c.children.len() as u32),
            SharedCollection::Integrated(c) => c.readonly(txn, |c, txn| Ok(c.len(txn))),
        }
    }

    #[wasm_bindgen(js_name = insert)]
    pub fn insert(
        &mut self,
        index: u32,
        xml_node: JsValue,
        txn: ImplicitTransaction,
    ) -> crate::Result<()> {
        Js::assert_xml_prelim(&xml_node)?;
        match &mut self.0 {
            SharedCollection::Prelim(c) => {
                c.children.insert(index as usize, xml_node);
                Ok(())
            }
            SharedCollection::Integrated(c) => c.mutably(txn, |c, txn| {
                c.insert(txn, index, Js::new(xml_node));
                Ok(())
            }),
        }
    }

    #[wasm_bindgen(js_name = push)]
    pub fn push(&mut self, xml_node: JsValue, txn: ImplicitTransaction) -> crate::Result<()> {
        Js::assert_xml_prelim(&xml_node)?;
        match &mut self.0 {
            SharedCollection::Prelim(c) => {
                c.children.push(xml_node);
                Ok(())
            }
            SharedCollection::Integrated(c) => c.mutably(txn, |c, txn| {
                c.push_back(txn, Js::new(xml_node));
                Ok(())
            }),
        }
    }

    #[wasm_bindgen(method, js_name = delete)]
    pub fn delete(
        &mut self,
        index: u32,
        length: Option<u32>,
        txn: ImplicitTransaction,
    ) -> crate::Result<()> {
        let length = length.unwrap_or(1);
        match &mut self.0 {
            SharedCollection::Prelim(c) => {
                c.children
                    .drain((index as usize)..((index + length) as usize));
                Ok(())
            }
            SharedCollection::Integrated(c) => c.mutably(txn, |c, txn| {
                c.remove_range(txn, index, length);
                Ok(())
            }),
        }
    }

    /// Returns a first child of this XML node.
    /// It can be either `YXmlElement`, `YXmlText` or `undefined` if current node has not children.
    #[wasm_bindgen(js_name = firstChild)]
    pub fn first_child(&self, txn: &ImplicitTransaction) -> crate::Result<JsValue> {
        match &self.0 {
            SharedCollection::Prelim(c) => {
                Ok(c.children.first().cloned().unwrap_or(JsValue::UNDEFINED))
            }
            SharedCollection::Integrated(c) => c.readonly(txn, |c, txn| match c.first_child() {
                None => Ok(JsValue::UNDEFINED),
                Some(xml) => Ok(Js::from_xml(xml, txn.doc().clone()).into()),
            }),
        }
    }

    /// Returns a next XML sibling node of this XMl node.
    /// It can be either `YXmlElement`, `YXmlText` or `undefined` if current node is a last child of
    /// parent XML node.
    #[wasm_bindgen(js_name = nextSibling)]
    pub fn next_sibling(&self, txn: &ImplicitTransaction) -> crate::Result<JsValue> {
        match &self.0 {
            SharedCollection::Prelim(_) => {
                Err(JsValue::from_str(crate::js::errors::INVALID_PRELIM_OP))
            }
            SharedCollection::Integrated(c) => c.readonly(txn, |c, txn| {
                let next = c.siblings(txn).next();
                match next {
                    Some(node) => Ok(Js::from_xml(node, txn.doc().clone()).into()),
                    None => Ok(JsValue::UNDEFINED),
                }
            }),
        }
    }

    /// Returns a previous XML sibling node of this XMl node.
    /// It can be either `YXmlElement`, `YXmlText` or `undefined` if current node is a first child
    /// of parent XML node.
    #[wasm_bindgen(js_name = prevSibling)]
    pub fn prev_sibling(&self, txn: &ImplicitTransaction) -> crate::Result<JsValue> {
        match &self.0 {
            SharedCollection::Prelim(_) => {
                Err(JsValue::from_str(crate::js::errors::INVALID_PRELIM_OP))
            }
            SharedCollection::Integrated(c) => c.readonly(txn, |c, txn| {
                let next = c.siblings(txn).next_back();
                match next {
                    Some(node) => Ok(Js::from_xml(node, txn.doc().clone()).into()),
                    None => Ok(JsValue::UNDEFINED),
                }
            }),
        }
    }

    /// Returns a parent `YXmlElement` node or `undefined` if current node has no parent assigned.
    #[wasm_bindgen(getter, js_name = parent)]
    pub fn parent(&self, txn: &ImplicitTransaction) -> crate::Result<JsValue> {
        match &self.0 {
            SharedCollection::Prelim(_) => {
                Err(JsValue::from_str(crate::js::errors::INVALID_PRELIM_OP))
            }
            SharedCollection::Integrated(c) => c.readonly(txn, |c, txn| match c.parent() {
                None => Ok(JsValue::UNDEFINED),
                Some(node) => Ok(Js::from_xml(node, txn.doc().clone()).into()),
            }),
        }
    }

    /// Returns a string representation of this XML node.
    #[wasm_bindgen(js_name = toString)]
    pub fn to_string(&self, txn: &ImplicitTransaction) -> crate::Result<String> {
        match &self.0 {
            SharedCollection::Prelim(c) => c.to_string(txn),
            SharedCollection::Integrated(c) => c.readonly(txn, |c, txn| Ok(c.get_string(txn))),
        }
    }

    /// Sets a `name` and `value` as new attribute for this XML node. If an attribute with the same
    /// `name` already existed on that node, its value with be overridden with a provided one.
    #[wasm_bindgen(js_name = setAttribute)]
    pub fn set_attribute(
        &mut self,
        name: &str,
        value: &str,
        txn: ImplicitTransaction,
    ) -> crate::Result<()> {
        match &mut self.0 {
            SharedCollection::Prelim(c) => {
                c.attributes.insert(name.to_string(), value.to_string());
                Ok(())
            }
            SharedCollection::Integrated(c) => c.mutably(txn, |c, txn| {
                c.insert_attribute(txn, name, value);
                Ok(())
            }),
        }
    }

    /// Returns a value of an attribute given its `name`. If no attribute with such name existed,
    /// `null` will be returned.
    #[wasm_bindgen(js_name = getAttribute)]
    pub fn get_attribute(&self, name: &str, txn: &ImplicitTransaction) -> crate::Result<JsValue> {
        let value = match &self.0 {
            SharedCollection::Integrated(c) => {
                c.readonly(txn, |c, txn| Ok(c.get_attribute(txn, name)))?
            }
            SharedCollection::Prelim(c) => c.attributes.get(name).cloned(),
        };
        match value {
            None => Ok(JsValue::UNDEFINED),
            Some(value) => Ok(JsValue::from_str(&value)),
        }
    }

    /// Removes an attribute from this XML node, given its `name`.
    #[wasm_bindgen(js_name = removeAttribute)]
    pub fn remove_attribute(
        &mut self,
        name: String,
        txn: ImplicitTransaction,
    ) -> crate::Result<()> {
        match &mut self.0 {
            SharedCollection::Prelim(c) => {
                c.attributes.remove(&name);
                Ok(())
            }
            SharedCollection::Integrated(c) => c.mutably(txn, |c, txn| {
                c.remove_attribute(txn, &name);
                Ok(())
            }),
        }
    }

    /// Returns an iterator that enables to traverse over all attributes of this XML node in
    /// unspecified order.
    #[wasm_bindgen(js_name = attributes)]
    pub fn attributes(&self, txn: &ImplicitTransaction) -> crate::Result<JsValue> {
        match &self.0 {
            SharedCollection::Prelim(c) => Ok(JsValue::from_serde(&c.attributes)
                .map_err(|_| JsValue::from_str(crate::js::errors::INVALID_PRELIM_OP))?),
            SharedCollection::Integrated(c) => c.readonly(txn, |c, txn| {
                let map = js_sys::Object::new();
                for (name, value) in c.attributes(txn) {
                    js_sys::Reflect::set(
                        &map,
                        &JsValue::from_str(name),
                        &JsValue::from_str(&value),
                    )?;
                }
                Ok(map.into())
            }),
        }
    }

    /// Returns an iterator that enables a deep traversal of this XML node - starting from first
    /// child over this XML node successors using depth-first strategy.
    #[wasm_bindgen(js_name = treeWalker)]
    pub fn tree_walker(&self, txn: &ImplicitTransaction) -> crate::Result<js_sys::Array> {
        match &self.0 {
            SharedCollection::Prelim(_) => {
                Err(JsValue::from_str(crate::js::errors::INVALID_PRELIM_OP))
            }
            SharedCollection::Integrated(c) => c.readonly(txn, |c, txn| {
                let doc = txn.doc();
                let walker = c.successors(txn).map(|n| {
                    let js: JsValue = Js::from_xml(n, doc.clone()).into();
                    js
                });
                let array = js_sys::Array::from_iter(walker);
                Ok(array.into())
            }),
        }
    }

    /// Subscribes to all operations happening over this instance of `YXmlElement`. All changes are
    /// batched and eventually triggered during transaction commit phase.
    /// Returns an `YObserver` which, when free'd, will unsubscribe current callback.
    #[wasm_bindgen(js_name = observe)]
    pub fn observe(&mut self, f: js_sys::Function) -> crate::Result<Observer> {
        match &self.0 {
            SharedCollection::Prelim(_) => {
                Err(JsValue::from_str(crate::js::errors::INVALID_PRELIM_OP))
            }
            SharedCollection::Integrated(c) => {
                let txn = c.transact()?;
                let array = c.resolve(&txn)?;
                Ok(Observer(array.observe(move |txn, e| {
                    let e = YXmlEvent::new(e, txn);
                    let txn = YTransaction::from_ref(txn);
                    f.call2(&JsValue::UNDEFINED, &e.into(), &txn.into())
                        .unwrap();
                })))
            }
        }
    }

    /// Subscribes to all operations happening over this Y shared type, as well as events in
    /// shared types stored within this one. All changes are batched and eventually triggered
    /// during transaction commit phase.
    /// Returns an `YEventObserver` which, when free'd, will unsubscribe current callback.
    #[wasm_bindgen(js_name = observeDeep)]
    pub fn observe_deep(&mut self, f: js_sys::Function) -> crate::Result<Observer> {
        match &self.0 {
            SharedCollection::Prelim(_) => {
                Err(JsValue::from_str(crate::js::errors::INVALID_PRELIM_OP))
            }
            SharedCollection::Integrated(c) => {
                let txn = c.transact()?;
                let array = c.resolve(&txn)?;
                Ok(Observer(array.observe_deep(move |txn, e| {
                    let e = crate::js::convert::events_into_js(txn, e);
                    let txn = YTransaction::from_ref(txn);
                    f.call2(&JsValue::UNDEFINED, &e, &txn.into()).unwrap();
                })))
            }
        }
    }
}
