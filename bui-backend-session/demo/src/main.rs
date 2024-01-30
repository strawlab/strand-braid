use http_body_util::BodyExt;

use bui_backend_session::future_session;
use bui_backend_session_types::AccessToken;

#[tokio::main]
async fn main() -> Result<(), bui_backend_session::Error> {
    env_logger::init();

    // this contacts the demo included with bui-backend
    let base = "http://[::1]:3410";

    // This makes initial contact to get cookie.
    future_session(base, AccessToken::NoToken).await?;

    // now make callback
    let bytes = r#"{"SetIsRecordingFmf":true}"#;
    let body = http_body_util::Full::new(bytes::Bytes::from(bytes))
        .map_err(|_: std::convert::Infallible| unreachable!())
        .boxed();

    let mut sess2 = future_session(base, AccessToken::NoToken).await?;
    let resp = sess2.post("callback", body).await;
    println!("got post response {:?}", resp);
    Ok(())
}
