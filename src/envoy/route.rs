use xds_api::pb::envoy::config::core::v3::data_source::Specifier;
use xds_api::pb::envoy::config::core::v3::DataSource;
use xds_api::pb::envoy::config::route::v3::{
    route::Action, route_match::PathSpecifier, DirectResponseAction, Route, RouteMatch, VirtualHost,
};

use crate::acme::ChallengeState;

/// Build a virtual host with the given routes
pub fn build_virtual_host(name: &str, domains: Vec<String>, routes: Vec<Route>) -> VirtualHost {
    VirtualHost {
        name: name.to_string(),
        domains,
        routes,
        ..Default::default()
    }
}

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

/// Build all ACME challenge routes from current challenge state
pub async fn build_acme_challenge_routes(challenge_state: &ChallengeState) -> Vec<Route> {
    challenge_state
        .get_all()
        .await
        .into_iter()
        .map(|c| build_acme_challenge_route(&c.token, &c.key_authorization))
        .collect()
}
