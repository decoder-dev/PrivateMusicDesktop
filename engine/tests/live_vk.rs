//! Talks to the real api.vk.ru with a deliberately invalid token, to check
//! that real VK answers parse the way the client expects. Needs internet, so
//! it only runs with `--ignored`. No credentials are involved.

use pm_engine::vk::{Session, VkClient};
use pm_engine::Error;

fn bogus() -> Session {
    Session {
        access_token: "this-is-not-a-valid-vk-token-0123456789".into(),
        user_id: Some(1),
        expires_at: None,
        api_user_agent: pm_engine::vk::API_USER_AGENT.into(),
        refresh_cookie: String::new(),
        web_user_agent: "test".into(),
    }
}

#[test]
#[ignore = "needs internet"]
fn vk_rejects_a_bad_token_as_unauthorized_not_as_a_parse_error() {
    let client = VkClient::new().unwrap();
    let error = client.library(&bogus(), 0, 10).unwrap_err();
    assert_eq!(error, Error::Unauthorized, "real VK error body must map to Unauthorized, got {error:?}");
    let error = client.search(&bogus(), "кино", 0, 10).unwrap_err();
    assert_eq!(error, Error::Unauthorized);
}

#[test]
#[ignore = "needs internet"]
fn the_web_token_endpoint_says_not_signed_in_without_cookies() {
    // A made-up cookie is not a session: VK must answer "not signed in".
    let result = pm_engine::vk::exchange_web_session("remixlang=0; remixsid=bogus", "Mozilla/5.0 test");
    assert_eq!(result.unwrap_err(), Error::Unauthorized);
}
