// Copyright 2022 The Matrix.org Foundation C.I.C.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, ToSocketAddrs},
    os::unix::net::UnixListener,
    sync::Arc,
};

use anyhow::Context;
use axum::{body::HttpBody, Extension, Router};
use listenfd::ListenFd;
use mas_config::{HttpBindConfig, HttpResource, HttpTlsConfig, UnixOrTcp};
use mas_handlers::AppState;
use mas_listener::{unix_or_tcp::UnixOrTcpListener, ConnectionInfo};
use mas_router::Route;
use rustls::ServerConfig;

#[allow(clippy::trait_duplication_in_bounds)]
pub fn build_router<B>(state: &Arc<AppState>, resources: &[HttpResource]) -> Router<AppState, B>
where
    B: HttpBody + Send + 'static,
    <B as HttpBody>::Data: Send,
    <B as HttpBody>::Error: std::error::Error + Send + Sync,
{
    let mut router = Router::with_state_arc(state.clone());

    for resource in resources {
        router = match resource {
            mas_config::HttpResource::Health => {
                router.merge(mas_handlers::healthcheck_router(state.clone()))
            }
            mas_config::HttpResource::Prometheus => {
                router.route_service("/metrics", crate::telemetry::prometheus_service())
            }
            mas_config::HttpResource::Discovery => {
                router.merge(mas_handlers::discovery_router(state.clone()))
            }
            mas_config::HttpResource::Human => {
                router.merge(mas_handlers::human_router(state.clone()))
            }
            mas_config::HttpResource::Static { web_root } => {
                let handler = mas_static_files::service(web_root);
                router.nest(mas_router::StaticAsset::route(), handler)
            }
            mas_config::HttpResource::OAuth => {
                router.merge(mas_handlers::api_router(state.clone()))
            }
            mas_config::HttpResource::Compat => {
                router.merge(mas_handlers::compat_router(state.clone()))
            }
            // TODO: do a better handler here
            mas_config::HttpResource::ConnectionInfo => router.route(
                "/connection-info",
                axum::routing::get(|connection: Extension<ConnectionInfo>| async move {
                    format!("{connection:?}")
                }),
            ),
        }
    }

    router
}

pub fn build_tls_server_config(config: &HttpTlsConfig) -> Result<ServerConfig, anyhow::Error> {
    let (key, chain) = config.load()?;
    let key = rustls::PrivateKey(key);
    let chain = chain.into_iter().map(rustls::Certificate).collect();

    let mut config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .context("failed to build TLS server config")?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    Ok(config)
}

pub fn build_listeners(
    fd_manager: &mut ListenFd,
    configs: &[HttpBindConfig],
) -> Result<Vec<UnixOrTcpListener>, anyhow::Error> {
    let mut listeners = Vec::with_capacity(configs.len());

    for bind in configs {
        let listener = match bind {
            HttpBindConfig::Listen { host, port } => {
                let addrs = match host.as_deref() {
                    Some(host) => (host, *port)
                        .to_socket_addrs()
                        .context("could not parse listener host")?
                        .collect(),

                    None => vec![
                        SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), *port),
                        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), *port),
                    ],
                };

                let listener = TcpListener::bind(&addrs[..]).context("could not bind address")?;
                listener.set_nonblocking(true)?;
                listener.try_into()?
            }

            HttpBindConfig::Address { address } => {
                let addr: SocketAddr = address
                    .parse()
                    .context("could not parse listener address")?;
                let listener = TcpListener::bind(addr).context("could not bind address")?;
                listener.set_nonblocking(true)?;
                listener.try_into()?
            }

            HttpBindConfig::Unix { socket } => {
                let listener = UnixListener::bind(socket).context("could not bind socket")?;
                listener.try_into()?
            }

            HttpBindConfig::FileDescriptor {
                fd,
                kind: UnixOrTcp::Tcp,
            } => {
                let listener = fd_manager
                    .take_tcp_listener(*fd)?
                    .context("no listener found on file descriptor")?;
                listener.set_nonblocking(true)?;
                listener.try_into()?
            }

            HttpBindConfig::FileDescriptor {
                fd,
                kind: UnixOrTcp::Unix,
            } => {
                let listener = fd_manager
                    .take_unix_listener(*fd)?
                    .context("no unix socket found on file descriptor")?;
                listener.set_nonblocking(true)?;
                listener.try_into()?
            }
        };

        listeners.push(listener);
    }

    Ok(listeners)
}