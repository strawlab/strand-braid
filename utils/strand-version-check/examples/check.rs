// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Download and parse the live version-check data, narrating each step.
//!
//! The library's [`VersionChecker::fetch`] intentionally collapses every
//! failure into `None` so a version check never disrupts the calling
//! application. That makes it useless as a diagnostic: a 404, a timeout, a
//! truncated body, and malformed JSON are all indistinguishable from "no update
//! available".
//!
//! This example instead performs the same request the library does
//! (`GET https://version-check.strawlab.org/<product>`) but reports the result
//! of every stage — HTTP status, bytes received, raw body, JSON parse, and
//! semver validation — and exits non-zero if any product fails to validate. It
//! doubles as a smoke test that the website is serving well-formed data.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example check
//! cargo run --example check -- braid   # check a single product
//! ```

use std::time::Duration;

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper_rustls::HttpsConnector;
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::TokioExecutor,
};
use serde::Deserialize;

/// Products the version-check service is expected to know about.
const PRODUCTS: &[&str] = &["braid", "strand-cam"];

type Body = BoxBody<Bytes, std::convert::Infallible>;

/// The wire format documented by the `strand-version-check` crate. Kept as its
/// own type here (the library's copy is private) so we can validate each field.
#[derive(Debug, Deserialize)]
struct VersionResponse {
    available: semver::Version,
    message: String,
    url: String,
}

fn empty_body() -> Body {
    Full::new(Bytes::new())
        .map_err(|never| match never {})
        .boxed()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Allow checking a single product passed on the command line; otherwise
    // check all of them.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let products: Vec<&str> = if args.is_empty() {
        PRODUCTS.to_vec()
    } else {
        args.iter().map(String::as_str).collect()
    };

    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_webpki_roots()
        .https_only()
        .enable_http1()
        .build();
    let client = Client::builder(TokioExecutor::new()).build::<_, Body>(https);
    let user_agent = concat!("strand-version-check-example/", env!("CARGO_PKG_VERSION"));

    let mut failures = 0;
    for product in &products {
        if !check_product(&client, user_agent, product).await {
            failures += 1;
        }
        println!();
    }

    if failures > 0 {
        eprintln!("RESULT: {failures} of {} product(s) failed", products.len());
        return Err(format!("{failures} product(s) returned no valid version data").into());
    }

    println!(
        "RESULT: all {} product(s) returned valid version data",
        products.len()
    );
    Ok(())
}

/// Run the full download-and-validate sequence for one product, printing the
/// outcome of each step. Returns `true` only if every step succeeded.
async fn check_product(
    client: &Client<HttpsConnector<HttpConnector>, Body>,
    user_agent: &str,
    product: &str,
) -> bool {
    let url = format!("https://version-check.strawlab.org/{product}");
    println!("=== {product} ===");

    // Step 1: build the request URI.
    let uri: hyper::Uri = match url.parse() {
        Ok(uri) => {
            println!("  [1/5] request URL ......... ok   GET {url}");
            uri
        }
        Err(e) => {
            println!("  [1/5] request URL ......... FAIL invalid URL {url}: {e}");
            return false;
        }
    };

    let req = hyper::Request::builder()
        .uri(&uri)
        .header(hyper::header::USER_AGENT, user_agent)
        .body(empty_body())
        .unwrap();

    // Step 2: send the request (bounded so a hung connection can't wedge us).
    let res = match tokio::time::timeout(Duration::from_secs(30), client.request(req)).await {
        Ok(Ok(res)) => res,
        Ok(Err(e)) => {
            println!("  [2/5] HTTP request ........ FAIL request failed: {e}");
            return false;
        }
        Err(_elapsed) => {
            println!("  [2/5] HTTP request ........ FAIL timed out after 30s");
            return false;
        }
    };

    // Step 3: check the status code.
    let status = res.status();
    if status == hyper::StatusCode::OK {
        println!("  [2/5] HTTP request ........ ok   {status}");
    } else {
        println!("  [2/5] HTTP request ........ FAIL unexpected status {status}");
        return false;
    }

    // Step 4: read the response body.
    let data = match res.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            println!("  [3/5] read body ........... FAIL could not read body: {e}");
            return false;
        }
    };
    println!("  [3/5] read body ........... ok   {} bytes", data.len());
    match std::str::from_utf8(&data) {
        Ok(s) => println!("        raw response: {}", s.trim()),
        Err(_) => println!("        raw response: <{} non-UTF-8 bytes>", data.len()),
    }

    // Step 5: parse the JSON and validate the fields.
    let parsed: VersionResponse = match serde_json::from_slice(&data) {
        Ok(v) => {
            println!("  [4/5] parse JSON .......... ok");
            v
        }
        Err(e) => {
            println!("  [4/5] parse JSON .......... FAIL {e}");
            return false;
        }
    };

    // serde already validated `available` as a semver::Version while parsing,
    // so reaching this point means every field is present and well-formed.
    println!("  [5/5] validate fields ..... ok");
    println!("        available version: {}", parsed.available);
    println!("        message:           {}", parsed.message);
    println!("        url:               {}", parsed.url);
    true
}
