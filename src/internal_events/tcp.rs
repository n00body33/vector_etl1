use std::net::SocketAddr;

use metrics::counter;
use vector_core::internal_event::InternalEvent;

use crate::{
    emit,
    internal_events::{ComponentEventsDropped, UNINTENTIONAL},
    tls::TlsError,
};
use vector_common::internal_event::{error_stage, error_type};

#[derive(Debug)]
pub struct TcpSocketConnectionEstablished {
    pub peer_addr: Option<std::net::SocketAddr>,
}

impl InternalEvent for TcpSocketConnectionEstablished {
    fn emit(self) {
        if let Some(peer_addr) = self.peer_addr {
            debug!(message = "Connected.", %peer_addr);
        } else {
            debug!(message = "Connected.", peer_addr = "unknown");
        }
        counter!("connection_established_total", 1, "mode" => "tcp");
    }
}

#[derive(Debug)]
pub struct TcpSocketConnectionShutdown;

impl InternalEvent for TcpSocketConnectionShutdown {
    fn emit(self) {
        debug!(message = "Received EOF from the server, shutdown.");
        counter!("connection_shutdown_total", 1, "mode" => "tcp");
    }
}

#[derive(Debug)]
pub struct TcpSocketTlsConnectionError {
    pub error: TlsError,
}

impl InternalEvent for TcpSocketTlsConnectionError {
    fn emit(self) {
        match self.error {
            // Specific error that occurs when the other side is only
            // doing SYN/SYN-ACK connections for healthcheck.
            // https://github.com/vectordotdev/vector/issues/7318
            TlsError::Handshake { ref source }
                if source.code() == openssl::ssl::ErrorCode::SYSCALL
                    && source.io_error().is_none() =>
            {
                debug!(
                    message = "Connection error, probably a healthcheck.",
                    error = %self.error,
                    internal_log_rate_secs = 10,
                );
            }
            _ => {
                error!(
                    message = "Connection error.",
                    error = %self.error,
                    error_code = "connection_failed",
                    error_type = error_type::WRITER_FAILED,
                    stage = error_stage::SENDING,
                    internal_log_rate_secs = 10,
                );
            }
        }
        counter!(
            "component_errors_total", 1,
            "error_code" => "connection_failed",
            "error_type" => error_type::WRITER_FAILED,
            "stage" => error_stage::SENDING,
            "mode" => "tcp",
        );
        // deprecated
        counter!(
            "connection_errors_total", 1,
            "mode" => "tcp",
        );
    }
}

#[derive(Debug)]
pub struct TcpSocketSendError {
    pub error: std::io::Error,
}

impl InternalEvent for TcpSocketSendError {
    fn emit(self) {
        let reason = "TCP socket send error.";
        error!(
            message = reason,
            error = %self.error,
            error_code = "socket_failed",
            error_type = error_type::WRITER_FAILED,
            stage = error_stage::SENDING,
            internal_log_rate_secs = 10,
        );
        counter!(
            "component_errors_total", 1,
            "error_code" => "socket_failed",
            "error_type" => error_type::WRITER_FAILED,
            "stage" => error_stage::SENDING,
            "mode" => "tcp",
        );
        // deprecated
        counter!(
            "connection_errors_total", 1,
            "mode" => "tcp",
        );

        emit!(ComponentEventsDropped::<UNINTENTIONAL> { count: 1, reason });
    }
}

#[derive(Debug)]
pub struct TcpSendAckError {
    pub error: std::io::Error,
}

impl InternalEvent for TcpSendAckError {
    fn emit(self) {
        error!(
            message = "Error writing acknowledgement, dropping connection.",
            error = %self.error,
            error_code = "ack_failed",
            error_type = error_type::WRITER_FAILED,
            stage = error_stage::SENDING,
            internal_log_rate_secs = 10,
        );
        counter!(
            "component_errors_total", 1,
            "error_code" => "ack_failed",
            "error_type" => error_type::WRITER_FAILED,
            "stage" => error_stage::SENDING,
            "mode" => "tcp",
        );
        // deprecated
        counter!(
            "connection_errors_total", 1,
            "mode" => "tcp",
        );
        counter!(
            "connection_send_ack_errors_total", 1,
            "mode" => "tcp",
        );
    }
}

#[derive(Debug)]
pub struct TcpBytesReceived {
    pub byte_size: usize,
    pub peer_addr: SocketAddr,
}

impl InternalEvent for TcpBytesReceived {
    fn emit(self) {
        trace!(
            message = "Bytes received.",
            protocol = "tcp",
            byte_size = %self.byte_size,
            peer_addr = %self.peer_addr,
        );
        counter!(
            "component_received_bytes_total", self.byte_size as u64,
            "protocol" => "tcp",
            "peer_addr" => self.peer_addr.to_string()
        );
    }
}
