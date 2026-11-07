#[tokio::main]
async fn main() {
    let port: u16 = std::env::args().nth(1).and_then(|p| p.parse().ok()).unwrap_or(4010);
    let (addr, handle) = enocone_fixture::serve(([127, 0, 0, 1], port).into()).await;
    eprintln!("enocone fixture app on http://{addr} (login: alice / secret)");
    handle.await.ok();
}
