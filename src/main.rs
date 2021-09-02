#[macro_use]
extern crate log;

pub use color_eyre::Result;
use futures::prelude::*;

use k8s_openapi::{
    api::core::v1::{
        Event,
    },
};

use kube::{
    api::{ListParams, ResourceExt},
    Api, Client, Config as KubeConfig,
};

use kube_runtime::{utils::try_flatten_applied, watcher};

use kube_policy_ddlog as myddlog;

// The `Relations` enum enumerates program relations
pub use myddlog::Relations;

// Type and function definitions generated for each ddlog program
pub mod ddtypes {
    pub use kube_policy_ddlog::typedefs::*;
}

pub use myddlog::typedefs::ddlog_std::Map as ddMap;
pub use myddlog::typedefs::ddlog_std::Option as ddOption;
pub use myddlog::typedefs::ddlog_std::Vec as ddVec;

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
pub use differential_datalog::ddval::DDValConvert;

// Generic type that wraps all DDlog values.
pub use differential_datalog::ddval::DDValue;

pub use differential_datalog::program::RelId; // Numeric relations id.
pub use differential_datalog::program::Update; // Type-safe representation of a DDlog command (insert/delete_val/delete_key/...)

use lazy_static::lazy_static;
use std::sync::Mutex;

mod pod;
mod node;
mod common;

pub use common::get_object_metadata;

use pod::pod_watcher;
use node::node_watcher;

lazy_static! {
    pub static ref hddlog_g: Mutex<HDDlog> = Mutex::new(init_ddlog());
}

fn perform_hddlog_transaction(updates: Vec<Update<DDValue>>) {
    let hddlog = hddlog_g.lock().unwrap();

    hddlog.transaction_start().unwrap();

    hddlog.apply_updates(&mut updates.into_iter()).unwrap();

    log::info!("Commiting changes");
    let mut delta = hddlog.transaction_commit_dump_changes().unwrap();
    dump_delta(&hddlog, &delta);
}

#[tokio::main]
async fn main() -> Result<()> {
    std::env::set_var("RUST_LOG", "info,kube=debug");
    env_logger::init();

    let pw_join = tokio::spawn(async {
        println!("await pod_watcher");
        pod_watcher().await;
    });

    let node_join = tokio::spawn(async {
        println!("await node_watcher");
        node_watcher().await;
    });

    tokio::join!(
        pw_join,
        node_join,
    );

    Ok(())
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
        /*unsafe {
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
        }*/
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
