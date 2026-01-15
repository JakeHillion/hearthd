/// A device in the hearthd system.
///
/// A device represents a physical or logical device that contains one or more entities.
#[allow(dead_code)]
pub struct Device {
    pub id: String,
    pub identifiers: Vec<(String, String)>,
    pub name: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub sw_version: Option<String>,
    pub entity_ids: Vec<String>,
}

#[allow(dead_code)]
impl Device {
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            identifiers: Vec::new(),
            name,
            manufacturer: None,
            model: None,
            sw_version: None,
            entity_ids: Vec::new(),
        }
    }

    pub fn add_entity(&mut self, entity_id: String) {
        if !self.entity_ids.contains(&entity_id) {
            self.entity_ids.push(entity_id);
        }
    }
}
