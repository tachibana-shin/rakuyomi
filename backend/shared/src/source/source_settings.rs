use std::collections::HashMap;

use anyhow::Result;

use crate::settings::SourceSettingValue;

use super::model::SettingDefinition;

#[derive(Default, Debug)]
pub struct SourceSettings(HashMap<String, SourceSettingValue>);

impl SourceSettings {
    pub fn new(
        setting_definitions: &[SettingDefinition],
        stored_settings: HashMap<String, SourceSettingValue>,
    ) -> Result<Self> {
        let default_settings: HashMap<_, _> = setting_definitions
            .iter()
            .flat_map(default_values_for_definition)
            .collect();

        let mut settings = HashMap::new();
        settings.extend(default_settings);
        settings.extend(stored_settings);

        // FIXME maybe we should check if a definition with no defaults is missing from the stored settings?
        Ok(Self(settings))
    }

    pub fn get(&self, key: &String) -> Option<&SourceSettingValue> {
        self.0.get(key)
    }
}

fn default_values_for_definition(
    setting_definition: &SettingDefinition,
) -> HashMap<String, SourceSettingValue> {
    match setting_definition {
        SettingDefinition::Group { items, .. } => items
            .iter()
            .flat_map(default_values_for_definition)
            .collect(),
        SettingDefinition::Select { key, default, .. } => {
            HashMap::from([(key.clone(), SourceSettingValue::String(default.clone()))])
        }
        SettingDefinition::MultiSelect { key, default, .. } => {
            HashMap::from([(key.clone(), SourceSettingValue::Vec(default.clone()))])
        }
        SettingDefinition::EditableList { key, default, .. } => {
            HashMap::from([(key.clone(), SourceSettingValue::Vec(default.clone()))])
        }
        SettingDefinition::Switch { key, default, .. } => {
            HashMap::from([(key.clone(), SourceSettingValue::Bool(*default))])
        }
        // FIXME use `if let` guard when they become stable
        SettingDefinition::Text { key, default, .. } if default.is_some() => HashMap::from([(
            key.clone(),
            SourceSettingValue::String(default.clone().unwrap()),
        )]),
        _ => HashMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{settings::SourceSettingValue, source::model::SettingDefinition};
    use std::collections::HashMap;

    use super::SourceSettings;

    #[test]
    fn it_defaults_to_definition_value_if_no_stored_setting_is_present() {
        let stored_settings = HashMap::new();
        let definition = SettingDefinition::Switch {
            title: "Ok?".into(),
            key: "ok".into(),
            default: true,
        };

        let source_settings = SourceSettings::new(&[definition], stored_settings).unwrap();

        assert_eq!(
            Some(SourceSettingValue::Bool(true)),
            source_settings.get(&"ok".into()).cloned()
        );
    }

    #[test]
    fn it_retrieves_stored_setting_value_if_present() {
        let mut stored_settings = HashMap::new();
        stored_settings.insert("ok".into(), SourceSettingValue::Bool(false));

        let definition = SettingDefinition::Switch {
            title: "Ok?".into(),
            key: "ok".into(),
            default: true,
        };

        let source_settings = SourceSettings::new(&[definition], stored_settings).unwrap();

        assert_eq!(
            Some(SourceSettingValue::Bool(false)),
            source_settings.get(&"ok".into()).cloned()
        );
    }
}
