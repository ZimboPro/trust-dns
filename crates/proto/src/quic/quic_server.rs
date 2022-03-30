// Copyright 2015-2022 Benjamin Fry <benjaminfry@me.com>
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use std::{io, net::SocketAddr, sync::Arc};

use futures_util::StreamExt;
use quinn::{
    Connecting, Connection, ConnectionError, Endpoint, EndpointConfig, Incoming, IncomingBiStreams,
    NewConnection, ServerConfig, TransportConfig, VarInt,
};
use rustls::{server::ServerConfig as TlsServerConfig, Certificate, PrivateKey};

use crate::{error::ProtoError, quic::quic_client_stream, udp::UdpSocket};

use super::quic_stream::{self, QuicStream};

pub struct QuicServer {
    endpoint: Endpoint,
    incoming: Incoming,
}

impl QuicServer {
    /// Construct the new Acceptor with the associated pkcs12 data
    pub async fn new(
        name_server: SocketAddr,
        cert: Vec<Certificate>,
        key: PrivateKey,
    ) -> Result<Self, ProtoError> {
        // setup a new socket for the server to use
        let socket = <tokio::net::UdpSocket as UdpSocket>::bind(name_server).await?;
        Self::with_socket(socket, cert, key)
    }

    /// Construct the new server with an existing socket
    pub fn with_socket(
        socket: tokio::net::UdpSocket,
        cert: Vec<Certificate>,
        key: PrivateKey,
    ) -> Result<Self, ProtoError> {
        let mut config = TlsServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(cert, key)?;

        config.alpn_protocols = vec![quic_stream::DOQ_ALPN.to_vec()];

        let server_config = ServerConfig::with_crypto(Arc::new(config));

        let socket = socket.into_std()?;

        let (endpoint, incoming) =
            Endpoint::new(EndpointConfig::default(), Some(server_config), socket)?;

        Ok(Self { endpoint, incoming })
    }

    /// Get the next incoming stream
    ///
    /// # Returns
    ///
    /// A remote connection that could have many potential bi-directional streams and the remote socket address
    pub async fn next(&mut self) -> Result<Option<(QuicStreams, SocketAddr)>, ProtoError> {
        let connecting = if let Some(conn) = self.incoming.next().await {
            conn
        } else {
            return Ok(None);
        };

        let remote_addr = connecting.remote_address();
        let conn = connecting.await?;
        let streams = QuicStreams {
            incoming_bi_streams: conn.bi_streams,
        };

        Ok(Some((streams, remote_addr)))
    }

    /// Returns the address this server is listening on
    ///
    /// This can be useful in tests, where a random port can be associated with the server by binding on `127.0.0.1:0` and then getting the
    ///   associated port address with this function.
    pub fn local_addr(&self) -> Result<SocketAddr, io::Error> {
        self.endpoint.local_addr()
    }
}

pub struct QuicStreams {
    incoming_bi_streams: IncomingBiStreams,
}

impl QuicStreams {
    /// Get the next bi directional stream from the client
    pub async fn next(&mut self) -> Option<Result<QuicStream, ProtoError>> {
        match self.incoming_bi_streams.next().await? {
            Ok((send_stream, receive_stream)) => {
                Some(Ok(QuicStream::new(send_stream, receive_stream)))
            }
            Err(e) => Some(Err(e.into())),
        }
    }
}
