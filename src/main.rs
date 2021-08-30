#[macro_use]
extern crate log;

use color_eyre::Result;
use futures::prelude::*;
use k8s_openapi::{
    api::core::v1::{
        Affinity, Event, NodeAffinity, NodeSelector, NodeSelectorRequirement, NodeSelectorTerm,
        Pod, PodAffinity, PodAffinityTerm,
    },
    apimachinery::pkg::apis::meta::v1::{LabelSelector, LabelSelectorRequirement},
};

use kube::{
    api::{ListParams, ResourceExt},
    Api, Client, Config as KubeConfig,
};

use kube_runtime::{utils::try_flatten_applied, watcher};

use kube_policy_ddlog as myddlog;

// The `Relations` enum enumerates program relations
use myddlog::Relations;

// Type and function definitions generated for each ddlog program
mod ddtypes {
    pub use kube_policy_ddlog::typedefs::*;
}

use myddlog::typedefs::ddlog_std::Map as ddMap;
use myddlog::typedefs::ddlog_std::Option as ddOption;
use myddlog::typedefs::ddlog_std::Vec as ddVec;

// The differential_datalog crate contains the DDlog runtime that is
// the same for all DDlog programs and simply gets copied to each generated
// DDlog workspace unmodified (this will change in future releases).

// The `differential_datalog` crate declares the `HDDlog` type that
// serves as a reference to a running DDlog program.
use differential_datalog::api::HDDlog;

// HDDlog implementa several traits:
use differential_datalog::{DDlog, DDlogDynamic, DDlogInventory};

// The `differential_datalog::program::config` module declares datatypes
//used to configure DDlog program on startup.
use differential_datalog::program::config::{Config, ProfilingConfig};

// Type that represents a set of changes to DDlog relations.
// Returned by `DDlog::transaction_commit_dump_changes()`.
use differential_datalog::DeltaMap;

// Trait to convert Rust types to/from DDValue.
// All types used in input and output relations, indexes, and
// primary keys implement this trait.
use differential_datalog::ddval::DDValConvert;

// Generic type that wraps all DDlog values.
use differential_datalog::ddval::DDValue;

use differential_datalog::program::RelId; // Numeric relations id.
use differential_datalog::program::Update; // Type-safe representation of a DDlog command (insert/delete_val/delete_key/...)

// The `record` module defines dynamically typed representation of DDlog values and commands.
use differential_datalog::record::Record; // Dynamically typed representation of DDlog values.
use differential_datalog::record::RelIdentifier; // Relation identifier: either `RelId` or `Cow<str>`g.
use differential_datalog::record::UpdCmd; // Dynamically typed representation of DDlog command.

use lazy_static::lazy_static;

static mut hddlog_g: Option<HDDlog> = None;

async fn pod_watcher() -> Result<()> {
    let client = Client::try_default().await?;
    let namespace = std::env::var("NAMESPACE").unwrap_or_else(|_| "default".into());
    let api = Api::<Pod>::namespaced(client, &namespace);
    let watcher = watcher(api, ListParams::default());
    try_flatten_applied(watcher)
        .try_for_each(|p| async move {
            log::debug!("Applied: {}", p.name());
            inject_pod_relation(&p);
            if let Some(unready_reason) = pod_unready(&p) {
                log::warn!("{}", unready_reason);
            }
            Ok(())
        })
        .await?;
    Ok(())
}

fn get_node_selector_requirement(
    nsrq: &NodeSelectorRequirement,
) -> ddtypes::affinity::NodeSelectorRequirement {
    let mut dd_selreq = ddtypes::affinity::NodeSelectorRequirement::default();
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

fn get_node_selectorterm(nst: &NodeSelectorTerm) -> ddtypes::affinity::NodeSelectorTerm {
    let mut dd_node_selterm = ddtypes::affinity::NodeSelectorTerm::default();

    if let Some(match_exprs) = &nst.match_expressions {
        let mut dd_match_exprs: ddVec<ddtypes::affinity::NodeSelectorRequirement> = ddVec::new();
        for expr in match_exprs {
            dd_match_exprs.push(get_node_selector_requirement(expr));
        }
        dd_node_selterm.match_expressions = ddOption::from(Some(dd_match_exprs));
    }

    if let Some(match_fields) = &nst.match_fields {
        let mut dd_match_fields: ddVec<ddtypes::affinity::NodeSelectorRequirement> = ddVec::new();
        for expr in match_fields {
            dd_match_fields.push(get_node_selector_requirement(expr));
        }
        dd_node_selterm.match_fields = ddOption::from(Some(dd_match_fields));
    }
    dd_node_selterm
}

fn get_node_affinity(af: &Affinity) -> Option<ddtypes::affinity::NodeAffinity> {
    if let Some(node_affinity) = &af.node_affinity {
        let mut dd_node_affinity = ddtypes::affinity::NodeAffinity::default();
        if let Some(required) = &node_affinity.required_during_scheduling_ignored_during_execution {
            let mut reqd_node_selector = ddtypes::affinity::NodeSelector::default();

            for term in &required.node_selector_terms {
                reqd_node_selector.terms.push(get_node_selectorterm(term));
            }
            dd_node_affinity.required = ddOption::from(Some(reqd_node_selector));
        }
        return Some(dd_node_affinity);
    }
    return None;
}

fn get_label_selector(ls: &LabelSelector) -> ddOption<ddtypes::affinity::LabelSelector> {
    let mut dd_label_selector = ddtypes::affinity::LabelSelector::default();

    if let Some(match_exprs) = &ls.match_expressions {
        let mut dd_match_exprs: ddVec<ddtypes::affinity::LabelSelectorRequirement> = ddVec::new();
        for expr in match_exprs {
            let mut dd_expr = ddtypes::affinity::LabelSelectorRequirement::default();
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

fn get_pod_affinity_term(pat: &PodAffinityTerm) -> ddtypes::affinity::PodAffinityTerm {
    let mut dd_pod_term = ddtypes::affinity::PodAffinityTerm::default();

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

fn get_pod_affinity(af: &Affinity) -> Option<ddtypes::affinity::PodAffinity> {
    if let Some(pod_affinity) = &af.pod_affinity {
        let mut dd_pod_affinity = ddtypes::affinity::PodAffinity::default();
        if let Some(required) = &pod_affinity.required_during_scheduling_ignored_during_execution {
            let mut pa_required: ddVec<ddtypes::affinity::PodAffinityTerm> = ddVec::new();

            for term in required {
                pa_required.push(get_pod_affinity_term(term));
            }
            dd_pod_affinity.required = ddOption::from(Some(pa_required));
        }
        return Some(dd_pod_affinity);
    }
    return None;
}

fn get_pod_antiaffinity(af: &Affinity) -> Option<ddtypes::affinity::PodAffinity> {
    get_pod_affinity(af)
}

fn get_affinity(af: &Affinity) -> ddOption<ddtypes::affinity::Affinity> {
    let mut affinity = ddtypes::affinity::Affinity::default();

    affinity.node_affinity = ddOption::from(get_node_affinity(&af));
    affinity.pod_affinity = ddOption::from(get_pod_affinity(&af));
    affinity.pod_anti_affinity = ddOption::from(get_pod_antiaffinity(&af));

    ddOption::from(Some(affinity))
}

fn get_pod_object(p: &Pod) -> ddtypes::pod::Pod {
    let pod_name = p.metadata.name.as_ref().unwrap().clone();
    let mut pod_obj = ddtypes::pod::Pod::default();

    pod_obj.metadata.name = ddOption::from(p.metadata.name.clone());
    pod_obj.metadata.cluster_name = ddOption::from(p.metadata.cluster_name.clone());
    pod_obj.metadata.namespace = ddOption::from(p.metadata.namespace.clone());

    if let Some(uid) = &p.metadata.uid {
        pod_obj.metadata.uid = ddtypes::pod::UID {
            uid: p.metadata.uid.as_ref().unwrap().clone(),
        };
    };

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

    unsafe {
        let hddlog = hddlog_g.as_ref().unwrap();
        hddlog.transaction_start().unwrap();

        let updates = vec![
        Update::Insert {
            relid: Relations::pod_Pod as RelId,
            v: pod_obj.into_ddvalue(),
        },
        ];
        hddlog.apply_updates(&mut updates.into_iter()).unwrap();

        log::info!("Commiting changes");
        let mut delta = hddlog.transaction_commit_dump_changes().unwrap();
        dump_delta(&hddlog, &delta);
    }
}

fn dump_pod_spec(p: &Pod) {
    let spec = p.spec.as_ref().unwrap();
    log::info!("podspec {:?}", spec);
}

#[tokio::main]
async fn main() -> Result<()> {
    std::env::set_var("RUST_LOG", "info,kube=debug");
    env_logger::init();

    unsafe {
        hddlog_g = Some(init_ddlog());
    }
    pod_watcher().await
}

async fn event_watcher() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "info,kube=debug");
    env_logger::init();

    let client = Client::try_default().await?;

    let events: Api<Event> = Api::all(client);
    let lp = ListParams::default();

    let mut ew = try_flatten_applied(watcher(events, lp)).boxed();

    // Get cluster config
    let cfg = KubeConfig::infer().await?;

    println!("config {:?}", cfg);

    while let Some(event) = ew.try_next().await? {
        handle_event(event)?;
    }
    Ok(())
}

// This function lets the app handle an added/modified event from k8s
fn handle_event(ev: Event) -> anyhow::Result<()> {
    let ev1 = ev.clone();
    info!(
        "New Event: {} (via \"{}\" {})",
        ev.message.unwrap(),
        ev.involved_object.kind.unwrap(),
        ev.involved_object.name.unwrap()
    );
    if ev1.involved_object.kind.unwrap() == String::from("Pod") {
        let pod_name = ev1.involved_object.name.unwrap().clone();
        let mut pod_obj = ddtypes::pod::Pod::default();
        pod_obj.metadata.name = ddOption::from(Some(pod_name));
        unsafe {
            let hddlog = hddlog_g.as_ref().unwrap();
            hddlog.transaction_start();

            let updates = vec![Update::Insert {
                // We are going to insert..
                relid: Relations::pod_Pod as RelId, // .. into relation with this Id.
                // `Word1` type, declared in the `types` crate has the same fields as
                // the corresponding DDlog type.
                v: pod_obj.into_ddvalue(),
            }];
            hddlog.apply_updates(&mut updates.into_iter());

            let mut delta = hddlog.transaction_commit_dump_changes().unwrap();
            dump_delta(&hddlog, &delta);
        }
    }
    Ok(())
}

// From https://github.com/vmware/differential-datalog/blob/master/test/datalog_tests/rust_api_test/src/main.rs
fn init_ddlog() -> HDDlog {
    let config = Config::new()
        .with_timely_workers(1)
        .with_profiling_config(ProfilingConfig::SelfProfiling);
    let (hddlog, init_state) = myddlog::run_with_config(config, false).unwrap();

    dump_delta(&hddlog, &init_state);

    log::info!("DDlog initialized!");
    hddlog
}

fn dump_delta(ddlog: &HDDlog, delta: &DeltaMap<DDValue>) {
    for (rel, changes) in delta.iter() {
        info!(
            "Changes to relation {}",
            ddlog.inventory.get_table_name(*rel).unwrap()
        );
        for (val, weight) in changes.iter() {
            info!("{} {:+}", val, weight);
        }
    }
}

fn pod_unready(p: &Pod) -> Option<String> {
    let status = p.status.as_ref().unwrap();
    if let Some(conds) = &status.conditions {
        let failed = conds
            .into_iter()
            .filter(|c| c.type_ == "Ready" && c.status == "False")
            .map(|c| c.message.clone().unwrap_or_default())
            .collect::<Vec<_>>()
            .join(",");
        if !failed.is_empty() {
            if p.metadata.labels.as_ref().unwrap().contains_key("job-name") {
                return None; // ignore job based pods, they are meant to exit 0
            }
            return Some(format!("Unready pod {}: {}", p.name(), failed));
        }
    }
    None
}
