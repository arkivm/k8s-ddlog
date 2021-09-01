
use futures::prelude::*;
use crate::{
    perform_hddlog_transaction,
    ddMap,
    ddOption,
    ddVec,
    Relations,
    ddtypes::*,
    DDValue,
    DDValConvert,
    RelId,
    Update,
    Result,
};

use crate::ddtypes::*;

use k8s_openapi::{
    api::core::v1::{
        Node,
    },
};

use kube::{
    api::{ListParams, ResourceExt},
    Api, Client, Config as KubeConfig,
};

use kube_runtime::{utils::try_flatten_applied, watcher};

pub async fn node_watcher() -> Result<()> {
    let client = Client::try_default().await?;
    let api = Api::<Node>::all(client);
    let watcher = watcher(api, ListParams::default());
    try_flatten_applied(watcher)
        .try_for_each(|n| async move {
            log::debug!("Applied: {}", n.name());
            inject_node_relation(&n);
            Ok(())
        })
        .await?;
    Ok(())
}

fn get_node_object(n: &Node) -> node::Node {
    let mut node_obj = node::Node::default();

    node_obj.metadata.name = ddOption::from(n.metadata.name.clone());
    node_obj.metadata.cluster_name = ddOption::from(n.metadata.cluster_name.clone());
    node_obj.metadata.namespace = ddOption::from(n.metadata.namespace.clone());

    if let Some(uid) = &n.metadata.uid {
        node_obj.metadata.uid = metadata::UID {
            uid: uid.clone(),
        };
    };

    if let Some(spec) = &n.spec {
        if let Some(pod_cidr) = &spec.pod_cidr {
            node_obj.spec.pod_cidr = ddOption::from(Some(pod_cidr.clone()));
        }
    };

    /*if let Some(status) = &n.status {
        if let Some(node_info) = &status.node_info {
            node_obj.status.node_info.system_uuid = node_info.system_uuid.clone();
        }
    };*/

    node_obj
}

fn inject_node_relation(n: &Node) {
    //dump_node_spec(&n);
    log::info!("injecting node relation for {}", n.name());

    let node_obj = get_node_object(n);

    let updates = vec![
        Update::Insert {
            relid: Relations::node_Node as RelId,
            v: node_obj.into_ddvalue(),
        },
    ];

    perform_hddlog_transaction(updates);
}

fn dump_node_spec(n: &Node) {
    let spec = n.spec.as_ref().unwrap();
    log::info!("nodespec {:?}", spec);
}
