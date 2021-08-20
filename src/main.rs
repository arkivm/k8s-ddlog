#[macro_use]
extern crate log;

use color_eyre::Result;
use futures::prelude::*;
use k8s_openapi::api::core::v1::Event;
use kube::{
    api::{ListParams, ResourceExt},
    Api, Client,
};
use kube_runtime::{utils::try_flatten_applied, watcher};

use kube_policy_ddlog as myddlog;

// The `Relations` enum enumerates program relations
use myddlog::Relations;

// Type and function definitions generated for each ddlog program
use myddlog::typedefs::*;

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "info,kube=debug");
    env_logger::init();

    let client = Client::try_default().await?;

    let events: Api<Event> = Api::all(client);
    let lp = ListParams::default();

    let mut ew = try_flatten_applied(watcher(events, lp)).boxed();

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
        let mut pod_obj = Pod::default();
        pod_obj.name = pod_name;

        unsafe {
            hddlog_g.transaction_start();

            let updates = vec![Update::Insert {
                // We are going to insert..
                relid: Relations::Pod as RelId, // .. into relation with this Id.
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
