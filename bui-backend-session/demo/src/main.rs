use bui_backend_session::future_session;
use bui_backend_types::AccessToken;

#[tokio::main]
async fn main() -> Result<(),hyper::Error> {
    env_logger::init();

    // this contacts the demo included with bui-backend
    let base = "http://[::1]:3410";

    // This makes initial contact to get cookie.
    future_session(base, AccessToken::NoToken).await?;

    // now make callback
    let bytes = r#"{"SetIsRecordingFmf":true}"#;
    let body = hyper::Body::from(bytes);

    let mut sess2 = future_session(base, AccessToken::NoToken).await?;
    let resp = sess2.post("callback", body).await;
    println!("got post response {:?}", resp);
    Ok(())
}
