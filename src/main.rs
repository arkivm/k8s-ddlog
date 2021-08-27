#[macro_use]
extern crate log;

use color_eyre::Result;
use futures::prelude::*;
use k8s_openapi::api::core::v1::{Pod, Event};
use kube::{
    api::{ListParams, ResourceExt},
    Api, Client,
};
use kube::Config as KubeConfig;

use kube_runtime::{utils::try_flatten_applied, watcher};

use kube_policy_ddlog as myddlog;

// The `Relations` enum enumerates program relations
use myddlog::Relations;

// Type and function definitions generated for each ddlog program
use myddlog::typedefs::*;

use myddlog::typedefs::ddlog_std::Option as ddOption;

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

lazy_static! {
    static ref hddlog_g: HDDlog = init_ddlog();
}

async fn pod_watcher() -> Result<()> {
    std::env::set_var("RUST_LOG", "info,kube=debug");
    env_logger::init();
    let client = Client::try_default().await?;
    let namespace = std::env::var("NAMESPACE").unwrap_or_else(|_| "default".into());
    let api = Api::<Pod>::namespaced(client, &namespace);
    let watcher = watcher(api, ListParams::default());
    try_flatten_applied(watcher)
        .try_for_each(|p| async move {
            log::debug!("Applied: {}", p.name());
            dump_pod_spec(&p);
            if let Some(unready_reason) = pod_unready(&p) {
                log::warn!("{}", unready_reason);
            }
            Ok(())
        })
        .await?;
    Ok(())
}

fn dump_pod_spec(p: &Pod) {
    let spec = p.spec.as_ref().unwrap();
    log::info!("podspec {:?}", spec);
}

#[tokio::main]
async fn main() -> Result<()> {
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
        let mut pod_obj = pod::DDPod::default();
        pod_obj.metadata.name = ddOption::from(Some(pod_name));

        unsafe {
            hddlog_g.transaction_start();

            let updates = vec![Update::Insert {
                // We are going to insert..
                relid: Relations::pod_DDPod as RelId, // .. into relation with this Id.
                // `Word1` type, declared in the `types` crate has the same fields as
                // the corresponding DDlog type.
                v: pod_obj.into_ddvalue(),
            }];
            hddlog_g.apply_updates(&mut updates.into_iter());

            let mut delta = hddlog_g.transaction_commit_dump_changes().unwrap();
            dump_delta(&hddlog_g, &delta);
        }
    }
    Ok(())
}

// From https://github.com/vmware/differential-datalog/blob/master/test/datalog_tests/rust_api_test/src/main.rs
//
fn init_ddlog() -> HDDlog {
    // Create a DDlog configuration with 1 worker thread and with the self-profiling feature
    // enabled.
    let config = Config::new()
        .with_timely_workers(1)
        .with_profiling_config(ProfilingConfig::SelfProfiling);
    // Instantiate the DDlog program with this configuration.
    // The second argument of `run_with_config` is a Boolean flag that indicates
    // whether DDlog will track the complete snapshot of output relations.  It
    // should only be set for debugging in order to dump the contents of output
    // tables using `HDDlog::dump_table()`.  Otherwise, indexes are the preferred
    // way to achieve this.
    let (hddlog, init_state) = myddlog::run_with_config(config, false).unwrap();

    // Alternatively, use `tutorial_ddlog::run` to instantiate the program with default
    // configuration.  The first argument specifies the number of workers.

    // let (hddlog, init_state) = tutorial_ddlog::run(1, false)? HDDlog,;

    println!("Initial state");
    dump_delta(&hddlog, &init_state);

    hddlog.transaction_start();

    // A transaction can consist of multiple `apply_updates()` calls, each taking
    // multiple updates.  An update inserts, deletes or modifies a record in a DDlog
    // relation.
    let updates = vec![
        /*Update::Insert {
            // We are going to insert..
            relid: Relations::Word1 as RelId, // .. into relation with this Id.
            // `Word1` type, declared in the `types` crate has the same fields as
            // the corresponding DDlog type.
            v: Word1 {
                word: "foo-".to_string(),
                cat: Category::CategoryOther,
            }
            .into_ddvalue(),
        },
        Update::Insert {
            relid: Relations::Word2 as RelId,
            v: Word2 {
                word: "bar".to_string(),
                cat: Category::CategoryOther,
            }
            .into_ddvalue(),
        },
        */
    ];
    hddlog.apply_updates(&mut updates.into_iter());

    // Commit the transaction; returns a `DeltaMap` object that contains the set
    // of changes to output relations produced by the transaction.
    let mut delta = hddlog.transaction_commit_dump_changes().unwrap();
    //assert_eq!(delta, delta_expected);

    dump_delta(&hddlog, &delta);
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
