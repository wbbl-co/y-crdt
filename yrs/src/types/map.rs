use crate::block::{ItemContent, ItemPosition, Prelim};
use crate::types::{Entries, Inner, InnerRef, TypePtr, Value, TYPE_REFS_MAP};
use crate::*;
use lib0::any::Any;
use std::collections::HashMap;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Map(InnerRef);

impl Map {
    pub fn to_json(&self, txn: &Transaction<'_>) -> Any {
        Self::to_json_inner(&*self.0.borrow(), txn)
    }

    pub(crate) fn to_json_inner(inner: &Inner, txn: &Transaction<'_>) -> Any {
        let mut res = HashMap::new();
        for (key, ptr) in inner.map.iter() {
            if let Some(item) = txn.store.blocks.get_item(ptr) {
                if !item.is_deleted() {
                    let any = if let Some(value) = item.content.get_content_last(txn) {
                        value.to_json(txn)
                    } else {
                        Any::Null
                    };
                    res.insert(key.clone(), any);
                }
            }
        }
        Any::Map(res)
    }

    pub fn len(&self, txn: &Transaction<'_>) -> usize {
        let mut len = 0;
        let inner = self.0.borrow();
        for ptr in inner.map.values() {
            //TODO: maybe it would be better to just cache len in the map itself?
            if let Some(item) = txn.store.blocks.get_item(ptr) {
                if !item.is_deleted() {
                    len += 1;
                }
            }
        }
        len
    }

    fn blocks<'a, 'b, 'txn>(&'a self, txn: &'b Transaction<'txn>) -> Entries<'b, 'txn> {
        let ptr = &self.0.borrow().ptr;
        Entries::new(ptr, txn)
    }

    pub fn keys<'a, 'b, 'txn>(&'a self, txn: &'b Transaction<'txn>) -> Keys<'b, 'txn> {
        Keys(self.blocks(txn))
    }

    pub fn values<'a, 'b, 'txn>(&'a self, txn: &'b Transaction<'txn>) -> Values<'b, 'txn> {
        Values(self.blocks(txn))
    }

    pub fn iter<'a, 'b, 'txn>(&'a self, txn: &'b Transaction<'txn>) -> Iter<'b, 'txn> {
        Iter(self.blocks(txn))
    }

    pub fn insert<V: Prelim>(&self, txn: &mut Transaction, key: String, value: V) -> Option<Value> {
        let previous = self.get(txn, &key);
        let pos = {
            let inner = self.0.borrow();
            let left = inner.map.get(&key);
            ItemPosition {
                parent: inner.ptr.clone(),
                left: left.cloned(),
                right: None,
                index: 0,
            }
        };

        txn.create_item(&pos, value, Some(key));
        previous
    }

    pub fn remove(&self, txn: &mut Transaction, key: &str) -> Option<Value> {
        let t = self.0.borrow();
        t.remove(txn, key)
    }

    pub fn get(&self, txn: &Transaction, key: &str) -> Option<Value> {
        let t = self.0.borrow();
        t.get(txn, key)
    }

    pub fn contains(&self, txn: &Transaction, key: &String) -> bool {
        let t = self.0.borrow();
        if let Some(ptr) = t.map.get(key) {
            if let Some(item) = txn.store.blocks.get_item(ptr) {
                return !item.is_deleted();
            }
        }
        false
    }

    pub fn clear(&self, txn: &mut Transaction<'_>) {
        let t = self.0.borrow();
        for (_, ptr) in t.map.iter() {
            if let Some(item) = txn.store.blocks.get_item(ptr) {
                if !item.is_deleted() {
                    item.mark_as_deleted();
                }
            }
        }
    }
}

pub struct Iter<'a, 'txn>(Entries<'a, 'txn>);

impl<'a, 'txn> Iterator for Iter<'a, 'txn> {
    type Item = (&'a String, Vec<Value>);

    fn next(&mut self) -> Option<Self::Item> {
        let (key, item) = self.0.next()?;
        Some((key, item.content.get_content(self.0.txn)))
    }
}

pub struct Keys<'a, 'txn>(Entries<'a, 'txn>);

impl<'a, 'txn> Iterator for Keys<'a, 'txn> {
    type Item = &'a String;

    fn next(&mut self) -> Option<Self::Item> {
        let (key, _) = self.0.next()?;
        Some(key)
    }
}

pub struct Values<'a, 'txn>(Entries<'a, 'txn>);

impl<'a, 'txn> Iterator for Values<'a, 'txn> {
    type Item = Vec<Value>;

    fn next(&mut self) -> Option<Self::Item> {
        let (_, item) = self.0.next()?;
        Some(item.content.get_content(self.0.txn))
    }
}

impl From<InnerRef> for Map {
    fn from(inner: InnerRef) -> Self {
        Map(inner)
    }
}

pub struct PrelimMap<T>(HashMap<String, T>);

impl<T> From<HashMap<String, T>> for PrelimMap<T> {
    fn from(map: HashMap<String, T>) -> Self {
        PrelimMap(map)
    }
}

impl<T: Prelim> Prelim for PrelimMap<T> {
    fn into_content(self, _txn: &mut Transaction, ptr: TypePtr) -> (ItemContent, Option<Self>) {
        let inner = InnerRef::new(Inner::new(ptr, TYPE_REFS_MAP, None));
        (ItemContent::Type(inner), Some(self))
    }

    fn integrate(self, txn: &mut Transaction, inner_ref: InnerRef) {
        let map = Map::from(inner_ref);
        for (key, value) in self.0 {
            map.insert(txn, key, value);
        }
    }
}

#[cfg(test)]
mod test {
    use crate::test_utils::exchange_updates;
    use crate::types::{Map, Value};
    use crate::{Doc, Transaction};
    use lib0::any::Any;
    use std::collections::HashMap;

    #[test]
    fn map_basic() {
        let d1 = Doc::with_client_id(1);
        let mut t1 = d1.transact();
        let m1 = t1.get_map("map");

        let d2 = Doc::with_client_id(2);
        let mut t2 = d2.transact();
        let m2 = t2.get_map("map");

        m1.insert(&mut t1, "number".to_owned(), 1);
        m1.insert(&mut t1, "string".to_owned(), "hello Y");
        m1.insert(&mut t1, "object".to_owned(), {
            let mut v = HashMap::new();
            v.insert("key2".to_owned(), "value");

            let mut map = HashMap::new();
            map.insert("key".to_owned(), v);
            map // { key: { key2: 'value' } }
        });
        m1.insert(&mut t1, "boolean1".to_owned(), true);
        m1.insert(&mut t1, "boolean0".to_owned(), false);

        //let m1m = t1.get_map("y-map");
        //let m1a = t1.get_text("y-text");
        //m1a.insert(&mut t1, 0, "a");
        //m1a.insert(&mut t1, 0, "b");
        //m1m.insert(&mut t1, "y-text".to_owned(), m1a);

        //TODO: YArray within YMap
        fn compare_all(t: &Transaction<'_>, m: &Map) {
            assert_eq!(m.len(&t), 5);
            assert_eq!(m.get(&t, &"number".to_owned()), Some(Value::from(1f64)));
            assert_eq!(m.get(&t, &"boolean0".to_owned()), Some(Value::from(false)));
            assert_eq!(m.get(&t, &"boolean1".to_owned()), Some(Value::from(true)));
            assert_eq!(
                m.get(&t, &"string".to_owned()),
                Some(Value::from("hello Y"))
            );
            assert_eq!(
                m.get(&t, &"object".to_owned()),
                Some(Value::from({
                    let mut m = HashMap::new();
                    let mut n = HashMap::new();
                    n.insert("key2".to_owned(), Any::String("value".to_owned()));
                    m.insert("key".to_owned(), Any::Map(n));
                    m
                }))
            );
        }

        compare_all(&t1, &m1);

        let update = d1.encode_state_as_update(&t1);
        d2.apply_update(&mut t2, update.as_slice());

        compare_all(&t2, &m2);
    }

    #[test]
    fn map_get_set() {
        let d1 = Doc::with_client_id(1);
        let mut t1 = d1.transact();
        let m1 = t1.get_map("map");

        m1.insert(&mut t1, "stuff".to_owned(), "stuffy");
        m1.insert(&mut t1, "null".to_owned(), None as Option<String>);

        let update = d1.encode_state_as_update(&t1);

        let d2 = Doc::with_client_id(2);
        let mut t2 = d2.transact();

        d2.apply_update(&mut t2, update.as_slice());

        let m2 = t2.get_map("map");
        assert_eq!(
            m2.get(&t2, &"stuff".to_owned()),
            Some(Value::from("stuffy"))
        );
        assert_eq!(m2.get(&t2, &"null".to_owned()), Some(Value::Any(Any::Null)));
    }

    #[test]
    fn map_get_set_sync_with_conflicts() {
        let d1 = Doc::with_client_id(1);
        let mut t1 = d1.transact();
        let m1 = t1.get_map("map");

        let d2 = Doc::with_client_id(2);
        let mut t2 = d2.transact();
        let m2 = t2.get_map("map");

        m1.insert(&mut t1, "stuff".to_owned(), "c0");
        m2.insert(&mut t2, "stuff".to_owned(), "c1");

        let u1 = d1.encode_state_as_update(&t1);
        let u2 = d2.encode_state_as_update(&t2);

        d1.apply_update(&mut t1, u2.as_slice());
        d2.apply_update(&mut t2, u1.as_slice());

        assert_eq!(m1.get(&t1, &"stuff".to_owned()), Some(Value::from("c1")));
        assert_eq!(m2.get(&t2, &"stuff".to_owned()), Some(Value::from("c1")));
    }

    #[test]
    fn map_len_remove() {
        let d1 = Doc::with_client_id(1);
        let mut t1 = d1.transact();
        let m1 = t1.get_map("map");

        let key1 = "stuff".to_owned();
        let key2 = "other-stuff".to_owned();

        m1.insert(&mut t1, key1.clone(), "c0");
        m1.insert(&mut t1, key2.clone(), "c1");
        assert_eq!(m1.len(&t1), 2);

        // remove 'stuff'
        assert_eq!(m1.remove(&mut t1, &key1), Some(Value::from("c0")));
        assert_eq!(m1.len(&t1), 1);

        // remove 'stuff' again - nothing should happen
        assert_eq!(m1.remove(&mut t1, &key1), None);
        assert_eq!(m1.len(&t1), 1);

        // remove 'other-stuff'
        assert_eq!(m1.remove(&mut t1, &key2), Some(Value::from("c1")));
        assert_eq!(m1.len(&t1), 0);
    }

    #[test]
    fn map_clear() {
        let d1 = Doc::with_client_id(1);
        let mut t1 = d1.transact();
        let m1 = t1.get_map("map");

        m1.insert(&mut t1, "key1".to_owned(), "c0");
        m1.insert(&mut t1, "key2".to_owned(), "c1");
        m1.clear(&mut t1);

        assert_eq!(m1.len(&t1), 0);
        assert_eq!(m1.get(&t1, &"key1".to_owned()), None);
        assert_eq!(m1.get(&t1, &"key2".to_owned()), None);

        let d2 = Doc::with_client_id(2);
        let mut t2 = d2.transact();

        let u1 = d1.encode_state_as_update(&t1);
        d2.apply_update(&mut t2, u1.as_slice());

        let m2 = t2.get_map("map");
        assert_eq!(m2.len(&t2), 0);
        assert_eq!(m2.get(&t2, &"key1".to_owned()), None);
        assert_eq!(m2.get(&t2, &"key2".to_owned()), None);
    }

    #[test]
    fn map_clear_sync() {
        let d1 = Doc::with_client_id(1);
        let d2 = Doc::with_client_id(2);
        let d3 = Doc::with_client_id(3);
        let d4 = Doc::with_client_id(4);

        {
            let mut t1 = d1.transact();
            let mut t2 = d2.transact();
            let mut t3 = d3.transact();

            let m1 = t1.get_map("map");
            let m2 = t2.get_map("map");
            let m3 = t3.get_map("map");

            m1.insert(&mut t1, "key1".to_owned(), "c0");
            m2.insert(&mut t2, "key1".to_owned(), "c1");
            m2.insert(&mut t2, "key1".to_owned(), "c2");
            m3.insert(&mut t3, "key1".to_owned(), "c3");
        }

        exchange_updates(&[&d1, &d2, &d3, &d4]);

        {
            let mut t1 = d1.transact();
            let mut t2 = d2.transact();
            let mut t3 = d3.transact();

            let m1 = t1.get_map("map");
            let m2 = t2.get_map("map");
            let m3 = t3.get_map("map");

            m1.insert(&mut t1, "key2".to_owned(), "c0");
            m2.insert(&mut t2, "key2".to_owned(), "c1");
            m2.insert(&mut t2, "key2".to_owned(), "c2");
            m3.insert(&mut t3, "key2".to_owned(), "c3");
            m3.clear(&mut t3);
        }

        exchange_updates(&[&d1, &d2, &d3, &d4]);

        for doc in [d1, d2, d3, d4] {
            let mut txn = doc.transact();
            let map = txn.get_map("map");

            assert_eq!(
                map.get(&txn, &"key1".to_owned()),
                None,
                "'key1' entry for peer {} should be removed",
                doc.client_id
            );
            assert_eq!(
                map.get(&txn, &"key2".to_owned()),
                None,
                "'key2' entry for peer {} should be removed",
                doc.client_id
            );
            assert_eq!(
                map.len(&txn),
                0,
                "all entries for peer {} should be removed",
                doc.client_id
            );
        }
    }

    #[test]
    fn map_get_set_with_3_way_conflicts() {
        let d1 = Doc::with_client_id(1);
        let d2 = Doc::with_client_id(2);
        let d3 = Doc::with_client_id(3);

        {
            let mut t1 = d1.transact();
            let mut t2 = d2.transact();
            let mut t3 = d3.transact();

            let m1 = t1.get_map("map");
            let m2 = t2.get_map("map");
            let m3 = t3.get_map("map");

            m1.insert(&mut t1, "stuff".to_owned(), "c0");
            m2.insert(&mut t2, "stuff".to_owned(), "c1");
            m2.insert(&mut t2, "stuff".to_owned(), "c2");
            m3.insert(&mut t3, "stuff".to_owned(), "c3");
        }

        exchange_updates(&[&d1, &d2, &d3]);

        for doc in [d1, d2, d3] {
            let mut txn = doc.transact();

            let map = txn.get_map("map");

            assert_eq!(
                map.get(&txn, &"stuff".to_owned()),
                Some(Value::from("c3")),
                "peer {} - map entry resolved to unexpected value",
                doc.client_id
            );
        }
    }

    #[test]
    fn map_get_set_remove_with_3_way_conflicts() {
        let d1 = Doc::with_client_id(1);
        let d2 = Doc::with_client_id(2);
        let d3 = Doc::with_client_id(3);
        let d4 = Doc::with_client_id(4);

        {
            let mut t1 = d1.transact();
            let mut t2 = d2.transact();
            let mut t3 = d3.transact();

            let m1 = t1.get_map("map");
            let m2 = t2.get_map("map");
            let m3 = t3.get_map("map");

            m1.insert(&mut t1, "key1".to_owned(), "c0");
            m2.insert(&mut t2, "key1".to_owned(), "c1");
            m2.insert(&mut t2, "key1".to_owned(), "c2");
            m3.insert(&mut t3, "key1".to_owned(), "c3");
        }

        exchange_updates(&[&d1, &d2, &d3, &d4]);

        {
            let mut t1 = d1.transact();
            let mut t2 = d2.transact();
            let mut t3 = d3.transact();
            let mut t4 = d4.transact();

            let m1 = t1.get_map("map");
            let m2 = t2.get_map("map");
            let m3 = t3.get_map("map");
            let m4 = t4.get_map("map");

            m1.insert(&mut t1, "key1".to_owned(), "deleteme");
            m2.insert(&mut t2, "key1".to_owned(), "c1");
            m3.insert(&mut t3, "key1".to_owned(), "c2");
            m4.insert(&mut t4, "key1".to_owned(), "c3");
            m4.remove(&mut t4, &"key1".to_owned());
        }

        exchange_updates(&[&d1, &d2, &d3, &d4]);

        for doc in [d1, d2, d3, d4] {
            let mut txn = doc.transact();
            let map = txn.get_map("map");

            assert_eq!(
                map.get(&txn, &"key1".to_owned()),
                None,
                "entry 'key1' on peer {} should be removed",
                doc.client_id
            );
        }
    }
}
