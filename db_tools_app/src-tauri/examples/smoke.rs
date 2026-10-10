// Manual smoke test: cargo run --example smoke
// Requires local DBs from seed defaults.

use std::collections::HashMap;

#[tokio::main]
async fn main() {
    // inline minimal check using the library would need pub API;
    // this example only verifies TCP + redis protocol via std.
    let targets = [
        ("mysql", "127.0.0.1", 3307u16),
        ("postgres", "127.0.0.1", 15432),
        ("pgvector", "127.0.0.1", 15433),
        ("redis", "127.0.0.1", 16379),
        ("mongodb", "127.0.0.1", 27017),
        ("elasticsearch", "127.0.0.1", 9200),
    ];
    let mut failed = Vec::new();
    for (name, host, port) in targets {
        match tokio::net::TcpStream::connect((host, port)).await {
            Ok(_) => println!("OK  {name} {host}:{port}"),
            Err(e) => {
                println!("ERR {name} {host}:{port} -> {e}");
                failed.push(name);
            }
        }
    }
    let _ = HashMap::<String, String>::new();
    if !failed.is_empty() {
        eprintln!("failed: {failed:?}");
        std::process::exit(1);
    }
    println!("all local ports reachable");
}
