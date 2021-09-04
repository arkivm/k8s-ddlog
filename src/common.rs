use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use crate::{
    ddMap,
    ddOption,
    ddtypes::*,
};

use std::collections::BTreeMap;

fn get_labels(label_tree : Option<BTreeMap<String, String>>) -> Option<ddMap<String, String>> {
    match label_tree {
        Some(lbl_map) => {
            let mut map : ddMap<String, String> = ddMap::default();
            for (k, v) in lbl_map {
                map.insert(k, v);
            }
            Some(map)
        },
        _ => None,
    }
}

pub fn get_object_metadata(meta : &ObjectMeta) -> metadata::ObjectMeta {
    let mut dd_meta = metadata::ObjectMeta::default();

    dd_meta.name = ddOption::from(meta.name.clone());
    dd_meta.cluster_name = ddOption::from(meta.cluster_name.clone());
    dd_meta.namespace = ddOption::from(meta.namespace.clone());

    dd_meta.labels = ddOption::from(get_labels(meta.labels.clone()));

    if let Some(uid) = &meta.uid {
        dd_meta.uid = metadata::UID {
            uid: uid.clone(),
        };
    };
    dd_meta
}
