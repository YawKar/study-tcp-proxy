use tokio::io::copy_bidirectional;
use tokio::net::TcpStream;
use tracing::instrument;

#[instrument(
    skip_all,
    fields(
        client = %client_stream.peer_addr().unwrap(),
        target = %target_stream.peer_addr().unwrap(),
    ),
)]
pub(crate) async fn handle_stream(
    client_stream: &mut TcpStream,
    target_stream: &mut TcpStream,
) -> Result<(u64, u64), std::io::Error> {
    copy_bidirectional(client_stream, target_stream).await
}
