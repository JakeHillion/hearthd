use std::collections::BTreeMap;

#[derive(Debug)]
pub struct Entity {}

#[derive(Debug, Default)]
pub struct State {
    entities: BTreeMap<String, Entity>,
}
