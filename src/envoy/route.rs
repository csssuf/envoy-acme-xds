use xds_api::pb::envoy::config::core::v3::DataSource;
use xds_api::pb::envoy::config::core::v3::data_source::Specifier;
use xds_api::pb::envoy::config::route::v3::{
    DirectResponseAction, Route, RouteMatch, route::Action, route_match::PathSpecifier,
};

/// Build an ACME challenge route that returns the key authorization
pub fn build_acme_challenge_route(token: &str, key_authorization: &str) -> Route {
    Route {
        name: format!("acme-challenge-{}", token),
        r#match: Some(RouteMatch {
            path_specifier: Some(PathSpecifier::Path(format!(
                "/.well-known/acme-challenge/{}",
                token
            ))),
            ..Default::default()
        }),
        action: Some(Action::DirectResponse(DirectResponseAction {
            status: 200,
            body: Some(DataSource {
                specifier: Some(Specifier::InlineString(key_authorization.to_string())),
                watched_directory: None,
            }),
        })),
        ..Default::default()
    }
}
