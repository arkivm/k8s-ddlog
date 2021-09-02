use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use crate::{
    ddOption,
    ddtypes::*,
};

pub fn get_object_metadata(meta : &ObjectMeta) -> metadata::ObjectMeta {
    let mut dd_meta = metadata::ObjectMeta::default();

    dd_meta.name = ddOption::from(meta.name.clone());
    dd_meta.cluster_name = ddOption::from(meta.cluster_name.clone());
    dd_meta.namespace = ddOption::from(meta.namespace.clone());

    if let Some(uid) = &meta.uid {
        dd_meta.uid = metadata::UID {
            uid: uid.clone(),
        };
    };
    dd_meta
}
