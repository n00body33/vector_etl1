use std::{fs::remove_file, path::PathBuf};

use bytes::BytesMut;
use codecs::StreamDecodingError;
use futures::StreamExt;
use tokio::net::UnixDatagram;
use tokio_util::codec::FramedRead;
use tracing::field;
use vector_common::internal_event::{ByteSize, BytesReceived, InternalEventHandle as _, Protocol};
use vector_core::EstimatedJsonEncodedSizeOf;

use crate::{
    codecs::Decoder,
    event::Event,
    internal_events::{
        SocketEventsReceived, SocketMode, SocketReceiveError, StreamClosedError,
        UnixSocketFileDeleteError,
    },
    shutdown::ShutdownSignal,
    sources::util::change_socket_permissions,
    sources::util::unix::UnixSocketMetadata,
    sources::Source,
    SourceSender,
};

/// Returns a `Source` object corresponding to a Unix domain datagram socket.
/// Passing in different functions for `decoder` and `handle_events` can allow
/// for different source-specific logic (such as decoding syslog messages in the
/// syslog source).
pub fn build_unix_datagram_source(
    listen_path: PathBuf,
    socket_file_mode: Option<u32>,
    max_length: usize,
    decoder: Decoder,
    handle_events: impl Fn(&mut [Event], &UnixSocketMetadata) + Clone + Send + Sync + 'static,
    shutdown: ShutdownSignal,
    out: SourceSender,
) -> crate::Result<Source> {
    Ok(Box::pin(async move {
        let socket = UnixDatagram::bind(&listen_path).expect("Failed to bind to datagram socket");
        info!(message = "Listening.", path = ?listen_path, r#type = "unix_datagram");

        change_socket_permissions(&listen_path, socket_file_mode)
            .expect("Failed to set socket permissions");

        let result = listen(socket, max_length, decoder, shutdown, handle_events, out).await;

        // Delete socket file.
        if let Err(error) = remove_file(&listen_path) {
            emit!(UnixSocketFileDeleteError {
                path: &listen_path,
                error
            });
        }

        result
    }))
}

async fn listen(
    socket: UnixDatagram,
    max_length: usize,
    decoder: Decoder,
    mut shutdown: ShutdownSignal,
    handle_events: impl Fn(&mut [Event], &UnixSocketMetadata) + Clone + Send + Sync + 'static,
    mut out: SourceSender,
) -> Result<(), ()> {
    let mut buf = BytesMut::with_capacity(max_length);
    let bytes_received = register!(BytesReceived::from(Protocol::UNIX));
    loop {
        buf.resize(max_length, 0);
        tokio::select! {
            recv = socket.recv_from(&mut buf) => {
                let (byte_size, address) = recv.map_err(|error| {
                    let error = codecs::decoding::Error::FramingError(error.into());
                    emit!(SocketReceiveError {
                        mode: SocketMode::Unix,
                        error: &error
                    })
                })?;

                let socket_metadata = get_socket_metadata(&socket, &address);
                let span = info_span!("datagram");
                span.record("peer_path", &field::debug(socket_metadata.peer_path_or_default()));

                bytes_received.emit(ByteSize(byte_size));

                let payload = buf.split_to(byte_size);

                let mut stream = FramedRead::new(payload.as_ref(), decoder.clone());

                loop {
                    match stream.next().await {
                        Some(Ok((mut events, _byte_size))) => {
                            emit!(SocketEventsReceived {
                                mode: SocketMode::Unix,
                                byte_size: events.estimated_json_encoded_size_of(),
                                count: events.len()
                            });

                            handle_events(&mut events, &socket_metadata);

                            let count = events.len();
                            if (out.send_batch(events).await).is_err() {
                                emit!(StreamClosedError { count });
                            }
                        },
                        Some(Err(error)) => {
                            emit!(SocketReceiveError {
                                mode: SocketMode::Unix,
                                error: &error
                            });
                            if !error.can_continue() {
                                break;
                            }
                        },
                        None => break,
                    }
                }
            }
            _ = &mut shutdown => return Ok(()),
        }
    }
}

fn get_socket_metadata(_socket: &tokio::net::UnixDatagram, peer_addr: &tokio::net::unix::SocketAddr) -> UnixSocketMetadata {
    let peer_path = if !peer_addr.is_unnamed() {
        peer_addr
            .as_pathname()
            .map(|p| { p.to_owned() })
            .map(|p| { p.to_string_lossy().into_owned().into() })
    } else {
        None
    };

    UnixSocketMetadata { peer_path }
}
