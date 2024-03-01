mod actix;
mod grpc;
mod routes;

use dash_network_api::ArcNetworkGraph;
use opentelemetry::global;
use tokio::spawn;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    ::ark_core::tracer::init_once();

    let signal = ::dash_pipe_provider::FunctionSignal::default();
    if let Err(error) = signal.trap_on_sigint() {
        error!("{error}");
        return;
    }

    let graph = ArcNetworkGraph::default();

    let handlers = vec![
        spawn(crate::actix::loop_forever(graph.clone())),
        spawn(crate::grpc::loop_forever(graph)),
    ];
    signal.wait_to_terminate().await;

    info!("Terminating...");
    for handler in handlers {
        handler.abort();
    }

    info!("Terminated.");
    global::shutdown_tracer_provider();
}