//! The ACME service binary: argument parsing and the socket live in the
//! library so the renewal loop is reachable from integration tests.

#[tokio::main]
async fn main() {
    if let Err(error) = acme_service::run().await {
        eprintln!("acme-service: {error}");
        std::process::exit(if error.kind() == anth_contract::ErrorKind::Invalid {
            2
        } else {
            1
        });
    }
}
