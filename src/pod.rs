
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
    get_object_metadata,
};

use crate::ddtypes::*;

use k8s_openapi::{
    api::core::v1::{
        Affinity, Event, NodeAffinity, NodeSelector, NodeSelectorRequirement, NodeSelectorTerm,
        Pod, PodAffinity, PodAffinityTerm,
        Node,
    },
    apimachinery::pkg::apis::meta::v1::{LabelSelector, LabelSelectorRequirement, ObjectMeta},
};

use kube::{
    api::{ListParams, ResourceExt},
    Api, Client, Config as KubeConfig,
};

use kube_runtime::{utils::try_flatten_applied, watcher};

pub async fn pod_watcher() -> Result<()> {
    let client = Client::try_default().await?;
    let namespace = std::env::var("NAMESPACE").unwrap_or_else(|_| "default".into());
    let api = Api::<Pod>::namespaced(client, &namespace);
    let watcher = watcher(api, ListParams::default());
    try_flatten_applied(watcher)
        .try_for_each(|p| async move {
            log::debug!("Applied: {}", p.name());
            inject_pod_relation(&p);
            Ok(())
        })
        .await?;
    Ok(())
}

fn get_node_selector_requirement(
    nsrq: &NodeSelectorRequirement,
) -> affinity::NodeSelectorRequirement {
    let mut dd_selreq = affinity::NodeSelectorRequirement::default();
    dd_selreq.key = nsrq.key.clone();
    dd_selreq.operator = nsrq.operator.clone();

    let mut dd_values: ddVec<String> = ddVec::new();
    if let Some(values) = &nsrq.values {
        for value in values {
            dd_values.push(value.clone());
        }
        dd_selreq.values = ddOption::from(Some(dd_values));
    }
    dd_selreq
}

fn get_node_selectorterm(nst: &NodeSelectorTerm) -> affinity::NodeSelectorTerm {
    let mut dd_node_selterm = affinity::NodeSelectorTerm::default();

    if let Some(match_exprs) = &nst.match_expressions {
        let mut dd_match_exprs: ddVec<affinity::NodeSelectorRequirement> = ddVec::new();
        for expr in match_exprs {
            dd_match_exprs.push(get_node_selector_requirement(expr));
        }
        dd_node_selterm.match_expressions = ddOption::from(Some(dd_match_exprs));
    }

    if let Some(match_fields) = &nst.match_fields {
        let mut dd_match_fields: ddVec<affinity::NodeSelectorRequirement> = ddVec::new();
        for expr in match_fields {
            dd_match_fields.push(get_node_selector_requirement(expr));
        }
        dd_node_selterm.match_fields = ddOption::from(Some(dd_match_fields));
    }
    dd_node_selterm
}

fn get_node_affinity(af: &Affinity) -> Option<affinity::NodeAffinity> {
    if let Some(node_affinity) = &af.node_affinity {
        let mut dd_node_affinity = affinity::NodeAffinity::default();
        if let Some(required) = &node_affinity.required_during_scheduling_ignored_during_execution {
            let mut reqd_node_selector = affinity::NodeSelector::default();

            for term in &required.node_selector_terms {
                reqd_node_selector.terms.push(get_node_selectorterm(term));
            }
            dd_node_affinity.required = ddOption::from(Some(reqd_node_selector));
        }
        return Some(dd_node_affinity);
    }
    return None;
}

fn get_label_selector(ls: &LabelSelector) -> ddOption<affinity::LabelSelector> {
    let mut dd_label_selector = affinity::LabelSelector::default();

    if let Some(match_exprs) = &ls.match_expressions {
        let mut dd_match_exprs: ddVec<affinity::LabelSelectorRequirement> = ddVec::new();
        for expr in match_exprs {
            let mut dd_expr = affinity::LabelSelectorRequirement::default();
            dd_expr.key = expr.key.clone();
            dd_expr.operator = expr.operator.clone();

            let mut dd_values: ddVec<String> = ddVec::new();
            if let Some(values) = &expr.values {
                for value in values {
                    dd_values.push(value.clone());
                }
                dd_expr.values = ddOption::from(Some(dd_values));
            }
            dd_match_exprs.push(dd_expr);
        }
        dd_label_selector.match_expressions = ddOption::from(Some(dd_match_exprs));
    }

    if let Some(match_labels) = &ls.match_labels {
        let mut dd_match_labels: ddMap<String, String> = ddMap::new();
        for (k, v) in match_labels {
            dd_match_labels.insert(k.clone(), v.clone());
        }
        dd_label_selector.match_labels = ddOption::from(Some(dd_match_labels));
    }
    ddOption::from(Some(dd_label_selector))
}

fn get_pod_affinity_term(pat: &PodAffinityTerm) -> affinity::PodAffinityTerm {
    let mut dd_pod_term = affinity::PodAffinityTerm::default();

    if let Some(lbl_selector) = &pat.label_selector {
        dd_pod_term.label_selector = get_label_selector(lbl_selector);
    }

    if let Some(ns_selector) = &pat.namespace_selector {
        dd_pod_term.namespace_selector = get_label_selector(ns_selector);
    }

    if let Some(ns_vec) = &pat.namespaces {
        let mut namespaces: ddVec<String> = ddVec::new();
        for ns in ns_vec.iter() {
            namespaces.push(ns.to_string());
        }
        dd_pod_term.namespaces = ddOption::from(Some(namespaces));
    }
    dd_pod_term.topology_key = pat.topology_key.clone();

    dd_pod_term
}

fn get_pod_affinity(af: &Affinity) -> Option<affinity::PodAffinity> {
    if let Some(pod_affinity) = &af.pod_affinity {
        let mut dd_pod_affinity = affinity::PodAffinity::default();
        if let Some(required) = &pod_affinity.required_during_scheduling_ignored_during_execution {
            let mut pa_required: ddVec<affinity::PodAffinityTerm> = ddVec::new();

            for term in required {
                pa_required.push(get_pod_affinity_term(term));
            }
            dd_pod_affinity.required = ddOption::from(Some(pa_required));
        }
        return Some(dd_pod_affinity);
    }
    return None;
}

fn get_pod_antiaffinity(af: &Affinity) -> Option<affinity::PodAffinity> {
    get_pod_affinity(af)
}

fn get_affinity(af: &Affinity) -> ddOption<affinity::Affinity> {
    let mut affinity = affinity::Affinity::default();

    affinity.node_affinity = ddOption::from(get_node_affinity(&af));
    affinity.pod_affinity = ddOption::from(get_pod_affinity(&af));
    affinity.pod_anti_affinity = ddOption::from(get_pod_antiaffinity(&af));

    ddOption::from(Some(affinity))
}

fn get_pod_object(p: &Pod) -> pod::Pod {
    let mut pod_obj = pod::Pod::default();

    pod_obj.metadata = get_object_metadata(&p.metadata);

    if let Some(spec) = &p.spec {
        pod_obj.spec.node_name = ddOption::from(spec.node_name.clone());

        if let Some(aff) = &spec.affinity {
            pod_obj.spec.affinity = get_affinity(aff);
        };
    };

    pod_obj
}

fn inject_pod_relation(p: &Pod) {
    //dump_pod_spec(&p);
    log::debug!("injecting pod relation for {}", p.metadata.name.as_ref().unwrap().clone());

    let pod_obj = get_pod_object(p);

    let updates = vec![
        Update::Insert {
            relid: Relations::pod_Pod as RelId,
            v: pod_obj.into_ddvalue(),
        },
    ];

    perform_hddlog_transaction(updates);
}

fn dump_pod_spec(p: &Pod) {
    let spec = p.spec.as_ref().unwrap();
    log::info!("podspec {:?}", spec);
}


