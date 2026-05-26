fn main() {
    use tokio_util::sync::CancellationToken;
    let cancel = CancellationToken::new();
    cancel.cancel();
    println!("Hello, world!");
}
