use crate::{model::SourceInformation, source_collection::SourceCollection};

pub fn list_installed_sources(source_collection: &impl SourceCollection) -> Vec<SourceInformation> {
    let mut source_informations: Vec<SourceInformation> = source_collection
        .sources()
        .into_iter()
        .map(|source| {
            let manifest_info: SourceInformation = source.manifest().into();
            SourceInformation {
                supported_sort_buckets: source
                    .supported_sort_buckets()
                    .into_iter()
                    .map(|b| format!("{:?}", b).to_lowercase())
                    .collect(),
                ..manifest_info
            }
        })
        .collect();

    source_informations.sort_by_key(|source| source.name.clone());

    source_informations
}
