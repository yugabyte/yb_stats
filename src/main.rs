//! yb_stats: a utility to extract all possible data from a YugabyteDB cluster.
//!
//! This utility can:
//! - Read YugabyteDB http endpoints and write the distinct groups of data into CSV files (`--snapshot`)
//! - Read YugabyteDB http endpoints and report results back directory (`--print-*` without a snapshot number),
//!   and the adhoc snapshot mode.
//! - Read yb_stats snapshots (CSV), and report the difference (`--*-diff`).
//! - Read yb_stats snapshots (CSV), and report the snapshot data (`--print-* <NR>`).
//!
//! This main file contains the [Opts] struct for commandline options via clap.
//! It then calls the tasks using the opts structure.
//!
#![allow(rustdoc::private_intra_doc_links)]
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate csv;

use clap::Parser;
use std::collections::HashMap;
use dotenv::dotenv;
use anyhow::Result;

mod snapshot;
mod statements;
mod threads;
mod memtrackers;
mod gflags;
mod loglines;
mod versions;
mod node_exporter;
mod entities;
mod masters;
mod rpcs;
mod pprof;
mod mems;
mod metrics;
mod utility;
mod isleader;
mod tablet_servers;
mod vars;
mod clocks;
mod cluster_config;
mod health_check;
mod table_detail;
mod tablet_detail;
mod tasks;
mod tablet_replication;
mod tablet_server_operations;
mod drives;

// constants
const DEFAULT_HOSTS: &str = "192.168.66.80,192.168.66.81,192.168.66.82";
const DEFAULT_PORTS: &str = "7000,9000,12000,13000";
const DEFAULT_PARALLEL: &str = "1";
/// Write the `.env` in the current working directory?
const WRITE_DOTENV: bool = true;
/// Accept certificates not signed by an official CA?
const ACCEPT_INVALID_CERTS: bool = true;

/// yb_stats switches
#[derive(Debug, Parser)]
#[clap(version, about, long_about = None)]
pub struct Opts {
    /// Snapshot input hostnames (comma separated)
    #[arg(short = 'H', long, value_name = "hostname,hostname")]
    hosts: Option<String>,
    /// Snapshot input port numbers (comma separated)
    #[arg(short = 'P', long, value_name = "port,port")]
    ports: Option<String>,
    /// Snapshot capture parallelism (default 1)
    #[arg(short = 'p', long, value_name = "nr")]
    parallel: Option<String>,
    /// Output filter for statistic names as regex
    #[arg(short, long, value_name = "regex")]
    stat_name_match: Option<String>,
    /// Output filter for table names as regex (requires --details-enable)
    #[arg(short, long, value_name = "regex")]
    table_name_match: Option<String>,
    /// Output filter for hostname or ports as regex
    #[arg(long, value_name = "regex")]
    hostname_match: Option<String>,
    /// Output setting to add statistics that are not counters
    #[arg(short, long)]
    gauges_enable: bool,
    /// Output setting to increase detail, such as report each table and tablet individually
    #[arg(short, long)]
    details_enable: bool,
    /// Snapshot setting to be as silent as possible, only errors are printed
    #[arg(long)]
    silent: bool,
    /// Perform a snapshot (creates stored JSON files)
    #[arg(long)]
    snapshot: bool,
    /// Snapshot add comment in snapshot overview
    #[arg(long, value_name = "\"comment\"")]
    snapshot_comment: Option<String>,
    /// Create a performance diff report using a begin and an end snapshot number.
    #[arg(long)]
    snapshot_diff: bool,
    /// Create a diff report using a begin and an end snapshot number without performance figures.
    #[arg(long)]
    snapshot_nonmetrics_diff: bool,
    /// Create a metric diff report using a begin and end snapshot number.
    #[arg(long)]
    metrics_diff: bool,
    /// Create an entity diff report using a begin and end snapshot number.
    #[arg(long)]
    entity_diff: bool,
    /// Create a masters diff report using a begin and end snapshot number.
    #[arg(long)]
    masters_diff: bool,
    /// Create a tablet servers diff report using a begin and end snapshot number.
    #[arg(long)]
    tablet_servers_diff: bool,
    /// Create a vars diff report using a begin and end snapshot number.
    #[arg(long)]
    vars_diff: bool,
    /// Create a node_exporter diff report using a begin and end snapshot number.
    #[arg(long)]
    node_exporter_diff: bool,
    /// Create a (YSQL) statements diff report using a begin and end snapshot number.
    #[arg(long)]
    statements_diff: bool,
    /// Create a versions diff report using a begin and end snapshot number.
    #[arg(long)]
    versions_diff: bool,
    /// Create an adhoc diff report only for metrics
    #[arg(long)]
    adhoc_metrics_diff: bool,
    /// Create an adhoc diff report only for node_exporter
    #[arg(long)]
    adhoc_node_exporter_diff: bool,
    /// Create an adhoc diff report excluding metrics
    #[arg(long)]
    adhoc_nonmetrics_diff: bool,
    /// Lists the snapshots in the yb_stats.snapshots in the current directory.
    #[arg(short = 'l', long)]
    snapshot_list: bool,
    /// Output setting to specify the begin snapshot number for diff report.
    #[arg(short = 'b', long, value_name = "snapshot number")]
    begin: Option<i32>,
    /// Output setting to specify the end snapshot number for diff report.
    #[arg(short = 'e', long, value_name = "snapshot number")]
    end: Option<i32>,
    /// Print memtrackers data for the given snapshot number
    #[arg(long, value_name = "snapshot number")]
    print_memtrackers: Option<Option<String>>,
    /// tail log data
    #[arg(long)]
    tail_log: bool,
    /// Print log data for the given snapshot number
    #[arg(long, value_name = "snapshot number")]
    print_log: Option<Option<String>>,
    /// Output log data severity to include: optional: I (use with --print_log)
    #[arg(long, default_value = "WEF")]
    log_severity: String,
    /// Print entity data for snapshot number, or get current.
    #[arg(long, value_name = "snapshot number")]
    print_entities: Option<Option<String>>,
    /// Print master server data for snapshot number, or get current.
    #[arg(long, value_name = "snapshot number")]
    print_masters: Option<Option<String>>,
    /// Print tablet server data for snapshot number, or get current.
    #[arg(long, value_name = "snapshot number")]
    print_tablet_servers: Option<Option<String>>,
    /// Print vars for snapshot number, or get current
    #[arg(long, value_name = "snapshot number")]
    print_vars: Option<Option<String>>,
    /// Print version data for snapshot number, or get current.
    #[arg(long, value_name = "snapshot number")]
    print_version: Option<Option<String>>,
    /// Print rpcs for the given snapshot number, or get current.
    #[arg(long, value_name = "snapshot number")]
    print_rpcs: Option<Option<String>>,
    /// print clocks for the given snapshot number, or get current.
    #[arg(long, value_name = "snapshot_number")]
    print_clocks: Option<Option<String>>,
    /// print master leader tablet server latencies
    #[arg(long, value_name = "snapshot_number")]
    print_latencies: Option<Option<String>>,
    /// Print threads data for the given snapshot number, or get current.
    #[arg(long, value_name = "snapshot number")]
    print_threads: Option<Option<String>>,
    /// Print gflags for the given snapshot number, or get current.
    #[arg(long, value_name = "snapshot number")]
    print_gflags: Option<Option<String>>,
    /// Print cluster-config for the given snapshot number, or get current.
    #[arg(long, value_name = "snapshot number")]
    print_cluster_config: Option<Option<String>>,
    /// Print health-check for the given snapshot number, or get current.
    #[arg(long, value_name = "snapshot number")]
    print_health_check: Option<Option<String>>,
    /// Print the drive info for the given snapshot number, or get current.
    #[arg(long, value_name = "snapshot number")]
    print_drives: Option<Option<String>>,
    /// Print the tablet server operations for the given snapshot number, or get current.
    #[arg(long, value_name = "snapshot number")]
    print_tablet_server_operations: Option<Option<String>>,
    /// Print the master tasks for the given snapshot number, or get current.
    #[arg(long, value_name = "snapshot number")]
    print_master_tasks: Option<Option<String>>,
    /// Print the table detail the given snapshot number, or get current.
    #[arg(long, value_name = "snapshot number")]
    print_table_detail: Option<Option<String>>,
    /// Print the tablet detail the given snapshot number, or get current.
    #[arg(long, value_name = "snapshot number")]
    print_tablet_detail: Option<Option<String>>,
    /// UUID for table-detail
    #[arg(long, value_name = "uuid", default_value = "")]
    uuid: String,
    /// Snapshot disable gathering of thread stacks from /threadz
    #[arg(long)]
    disable_threads: bool,
    /// Snapshot add very detailed data to snapshot
    #[arg(long)]
    extra_data: bool,
    /// Output setting for the length of the SQL text to display
    #[arg(long, value_name = "nr", default_value = "80")]
    sql_length: usize,
    /// Get the hostname for the tablet leader of a colocated YSQL database.
    #[arg(long, hide = true, value_name = "ysql colocated database name")]
    get_coloc_leader_host: Option<String>,
}

/// The entrypoint of the executable.
#[tokio::main]
async fn main() -> Result<()>
{
    env_logger::init();
    let mut changed_options = HashMap::new();
    dotenv().ok();
    let options = Opts::parse();

    let hosts = utility::set_hosts(&options.hosts, &mut changed_options);
    let ports = utility::set_ports(&options.ports, &mut changed_options);
    let parallel = utility::set_parallel(&options.parallel, &mut changed_options);

    match &options {
        Opts { snapshot, ..                 } if *snapshot                       => snapshot::perform_snapshot(hosts, ports, parallel, &options).await?,
        Opts { snapshot_diff, ..            } if *snapshot_diff                  => snapshot::snapshot_diff(&options).await?,
        Opts { snapshot_nonmetrics_diff, .. } if *snapshot_nonmetrics_diff       => snapshot::snapshot_nonmetrics_diff(&options).await?,
        Opts { snapshot_list, ..            } if *snapshot_list                  => snapshot::snapshot_diff(&options).await?,
        Opts { metrics_diff, ..              } if *metrics_diff                    => metrics::metrics_diff(&options).await?,
        Opts { entity_diff, ..              } if *entity_diff                    => entities::entity_diff(&options).await?,
        Opts { masters_diff, ..             } if *masters_diff                   => masters::masters_diff(&options).await?,
        Opts { tablet_servers_diff, ..             } if *tablet_servers_diff                   => tablet_servers::tablet_servers_diff(&options).await?,
        Opts { vars_diff, ..             } if *vars_diff                   => vars::vars_diff(&options).await?,
        Opts { node_exporter_diff, ..             } if *node_exporter_diff                   => node_exporter::node_exporter_diff(&options).await?,
        Opts { statements_diff, ..             } if *statements_diff                   => statements::statements_diff(&options).await?,
        Opts { versions_diff, ..            } if *versions_diff                  => versions::versions_diff(&options).await?,
        Opts { print_memtrackers, ..        } if print_memtrackers.is_some()     => memtrackers::print_memtrackers(hosts, ports, parallel, &options).await?,
        Opts { print_version, ..            } if print_version.is_some()         => versions::print_version(hosts, ports, parallel, &options).await?,
        Opts { print_threads, ..            } if print_threads.is_some()         => threads::print_threads(hosts, ports, parallel, &options).await?,
        Opts { print_entities, ..           } if print_entities.is_some()        => entities::print_entities(hosts, ports, parallel, &options).await?,
        Opts { print_masters, ..            } if print_masters.is_some()         => masters::print_masters(hosts, ports, parallel, &options).await?,
        Opts { print_tablet_servers, ..     } if print_tablet_servers.is_some()  => tablet_servers::print_tablet_servers(hosts, ports, parallel, &options).await?,
        Opts { print_vars, ..               } if print_vars.is_some()            => vars::print_vars(hosts, ports, parallel, &options).await?,
        Opts { print_clocks, ..             } if print_clocks.is_some()          => clocks::print_clocks(hosts, ports, parallel, &options).await?,
        Opts { print_latencies, ..          } if print_latencies.is_some()       => clocks::print_latencies(hosts, ports, parallel, &options).await?,
        Opts { print_rpcs, ..               } if print_rpcs.is_some()            => rpcs::print_rpcs(hosts, ports, parallel, &options).await?,
        Opts { print_log, ..                } if print_log.is_some()             => loglines::print_loglines(hosts, ports, parallel, &options).await?,
        Opts { tail_log, ..                 } if *tail_log                       => loglines::tail_loglines(hosts, ports, parallel, &options).await?,
        Opts { adhoc_metrics_diff, ..       } if *adhoc_metrics_diff             => snapshot::adhoc_metrics_diff(hosts, ports, parallel, &options).await?,
        Opts { adhoc_node_exporter_diff, ..       } if *adhoc_node_exporter_diff             => snapshot::adhoc_node_exporter_diff(hosts, ports, parallel, &options).await?,
        Opts { adhoc_nonmetrics_diff, ..    } if *adhoc_nonmetrics_diff          => snapshot::adhoc_nonmetrics_diff(hosts, ports, parallel, &options).await?,
        Opts { print_gflags, ..             } if print_gflags.is_some()          => gflags::print_gflags(hosts, ports, parallel, &options).await?,
        Opts { print_cluster_config, ..     } if print_cluster_config.is_some()  => cluster_config::print_cluster_config(hosts, ports, parallel, &options).await?,
        Opts { print_health_check, ..       } if print_health_check.is_some()    => health_check::print_health_check(hosts, ports, parallel, &options).await?,
        Opts { print_drives, ..       } if print_drives.is_some()    => drives::print_drives(hosts, ports, parallel, &options).await?,
        Opts { print_tablet_server_operations, ..       } if print_tablet_server_operations.is_some()    => tablet_server_operations::print_operations(hosts, ports, parallel, &options).await?,
        Opts { print_master_tasks, ..       } if print_master_tasks.is_some()    => tasks::print_tasks(hosts, ports, parallel, &options).await?,
        Opts { print_table_detail, ..       } if print_table_detail.is_some()    => table_detail::print_table_detail(hosts, ports, parallel, &options).await?,
        Opts { print_tablet_detail, ..       } if print_tablet_detail.is_some()    => tablet_detail::print_tablet_detail(hosts, ports, parallel, &options).await?,
        Opts { get_coloc_leader_host, ..    } if get_coloc_leader_host.is_some() => entities::print_coloc_leader_host(hosts, ports, parallel, &options).await?,
        _                                                                        => snapshot::adhoc_diff(hosts, ports, parallel, &options).await?,
    };
    // if we are allowed to write, and changed_options does contain values, write them to '.env'
    utility::dotenv_writer(WRITE_DOTENV, changed_options)?;

    Ok(())
}
