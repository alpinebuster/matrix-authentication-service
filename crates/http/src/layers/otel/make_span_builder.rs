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

use std::borrow::Cow;

use http::{Method, Request, Version};
use hyper::client::connect::dns::Name;
use opentelemetry::trace::{SpanBuilder, SpanKind};
use opentelemetry_semantic_conventions::trace::{
    HTTP_FLAVOR, HTTP_METHOD, HTTP_URL, NET_HOST_NAME,
};

pub trait MakeSpanBuilder<R> {
    fn make_span_builder(&self, request: &R) -> SpanBuilder;
}

#[derive(Debug, Clone, Copy)]
pub struct DefaultMakeSpanBuilder {
    operation: &'static str,
}

impl DefaultMakeSpanBuilder {
    #[must_use]
    pub fn new(operation: &'static str) -> Self {
        Self { operation }
    }
}

impl Default for DefaultMakeSpanBuilder {
    fn default() -> Self {
        Self {
            operation: "service",
        }
    }
}

impl<R> MakeSpanBuilder<R> for DefaultMakeSpanBuilder {
    fn make_span_builder(&self, _request: &R) -> SpanBuilder {
        SpanBuilder::from_name(self.operation)
    }
}

#[inline]
fn http_method_str(method: &Method) -> Cow<'static, str> {
    match method {
        &Method::OPTIONS => "OPTIONS".into(),
        &Method::GET => "GET".into(),
        &Method::POST => "POST".into(),
        &Method::PUT => "PUT".into(),
        &Method::DELETE => "DELETE".into(),
        &Method::HEAD => "HEAD".into(),
        &Method::TRACE => "TRACE".into(),
        &Method::CONNECT => "CONNECT".into(),
        &Method::PATCH => "PATCH".into(),
        other => other.to_string().into(),
    }
}

#[inline]
fn http_flavor(version: Version) -> Cow<'static, str> {
    match version {
        Version::HTTP_09 => "0.9".into(),
        Version::HTTP_10 => "1.0".into(),
        Version::HTTP_11 => "1.1".into(),
        Version::HTTP_2 => "2.0".into(),
        Version::HTTP_3 => "3.0".into(),
        other => format!("{:?}", other).into(),
    }
}

#[derive(Debug, Clone)]
pub struct SpanFromHttpRequest {
    operation: &'static str,
    span_kind: SpanKind,
}

impl SpanFromHttpRequest {
    #[must_use]
    pub fn server() -> Self {
        Self {
            operation: "http-server",
            span_kind: SpanKind::Server,
        }
    }

    #[must_use]
    pub fn inner_client() -> Self {
        Self {
            operation: "http-client",
            span_kind: SpanKind::Client,
        }
    }

    #[must_use]
    pub fn client(operation: &'static str) -> Self {
        Self {
            operation,
            span_kind: SpanKind::Client,
        }
    }
}

impl<B> MakeSpanBuilder<Request<B>> for SpanFromHttpRequest {
    fn make_span_builder(&self, request: &Request<B>) -> SpanBuilder {
        let attributes = vec![
            HTTP_METHOD.string(http_method_str(request.method())),
            HTTP_FLAVOR.string(http_flavor(request.version())),
            HTTP_URL.string(request.uri().to_string()),
        ];

        SpanBuilder::from_name(self.operation)
            .with_kind(self.span_kind.clone())
            .with_attributes(attributes)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SpanFromDnsRequest;

impl MakeSpanBuilder<Name> for SpanFromDnsRequest {
    fn make_span_builder(&self, request: &Name) -> SpanBuilder {
        let attributes = vec![NET_HOST_NAME.string(request.as_str().to_string())];

        SpanBuilder::from_name("resolve")
            .with_kind(SpanKind::Client)
            .with_attributes(attributes)
    }
}